//! The crucible harness: the single `PhysicsApi` chokepoint, the two-world
//! (visible + hidden replay) driver, and the rendered-app entry point.
//!
//! [`CrucibleWorld`] is the *only* place in the app that constructs or commands
//! physics. It owns one [`PhysicsApi`] and a parallel registry of [`CrucibleBody`]
//! metadata. The physics module exposes exactly one facade, so its snapshot /
//! record / contact / material types are **unnameable** from this app — the world
//! therefore reads them only as inferred locals and immediately projects them into
//! the app-owned value types in [`crate::crucible_report`] ([`BodyState`],
//! [`ContactInfo`], [`StepCounts`]). Nothing outside this method-set ever touches a
//! physics type other than the three public handles. [`Crucible`] runs two
//! identical worlds in lock-step, which makes determinism a *visible* property.

use axiom::prelude::{
    Angle, App, Assets, Camera, Color, DefaultPlugins, DirectionalLight, Material, Mesh,
    PerspectiveProjection, PointLight, Renderable, RunningApp, SceneCommands, Texture, Transform,
    Vec3, Window,
};
use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

use crate::crucible_camera;
use crate::crucible_report::{BodyState, ContactInfo, CrucibleReport, StepCounts};
use crate::crucible_scenario::{
    BodySpec, CrucibleKind, CrucibleShape, KindTag, Station, FIXED_STEP_NANOS, HERO_STEP, RUN_STEPS,
};
use crate::crucible_station::CrucibleStation;
use crate::debug_geometry::{self, palette, CrucibleMesh, RenderInstance};
use crate::physics_to_render;

/// Build the explicit fixed [`RuntimeStep`] for global step `n` (frame == tick ==
/// sequence == `n`). The crucible always feeds this exact delta, so every run is
/// byte-reproducible.
pub fn runtime_step(n: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(n), Tick::new(n), FIXED_STEP_NANOS, n)
}

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).expect("crucible authored a finite ratio")
}

fn meters(v: f32) -> Meters {
    Meters::new(v).expect("crucible authored a finite length")
}

/// App-side metadata for one body the crucible created, recorded at spawn time so
/// translation never needs to interrogate physics for a body's kind.
#[derive(Debug, Clone, Copy)]
pub struct CrucibleBody {
    pub handle: PhysicsBodyHandle,
    pub station: CrucibleStation,
    pub kind: KindTag,
    pub shape: CrucibleShape,
    pub is_trigger: bool,
}

/// One physics world plus the app-side body registry. The single point in the app
/// that touches `PhysicsApi`.
#[derive(Debug)]
pub struct CrucibleWorld {
    physics: PhysicsApi,
    bodies: Vec<CrucibleBody>,
    total_events: u64,
    last_step: Option<u64>,
}

impl CrucibleWorld {
    /// A fresh world with the deterministic default configuration (gravity
    /// `(0, -9.8, 0)`, 8 solver iterations).
    pub fn new() -> Self {
        CrucibleWorld {
            physics: PhysicsApi::new(),
            bodies: Vec::new(),
            total_events: 0,
            last_step: None,
        }
    }

    /// Create a body + collider from a [`BodySpec`], offset into `station`'s cell,
    /// and record its render metadata. Returns the new body handle. The physics
    /// material/collider values live only as inferred locals here — never named.
    pub fn spawn(&mut self, station: CrucibleStation, spec: BodySpec) -> PhysicsBodyHandle {
        let world_pos = station.origin().add(spec.local);
        let transform = Transform::from_translation(world_pos);
        let handle = match spec.kind {
            CrucibleKind::Static => self
                .physics
                .create_static_body(transform)
                .expect("static body"),
            CrucibleKind::Dynamic { mass } => self
                .physics
                .create_dynamic_body(transform, ratio(mass))
                .expect("dynamic body"),
            CrucibleKind::Kinematic => self
                .physics
                .create_kinematic_body(transform)
                .expect("kinematic body"),
        };
        let material = PhysicsApi::material(
            ratio(spec.material.friction),
            ratio(spec.material.restitution),
            ratio(spec.material.density),
        )
        .expect("material");
        match spec.shape {
            CrucibleShape::Sphere { radius } => {
                self.physics
                    .attach_sphere_collider(handle, meters(radius), material, spec.is_trigger)
                    .expect("sphere collider");
            }
            CrucibleShape::BoxShape { half_extents } => {
                self.physics
                    .attach_box_collider(handle, half_extents, material, spec.is_trigger)
                    .expect("box collider");
            }
            CrucibleShape::Plane { normal, distance } => {
                self.physics
                    .attach_plane_collider(
                        handle,
                        normal,
                        meters(distance),
                        material,
                        spec.is_trigger,
                    )
                    .expect("plane collider");
            }
            CrucibleShape::Capsule {
                radius,
                half_height,
            } => {
                self.physics
                    .attach_capsule_collider(
                        handle,
                        meters(radius),
                        meters(half_height),
                        material,
                        spec.is_trigger,
                    )
                    .expect("capsule collider");
            }
        }
        self.bodies.push(CrucibleBody {
            handle,
            station,
            kind: spec.kind.tag(),
            shape: spec.shape,
            is_trigger: spec.is_trigger,
        });
        handle
    }

    /// Queue a continuous force on a body (applied at the next step).
    pub fn apply_force(&mut self, body: PhysicsBodyHandle, force: Vec3) {
        self.physics.apply_force(body, force).expect("apply force");
    }

    /// Queue an instantaneous impulse on a body.
    pub fn apply_impulse(&mut self, body: PhysicsBodyHandle, impulse: Vec3) {
        self.physics
            .apply_impulse(body, impulse)
            .expect("apply impulse");
    }

    /// Queue enabling a body.
    pub fn enable(&mut self, body: PhysicsBodyHandle) {
        self.physics.enable_body(body).expect("enable body");
    }

    /// Queue disabling a body.
    pub fn disable(&mut self, body: PhysicsBodyHandle) {
        self.physics.disable_body(body).expect("disable body");
    }

    /// Advance one fixed step, draining the event log to keep it bounded and
    /// accumulating the total event count for the report.
    pub fn step(&mut self, n: u64) {
        self.physics
            .step(runtime_step(n))
            .expect("deterministic step");
        self.total_events += self.physics.drain_events().len() as u64;
        self.last_step = Some(n);
    }

    /// The current per-body state, projected into the app-owned [`BodyState`] (the
    /// physics snapshot type is unnameable here). Insertion order, so two equal
    /// runs yield equal `Vec<BodyState>`.
    pub fn body_states(&self) -> Vec<BodyState> {
        self.physics
            .snapshot()
            .bodies()
            .iter()
            .map(|b| BodyState {
                handle: b.handle(),
                translation: b.transform().translation,
                linear_velocity: b.linear_velocity(),
                enabled: b.enabled(),
            })
            .collect()
    }

    /// The contacts resolved during the most recent step, projected into the
    /// app-owned [`ContactInfo`].
    pub fn contacts(&self) -> Vec<ContactInfo> {
        self.physics
            .latest_contacts()
            .iter()
            .map(|c| ContactInfo {
                body_a: c.body_a(),
                body_b: c.body_b(),
                normal: c.normal(),
                depth: c.depth().get(),
                point: c.point(),
            })
            .collect()
    }

    /// The most recent step's diagnostic counts, projected into the app-owned
    /// [`StepCounts`].
    pub fn step_counts(&self) -> StepCounts {
        let r = self.physics.latest_step_record();
        StepCounts {
            step_index: r.step_index(),
            body_count: r.body_count(),
            collider_count: r.collider_count(),
            dynamic_body_count: r.dynamic_body_count(),
            command_count: r.command_count(),
            event_count: r.event_count(),
            integration_count: r.integration_count(),
            broad_phase_pair_count: r.broad_phase_pair_count(),
            contact_pair_count: r.contact_pair_count(),
            solved_contact_count: r.solved_contact_count(),
            solver_iteration_count: r.solver_iteration_count(),
            substep_count: r.substep_count(),
        }
    }

    /// Cast a ray; returns the nearest solid body hit within `max`.
    pub fn raycast(&self, origin: Vec3, direction: Vec3, max: f32) -> Option<PhysicsBodyHandle> {
        self.physics.raycast(origin, direction, meters(max))
    }

    /// Bodies overlapping a query sphere (triggers included).
    pub fn overlap_sphere(&self, center: Vec3, radius: f32) -> Vec<PhysicsBodyHandle> {
        self.physics.overlap_sphere(center, meters(radius))
    }

    /// The app-side body registry, in spawn order.
    pub fn bodies(&self) -> &[CrucibleBody] {
        &self.bodies
    }

    /// The recorded metadata for `handle`, if it was created here.
    pub fn body(&self, handle: PhysicsBodyHandle) -> Option<&CrucibleBody> {
        self.bodies.iter().find(|b| b.handle == handle)
    }

    /// The handles of `station`'s bodies, in spawn order. Lets a station script a
    /// specific body it created without holding mutable state (handles are
    /// deterministic, so the same lookup resolves identically in both worlds).
    pub fn station_bodies(&self, station: CrucibleStation) -> Vec<PhysicsBodyHandle> {
        self.bodies
            .iter()
            .filter(|b| b.station == station)
            .map(|b| b.handle)
            .collect()
    }

    /// The `index`-th body spawned by `station`, if any.
    pub fn nth_body(&self, station: CrucibleStation, index: usize) -> Option<PhysicsBodyHandle> {
        self.bodies
            .iter()
            .filter(|b| b.station == station)
            .nth(index)
            .map(|b| b.handle)
    }

    /// The current world-space translation of `handle`, read from the snapshot.
    pub fn position_of(&self, handle: PhysicsBodyHandle) -> Option<Vec3> {
        self.body_states()
            .into_iter()
            .find(|b| b.handle == handle)
            .map(|b| b.translation)
    }

    /// The total events emitted (and drained) across the whole run so far.
    pub fn total_events(&self) -> u64 {
        self.total_events
    }

    /// The last global step index advanced, if any.
    pub fn last_step(&self) -> Option<u64> {
        self.last_step
    }
}

impl Default for CrucibleWorld {
    fn default() -> Self {
        CrucibleWorld::new()
    }
}

/// The full crucible: the visible world, the hidden replay world, and the station
/// scripts that drive both. Both worlds receive byte-identical inputs, so their
/// projected states stay equal — and the replay station can deliberately perturb
/// to prove divergence is detected.
#[derive(Debug)]
pub struct Crucible {
    visible: CrucibleWorld,
    replay: CrucibleWorld,
    stations: Vec<Box<dyn Station>>,
    step: u64,
    perturb_replay_at: Option<u64>,
}

impl Crucible {
    /// Build a crucible over the given stations, populating both worlds identically.
    pub fn new(stations: Vec<Box<dyn Station>>) -> Self {
        let mut crucible = Crucible {
            visible: CrucibleWorld::new(),
            replay: CrucibleWorld::new(),
            stations,
            step: 0,
            perturb_replay_at: None,
        };
        for station in &crucible.stations {
            station.populate(&mut crucible.visible);
            station.populate(&mut crucible.replay);
        }
        crucible
    }

    /// Inject a deliberate one-impulse perturbation into the replay world at
    /// `step`, so the replay proof can show divergence is *detected*.
    pub fn perturb_replay_at(&mut self, step: u64) {
        self.perturb_replay_at = Some(step);
    }

    /// Advance both worlds one step, applying every station's script first.
    pub fn step(&mut self) {
        let n = self.step;
        for station in &self.stations {
            station.script(&mut self.visible, n);
            station.script(&mut self.replay, n);
        }
        if self.perturb_replay_at == Some(n) {
            let nudge = self
                .replay
                .bodies()
                .iter()
                .find(|b| b.kind == KindTag::Dynamic)
                .map(|b| b.handle);
            if let Some(body) = nudge {
                self.replay.apply_impulse(body, Vec3::new(2.0, 0.0, 0.0));
            }
        }
        self.visible.step(n);
        self.replay.step(n);
        self.step += 1;
    }

    /// Run the scripted scenario to completion (`RUN_STEPS`).
    pub fn run(&mut self) {
        self.run_to(RUN_STEPS);
    }

    /// Advance until the global step reaches `last` (inclusive).
    pub fn run_to(&mut self, last: u64) {
        while self.step <= last {
            self.step();
        }
    }

    /// Whether the visible and replay worlds currently agree (projected states).
    pub fn replay_matches(&self) -> bool {
        self.visible.body_states() == self.replay.body_states()
    }

    /// The visible world (the one the renderer reads).
    pub fn visible(&self) -> &CrucibleWorld {
        &self.visible
    }

    /// The hidden replay world.
    pub fn replay(&self) -> &CrucibleWorld {
        &self.replay
    }

    /// The number of steps advanced so far.
    pub fn steps_run(&self) -> u64 {
        self.step
    }

    /// The deterministic report for the current state, including a canonical
    /// query-hit count from the query bay's standing overlap probe.
    pub fn report(&self) -> CrucibleReport {
        let (center, radius) = crate::query_bay::probe_world();
        let hits = self.visible.overlap_sphere(center, radius).len() as u32;
        CrucibleReport::build(
            self.step,
            &self.visible.body_states(),
            &self.visible.step_counts(),
            &self.visible.contacts(),
            self.visible.total_events(),
            self.replay_matches(),
        )
        .with_query_hits(hits)
    }
}

// ---------------------------------------------------------------------------
// Rendered entry point
// ---------------------------------------------------------------------------

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// The canvas id the browser demo binds its surface to (matches the gallery
/// manifest's `canvasId`). Harmless for the headless `axiom-shot` path, which
/// reads render data rather than presenting to a surface.
pub const CANVAS_ID: &str = "axiom-physics-crucible-canvas";

fn ch(v: f32) -> Ratio {
    Ratio::new(v).expect("crucible authored a finite colour channel")
}

fn color3(rgb: [f32; 3]) -> Color {
    Color::linear_rgb(ch(rgb[0]), ch(rgb[1]), ch(rgb[2]))
}

/// Pre-simulate the room to the hero step and translate it into render instances.
///
/// The Axiom umbrella `App` has no per-frame external hook, and `PhysicsApi`
/// exposes no teleport, so the crucible cannot stream live physics transforms into
/// the renderer through the umbrella (documented in `README.md`). Instead it
/// pre-simulates deterministically to [`HERO_STEP`] and emits one renderable per
/// body at its settled transform — a faithful static frame of a *real* simulation,
/// with per-step motion proven by the headless harness and the test suite.
fn crucible_instances() -> Vec<RenderInstance> {
    let stations = crate::all_stations();
    let mut world = CrucibleWorld::new();
    for station in &stations {
        station.populate(&mut world);
    }
    let mut step = 0;
    while step <= HERO_STEP {
        for station in &stations {
            station.script(&mut world, step);
        }
        world.step(step);
        step += 1;
    }

    let mut instances = physics_to_render::render_instances(&world.body_states(), world.bodies());
    for station in &stations {
        for shape in station.debug_shapes(&world) {
            instances.extend(debug_geometry::debug_instances(shape));
        }
    }
    for station in CrucibleStation::ALL {
        let here = station.origin().add(Vec3::new(0.0, 7.5, 0.0));
        instances.push(RenderInstance::marker(here, palette::LABEL, 0.4));
    }
    instances
}

/// The crucible as an authored umbrella [`App`] (not yet built): the pre-simulated
/// room as renderables, a high overview camera, a directional sun, and a fill
/// light, bound to [`CANVAS_ID`]. The browser demo `.run()`s it; the headless
/// screenshot path `.build()`s it (see [`build_physics_crucible`]).
pub fn crucible_app() -> App {
    build_render_app(crucible_instances())
}

/// Build the crucible and return the headless [`RunningApp`] the `axiom-shot`
/// screenshot tool composes into a real frame.
pub fn build_physics_crucible() -> RunningApp {
    crucible_app().build()
}

/// Turn the precomputed render instances into an authored umbrella app: three
/// primitive meshes, one lit material per instance colour, a high overview camera,
/// a directional sun, and a fill point light.
fn build_render_app(instances: Vec<RenderInstance>) -> App {
    App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(color3([0.04, 0.05, 0.07])),
        )
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let sphere = meshes.add(Mesh::sphere());
            let plane = meshes.add(Mesh::plane());
            for instance in &instances {
                let mesh = match instance.mesh {
                    CrucibleMesh::Cube => cube,
                    CrucibleMesh::Sphere => sphere,
                    CrucibleMesh::Plane => plane,
                };
                let material = materials.add(Material::lit(color3(instance.color)));
                world.spawn((instance.transform, Renderable { mesh, material }));
            }
            let floor_mat = materials
                .add(Material::lit(color3(palette::FLOOR)).with_texture(Texture::UvGrid));
            world.spawn((
                Transform::combine(
                    Transform::from_translation(Vec3::new(0.0, -0.05, 0.0)),
                    Transform::from_scale(Vec3::new(64.0, 1.0, 48.0)),
                ),
                Renderable {
                    mesh: plane,
                    material: floor_mat,
                },
            ));
            let (eye, target) = crucible_camera::overview();
            world.spawn((
                Transform::from_translation(eye)
                    .looking_at(target, Vec3::UNIT_Y)
                    .expect("overview camera aims at the room"),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(55.0),
                    near: Meters::new(0.1).expect("near"),
                    far: Meters::new(400.0).expect("far"),
                }),
            ));
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.4, -1.0, 0.35),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 18.0, 18.0)),
                PointLight {
                    color: Color::WHITE,
                    intensity: ch(40.0),
                },
            ));
        })
}

// ---------------------------------------------------------------------------
// Live (browser) authoring
// ---------------------------------------------------------------------------

/// The live demo's surface size.
pub fn live_surface_size() -> (u32, u32) {
    (WIDTH, HEIGHT)
}

/// The live backend's per-instance buffer capacity. The room never approaches
/// this many renderables (bodies + contacts + markers), so a per-frame re-author
/// can vary the instance count freely without overflowing the buffer.
pub const LIVE_CAPACITY: u32 = 2048;

/// The renderables for one live frame: every body at its current transform, a
/// marker at each resolved contact, and a status marker over each station cell.
pub fn live_instances(world: &CrucibleWorld) -> Vec<RenderInstance> {
    let mut instances = physics_to_render::render_instances(&world.body_states(), world.bodies());
    for contact in world.contacts() {
        instances.push(RenderInstance::marker(contact.point, palette::CONTACT_POINT, 0.2));
    }
    for station in CrucibleStation::ALL {
        let here = station.origin().add(Vec3::new(0.0, 7.5, 0.0));
        instances.push(RenderInstance::marker(here, palette::LABEL, 0.4));
    }
    instances
}

/// Author one live frame into the scene: three primitive meshes, the fixed
/// palette material set (stable ids across re-authors), one renderable per
/// instance, an orbiting camera looking from `eye` at `target`, and the lights.
/// Re-run every frame by the browser loop via [`RunningApp::reauthor`].
pub fn author_live(
    world: &mut SceneCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<Material>,
    instances: &[RenderInstance],
    eye: Vec3,
    target: Vec3,
) {
    let cube = meshes.add(Mesh::cube());
    let sphere = meshes.add(Mesh::sphere());
    let plane = meshes.add(Mesh::plane());
    let palette: Vec<_> = debug_geometry::LIVE_PALETTE
        .iter()
        .map(|c| materials.add(Material::lit(color3(*c))))
        .collect();
    for instance in instances {
        let mesh = match instance.mesh {
            CrucibleMesh::Cube => cube,
            CrucibleMesh::Sphere => sphere,
            CrucibleMesh::Plane => plane,
        };
        let material = palette[debug_geometry::palette_index(instance.color)];
        world.spawn((instance.transform, Renderable { mesh, material }));
    }
    world.spawn((
        Transform::from_translation(eye)
            .looking_at(target, Vec3::UNIT_Y)
            .expect("live camera aims at the room"),
        Camera::perspective(PerspectiveProjection {
            fov_y: Angle::degrees(55.0),
            near: Meters::new(0.1).expect("near"),
            far: Meters::new(400.0).expect("far"),
        }),
    ));
    world.spawn((
        Transform::IDENTITY,
        DirectionalLight {
            direction: Vec3::new(0.4, -1.0, 0.35),
            color: Color::WHITE,
            intensity: ch(1.0),
        },
    ));
    world.spawn((
        Transform::from_translation(Vec3::new(0.0, 18.0, 18.0)),
        PointLight {
            color: Color::WHITE,
            intensity: ch(40.0),
        },
    ));
}

/// Build the initial live [`RunningApp`] (the room at `instances`, camera at
/// `eye`/`target`) the browser loop then drives and re-authors each frame.
pub fn live_app(instances: Vec<RenderInstance>, eye: Vec3, target: Vec3) -> RunningApp {
    App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(color3([0.04, 0.05, 0.07])),
        )
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            author_live(world, meshes, materials, &instances, eye, target)
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_room_builds_and_renders_draws() {
        let mut world = CrucibleWorld::new();
        for station in crate::all_stations() {
            station.populate(&mut world);
        }
        let instances = live_instances(&world);
        assert!(!instances.is_empty(), "the live room has renderables");
        let (eye, target) = crate::crucible_camera::orbit(0);
        let mut app = live_app(instances, eye, target);
        let outcome = app.tick(0);
        assert!(
            !outcome.draws().is_empty(),
            "the live room renders draws through the umbrella"
        );
    }

    #[test]
    fn spawning_records_metadata_and_returns_valid_handles() {
        let mut world = CrucibleWorld::new();
        let h = world.spawn(
            CrucibleStation::BodyBay,
            BodySpec::dynamic_sphere(Vec3::new(0.0, 5.0, 0.0), 0.5, 1.0),
        );
        assert!(h.is_valid());
        assert_eq!(world.bodies().len(), 1);
        let body = world.body(h).expect("recorded");
        assert_eq!(body.kind, KindTag::Dynamic);
        assert_eq!(body.station, CrucibleStation::BodyBay);
    }

    #[test]
    fn apply_force_accelerates_a_body_along_the_force() {
        let mut world = CrucibleWorld::new();
        let body = world.spawn(
            CrucibleStation::BodyBay,
            BodySpec::dynamic_sphere(Vec3::new(0.0, 10.0, 0.0), 0.5, 1.0),
        );
        world.apply_force(body, Vec3::new(20.0, 0.0, 0.0));
        world.step(0);
        let vx = world
            .body_states()
            .into_iter()
            .find(|s| s.handle == body)
            .unwrap()
            .linear_velocity
            .x;
        assert!(vx > 0.0, "a horizontal force should add +x velocity, got {vx}");
    }

    #[test]
    fn a_dynamic_body_falls_and_a_static_body_does_not() {
        let mut world = CrucibleWorld::new();
        let faller = world.spawn(
            CrucibleStation::BodyBay,
            BodySpec::dynamic_sphere(Vec3::new(0.0, 5.0, 0.0), 0.5, 1.0),
        );
        let fixed = world.spawn(
            CrucibleStation::BodyBay,
            BodySpec::static_box(Vec3::new(3.0, 1.0, 0.0), Vec3::ONE),
        );
        let start = world.position_of(faller).unwrap().y;
        for n in 0..30 {
            world.step(n);
        }
        assert!(world.position_of(faller).unwrap().y < start - 0.1);
        assert_eq!(world.position_of(fixed).unwrap().y, 1.0);
    }
}
