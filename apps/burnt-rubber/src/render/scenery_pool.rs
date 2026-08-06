//! The roadside instance pool: the bridge between "what scenery exists here"
//! ([`super::scenery`]) and "what the renderer draws".
//!
//! Props are generated per chunk and cached; the cache is refreshed only when
//! the active chunk range changes, which at racing speed is about once a second.
//! Each frame, the cached props are culled and level-of-detailed through the
//! engine's [`axiom_visibility::VisibilityApi`], and the survivors are written
//! into per-kind entity pools.
//!
//! Two engine capabilities do the heavy lifting here rather than app code:
//! `visible_mask` (frustum culling against the camera's clip matrix) and
//! `lod_levels` (distance banding). Reusing them is the point — "what can the
//! camera see, and how finely" is exactly the kind of general question that
//! belongs in a module, and the app has no business re-deriving a frustum test.
//! What stays app-local is the *racing* decision: which archetypes a zone uses,
//! how far each kind is worth drawing, and what a reduced tier looks like.

use axiom::prelude::{Entity, Handle, Material, Mesh, RunningApp, Spawn, Transform, Vec3, Visible};
use axiom_math::{Aabb, Mat4, Quat};
use axiom_visibility::VisibilityApi;

use crate::track::Track;
use crate::tuning::CourseTuning;

use super::chunks::{CHUNKS_AHEAD, CHUNKS_BEHIND};
use super::palette::ScenePalette;
use super::scenery::{prop_bounds, props_for_chunk, PropInstance, PropKind};

/// The LOD distance bands, in metres. Tier 0 is full detail; every tier beyond
/// it is drawn *reduced*.
///
/// These bands decide how finely a prop is drawn. They deliberately do **not**
/// decide *whether* it is drawn — that is [`PropKind::draw_distance`]'s job, and
/// its alone. The two used to disagree: a tier past the last band was skipped
/// outright, which quietly capped every kind at 340 m and made the per-kind
/// distances (a tree at 700 m, a building at 900 m) dead numbers. The visible
/// symptom was a roadside that stopped dead a third of the way down the road and
/// a horizon with nothing on it but hills — an avenue of trees has to recede all
/// the way to the vanishing point or it does not read as an avenue at all.
///
/// Two culls that answer the same question in two places will always drift
/// apart, so there is now one: the frustum test, and each kind's own distance.
pub const LOD_BANDS: [f32; 2] = [120.0, 340.0];

/// How much a tier-1 prop is shrunk. Reducing the silhouette rather than
/// swapping the mesh keeps the pool to one mesh per kind (and therefore one draw
/// call per kind), which matters far more at this distance than the shape does.
pub const LOD_FAR_SCALE: f32 = 0.82;

/// One kind's entity pool.
#[derive(Debug, Clone)]
struct KindPool {
    kind: PropKind,
    entities: Vec<Entity>,
}

/// The roadside scenery pool and its per-chunk cache.
#[derive(Debug, Clone)]
pub struct SceneryField {
    pools: Vec<KindPool>,
    /// Cached props, keyed by chunk index. Bounded by the active range.
    cache: Vec<(usize, Vec<PropInstance>)>,
    /// Scratch buffers, reused every refresh so a chunk change allocates nothing.
    scratch: Vec<PropInstance>,
    candidates: Vec<PropInstance>,
    boxes: Vec<Aabb>,
    active_range: Option<(usize, usize)>,
    seed: u64,
    drawn: usize,
}

impl SceneryField {
    /// Spawn every pool, retired, plus the static distant hills.
    pub fn install(
        app: &mut RunningApp,
        palette: &ScenePalette,
        track: &Track,
        seed: u64,
    ) -> SceneryField {
        let cube = app.add_mesh(Mesh::cube());
        let cylinder = app.add_mesh(Mesh::cylinder());
        let cone = super::prop_meshes::install_cone(app);
        let frond_fan = super::prop_meshes::install_palm_crown(app);

        let pools = PropKind::ALL
            .iter()
            .map(|kind| {
                let (mesh, material) =
                    mesh_and_material(*kind, palette, cube, cylinder, cone, frond_fan);
                KindPool {
                    kind: *kind,
                    entities: (0..kind.pool_capacity())
                        .map(|_| retired(app, mesh, material))
                        .collect(),
                }
            })
            .collect();

        // The horizon: spawned once, visible always, never touched again. There
        // are a few dozen of them and they are visible from everywhere, so
        // streaming them would be pure overhead.
        for hill in super::scenery::distant_hills(seed, track) {
            app.spawn(Spawn::new(
                Transform::new(
                    hill.position,
                    Quat::from_euler_xyz(0.0, hill.yaw, 0.0),
                    hill.scale,
                ),
                cube,
                palette.stone,
            ));
        }

        SceneryField {
            pools,
            cache: Vec::with_capacity(CHUNKS_AHEAD + CHUNKS_BEHIND + 2),
            scratch: Vec::new(),
            candidates: Vec::new(),
            boxes: Vec::new(),
            active_range: None,
            seed,
            drawn: 0,
        }
    }

    /// How many prop instances were drawn last frame.
    pub const fn drawn_count(&self) -> usize {
        self.drawn
    }

    /// How many chunks of scenery are cached.
    pub fn cached_chunks(&self) -> usize {
        self.cache.len()
    }

    /// Refresh the cache for the active chunk range. Cheap when nothing changed.
    pub fn refresh(
        &mut self,
        track: &Track,
        tuning: &CourseTuning,
        range: (usize, usize),
    ) -> bool {
        if self.active_range == Some(range) {
            return false;
        }
        self.active_range = Some(range);
        // Drop what left the window; keep what stayed (so its props are not
        // regenerated); generate what entered.
        self.cache.retain(|(index, _)| *index >= range.0 && *index <= range.1);
        for index in range.0..=range.1 {
            if self.cache.iter().any(|(i, _)| *i == index) {
                continue;
            }
            props_for_chunk(self.seed, track, index, tuning, &mut self.scratch);
            self.cache.push((index, std::mem::take(&mut self.scratch)));
        }
        // A stable order, so the pool assignment below is deterministic.
        self.cache.sort_by_key(|(index, _)| *index);
        true
    }

    /// Cull, level-of-detail and pose everything for this frame.
    pub fn pose(&mut self, app: &mut RunningApp, camera_eye: Vec3, view_proj: Mat4) {
        self.candidates.clear();
        self.boxes.clear();
        for (_, props) in &self.cache {
            for prop in props {
                // A cheap distance reject first, so the frustum test and the
                // bounding boxes are only built for plausible candidates.
                if prop.position.distance(camera_eye) > prop.kind.draw_distance() {
                    continue;
                }
                let (centre, half) = prop_bounds(prop);
                let Ok(aabb) = Aabb::from_center_extents(centre, half) else {
                    continue;
                };
                self.candidates.push(*prop);
                self.boxes.push(aabb);
            }
        }

        let visible = VisibilityApi::visible_mask(view_proj, &self.boxes);
        let bands = [
            axiom::prelude::Meters::finite_or_zero(LOD_BANDS[0]),
            axiom::prelude::Meters::finite_or_zero(LOD_BANDS[1]),
        ];
        let levels = VisibilityApi::lod_levels(camera_eye, &self.boxes, &bands);

        self.drawn = 0;
        for pool in &self.pools {
            let mut slot = 0usize;
            for (index, prop) in self.candidates.iter().enumerate() {
                if prop.kind != pool.kind {
                    continue;
                }
                if !visible.get(index).copied().unwrap_or(false) {
                    continue;
                }
                let level = levels.get(index).copied().unwrap_or(0);
                let Some(entity) = pool.entities.get(slot) else {
                    // The pool is full. This is a hard ceiling by design, and
                    // the test suite proves the generator never reaches it.
                    break;
                };
                let shrink = if level == 0 { 1.0 } else { LOD_FAR_SCALE };
                app.set(
                    *entity,
                    prop_transform(prop, pool.kind, shrink),
                );
                app.set(*entity, Visible(true));
                slot += 1;
                self.drawn += 1;
            }
            for entity in pool.entities.iter().skip(slot) {
                app.set(*entity, Visible(false));
            }
        }
    }
}

/// The transform a prop instance is drawn with.
fn prop_transform(prop: &PropInstance, kind: PropKind, shrink: f32) -> Transform {
    let half = kind.half_extents();
    let scale = Vec3::new(
        half.x * 2.0 * prop.scale.x * shrink,
        half.y * 2.0 * prop.scale.y * shrink,
        half.z * 2.0 * prop.scale.z * shrink,
    );
    // Props stand on their base, so the mesh centre is half a height up.
    let lift = Vec3::new(0.0, scale.y * 0.5, 0.0);
    Transform::new(
        prop.position.add(lift),
        Quat::from_euler_xyz(0.0, prop.yaw, 0.0),
        scale,
    )
}

/// The mesh and material a kind draws with.
fn mesh_and_material(
    kind: PropKind,
    palette: &ScenePalette,
    cube: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    cone: Handle<Mesh>,
    frond_fan: Handle<Mesh>,
) -> (Handle<Mesh>, Handle<Material>) {
    match kind {
        PropKind::Post => (cube, palette.post),
        PropKind::Tree => (cone, palette.foliage),
        PropKind::Rock => (cube, palette.stone),
        PropKind::Pole => (cylinder, palette.timber),
        PropKind::Sign => (cube, palette.sign),
        PropKind::TunnelLight => (cube, palette.lamp),
        PropKind::Building => (cube, palette.building),
        PropKind::PalmTrunk => (cylinder, palette.timber),
        PropKind::PalmCrown => (frond_fan, palette.foliage),
    }
}

/// Spawn a pool slot, parked and invisible.
fn retired(app: &mut RunningApp, mesh: Handle<Mesh>, material: Handle<Material>) -> Entity {
    let entity = app.spawn(Spawn::new(Transform::IDENTITY, mesh, material));
    app.set(entity, Visible(false));
    entity
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn fixture() -> (RunningApp, Track, SceneryField) {
        let track = Track::generate(crate::DEFAULT_SEED, &CourseTuning::DEFAULT);
        let mut app = App::new()
            .window(Window::new(320, 200))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build();
        let palette = ScenePalette::install(&mut app);
        let field = SceneryField::install(&mut app, &palette, &track, crate::DEFAULT_SEED);
        (app, track, field)
    }

    /// A view matrix looking down the road from `eye`.
    fn view_proj(eye: Vec3, target: Vec3) -> Mat4 {
        let view = Mat4::look_at(eye, target, Vec3::UNIT_Y).expect("a valid view");
        let projection = Mat4::perspective(1.2, 16.0 / 9.0, 0.3, 1_500.0).expect("a projection");
        projection.multiply(view)
    }

    #[test]
    fn installing_creates_every_pool_retired() {
        let (app, _, field) = fixture();
        assert_eq!(field.pools.len(), PropKind::ALL.len());
        assert_eq!(field.drawn_count(), 0);
        for pool in &field.pools {
            assert_eq!(pool.entities.len(), pool.kind.pool_capacity());
            for e in &pool.entities {
                assert_eq!(app.get::<Visible>(*e), Some(Visible(false)));
            }
        }
    }

    #[test]
    fn the_cache_follows_the_active_range_and_stays_bounded() {
        let (_, track, mut field) = fixture();
        let t = CourseTuning::DEFAULT;
        assert!(field.refresh(&track, &t, (0, 16)));
        assert_eq!(field.cached_chunks(), 17);
        assert!(!field.refresh(&track, &t, (0, 16)), "an unchanged range is free");

        assert!(field.refresh(&track, &t, (4, 20)));
        assert_eq!(field.cached_chunks(), 17, "still bounded");
    }

    /// The recycling guarantee, end to end: leaving a chunk and coming back
    /// gives byte-identical scenery.
    #[test]
    fn revisiting_a_chunk_regenerates_identical_scenery() {
        let (_, track, mut field) = fixture();
        let t = CourseTuning::DEFAULT;
        field.refresh(&track, &t, (10, 26));
        let before = field
            .cache
            .iter()
            .find(|(i, _)| *i == 12)
            .map(|(_, props)| props.clone())
            .expect("chunk 12 is cached");

        // Drive far away and come back.
        field.refresh(&track, &t, (60, 76));
        assert!(!field.cache.iter().any(|(i, _)| *i == 12), "it was evicted");
        field.refresh(&track, &t, (10, 26));
        let after = field
            .cache
            .iter()
            .find(|(i, _)| *i == 12)
            .map(|(_, props)| props.clone())
            .expect("chunk 12 is back");
        assert_eq!(before, after);
    }

    #[test]
    fn a_chunk_that_stays_in_range_is_not_regenerated() {
        let (_, track, mut field) = fixture();
        let t = CourseTuning::DEFAULT;
        field.refresh(&track, &t, (10, 26));
        let kept: *const PropInstance = field
            .cache
            .iter()
            .find(|(i, _)| *i == 20)
            .map(|(_, props)| props.as_ptr())
            .expect("cached");
        field.refresh(&track, &t, (11, 27));
        let still: *const PropInstance = field
            .cache
            .iter()
            .find(|(i, _)| *i == 20)
            .map(|(_, props)| props.as_ptr())
            .expect("still cached");
        assert_eq!(kept, still, "the same allocation was kept, not rebuilt");
    }

    #[test]
    fn posing_draws_props_in_front_of_the_camera_and_nothing_behind() {
        let (mut app, track, mut field) = fixture();
        let t = CourseTuning::DEFAULT;
        field.refresh(&track, &t, (2, 18));
        let here = track.sample_at(300.0);
        let eye = here.position.add(Vec3::new(0.0, 3.0, 0.0));
        let ahead = track.sample_at(360.0).position;
        field.pose(&mut app, eye, view_proj(eye, ahead));

        assert!(field.drawn_count() > 0, "something is drawn");
        // Everything drawn is genuinely in front of the eye.
        let forward = ahead.subtract(eye).normalize().unwrap();
        for pool in &field.pools {
            for entity in &pool.entities {
                if app.get::<Visible>(*entity) != Some(Visible(true)) {
                    continue;
                }
                let p = app.get::<Transform>(*entity).unwrap().translation;
                assert!(
                    p.subtract(eye).dot(forward) > -40.0,
                    "a {:?} at {p:?} is behind the camera",
                    pool.kind
                );
            }
        }
    }

    #[test]
    fn turning_the_camera_away_culls_the_scenery() {
        let (mut app, track, mut field) = fixture();
        let t = CourseTuning::DEFAULT;
        field.refresh(&track, &t, (2, 18));
        let here = track.sample_at(300.0);
        let eye = here.position.add(Vec3::new(0.0, 3.0, 0.0));

        field.pose(&mut app, eye, view_proj(eye, track.sample_at(360.0).position));
        let looking_at_it = field.drawn_count();
        // Look back the way we came: the road ahead leaves the frustum.
        let behind = track.sample_at(240.0).position;
        field.pose(&mut app, eye, view_proj(eye, behind));
        let looking_away = field.drawn_count();
        assert!(
            looking_away < looking_at_it,
            "culling does something: {looking_at_it} -> {looking_away}"
        );
    }

    /// The one cull: a prop is drawn until *its own kind's* distance runs out.
    ///
    /// The distance bands only reduce it. Reinstating a band-based drop here is
    /// exactly the edit that empties the middle distance again, so it fails.
    #[test]
    fn only_a_kinds_own_draw_distance_stops_it_being_drawn() {
        let (mut app, track, mut field) = fixture();
        let t = CourseTuning::DEFAULT;
        field.refresh(&track, &t, (2, 18));
        let here = track.sample_at(300.0);
        let eye = here.position.add(Vec3::new(0.0, 3.0, 0.0));
        field.pose(&mut app, eye, view_proj(eye, track.sample_at(360.0).position));

        let mut beyond_the_last_band = 0;
        for pool in &field.pools {
            for entity in &pool.entities {
                if app.get::<Visible>(*entity) != Some(Visible(true)) {
                    continue;
                }
                let p = app.get::<Transform>(*entity).unwrap().translation;
                let range = p.distance(eye);
                assert!(
                    range <= pool.kind.draw_distance() + 60.0,
                    "a {:?} is drawn at {range} m, past its {} m limit",
                    pool.kind,
                    pool.kind.draw_distance()
                );
                if range > LOD_BANDS[LOD_BANDS.len() - 1] {
                    beyond_the_last_band += 1;
                }
            }
        }
        assert!(
            beyond_the_last_band > 0,
            "the middle distance is empty: nothing survives past {} m even though \
             a tree is worth drawing to {} m",
            LOD_BANDS[LOD_BANDS.len() - 1],
            PropKind::Tree.draw_distance()
        );
        assert!(LOD_FAR_SCALE < 1.0, "the far tier is genuinely reduced");
    }

    #[test]
    fn the_drawn_count_never_exceeds_the_pools() {
        let (mut app, track, mut field) = fixture();
        let t = CourseTuning::DEFAULT;
        let ceiling: usize = PropKind::ALL.iter().map(|k| k.pool_capacity()).sum();
        for chunk in (0..60).step_by(3) {
            field.refresh(&track, &t, (chunk, chunk + 16));
            let sample = track.sample_at(chunk as f32 * 100.0 + 50.0);
            let eye = sample.position.add(Vec3::new(0.0, 3.0, 0.0));
            let ahead = track.sample_at(sample.distance + 60.0).position;
            field.pose(&mut app, eye, view_proj(eye, ahead));
            assert!(field.drawn_count() <= ceiling, "{} props drawn", field.drawn_count());
        }
    }

    #[test]
    fn props_stand_on_the_ground_rather_than_sinking_through_it() {
        let prop = PropInstance {
            kind: PropKind::Tree,
            position: Vec3::new(0.0, 10.0, 0.0),
            yaw: 0.0,
            scale: Vec3::ONE,
        };
        let t = prop_transform(&prop, PropKind::Tree, 1.0);
        let base = t.translation.y - t.scale.y * 0.5;
        assert!((base - 10.0).abs() < 1.0e-4, "the base sits at the position");
        let shrunk = prop_transform(&prop, PropKind::Tree, LOD_FAR_SCALE);
        assert!(shrunk.scale.y < t.scale.y);
        let shrunk_base = shrunk.translation.y - shrunk.scale.y * 0.5;
        assert!((shrunk_base - 10.0).abs() < 1.0e-4, "and still does when reduced");
    }

    #[test]
    fn every_kind_maps_to_a_mesh_and_material() {
        let mut app = App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build();
        let palette = ScenePalette::install(&mut app);
        let cube = app.add_mesh(Mesh::cube());
        let cylinder = app.add_mesh(Mesh::cylinder());
        let cone = super::super::prop_meshes::install_cone(&mut app);
        let frond_fan = super::super::prop_meshes::install_palm_crown(&mut app);
        for kind in PropKind::ALL {
            let (_, material) = mesh_and_material(kind, &palette, cube, cylinder, cone, frond_fan);
            // Every kind gets a real material handle rather than a default.
            assert!(
                [
                    palette.post,
                    palette.foliage,
                    palette.stone,
                    palette.timber,
                    palette.sign,
                    palette.lamp,
                    palette.building
                ]
                .contains(&material),
                "{kind:?} has no material"
            );
        }
    }
}
