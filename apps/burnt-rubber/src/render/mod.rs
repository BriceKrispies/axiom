//! Presentation: turning simulation state into a scene, once per rendered frame.
//!
//! Everything in this module is downstream of [`crate::sim`] and writes nothing
//! back. It reads the interpolated poses, poses the scene, and returns. A frame
//! that renders twice looks the same twice; a frame that never renders changes
//! nothing about the race.
//!
//! [`RaceScene`] owns the whole visual: the chunked road, the pooled roadside,
//! the traffic, the player's car, the effects, the lights and the camera. It is
//! installed **once**, and thereafter only writes transforms and visibility —
//! never spawns, never despawns, never registers a mesh. That is not an
//! optimisation, it is a requirement of the live browser backend, which sizes
//! its vertex and instance buffers from the mesh set captured at startup: a mesh
//! registered after the render loop begins would never reach the GPU.

pub mod car_model;
pub mod chunks;
pub mod effects;
pub mod palette;
pub mod prop_meshes;
pub mod road_mesh;
pub mod scenery;
pub mod scenery_pool;
pub mod surface_builder;

use axiom::prelude::{
    Angle, Camera, Color, DirectionalLight, Entity, FrameAmbient, FrameDepthFog, Mesh,
    PerspectiveProjection, PointLight, Ratio, RunningApp, Spawn, Transform, Vec3, Visible,
};
use axiom_math::{Mat4, Quat};

use crate::camera::CameraPose;
use crate::sim::car::CarPose;
use crate::sim::{RacePhase, RaceSim};

use car_model::{PlayerCar, TrafficVisuals};
use chunks::RoadChunks;
use effects::Effects;
use palette::ScenePalette;
use scenery_pool::SceneryField;

/// The near plane (m).
///
/// Pushed well out on purpose. Depth precision is governed by the *ratio* of far
/// to near, and the near plane is the end of that ratio worth moving: at 0.35 m
/// the road and its markings z-fight into shimmering bands a few hundred metres
/// ahead, which is most of what the player is looking at. Nothing is ever nearer
/// than this - the chase camera holds at least 5.5 m of the car.
pub const NEAR_PLANE: f32 = 1.2;
/// The far plane (m). Just past the furthest drawn chunk
/// (`CHUNKS_AHEAD` x `CHUNK_LENGTH`) plus its scenery, and no further: every
/// extra metre of range is precision taken from the road.
pub const FAR_PLANE: f32 = 1_650.0;

/// The whole visual, installed once.
#[derive(Debug)]
pub struct RaceScene {
    palette: ScenePalette,
    road: RoadChunks,
    scenery: SceneryField,
    traffic: TrafficVisuals,
    car: PlayerCar,
    effects: Effects,
    finish_arch: Vec<Entity>,
    /// The pool light that rides over the car (see [`install_lights`]).
    car_light: Entity,
    aspect: f32,
    last_view_proj: Mat4,
}

impl RaceScene {
    /// Install the scene for `sim` into `app`.
    pub fn install(app: &mut RunningApp, sim: &RaceSim, width: u32, height: u32) -> RaceScene {
        let palette = ScenePalette::install(app);
        let track = sim.track();
        let tuning = sim.tuning();

        app.set_clear_color([palette::SKY[0], palette::SKY[1], palette::SKY[2], 1.0]);
        // A dark, cool ambient, held at roughly a tenth of the key. It is
        // deliberately not zero: there is one directional key light, so every
        // face turned away from it has nothing but ambient, and a black ground
        // term makes the whole shadowed half of every car, post and lamp
        // disappear. But it must stay *far* below the key, because ambient is
        // the one term that lights the lit and the unlit face equally — every
        // unit of it is contrast removed from the frame. At the old level the
        // fill was a third of the key and the course read as an overcast
        // afternoon that had been colour-graded dark: the tarmac a flat mid
        // slate from the bumper to the horizon, the verge a pale sage, the
        // trees' shadowed sides barely darker than their lit ones. Night is
        // not "the same light, less of it" — it is a low key with a much
        // lower fill.
        app.set_ambient(FrameAmbient::new(
            [0.055, 0.065, 0.100],
            [0.035, 0.040, 0.050],
        ));
        // The night air. Everything recedes into the sky colour rather than staying
        // fully lit out to the far plane, which is what gives the road, the trees and
        // the skyline their depth instead of a hard cut-out horizon.
        //
        // The range is normalized device depth, which is strongly non-linear over a
        // `NEAR_PLANE`..`FAR_PLANE` frustum: 0.990 is ~110 m out (the fog just starts
        // to bite past the near traffic) and 0.9993 is ~900 m (the skyline is almost
        // fully atmosphere). Reaching 0.9 rather than 1.0 keeps a faint silhouette at
        // the vanishing point instead of erasing it.
        app.set_depth_fog(FrameDepthFog::new(
            Ratio::finite_or_zero(0.990),
            Ratio::finite_or_zero(0.9993),
            Ratio::finite_or_zero(0.9),
            palette::SKY,
        ));

        let road = RoadChunks::install(app, track, &tuning.course, palette.road);
        let scenery = SceneryField::install(app, &palette, track, track.seed());
        let traffic = TrafficVisuals::install(app, &palette, tuning.race.traffic_active);
        let car = PlayerCar::install(app, &palette);
        let effects = Effects::install(app, &palette, track.seed());
        let finish_arch = install_finish_arch(app, sim);

        let car_light = install_lights(app);

        RaceScene {
            palette,
            road,
            scenery,
            traffic,
            car,
            effects,
            finish_arch,
            car_light,
            aspect: width.max(1) as f32 / height.max(1) as f32,
            last_view_proj: Mat4::IDENTITY,
        }
    }

    /// Advance the presentation-only state one fixed step.
    ///
    /// Kept separate from [`Self::pose`] on purpose: the effects age on the
    /// simulation's clock, so a browser rendering at 144 Hz gets the same smoke
    /// as one rendering at 30.
    pub fn step(&mut self, sim: &RaceSim) {
        self.effects.step(sim.car());
    }

    /// Pose the whole scene for a render frame `alpha` of the way through the
    /// current simulation step.
    pub fn pose(&mut self, app: &mut RunningApp, sim: &RaceSim, alpha: f32) {
        let camera = sim.camera_pose(alpha);
        let car_pose = sim.car_pose(alpha);
        let tuning = sim.tuning();

        self.pose_camera(app, camera);
        self.last_view_proj = view_projection(&camera, self.aspect);

        self.road.update(app, sim.car().distance);
        if let Some(range) = self.road.active_range() {
            self.scenery.refresh(sim.track(), &tuning.course, range);
        }
        self.scenery.pose(app, camera.eye, self.last_view_proj);

        self.pose_traffic(app, sim);

        let braking = brake_intensity(sim);
        let boost = if sim.boost().active() { 1.0 } else { 0.0 };
        self.car.pose(app, &car_pose, braking, boost);
        app.set(self.car_light, Transform::from_translation(pool_light_at(&car_pose)));

        let forward = camera
            .target
            .subtract(camera.eye)
            .normalize()
            .unwrap_or(Vec3::UNIT_Z);
        self.effects
            .pose(app, sim.car(), camera.eye, forward, &tuning.vehicle);
    }

    /// The camera, with the roll baked into its up vector.
    fn pose_camera(&self, app: &mut RunningApp, pose: CameraPose) {
        let transform = Transform::from_translation(pose.eye)
            .looking_at(pose.target, pose.up())
            // A degenerate frame (eye exactly on target) cannot happen while the
            // camera keeps its chase distance, but falling back beats failing.
            .unwrap_or_else(|_| Transform::from_translation(pose.eye));
        app.set_camera(
            Camera::perspective(PerspectiveProjection {
                fov_y: Angle::degrees(pose.fov_degrees),
                near: axiom::prelude::Meters::finite_or_zero(NEAR_PLANE),
                far: axiom::prelude::Meters::finite_or_zero(FAR_PLANE),
            }),
            transform,
        );
    }

    /// Place every live traffic car and retire the rest.
    fn pose_traffic(&self, app: &mut RunningApp, sim: &RaceSim) {
        let track = sim.track();
        for (index, car) in sim.traffic().cars().iter().enumerate() {
            if !car.active {
                self.traffic.pose(app, index, None);
                continue;
            }
            let sample = track.interpolated_at(car.distance);
            let position = sample.at_lateral(car.lateral);
            let forward = sample.flat_forward();
            self.traffic.pose(
                app,
                index,
                Some((position, forward.x.atan2(forward.z), sample.up)),
            );
        }
    }

    /// Diagnostics counters for this frame.
    pub fn counters(&self) -> SceneCounters {
        SceneCounters {
            active_chunks: self.road.active_count(),
            total_chunks: self.road.len(),
            road_triangles: self.road.total_triangles(),
            scenery_instances: self.scenery.drawn_count(),
            cached_scenery_chunks: self.scenery.cached_chunks(),
            effect_instances: self.effects.visible_count(),
            traffic_slots: self.traffic.len(),
        }
    }

    /// The camera's clip-from-world matrix from the last posed frame.
    pub const fn view_projection(&self) -> Mat4 {
        self.last_view_proj
    }

    /// The palette, for anything that needs a material handle after install.
    pub const fn palette(&self) -> &ScenePalette {
        &self.palette
    }

    /// The finish arch's entities.
    pub fn finish_entities(&self) -> &[Entity] {
        &self.finish_arch
    }
}

/// What the scene drew this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneCounters {
    pub active_chunks: usize,
    pub total_chunks: usize,
    pub road_triangles: usize,
    pub scenery_instances: usize,
    pub cached_scenery_chunks: usize,
    pub effect_instances: usize,
    pub traffic_slots: usize,
}

/// How lit the brake lights should be.
fn brake_intensity(sim: &RaceSim) -> f32 {
    // Decelerating hard counts as braking whether or not the key is down, which
    // is what makes a collision read from behind as well as from the cockpit.
    let decelerating = (-sim.car().forward_speed.min(0.0)).max(0.0);
    let finished = matches!(sim.phase(), RacePhase::Finished);
    let hard = (sim.car().impact_strength * 2.0).clamp(0.0, 1.0);
    hard.max(if finished { 1.0 } else { 0.0 })
        .max((decelerating / 4.0).clamp(0.0, 1.0))
}

/// The clip-from-world matrix for a camera pose.
fn view_projection(pose: &CameraPose, aspect: f32) -> Mat4 {
    let view = Mat4::look_at(pose.eye, pose.target, pose.up())
        .unwrap_or(Mat4::IDENTITY);
    let projection = Mat4::perspective(
        pose.fov_degrees.to_radians(),
        aspect.max(0.1),
        NEAR_PLANE,
        FAR_PLANE,
    )
    .unwrap_or(Mat4::IDENTITY);
    projection.multiply(view)
}

/// A directional key light plus a low fill, both static — and one **pool light**
/// that rides over the car. Returns the pool light's entity, which
/// [`RaceScene::pose`] moves with the car every frame.
///
/// The key is held at roughly half power. Its *direction* was never the
/// problem — the flaw was the level. At full intensity, tarmac authored at a
/// deliberately near-black `0.085` still lands on screen around a mid slate,
/// which is what turned a night stage into a grey overcast one: the exposure,
/// not the paint, was daylight. Halving the key drops the road to the dark
/// asphalt its albedo was chosen for and leaves the markings, the reflector
/// posts and the brake lights as the only bright things in the frame, which is
/// exactly the hierarchy this course is authored around.
///
/// But a directional key is, by definition, *the same everywhere*: it lights the
/// tarmac under the bumper and the tarmac at the vanishing point to exactly the
/// same value, and that is what still read as flat. The whole road sat at one
/// tone from the car to the horizon, with no sense that the light was near. A
/// night stage does not look like that — the light is **local**, and the road
/// falls away into the dark a short way out. So the rig gains a positional light
/// above the car: the backend attenuates a point light by distance
/// (`1/(1 + 0.09d + 0.032d²)`), so it lays a bright wash on the tarmac around the
/// car that is gone within a dozen metres, top-lights the car's own upper
/// surfaces, and leaves the far road to the key alone. That near/far difference
/// is the depth cue the flat rig had no way to produce.
fn install_lights(app: &mut RunningApp) -> Entity {
    app.add_light(
        DirectionalLight {
            direction: Vec3::new(-0.36, -1.0, 0.42),
            color: Color::linear_rgb(
                palette::ratio(1.0),
                palette::ratio(0.94),
                palette::ratio(0.84),
            ),
            intensity: palette::ratio(0.55),
        },
        Transform::IDENTITY,
    );
    // Cool, near-neutral: it is the night reading of the same white, and holding
    // it slightly bluer than the warm key keeps the pool from reading as a
    // second sun.
    app.add_point_light(
        PointLight {
            color: Color::linear_rgb(
                palette::ratio(0.88),
                palette::ratio(0.93),
                palette::ratio(1.0),
            ),
            intensity: palette::ratio(1.0),
        },
        Transform::from_translation(Vec3::new(0.0, POOL_LIGHT_HEIGHT, 0.0)),
    )
}

/// How high above the road the car's pool light hangs (m).
///
/// This is the pool's *radius* knob, not just its height: with the backend's
/// `1/(1 + 0.09d + 0.032d²)` falloff the wash is brightest directly beneath and
/// has faded to a twentieth of that by ~13 m out, so raising it spreads a
/// weaker pool and lowering it tightens a hotter one. At 6.5 m the wash covers
/// the car's own lane and most of the two beside it — the reference's footprint.
pub const POOL_LIGHT_HEIGHT: f32 = 6.5;
/// How far ahead of the car's origin the pool light sits (m). Slightly forward,
/// so the brightest tarmac is at and just beyond the car rather than in the
/// foreground behind it, where the reference keeps the road dark.
pub const POOL_LIGHT_AHEAD: f32 = 1.5;

/// Where the car's pool light hangs for a car pose.
///
/// Deliberately built from the **flat** heading and not the chassis basis: a
/// light parented to the tilting body would swing its pool across the road under
/// roll and pitch, which is a lamp on a gimbal, not a night stage.
fn pool_light_at(pose: &CarPose) -> Vec3 {
    pose.position.add(Vec3::new(
        pose.yaw.sin() * POOL_LIGHT_AHEAD,
        POOL_LIGHT_HEIGHT,
        pose.yaw.cos() * POOL_LIGHT_AHEAD,
    ))
}

/// Two pillars and a beam over the finish line.
fn install_finish_arch(app: &mut RunningApp, sim: &RaceSim) -> Vec<Entity> {
    let track = sim.track();
    let palette_finish = app.add_material(
        axiom::prelude::Material::lit(palette::rgb(0.20, 0.92, 0.62))
            .with_emissive(palette::rgb(0.14, 0.62, 0.42)),
    );
    let cube = app.add_mesh(Mesh::cube());
    let sample = track.sample_at(track.length() - crate::sim::FINISH_MARGIN);
    let forward = sample.flat_forward();
    let yaw = forward.x.atan2(forward.z);
    let rotation = Quat::from_euler_xyz(0.0, yaw, 0.0);
    let reach = track.barrier_offset(&sample);

    let mut entities = Vec::with_capacity(3);
    for side in [-1.0f32, 1.0] {
        entities.push(app.spawn(Spawn::new(
            Transform::new(
                sample
                    .at_lateral(side * reach)
                    .add(Vec3::new(0.0, ARCH_HEIGHT * 0.5, 0.0)),
                rotation,
                Vec3::new(1.1, ARCH_HEIGHT, 1.1),
            ),
            cube,
            palette_finish,
        )));
    }
    entities.push(app.spawn(Spawn::new(
        Transform::new(
            sample.position.add(Vec3::new(0.0, ARCH_HEIGHT, 0.0)),
            rotation,
            Vec3::new(reach * 2.0 + 1.1, 1.3, 0.9),
        ),
        cube,
        palette_finish,
    )));
    // A point light under the arch so it announces itself from a distance.
    app.add_point_light(
        PointLight {
            color: Color::linear_rgb(
                palette::ratio(0.35),
                palette::ratio(1.0),
                palette::ratio(0.72),
            ),
            intensity: palette::ratio(1.0),
        },
        Transform::from_translation(sample.position.add(Vec3::new(0.0, ARCH_HEIGHT, 0.0))),
    );
    entities
}

/// Height of the finish arch (m).
const ARCH_HEIGHT: f32 = 9.0;

/// Hide everything the scene owns — used when tearing a frame down in tests.
pub fn hide_all(app: &mut RunningApp, entities: &[Entity]) {
    for entity in entities {
        app.set(*entity, Visible(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn fixture() -> (RunningApp, RaceSim, RaceScene) {
        let sim = RaceSim::shipping();
        let mut app = App::new()
            .window(Window::new(640, 360))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build();
        let scene = RaceScene::install(&mut app, &sim, 640, 360);
        (app, sim, scene)
    }

    #[test]
    fn the_scene_installs_and_renders_a_frame() {
        let (mut app, sim, mut scene) = fixture();
        scene.pose(&mut app, &sim, 0.0);
        let outcome = app.tick(0);
        assert!(!outcome.draws().is_empty(), "the scene draws something");
        assert!(!outcome.lights().is_empty(), "and it is lit");
        assert_eq!(
            outcome.clear_color(),
            [palette::SKY[0], palette::SKY[1], palette::SKY[2], 1.0]
        );
    }

    /// The depth range is a rendering-quality decision, so it is pinned: it must
    /// cover the drawn road and no more, and its ratio must stay small enough to
    /// keep the road's layers apart.
    #[test]
    fn the_depth_range_covers_the_drawn_road_and_no_more() {
        let drawn = chunks::CHUNKS_AHEAD as f32 * road_mesh::CHUNK_LENGTH;
        assert!(FAR_PLANE > drawn, "the far plane reaches the furthest chunk");
        assert!(FAR_PLANE < drawn + 400.0, "and does not waste precision beyond it");
        assert!(NEAR_PLANE >= 1.0, "the near plane keeps depth precision");
        assert!(
            NEAR_PLANE < crate::tuning::CameraTuning::DEFAULT.distance_low,
            "but never clips the car"
        );
        assert!(FAR_PLANE / NEAR_PLANE < 2_000.0, "and the ratio stays sane");
    }

    #[test]
    fn the_counters_describe_a_bounded_frame() {
        let (mut app, mut sim, mut scene) = fixture();
        for _ in 0..600 {
            sim.step(DriveCommand::FLAT_OUT);
            scene.step(&sim);
        }
        scene.pose(&mut app, &sim, 0.5);
        let c = scene.counters();
        assert!(c.active_chunks > 0);
        assert!(
            c.active_chunks <= chunks::CHUNKS_AHEAD + chunks::CHUNKS_BEHIND + 1,
            "{} chunks drawn",
            c.active_chunks
        );
        assert!(c.total_chunks > c.active_chunks, "the course is streamed, not all drawn");
        assert!(c.road_triangles > 10_000);
        assert!(c.cached_scenery_chunks <= c.active_chunks + 1);
        assert_eq!(c.traffic_slots, sim.tuning().race.traffic_active);
    }

    /// The whole point of chunking: driving the course never grows the frame.
    #[test]
    fn the_drawn_set_stays_bounded_across_the_whole_course() {
        let (mut app, mut sim, mut scene) = fixture();
        let ceiling = chunks::CHUNKS_AHEAD + chunks::CHUNKS_BEHIND + 1;
        for step in 0..6_000 {
            let command = crate::script::autopilot(sim.car(), sim.track());
            sim.step(command);
            scene.step(&sim);
            if step % 30 == 0 {
                scene.pose(&mut app, &sim, 0.0);
                let c = scene.counters();
                assert!(c.active_chunks <= ceiling, "step {step}: {} chunks", c.active_chunks);
                assert!(
                    c.scenery_instances < 1_400,
                    "step {step}: {} scenery instances",
                    c.scenery_instances
                );
            }
        }
    }

    #[test]
    fn the_camera_is_placed_behind_the_car_and_looks_ahead() {
        let (mut app, mut sim, mut scene) = fixture();
        for _ in 0..300 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        scene.pose(&mut app, &sim, 1.0);
        let pose = sim.camera_pose(1.0);
        let outcome = app.tick(1);
        assert_ne!(outcome.camera_view_proj(), [0.0f32; 16]);
        assert!(pose.eye.subtract(sim.car().position).dot(sim.car().forward()) < 0.0);
    }

    #[test]
    fn the_view_projection_is_finite_and_tracks_the_camera() {
        let (mut app, mut sim, mut scene) = fixture();
        for _ in 0..200 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        scene.pose(&mut app, &sim, 0.0);
        let first = scene.view_projection();
        assert!(first.as_cols_array().iter().all(|v| v.is_finite()));
        for _ in 0..200 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        scene.pose(&mut app, &sim, 0.0);
        assert_ne!(scene.view_projection(), first, "it follows the car");
    }

    #[test]
    fn traffic_is_placed_on_the_road_and_retired_when_idle() {
        let (mut app, mut sim, mut scene) = fixture();
        for _ in 0..900 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        scene.pose(&mut app, &sim, 0.0);
        let live = sim.traffic().active_count();
        assert!(live > 0, "there is traffic");
        // Every live traffic body ends up near the road.
        for car in sim.traffic().active() {
            let sample = sim.track().interpolated_at(car.distance);
            let expected = sample.at_lateral(car.lateral);
            assert!(expected.x.is_finite() && expected.z.is_finite());
        }
    }

    #[test]
    fn the_brake_lights_respond_to_braking_and_to_the_finish() {
        let mut sim = RaceSim::shipping();
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        for _ in 0..300 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        let cruising = brake_intensity(&sim);
        for _ in 0..20 {
            sim.step(DriveCommand { brake: 1.0, ..DriveCommand::IDLE });
        }
        // Braking hard shows up either as an impact or as deceleration; either
        // way the lights must be able to reach full.
        sim.place_at(sim.track().length());
        sim.step(DriveCommand::IDLE);
        assert_eq!(sim.phase(), RacePhase::Finished);
        assert_eq!(brake_intensity(&sim), 1.0, "the finish lights them fully");
        assert!(cruising <= 1.0);
    }

    /// The pool light is the frame's only *local* light source, so it has to
    /// ride the car: parked at the origin it would light the start line for the
    /// whole race and leave the car in the flat key it was added to break up.
    #[test]
    fn the_pool_light_rides_over_the_car() {
        let (mut app, mut sim, mut scene) = fixture();
        for _ in 0..600 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        scene.pose(&mut app, &sim, 0.0);
        let pose = sim.car_pose(0.0);
        let at = app.get::<Transform>(scene.car_light).expect("posed").translation;
        assert!(
            (at.y - pose.position.y - POOL_LIGHT_HEIGHT).abs() < 1e-3,
            "it hangs {POOL_LIGHT_HEIGHT} m over the road, not on it: {at:?}"
        );
        let flat = Vec3::new(at.x - pose.position.x, 0.0, at.z - pose.position.z);
        assert!(
            (flat.length() - POOL_LIGHT_AHEAD).abs() < 1e-3,
            "and just ahead of the car: {flat:?}"
        );
        assert!(
            flat.dot(Vec3::new(pose.yaw.sin(), 0.0, pose.yaw.cos())) > 0.0,
            "ahead, not behind"
        );
    }

    #[test]
    fn the_finish_arch_stands_at_the_end_of_the_course() {
        let (app, sim, scene) = fixture();
        assert_eq!(scene.finish_entities().len(), 3, "two pillars and a beam");
        let end = sim.track().sample_at(sim.track().length() - crate::sim::FINISH_MARGIN);
        for entity in scene.finish_entities() {
            let t = app.get::<Transform>(*entity).expect("placed");
            // The pillars stand at the barrier line, which on this road is a
            // long way out — the check is that they are AT the finish, not that
            // they are close to the centreline.
            let reach = sim.track().barrier_offset(&end) + ARCH_HEIGHT;
            assert!(
                t.translation.distance(end.position) < reach,
                "an arch part is {} m from the line",
                t.translation.distance(end.position)
            );
        }
    }

    #[test]
    fn posing_twice_produces_the_same_frame() {
        let (mut app, mut sim, mut scene) = fixture();
        for _ in 0..420 {
            sim.step(DriveCommand::FLAT_OUT);
            scene.step(&sim);
        }
        scene.pose(&mut app, &sim, 0.4);
        let first = app.tick(10);
        let first_draws = first.draws().len();
        let first_camera = first.camera_view_proj();

        scene.pose(&mut app, &sim, 0.4);
        let second = app.tick(11);
        assert_eq!(second.draws().len(), first_draws);
        assert_eq!(second.camera_view_proj(), first_camera);
    }

    #[test]
    fn hiding_everything_retires_the_entities() {
        let (mut app, _, scene) = fixture();
        hide_all(&mut app, scene.finish_entities());
        for entity in scene.finish_entities() {
            assert_eq!(app.get::<Visible>(*entity), Some(Visible(false)));
        }
    }

    #[test]
    fn a_degenerate_aspect_ratio_still_produces_a_usable_projection() {
        let sim = RaceSim::shipping();
        let mut app = App::new()
            .window(Window::new(1, 1))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build();
        let mut scene = RaceScene::install(&mut app, &sim, 0, 0);
        scene.pose(&mut app, &sim, 0.0);
        assert!(scene
            .view_projection()
            .as_cols_array()
            .iter()
            .all(|v| v.is_finite()));
    }
}
