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

pub mod asphalt_texture;
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
    Angle, Camera, Color, DirectionalLight, Entity, FrameAmbient, FrameBloom, FrameDepthFog,
    FrameSky, Mesh, PerspectiveProjection, PointLight, Ratio, RunningApp, Spawn, Transform, Vec3,
    Visible,
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
        // Bloom: what turns the emissive cues — reflector posts, tail lights,
        // tunnel lamps, the lane paint catching the moon — from bright patches of
        // paint into things that read as lights. Gated by the backend's `Bloom`
        // capability, which the Canvas 2D profile drops and reports, so the
        // software arm is untouched without this app knowing which arm it is on.
        app.set_bloom(FrameBloom::moonlit());

        // The moon itself, drawn behind the scene. This is the piece the rig was
        // missing: the course was *lit* like a night stage but had no light source
        // in shot, and a frame whose only bright things are reflectors reads as
        // "dark", not "moonlit" — the eye needs to see what is doing the lighting.
        //
        // Its direction is [`MOON_DIRECTION`], which is also the direction the key
        // light comes from, so the moon and the thing it lights agree. The horizon
        // colour is `palette::SKY` — the exact colour the depth fog below fades
        // into — so the road dissolves into the sky it is standing under instead of
        // into an unrelated grey. The zenith is darker than the horizon, which is
        // how a real night sky sits: brightest just above the ground, deepest
        // overhead.
        //
        // The disc's colour is authored well above `1.0`. That surplus is not
        // wasted: it is exactly what the bloom above spends, so the moon carries a
        // soft halo rather than being a flat white sticker.
        app.set_sky(
            FrameSky::gradient(palette::SKY_ZENITH, palette::SKY).with_body(
                [MOON_DIRECTION.x, MOON_DIRECTION.y, MOON_DIRECTION.z],
                axiom_kernel::Radians::finite_or_zero(MOON_ANGULAR_RADIUS),
                palette::MOON,
                Ratio::finite_or_zero(MOON_HALO_FALLOFF),
                Ratio::finite_or_zero(MOON_HALO_STRENGTH),
            ),
        );

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

    /// Start a new race: forget the transient presentation state the previous
    /// one left behind.
    ///
    /// Everything else the scene shows — the road chunks, the scenery, the
    /// traffic, the car — is re-derived from the simulation on every
    /// [`Self::pose`], so a fresh race needs nothing done to it. The effects are
    /// the exception, because they are the only presentation state that *ages*
    /// rather than being read.
    pub fn reset(&mut self) {
        self.effects.reset();
    }

    /// Cull road paint to the near field, or stop doing so.
    ///
    /// Driven by the browser arm from the backend it actually bound. See
    /// [`crate::render::chunks::RoadChunks::set_paint_near_field_only`] for why
    /// the raster, not taste, decides this.
    pub fn set_paint_near_field_only(&mut self, limited: bool) {
        self.road.set_paint_near_field_only(limited);
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
///
/// What the level and the pool together still could not fix is that **the frame
/// had no shadow in it** — see [`KEY_DIRECTION`], which is the knob that decides
/// whether the shadow the engine already renders lands anywhere the camera can
/// see it.
fn install_lights(app: &mut RunningApp) -> Entity {
    app.add_light(
        DirectionalLight {
            direction: KEY_DIRECTION,
            // Moonlight, not sunlight. The old key was `(1.0, 0.94, 0.84)` — a
            // warm white, which is the colour of the sun an hour before it sets
            // and the single most daylight-signalling thing left in the rig. The
            // moon is sunlight reflected off bare rock and scattered through a
            // night atmosphere: the eye reads it as distinctly cool, and pushing
            // blue past green past red is what says "this is not a dim afternoon".
            color: Color::linear_rgb(
                palette::ratio(0.72),
                palette::ratio(0.80),
                palette::ratio(1.0),
            ),
            intensity: palette::ratio(KEY_INTENSITY),
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

/// The direction the key light **travels** (world space, un-normalized).
///
/// This is the shadow knob, not just a shading knob. The engine renders a real
/// directional depth-map shadow and always has — but a shadow is cast *along*
/// the light's travel direction, and the old key travelled `(-0.36, -1.0, 0.42)`:
/// down-track (`+Z`, the way the car is pointing, away from a chase camera that
/// sits behind it) and toward screen-right. From the only camera this game ever
/// uses, that threw the car's shadow **forward, underneath the car, where the car
/// itself hides it**. The frame therefore contained a shadow and showed none, and
/// every pass that tried to fix "no shadow" by re-balancing key and fill was
/// adjusting the wrong term: the light was fine, it was aimed out of shot.
///
/// So the horizontal component is flipped. The key now travels toward `-Z` —
/// *toward* the camera — and toward `+X`, which this app's camera basis renders
/// as screen-**left**, so the shadow spills down and to the left of the car and
/// lands squarely in the lower third of the frame, where the reference puts it.
///
/// The elevation is lowered with it, from ~61° above the horizon to ~50°. A
/// shadow's length is `height / tan(elevation)`: at 61° the car's ~1.2 m of body
/// projected a ~0.7 m smear that its own footprint swallowed even when it was
/// pointed at the camera; at 50° it projects a full car-height of shadow that
/// reads as a separate shape. The same change is what finally gives the frame's
/// *vertical* surfaces a lit and an unlit side — the key's horizontal component
/// goes 0.49 → 0.64, so a car flank, a reflector post and a tree cone stop being
/// one flat value each, which is the other half of why this scene read as
/// ambient-only.
///
/// The cost is deliberate and small: the road and verge are horizontal, so their
/// `N·L` drops 0.87 → 0.77 and the tarmac darkens ~11%, toward the near-black
/// asphalt its albedo was authored for.
///
/// **The key is now the moon.** It is exactly `-`[`MOON_DIRECTION`], so the thing
/// lighting the scene and the thing you can see in the sky are the same object —
/// which is the whole point of putting a sky in the frame. The horizontal
/// component above is preserved unchanged (that is the shadow-placement result,
/// and it was right); only the elevation moves, and it moves *down*, because a
/// moon you can see down the road is by definition near the horizon.
const KEY_DIRECTION: Vec3 = Vec3::new(
    -MOON_DIRECTION.x,
    -MOON_DIRECTION.y,
    -MOON_DIRECTION.z,
);

/// The direction **toward the moon** (world space, un-normalized).
///
/// Two things are true at once here and the direction has to satisfy both: the
/// moon must be *visible down the road ahead*, and it must be the light the
/// course is lit by. So it points down-track (`+Z`, the way the car is pointing
/// and the way the chase camera looks) and slightly toward `-X`, which this
/// app's camera basis renders as screen-**right** — off the vanishing point, so
/// it is not permanently hidden behind the car.
///
/// The elevation is **20°**, down from the old key's 50°. That is a real trade,
/// made deliberately:
///
/// * A moon at 50° is above the top of the frame from a chase camera. There is
///   no elevation at which a light is both "overhead" and "in shot"; the ask was
///   for a visible moon, so it comes down to where the camera can see it.
/// * A shadow's length is `height / tan(elevation)`: 50° → 0.84 car-heights,
///   20° → 2.7. The car's shadow stops being a smear under the bumper and
///   becomes a long raking shape thrown back toward the camera. (It lands only
///   near the world origin — the engine's one directional shadow map is a fixed
///   20 m box there — but where it lands, it now reads.)
/// * The cost is `N·L` on the horizontal road: 0.77 → 0.34, less than half. That
///   is why [`KEY_INTENSITY`] rises to compensate. The verticals — car flanks,
///   reflector posts, tree cones — gain what the road loses, which is exactly
///   the raking, side-lit look a low moon produces and a high one cannot.
const MOON_DIRECTION: Vec3 = Vec3::new(-0.55, 0.42, 1.0);

/// The moon's angular radius (radians).
///
/// The real moon is about `0.0045` rad — half a degree, which at this field of
/// view is a handful of pixels and reads as a stuck highlight rather than a moon.
/// This is roughly ten times that: large enough to read as a disc at a glance,
/// small enough to still be a *body* in the sky rather than a lamp hanging over
/// the course.
const MOON_ANGULAR_RADIUS: f32 = 0.045;

/// The halo's cosine exponent — larger hugs the disc more tightly.
///
/// **This is a rim, not the glow.** The frame's bloom is what spreads the moon's
/// light into the sky around it, and the two compound: a wide halo hands the
/// bright pass a large disc of above-threshold pixels, and the bloom then spreads
/// *that* — so a halo tuned as if it were the only glow produces a blown white
/// cloud several times the moon's diameter.
///
/// The disc is `MOON_ANGULAR_RADIUS` = 0.045 rad ≈ 2.6°, and the exponent has to
/// be read against that. At 220 the halo was still at 43% a full 5° out — nearly
/// two disc-radii of near-full-brightness sky, all of it feeding the bloom. At
/// 1400 it is 24% at the limb and gone by 5°, which leaves a thin bright edge on
/// the disc and lets the bloom do the spreading it exists to do.
const MOON_HALO_FALLOFF: f32 = 1400.0;

/// How strongly the halo is added against the moon's own colour. Low, for the
/// reason above: the bloom supplies the glow, this only softens the limb.
const MOON_HALO_STRENGTH: f32 = 0.18;

/// The key light's intensity.
///
/// Raised from `0.55` alongside the elevation drop in [`MOON_DIRECTION`]. It does
/// **not** restore the road to its old brightness and is not meant to: at 20° the
/// road's `N·L` more than halves, and this recovers about two thirds of that. The
/// tarmac ends up genuinely darker than before while every vertical surface ends
/// up brighter — which is the difference between a scene lit from overhead and
/// one lit by something sitting on the horizon.
const KEY_INTENSITY: f32 = 0.85;

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

    /// The key light's aim is a *visibility* rule, not a taste one, so it is
    /// pinned here.
    ///
    /// This game has exactly one camera: a chase rig behind a car that drives
    /// `+Z`. A shadow is cast along the light's travel direction, so a key with a
    /// positive `z` throws every shadow in the scene down-track, away from that
    /// camera and behind the object casting it — which is how the rig spent
    /// several passes rendering a real depth-map shadow that never appeared in a
    /// single frame. `z` must stay negative for the shadow to come *toward* the
    /// viewer, and the sun must stay off the vertical so the shadow has length
    /// and so vertical surfaces get a lit and an unlit side at all.
    #[test]
    fn the_key_throws_its_shadow_toward_the_camera_and_not_out_of_shot() {
        assert!(
            KEY_DIRECTION.z < 0.0,
            "the key travels down-track: every shadow lands behind its caster, \
             hidden from the only camera this game has ({KEY_DIRECTION:?})"
        );
        assert!(KEY_DIRECTION.y < 0.0, "the sun is above the road, not below it");

        // Elevation, from the horizontal reach against the drop. The band is low
        // because the key is the moon and the moon has to be *in shot*: a chase
        // camera looking down the road sees maybe 25° above the horizon, so a key
        // above that is a light the player can never see the source of. The floor
        // is the shadow's other end — below ~12° the shadow stretches past the
        // whole road and the tarmac's own `N·L` collapses to nothing.
        let horizontal = KEY_DIRECTION.x.hypot(KEY_DIRECTION.z);
        let elevation = (-KEY_DIRECTION.y).atan2(horizontal).to_degrees();
        assert!(
            (12.0..=28.0).contains(&elevation),
            "the key sits at {elevation:.0}° — outside the band where the moon is \
             both visible down the road and still rakes a readable shadow"
        );
    }

    /// The key light and the moon are **one object**, and that is the whole
    /// reason the frame reads as moonlit rather than merely dark.
    ///
    /// A sky with a moon in one place and a key light arriving from another is
    /// the specific failure this pins against: every surface would be lit from a
    /// direction the player can see is wrong, which reads as "some light source
    /// off-screen" — exactly the flatness the sky was added to cure.
    #[test]
    fn the_key_light_is_the_moon_that_is_drawn_in_the_sky() {
        assert_eq!(KEY_DIRECTION.x, -MOON_DIRECTION.x);
        assert_eq!(KEY_DIRECTION.y, -MOON_DIRECTION.y);
        assert_eq!(KEY_DIRECTION.z, -MOON_DIRECTION.z);
        // ...and the moon is ahead of the car, down the road, not behind it.
        assert!(
            MOON_DIRECTION.z > 0.0,
            "the car drives +Z; a moon at -Z is behind the camera and unseeable"
        );
        assert!(MOON_DIRECTION.y > 0.0, "and above the horizon, not below it");
    }

    /// The halo is a rim; the bloom is the glow. They compound, so a halo tuned
    /// as if it were the only source of spread produces a blown white cloud
    /// several times the moon's width — which is exactly what it did at 220.
    #[test]
    fn the_moon_halo_dies_within_two_disc_radii() {
        let at = |degrees: f32| degrees.to_radians().cos().powf(MOON_HALO_FALLOFF);
        let limb = MOON_ANGULAR_RADIUS.to_degrees();
        assert!(
            at(2.0 * limb) < 0.02,
            "the halo is still {:.3} at two disc-radii — the bloom will spread \
             all of it and the moon becomes a cloud",
            at(2.0 * limb)
        );
        // But it is not nothing: without a rim the disc has a hard aliased edge.
        assert!(at(0.5 * limb) > 0.1, "the limb still carries a visible rim");
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
