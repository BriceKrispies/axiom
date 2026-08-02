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

use super::road_mesh::{build_chunk, chunk_count, CHUNK_LENGTH};

/// Chunks drawn ahead of the car. At 100 m each this is 1.4 km of road — beyond
/// the far plane's useful range, so nothing pops in even at boosted speed.
pub const CHUNKS_AHEAD: usize = 14;

/// Chunks kept behind the car. Two is enough to cover the chase camera's
/// pull-back and a moment of looking backwards after a spin.
pub const CHUNKS_BEHIND: usize = 2;

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
/// Past 50 m the road's own converging edges and the tarmac/verge boundary
/// carry the sense of distance — the job End Zone hands to broad turf bands
/// rather than to lines.
pub const PAINT_AHEAD_METRES: f32 = 50.0;

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
    /// The paint entities' own, shorter active range. `None` until the first
    /// update after the window is engaged.
    paint_active: Option<(usize, usize)>,
    /// Whether paint is culled to [`PAINT_AHEAD_METRES`] instead of running the
    /// full road distance. Set by the app from the backend it actually bound —
    /// see [`RoadChunks::limit_paint_to_near_field`].
    paint_window: bool,
    triangles: usize,
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
        materials: RoadMaterials,
    ) -> RoadChunks {
        let count = chunk_count(track);
        let mut chunks = Vec::with_capacity(count);
        let mut triangles = 0usize;
        for index in 0..count {
            let meshes = build_chunk(track, index, tuning);
            triangles += (meshes.surface.indices().len()
                + meshes.paint.indices().len()
                + meshes.rail.indices().len()
                + meshes.verge.indices().len())
                / 3;
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
        RoadChunks {
            chunks,
            active: None,
            paint_active: None,
            paint_window: false,
            triangles,
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
            self.paint_active = None;
        }
    }

    /// Whether paint is currently culled to the near field.
    pub const fn paint_is_near_field_only(&self) -> bool {
        self.paint_window
    }

    /// The chunk range road **paint** should occupy for a car at `distance`.
    ///
    /// Without the window this is exactly [`Self::range_for`]. With it, the
    /// range stops at the chunk a [`PAINT_AHEAD_METRES`] look-ahead lands in —
    /// so the granularity is a chunk ([`CHUNK_LENGTH`] m), not a metre, and a
    /// car entering a chunk keeps paint for up to a chunk further than the
    /// nominal distance. That is deliberate: the alternative is rebuilding a
    /// paint mesh every frame, and the flicker this exists to remove comes from
    /// paint *hundreds* of metres out, which a chunk-granular window already
    /// removes.
    pub fn paint_range_for(&self, distance: f32) -> (usize, usize) {
        let full = self.range_for(distance);
        [
            full,
            (
                full.0,
                self.chunk_at(distance + PAINT_AHEAD_METRES).min(full.1),
            ),
        ][usize::from(self.paint_window)]
    }

    /// How many chunks the course has.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether the course produced no chunks at all.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// The total triangle count of the whole course's road geometry.
    pub const fn total_triangles(&self) -> usize {
        self.triangles
    }

    /// The currently active `[first, last]` chunk range, if any.
    pub const fn active_range(&self) -> Option<(usize, usize)> {
        self.active
    }

    /// How many chunks are currently drawn.
    pub fn active_count(&self) -> usize {
        self.active.map_or(0, |(a, b)| b - a + 1)
    }

    /// The chunk index containing `distance`.
    pub fn chunk_at(&self, distance: f32) -> usize {
        ((distance / CHUNK_LENGTH).floor().max(0.0) as usize).min(self.chunks.len().saturating_sub(1))
    }

    /// The range that *should* be active for a car at `distance`.
    pub fn range_for(&self, distance: f32) -> (usize, usize) {
        let centre = self.chunk_at(distance);
        let last = self.chunks.len().saturating_sub(1);
        (
            centre.saturating_sub(CHUNKS_BEHIND),
            (centre + CHUNKS_AHEAD).min(last),
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
        let wanted_paint = self.paint_range_for(distance);
        if self.active == Some(wanted) && self.paint_active == Some(wanted_paint) {
            return false;
        }
        let previous = self.active;
        let previous_paint = self.paint_active;
        self.active = Some(wanted);
        self.paint_active = Some(wanted_paint);
        // While the window is engaged, paint gets its own pass — its range is
        // much shorter and so moves far more often than the surface range does,
        // and riding the surface's early-out would leave markings a chunk behind
        // the car. While it is off, `set_visible` owns the paint entity along
        // with the other three and this pass does not run at all, so the GPU
        // path is exactly what it was before the window existed.
        if self.paint_window && previous_paint != Some(wanted_paint) {
            for (index, chunk) in self.chunks.iter().enumerate() {
                let show = index >= wanted_paint.0 && index <= wanted_paint.1;
                app.set(chunk.paint, Visible(show));
            }
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
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn fixture() -> (RunningApp, Track, RoadChunks) {
        let track = Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT);
        let mut app = App::new()
            .window(Window::new(320, 200))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build();
        let materials = palette::road_materials(&mut app);
        let chunks = RoadChunks::install(&mut app, &track, &CourseTuning::DEFAULT, materials);
        (app, track, chunks)
    }

    #[test]
    fn installing_creates_one_entity_set_per_chunk_all_retired() {
        let (app, track, chunks) = fixture();
        assert_eq!(chunks.len(), chunk_count(&track));
        assert!(!chunks.is_empty());
        assert_eq!(chunks.active_count(), 0, "nothing is drawn until the first update");
        assert!(chunks.total_triangles() > 10_000, "the course has real geometry");
        for chunk in &chunks.chunks {
            for entity in chunk.each() {
                assert_eq!(app.get::<Visible>(entity), Some(Visible(false)));
            }
        }
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

    #[test]
    fn the_paint_window_culls_markings_far_sooner_than_the_road_surface() {
        let (_app, _track, mut road) = fixture();
        // Off by default: paint runs exactly as far as the tarmac.
        assert!(!road.paint_is_near_field_only());
        let distance = CHUNK_LENGTH * 3.0;
        assert_eq!(road.paint_range_for(distance), road.range_for(distance));

        road.set_paint_near_field_only(true);
        assert!(road.paint_is_near_field_only());
        let surface = road.range_for(distance);
        let paint = road.paint_range_for(distance);
        assert_eq!(paint.0, surface.0, "paint keeps the same chunks behind");
        assert!(
            paint.1 < surface.1,
            "paint stops well short of the road: paint {paint:?} vs surface {surface:?}"
        );
        // Its edge is the look-ahead distance, not a chunk count.
        assert_eq!(paint.1, road.chunk_at(distance + PAINT_AHEAD_METRES));
    }

    #[test]
    fn engaging_the_paint_window_hides_paint_the_road_still_draws() {
        let (mut app, _track, mut road) = fixture();
        let distance = CHUNK_LENGTH * 3.0;
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
        let distance = CHUNK_LENGTH * 3.0;
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
        assert_eq!(chunks.chunk_at(CHUNK_LENGTH * 1.5), 1);
        assert_eq!(chunks.chunk_at(track.length() * 5.0), chunks.len() - 1);
    }
}
