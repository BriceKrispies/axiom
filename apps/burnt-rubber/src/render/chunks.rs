//! The road-chunk lifecycle: which stretch of road is currently drawn.
//!
//! Every chunk's geometry is built **once**, at install, and spawned as four
//! retired entities. Streaming is then purely a visibility decision: as the car
//! moves, chunks entering the active range are shown and chunks leaving it are
//! hidden. Nothing is rebuilt, nothing is despawned, nothing is re-uploaded.
//!
//! That is deliberate and it is the right shape for *this* game. The course is
//! bounded (nine kilometres, ~92 chunks), so its geometry fits comfortably in
//! memory; and the live browser backend sizes its vertex buffers once at
//! startup, so a mesh created after the render loop begins would never reach the
//! GPU at all. Rebuilding chunks on the fly would therefore be both slower and
//! wrong. What genuinely has to be bounded is the number of chunks *drawn*, and
//! that is exactly what the active range bounds.
//!
//! The range is deliberately lopsided — far more road ahead than behind —
//! because at 90 m/s the player covers a chunk every 1.1 s and needs to see the
//! next corner long before arriving at it, while the road behind is out of frame
//! the moment it is passed.

use axiom::prelude::{Entity, Handle, Material, Mesh, RunningApp, Spawn, Transform, Visible};

use crate::track::Track;
use crate::tuning::CourseTuning;

use super::road_mesh::{
    build_draw_mesh, build_paint_chunk, draw_count, paint_chunk_count, DRAW_SPAN,
    PAINT_CHUNK_LENGTH,
};

/// **Scenery** cells drawn ahead of the car, in
/// [`super::road_mesh::CHUNK_LENGTH`] units. At 100 m each this is 1.4 km —
/// beyond the far plane's useful range, so nothing pops in even at boosted speed.
///
/// This pair sizes the roadside: which cells the generator populates and how deep
/// [`super::scenery_pool`] makes its instance pools. It is deliberately *not* the
/// road's own window (see [`DRAWS_AHEAD`]). The road batches several cells into
/// one mesh to save draw calls, and if the scenery simply followed that window it
/// would widen with it — the same props, generated over a third more course,
/// overflowing pools that were sized correctly. How the road is batched and how
/// much roadside is alive are two questions, and only one of them was ever about
/// draw calls.
pub const CHUNKS_AHEAD: usize = 14;

/// Scenery cells kept behind the car. Two is enough to cover the chase camera's
/// pull-back and a moment of looking backwards after a spin.
pub const CHUNKS_BEHIND: usize = 2;

/// **Road meshes** drawn ahead of the car, in [`DRAW_SPAN`] units — 1.6-2.0 km,
/// past [`crate::render::FAR_PLANE`].
///
/// Counted in drawn meshes rather than cells, which is the point of the split:
/// this covers *more* road than the old 14-cell window did, in five draws instead
/// of fifteen. See [`super::road_mesh::MESHES_PER_DRAW`] for the measurement.
pub const DRAWS_AHEAD: usize = 4;

/// Road meshes kept behind the car. One [`DRAW_SPAN`] is 400-800 m, which covers
/// the chase camera's pull-back and a look backwards after a spin several times
/// over.
///
/// This is the coarse batch's one genuine waste: most frames draw a few hundred
/// metres of road nobody can see, because the granularity that would make the
/// window tight is exactly the granularity that made it expensive. It costs four
/// draw calls and it buys the rest.
pub const DRAWS_BEHIND: usize = 1;

/// How far ahead of the car road **paint** is drawn once the near-field paint
/// window is engaged, metres.
///
/// The tarmac still runs to the horizon; only the markings stop. The reason is
/// End Zone's (`apps/end-zone/src/field/paint.rs`) and it is a property of the
/// raster rather than of taste: the Canvas 2D software rasterizer runs a
/// low-resolution framebuffer, and a lane dash a few tens of metres out
/// projects to *less than one pixel*. Sub-pixel geometry cannot be drawn
/// stably — its coverage flips on and off as the camera moves, so it shimmers —
/// and it costs a projection and a shade per triangle to produce that shimmer.
/// End Zone's rule is the one applied here: cull a marking while it is still
/// several pixels across, never once it has decayed into an unstable fragment.
///
/// Past the window the road's own converging edges and the tarmac/verge
/// boundary carry the sense of distance — the job End Zone hands to broad turf
/// bands rather than to lines.
///
/// Five metres, not the fifty this was: the markings the software raster draws
/// well are the ones under and immediately in front of the car, where a dash is
/// tens of pixels long. Everything past that was the shimmer. Note this is a
/// *distance*, and it only became an honest one when paint got its own
/// [`PAINT_CHUNK_LENGTH`] chunking — against the surface's 100 m chunks the same
/// number bought between 80 m and 150 m of markings depending on where in a
/// chunk the car happened to be.
pub const PAINT_AHEAD_METRES: f32 = 5.0;

/// How far *behind* the car road paint is drawn once the window is engaged,
/// metres, for a given chase rig.
///
/// It is not a taste number and it is not a constant: it is **wherever the eye
/// is**. The bottom edge of the frame is road a little in front of the camera,
/// and the camera sits behind the car — so the window has to reach back past
/// the eye or the bottom band of the picture is bare tarmac with the markings
/// starting part-way up it. The furthest back the rig ever puts the eye is its
/// top-speed chase distance plus the boost pull, and that is exactly this.
///
/// Hard-coding it worked only while the rig was one fixed arm authored for one
/// fixed frame. [`crate::tuning::CameraTuning::framed_for_aspect`] makes the arm
/// a function of the frame's shape, so a fixed 6 m is a number that silently
/// stops being true on a taller screen — and only on the software raster, the
/// one arm nobody scores and everybody has to keep legible.
pub fn paint_behind_metres(camera: &crate::tuning::CameraTuning) -> f32 {
    camera.distance_high + camera.distance_boost
}

/// The four material-separated entities one chunk occupies.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ChunkEntities {
    surface: Entity,
    paint: Entity,
    rail: Entity,
    verge: Entity,
}

impl ChunkEntities {
    fn each(&self) -> [Entity; 4] {
        [self.surface, self.paint, self.rail, self.verge]
    }
}

/// The road's chunk set and its active window.
#[derive(Debug, Clone)]
pub struct RoadChunks {
    chunks: Vec<ChunkEntities>,
    active: Option<(usize, usize)>,
    /// The near-field paint set: the same markings as the chunks' own `paint`,
    /// cut at [`PAINT_CHUNK_LENGTH`] instead of [`CHUNK_LENGTH`].
    ///
    /// Two chunkings of one set of markings, and only ever one of them on
    /// screen. The coarse set is what the GPU draws, because at 100 m a chunk it
    /// is a dozen draw calls for the whole visible road. The fine set exists so
    /// the Canvas 2D window can be *sharp* — a window is only as precise as the
    /// geometry it switches, and switching 100 m chunks is how "five metres
    /// ahead" turned into a hundred and fifty.
    fine_paint: Vec<Entity>,
    /// The fine paint set's active range, in paint-chunk indices.
    fine_paint_active: Option<(usize, usize)>,
    /// Whether paint is culled to [`PAINT_AHEAD_METRES`] instead of running the
    /// full road distance. Set by the app from the backend it actually bound —
    /// see [`RoadChunks::limit_paint_to_near_field`].
    paint_window: bool,
    /// How far behind the car the near-field window reaches — the rig's own
    /// eye, from [`paint_behind_metres`], because the frame's bottom edge is
    /// road just in front of the camera and the camera is behind the car.
    paint_behind: f32,
    /// Each chunk's own triangle count, so the drawn total is a sum over the
    /// active range.
    per_chunk: Vec<usize>,
}

/// The material handles a chunk's four meshes are drawn with.
#[derive(Debug, Clone, Copy)]
pub struct RoadMaterials {
    pub surface: Handle<Material>,
    pub paint: Handle<Material>,
    pub rail: Handle<Material>,
    pub verge: Handle<Material>,
}

impl RoadChunks {
    /// Build and spawn every chunk of `track`, all retired.
    pub fn install(
        app: &mut RunningApp,
        track: &Track,
        tuning: &CourseTuning,
        camera: &crate::tuning::CameraTuning,
        materials: RoadMaterials,
    ) -> RoadChunks {
        let count = draw_count(track);
        let mut chunks = Vec::with_capacity(count);
        let mut per_chunk = Vec::with_capacity(count);
        for index in 0..count {
            let meshes = build_draw_mesh(track, index, tuning);
            let chunk_triangles = (meshes.surface.indices().len()
                + meshes.paint.indices().len()
                + meshes.rail.indices().len()
                + meshes.verge.indices().len())
                / 3;
            per_chunk.push(chunk_triangles);
            let spawn_part = |app: &mut RunningApp, data, material| {
                spawn_retired(app, data, material)
            };
            chunks.push(ChunkEntities {
                surface: spawn_part(app, meshes.surface, materials.surface),
                paint: spawn_part(app, meshes.paint, materials.paint),
                rail: spawn_part(app, meshes.rail, materials.rail),
                verge: spawn_part(app, meshes.verge, materials.verge),
            });
        }
        // The fine paint set. Retired like everything else, and never touched at
        // all until an arm asks for the window — a GPU-only session pays for the
        // meshes and not a draw call more.
        let fine_paint = (0..paint_chunk_count(track))
            .map(|index| {
                let data = build_paint_chunk(track, index, tuning);
                spawn_retired(app, data, materials.paint)
            })
            .collect();

        RoadChunks {
            chunks,
            active: None,
            fine_paint,
            fine_paint_active: None,
            paint_window: false,
            paint_behind: paint_behind_metres(camera),
            per_chunk,
        }
    }

    /// Cull road paint to the near field, or stop doing so.
    ///
    /// Driven by the backend the app actually bound
    /// (`WindowingApi::bound_backend`): on for the Canvas 2D software
    /// rasterizer, off for the GPU, whose framebuffer resolves distant markings
    /// perfectly well. It is a property of the raster, not a preference — which
    /// is why the app asks the backend rather than guessing from the URL: a page
    /// that fell back to Canvas 2D because the GPU refused a device needs this
    /// exactly as much as one that asked for it.
    ///
    /// Settable both ways, and not a latch, because the bound backend is not
    /// one: a device-loss rebuild re-runs the cascade, so a page that started on
    /// the GPU can come back on Canvas 2D and vice versa.
    pub fn set_paint_near_field_only(&mut self, limited: bool) {
        let changed = limited != self.paint_window;
        self.paint_window = limited;
        // Force the next update to re-evaluate rather than early-out on an
        // unchanged range: the ranges may be identical, but which pass owns the
        // paint entity is not. Only on a real change, or the early-out — the
        // whole reason `update` is cheap — would never fire again.
        if changed {
            self.active = None;
        }
    }

    /// Whether paint is currently culled to the near field.
    pub const fn paint_is_near_field_only(&self) -> bool {
        self.paint_window
    }

    /// The **fine** paint-chunk range for a car at `distance` — the near-field
    /// window, in [`PAINT_CHUNK_LENGTH`] units.
    ///
    /// This is the window the player actually sees: markings from just behind
    /// the car to [`PAINT_AHEAD_METRES`] in front of it, and nothing beyond.
    /// The granularity is now ten metres rather than a hundred, so the number
    /// means what it says to within one dash.
    pub fn fine_paint_range_for(&self, distance: f32) -> (usize, usize) {
        let last = self.fine_paint.len().saturating_sub(1);
        let first = ((distance - self.paint_behind).max(0.0) / PAINT_CHUNK_LENGTH) as usize;
        let end = ((distance + PAINT_AHEAD_METRES).max(0.0) / PAINT_CHUNK_LENGTH) as usize;
        (first.min(last), end.min(last))
    }

    /// How many chunks the course has.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether the course produced no chunks at all.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// The triangles the road is actually **drawing** this frame: the active
    /// range's own, not the whole course's.
    ///
    /// This replaced a course-total counter, which is what made the old
    /// telemetry useless: a figure labelled "what the scene drew" that reads
    /// 109,916 in every section of the course cannot answer a question about
    /// any of them.
    pub fn active_triangles(&self) -> usize {
        self.active.map_or(0, |(lo, hi)| {
            self.per_chunk
                .get(lo..=hi.min(self.per_chunk.len().saturating_sub(1)))
                .map_or(0, |span| span.iter().sum())
        })
    }

    /// The currently active `[first, last]` **drawn-mesh** range, if any.
    pub const fn active_range(&self) -> Option<(usize, usize)> {
        self.active
    }

    /// The **scenery** cell window for a car at `distance`, in
    /// [`super::road_mesh::CHUNK_LENGTH`] units — or `None` before the road has
    /// any drawn mesh active at all, which is the signal that there is nothing to
    /// dress.
    ///
    /// Derived from the distance directly rather than from the road's own window,
    /// because the two windows answer different questions and have different
    /// units. Keying the roadside off the road's batching would make how many
    /// shrubs exist a consequence of how many draw calls the road spends, which is
    /// how a batching change turns into a pool overflow.
    ///
    /// Bounded to the course's cell count so the generator is never handed a cell
    /// that does not exist — the road's own window is clamped the same way, and
    /// the last drawn mesh reaches past the last cell whenever the course does not
    /// divide evenly by [`super::road_mesh::MESHES_PER_DRAW`].
    pub fn scenery_range_for(&self, track: &Track, distance: f32) -> Option<(usize, usize)> {
        self.active?;
        let last = super::road_mesh::chunk_count(track).saturating_sub(1);
        let centre = ((distance / super::road_mesh::CHUNK_LENGTH).floor().max(0.0) as usize)
            .min(last);
        Some((
            centre.saturating_sub(CHUNKS_BEHIND),
            (centre + CHUNKS_AHEAD).min(last),
        ))
    }

    /// How many meshes the road is currently drawing.
    pub fn active_count(&self) -> usize {
        self.active.map_or(0, |(a, b)| b - a + 1)
    }

    /// The drawn-mesh index containing `distance`.
    pub fn chunk_at(&self, distance: f32) -> usize {
        ((distance / DRAW_SPAN).floor().max(0.0) as usize).min(self.chunks.len().saturating_sub(1))
    }

    /// The drawn-mesh range that *should* be active for a car at `distance`.
    pub fn range_for(&self, distance: f32) -> (usize, usize) {
        let centre = self.chunk_at(distance);
        let last = self.chunks.len().saturating_sub(1);
        (
            centre.saturating_sub(DRAWS_BEHIND),
            (centre + DRAWS_AHEAD).min(last),
        )
    }

    /// Show the chunks around `distance` and hide the rest.
    ///
    /// Returns `true` if the visible set actually changed. The early-out is the
    /// whole reason this is cheap: the range changes roughly once a second at
    /// racing speed, so on every other frame this costs one comparison.
    pub fn update(&mut self, app: &mut RunningApp, distance: f32) -> bool {
        if self.chunks.is_empty() {
            return false;
        }
        let wanted = self.range_for(distance);
        // `None` when the window is off, which is also what `fine_paint_active`
        // reads once the fine set has been retired — so the early-out below is
        // one comparison on the GPU path, exactly as it was.
        let wanted_paint = self
            .paint_window
            .then(|| self.fine_paint_range_for(distance));
        if self.active == Some(wanted) && self.fine_paint_active == wanted_paint {
            return false;
        }
        let previous = self.active;
        let previous_paint = self.fine_paint_active;
        self.active = Some(wanted);
        self.fine_paint_active = wanted_paint;
        // The two paint sets hand over to each other here, and exactly one of
        // them is ever on screen. Engaging the window retires the coarse set the
        // chunks own; releasing it retires the fine set. `set_paint_near_field_only`
        // clears both ranges, so a `None` on either side *is* that transition.
        match (previous_paint, wanted_paint) {
            (_, Some(range)) => {
                previous_paint
                    .is_none()
                    .then(|| self.retire_coarse_paint(app));
                self.show_fine_paint(app, previous_paint, range);
            }
            (Some(_), None) => self.retire_fine_paint(app),
            (None, None) => {}
        }
        // Hide only what actually left the window, and show only what entered.
        if let Some((old_lo, old_hi)) = previous {
            for index in old_lo..=old_hi {
                if index < wanted.0 || index > wanted.1 {
                    self.set_visible(app, index, false);
                }
            }
            for index in wanted.0..=wanted.1 {
                if index < old_lo || index > old_hi {
                    self.set_visible(app, index, true);
                }
            }
        } else {
            for (index, _) in self.chunks.iter().enumerate() {
                let show = index >= wanted.0 && index <= wanted.1;
                self.set_visible(app, index, show);
            }
        }
        true
    }

    /// Place the fine paint set: show what entered the window, hide what left.
    ///
    /// `previous` is `None` on the frame the window engages, and then every fine
    /// chunk is written once — the set is a thousand entities and only a handful
    /// are ever wanted, so the alternative is a thousand stale `Visible(true)`s
    /// nobody clears.
    fn show_fine_paint(
        &self,
        app: &mut RunningApp,
        previous: Option<(usize, usize)>,
        wanted: (usize, usize),
    ) {
        match previous {
            Some((old_lo, old_hi)) => {
                (old_lo..=old_hi)
                    .filter(|i| *i < wanted.0 || *i > wanted.1)
                    .for_each(|i| self.set_fine_paint(app, i, false));
                (wanted.0..=wanted.1)
                    .filter(|i| *i < old_lo || *i > old_hi)
                    .for_each(|i| self.set_fine_paint(app, i, true));
            }
            None => (0..self.fine_paint.len())
                .for_each(|i| self.set_fine_paint(app, i, i >= wanted.0 && i <= wanted.1)),
        }
    }

    /// Hide every fine paint chunk — the fine set handing back to the coarse one.
    fn retire_fine_paint(&self, app: &mut RunningApp) {
        (0..self.fine_paint.len()).for_each(|i| self.set_fine_paint(app, i, false));
    }

    /// Hide every chunk's own paint — the coarse set handing over to the fine one.
    fn retire_coarse_paint(&self, app: &mut RunningApp) {
        self.chunks
            .iter()
            .for_each(|chunk| {
                app.set(chunk.paint, Visible(false));
            });
    }

    /// Show or hide one fine paint chunk.
    fn set_fine_paint(&self, app: &mut RunningApp, index: usize, visible: bool) {
        self.fine_paint
            .get(index)
            .map(|entity| app.set(*entity, Visible(visible)));
    }

    /// Show or hide one chunk's meshes.
    ///
    /// Paint is skipped while the near-field window is engaged: it is owned by
    /// the paint pass in [`Self::update`], and writing it here too would show
    /// markings across the whole surface range on every frame that range moved.
    fn set_visible(&self, app: &mut RunningApp, index: usize, visible: bool) {
        if let Some(chunk) = self.chunks.get(index) {
            for entity in chunk.each() {
                let owned_by_paint_pass = self.paint_window && entity == chunk.paint;
                if !owned_by_paint_pass {
                    app.set(entity, Visible(visible));
                }
            }
        }
    }
}

/// Spawn one chunk mesh, retired (parked at the origin and invisible) until the
/// active range claims it.
///
/// The geometry is authored in **world space**, so the entity transform is the
/// identity: the road is not a model placed on the course, it *is* the course.
fn spawn_retired(
    app: &mut RunningApp,
    data: axiom::prelude::MeshData,
    material: Handle<Material>,
) -> Entity {
    // An empty mesh (a chunk with no guardrail, say) still gets an entity, so
    // the four-entity-per-chunk layout is uniform and indexable by arithmetic.
    let mesh = app
        .add_mesh_data(data)
        .unwrap_or_else(|_| app.add_mesh(Mesh::cube()));
    let entity = app.spawn(Spawn::new(Transform::IDENTITY, mesh, material));
    app.set(entity, Visible(false));
    entity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::palette;
    use crate::render::road_mesh::{chunk_count, draw_count};
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn fixture() -> (RunningApp, Track, RoadChunks) {
        let track = Track::fixture(crate::DEFAULT_SEED);
        let mut app = App::new()
            .window(Window::new(320, 200))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build();
        let materials = palette::road_materials(&mut app);
        let chunks = RoadChunks::install(
            &mut app,
            &track,
            &CourseTuning::DEFAULT,
            &crate::tuning::CameraTuning::DEFAULT,
            materials,
        );
        (app, track, chunks)
    }

    #[test]
    fn installing_creates_one_entity_set_per_chunk_all_retired() {
        let (app, track, chunks) = fixture();
        assert_eq!(chunks.len(), draw_count(&track));
        // The split this file exists to keep: the road is spawned per *drawn
        // mesh*, and there are strictly fewer of those than authoring cells.
        assert!(
            draw_count(&track) < chunk_count(&track),
            "the road is batching several cells per draw"
        );
        assert!(!chunks.is_empty());
        assert_eq!(chunks.active_count(), 0, "nothing is drawn until the first update");
        assert_eq!(
            chunks.active_triangles(),
            0,
            "nothing is drawn, so nothing is counted as drawn"
        );
        for chunk in &chunks.chunks {
            for entity in chunk.each() {
                assert_eq!(app.get::<Visible>(entity), Some(Visible(false)));
            }
        }
    }

    #[test]
    fn the_drawn_triangle_count_is_the_active_range_not_the_course() {
        let (mut app, _track, mut chunks) = fixture();
        chunks.update(&mut app, 0.0);
        let near = chunks.active_triangles();
        assert!(near > 0, "an active range draws real geometry");

        // The point of the counter: it must be a *fraction* of the course, or it
        // is the course total under a different name and answers nothing.
        let whole_course: usize = chunks.per_chunk.iter().sum();
        assert!(whole_course > 10_000, "the course has real geometry");
        assert!(
            near < whole_course / 2,
            "the drawn count ({near}) should be far below the course total ({whole_course})"
        );

        // And it moves with the car rather than staying pinned to chunk zero.
        chunks.update(&mut app, 4_000.0);
        let (lo, _) = chunks.active_range().expect("a range is active");
        assert!(lo > 0, "the car has left the opening chunks");
        assert!(chunks.active_triangles() > 0);
    }

    #[test]
    fn the_active_range_follows_the_car_and_is_bounded() {
        let (mut app, track, mut chunks) = fixture();
        for distance in [0.0f32, 500.0, 2_400.0, 6_000.0, track.length()] {
            chunks.update(&mut app, distance);
            let (lo, hi) = chunks.active_range().expect("a range is active");
            assert!(lo <= hi);
            assert!(
                chunks.active_count() <= CHUNKS_AHEAD + CHUNKS_BEHIND + 1,
                "at {distance} m, {} chunks are drawn",
                chunks.active_count()
            );
            let centre = chunks.chunk_at(distance);
            assert!(lo <= centre && centre <= hi, "the car's own chunk is drawn");
        }
    }

    #[test]
    fn only_the_active_chunks_are_visible() {
        let (mut app, _, mut chunks) = fixture();
        chunks.update(&mut app, 3_000.0);
        let (lo, hi) = chunks.active_range().expect("active");
        for (index, chunk) in chunks.chunks.iter().enumerate() {
            let expected = index >= lo && index <= hi;
            for entity in chunk.each() {
                assert_eq!(
                    app.get::<Visible>(entity),
                    Some(Visible(expected)),
                    "chunk {index} visibility"
                );
            }
        }
    }

    /// Chunk activation is a pure function of distance, so two cars at the same
    /// point on the course see exactly the same road.
    #[test]
    fn chunk_activation_is_deterministic() {
        let (mut app_a, _, mut a) = fixture();
        let (mut app_b, _, mut b) = fixture();
        // Reached by different routes: one jumps straight there, the other
        // crawls. The active set must be identical.
        a.update(&mut app_a, 4_250.0);
        for step in 0..=170 {
            b.update(&mut app_b, step as f32 * 25.0);
        }
        assert_eq!(a.active_range(), b.active_range());
    }

    #[test]
    fn an_unchanged_range_does_no_work() {
        let (mut app, _, mut chunks) = fixture();
        assert!(chunks.update(&mut app, 1_000.0), "the first update always applies");
        assert!(!chunks.update(&mut app, 1_000.0), "an identical position is a no-op");
        assert!(
            !chunks.update(&mut app, 1_010.0),
            "and so is moving within the same chunk"
        );
        assert!(
            chunks.update(&mut app, 1_200.0),
            "crossing a chunk boundary does apply"
        );
    }

    #[test]
    fn the_range_clamps_at_both_ends_of_the_course() {
        let (mut app, track, mut chunks) = fixture();
        chunks.update(&mut app, -5_000.0);
        let (lo, hi) = chunks.active_range().expect("active");
        assert_eq!(lo, 0);
        assert!(hi < chunks.len());

        chunks.update(&mut app, track.length() * 10.0);
        let (lo, hi) = chunks.active_range().expect("active");
        assert_eq!(hi, chunks.len() - 1);
        assert!(lo <= hi);
    }

    /// Recycling the visible window cannot change what a chunk contains: the
    /// geometry was built once and is only being shown and hidden.
    #[test]
    fn cycling_the_window_does_not_change_the_chunks() {
        let (mut app, _, mut chunks) = fixture();
        chunks.update(&mut app, 500.0);
        let before: Vec<[Entity; 4]> = chunks.chunks.iter().map(|c| c.each()).collect();
        for i in 0..80 {
            chunks.update(&mut app, i as f32 * 120.0);
        }
        chunks.update(&mut app, 500.0);
        let after: Vec<[Entity; 4]> = chunks.chunks.iter().map(|c| c.each()).collect();
        assert_eq!(before, after, "the same entities, never respawned");
    }

    /// The window is a distance, and this is the test that makes it one.
    ///
    /// Against the surface's 100 m chunks the same `PAINT_AHEAD_METRES` bought
    /// between 80 m and 150 m of markings depending on where in a chunk the car
    /// stood — the window was nominally 50 m and never once delivered it. Paint
    /// has its own chunking now, so the answer has to hold *wherever* the car is,
    /// which is why this sweeps a whole chunk's worth of offsets rather than
    /// checking one convenient distance.
    #[test]
    fn the_paint_window_is_metres_of_road_wherever_the_car_stands() {
        let (_app, _track, mut road) = fixture();
        assert!(!road.paint_is_near_field_only());
        road.set_paint_near_field_only(true);
        assert!(road.paint_is_near_field_only());

        // One paint chunk of slack either side: the window switches whole
        // chunks, so it can only ever be as sharp as one of them.
        let slack = PAINT_CHUNK_LENGTH;
        for offset in [0.0, 3.0, 17.0, 49.0, 83.0, 99.0] {
            let distance = DRAW_SPAN * 3.0 + offset;
            let (first, last) = road.fine_paint_range_for(distance);
            let starts = first as f32 * PAINT_CHUNK_LENGTH;
            let ends = (last + 1) as f32 * PAINT_CHUNK_LENGTH;
            assert!(
                ends > distance,
                "there is always paint under and ahead of the car (offset {offset})"
            );
            assert!(
                ends - distance <= PAINT_AHEAD_METRES + slack,
                "markings run {:.0} m ahead at offset {offset}, not ~{PAINT_AHEAD_METRES}",
                ends - distance
            );
            assert!(
                distance - starts <= road.paint_behind + slack,
                "and {:.0} m behind, not the whole chunk",
                distance - starts
            );
        }
    }

    /// The two paint sets are one set of markings cut two ways, and exactly one
    /// is ever on screen: a frame showing both would double-draw every dash.
    #[test]
    fn only_one_of_the_two_paint_sets_is_ever_visible() {
        let (mut app, _track, mut road) = fixture();
        let distance = DRAW_SPAN * 3.0;
        road.update(&mut app, distance);

        let coarse_on = |app: &RunningApp, road: &RoadChunks| {
            road.chunks
                .iter()
                .filter(|c| app.get::<Visible>(c.paint) == Some(Visible(true)))
                .count()
        };
        let fine_on = |app: &RunningApp, road: &RoadChunks| {
            road.fine_paint
                .iter()
                .filter(|e| app.get::<Visible>(**e) == Some(Visible(true)))
                .count()
        };

        assert!(coarse_on(&app, &road) > 0, "the GPU path draws the coarse set");
        assert_eq!(fine_on(&app, &road), 0, "and none of the fine one");

        road.set_paint_near_field_only(true);
        road.update(&mut app, distance);
        assert_eq!(coarse_on(&app, &road), 0, "engaging retires the coarse set");
        let near = fine_on(&app, &road);
        assert!(near > 0, "and places the fine one");
        assert!(near <= 3, "a handful of chunks, not the road: {near}");

        road.set_paint_near_field_only(false);
        road.update(&mut app, distance);
        assert!(coarse_on(&app, &road) > 0, "releasing hands it back");
        assert_eq!(fine_on(&app, &road), 0, "and retires the fine set");
    }

    #[test]
    fn engaging_the_paint_window_hides_paint_the_road_still_draws() {
        let (mut app, _track, mut road) = fixture();
        let distance = DRAW_SPAN * 3.0;
        road.update(&mut app, distance);
        let far = road.range_for(distance).1;
        let far_paint = road.chunks[far].paint;
        let far_surface = road.chunks[far].surface;
        assert_eq!(
            app.get::<Visible>(far_paint),
            Some(Visible(true)),
            "without the window the farthest chunk paints"
        );

        road.set_paint_near_field_only(true);
        road.update(&mut app, distance);
        assert_eq!(
            app.get::<Visible>(far_surface),
            Some(Visible(true)),
            "the tarmac still runs to the horizon"
        );
        assert_eq!(
            app.get::<Visible>(far_paint),
            Some(Visible(false)),
            "but its markings are culled before they go sub-pixel"
        );

        // A device-loss rebuild can put the page back on the GPU, so the window
        // has to come off as cleanly as it went on.
        road.set_paint_near_field_only(false);
        road.update(&mut app, distance);
        assert_eq!(
            app.get::<Visible>(far_paint),
            Some(Visible(true)),
            "back on the GPU the far markings return"
        );
    }

    #[test]
    fn setting_the_paint_window_to_what_it_already_is_keeps_the_early_out() {
        // The early-out is the whole reason `update` is cheap. Feeding the
        // window the same answer every frame — which is exactly what an app
        // polling the bound backend does — must not defeat it.
        let (mut app, _track, mut road) = fixture();
        let distance = DRAW_SPAN * 3.0;
        road.set_paint_near_field_only(true);
        assert!(road.update(&mut app, distance), "the first update places");

        road.set_paint_near_field_only(true);
        assert!(
            !road.update(&mut app, distance),
            "an unchanged window and an unchanged distance change nothing"
        );
    }

    #[test]
    fn the_chunk_index_maps_distance_to_a_valid_chunk() {
        let (_, track, chunks) = fixture();
        assert_eq!(chunks.chunk_at(-100.0), 0);
        assert_eq!(chunks.chunk_at(0.0), 0);
        assert_eq!(chunks.chunk_at(DRAW_SPAN * 1.5), 1);
        assert_eq!(chunks.chunk_at(track.length() * 5.0), chunks.len() - 1);
    }
}
