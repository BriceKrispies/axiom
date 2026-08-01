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
            triangles,
        }
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
        if self.active == Some(wanted) {
            return false;
        }
        let previous = self.active;
        self.active = Some(wanted);
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

    fn set_visible(&self, app: &mut RunningApp, index: usize, visible: bool) {
        if let Some(chunk) = self.chunks.get(index) {
            for entity in chunk.each() {
                app.set(entity, Visible(visible));
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
    fn the_chunk_index_maps_distance_to_a_valid_chunk() {
        let (_, track, chunks) = fixture();
        assert_eq!(chunks.chunk_at(-100.0), 0);
        assert_eq!(chunks.chunk_at(0.0), 0);
        assert_eq!(chunks.chunk_at(CHUNK_LENGTH * 1.5), 1);
        assert_eq!(chunks.chunk_at(track.length() * 5.0), chunks.len() - 1);
    }
}
