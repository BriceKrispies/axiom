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
    FramePostProcess, FrameSky, Mesh, PerspectiveProjection, PointLight, Ratio, RunningApp, Spawn,
    Transform, Vec3, Visible,
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
    /// The agent's car. The same model in a translucent livery, posed from a
    /// simulation this scene never steps — see [`crate::ghost`].
    ghost_car: PlayerCar,
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
        // **Daylight sky-fill.** This is the term that decides what a shadow
        // looks like, and on a sunlit coast road it is not a rounding error: it
        // is the entire open sky, roughly a quarter of the sun's own strength,
        // arriving on every up-facing surface from every direction at once.
        //
        // The previous values — `0.014 / 0.016 / 0.026`, a mean of 0.019 — were
        // authored for a moonlit stage where the only real light was a lamp
        // riding over the car, and they are the single reason the champion's
        // shadows and every sun-facing surface's dark side read as *holes*
        // rather than as shadow. Under a directional key there is exactly one
        // lit direction; everything turned away from it gets the ambient and
        // nothing else. At 0.019 that is black. In the reference, the palm's
        // cast shadow on the tarmac is a *blue-grey*, clearly lifted and clearly
        // cooler than the sunlit road beside it — that colour is this term.
        //
        // Hemisphere, so the two halves say where the fill comes from:
        //
        // * **Sky (up)** is blue by 1.9x red — the scattered dome the reference
        //   is shot under. It is what tints the shadows cool.
        // * **Ground (down)** is warm and slightly weaker — the sun bouncing off
        //   sand and pale tarmac back onto undersides, wheel arches, the car's
        //   sills and the underside of every palm frond.
        //
        // Level: the sky term (mean 0.267) is ~21% of what the key lays on flat
        // road (1.242), so it fills without becoming a second key — see the
        // ceiling `the_sun_out_lights_every_other_term_in_the_frame` pins.
        app.set_ambient(FrameAmbient::new(
            [0.19, 0.25, 0.36],
            [0.24, 0.21, 0.15],
        ));
        // The air. Everything recedes into the sky colour rather than staying
        // fully lit out to the far plane, which is what gives the road, the trees and
        // the skyline their depth instead of a hard cut-out horizon. On a daylight
        // stage this is the *dominant* atmospheric cue rather than a finishing
        // touch: the reference's road, palms and headland all wash toward the pale
        // haze at the vanishing point long before they reach it.
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

        // The sun itself, drawn behind the scene. This is the piece the rig was
        // missing: the course was *lit* by a key with no source in shot, and the
        // eye needs to see the thing that is doing the lighting.
        //
        // Its direction is [`MOON_DIRECTION`], which is also the direction the key
        // light comes from, so the disc and the thing it lights agree. (The name
        // still says moon; the *geometry* constants belong to the light rig and
        // are left for the pass that re-aims the key, but the colour it hands the
        // sky is now `palette::SUN`.) The horizon colour is `palette::SKY` — the
        // exact colour the depth fog below fades into — so the road dissolves into
        // the sky it is standing under instead of into an unrelated grey. The
        // zenith is the deeper, bluer end and the horizon the pale hazy one, which
        // is how a clear day sits: you are looking through the most air at the
        // ground line and the least of it overhead.
        //
        // The disc's colour is authored well above `1.0`. That surplus is not
        // wasted: it is exactly what the bloom above spends, so the sun carries a
        // soft flare rather than being a flat white sticker.
        app.set_sky(
            FrameSky::gradient(palette::SKY_ZENITH, palette::SKY).with_body(
                [MOON_DIRECTION.x, MOON_DIRECTION.y, MOON_DIRECTION.z],
                axiom_kernel::Radians::finite_or_zero(MOON_ANGULAR_RADIUS),
                palette::SUN,
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

        // The grade. See [`GRADE`] for why a daylight frame takes the opposite
        // one from the night frame this scene used to be.
        app.set_postprocess(GRADE);

        let road = RoadChunks::install(app, track, &tuning.course, palette.road);
        let scenery = SceneryField::install(app, &palette, track, track.seed());
        let traffic = TrafficVisuals::install(app, &palette, tuning.race.traffic_active);
        let car = PlayerCar::install(app, &palette.player_livery());
        // Installed unconditionally at startup, even though a race may never
        // show it: the live browser backend sizes its vertex and instance
        // buffers from the mesh set captured here, so nothing may be spawned
        // later (see the module note at the top of this file).
        let ghost_car = PlayerCar::install(app, &palette.ghost);
        let effects = Effects::install(app, &palette, track.seed());
        let finish_arch = install_finish_arch(app, sim);

        let car_light = install_lights(app);

        RaceScene {
            palette,
            road,
            scenery,
            traffic,
            car,
            ghost_car,
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
    pub fn pose(
        &mut self,
        app: &mut RunningApp,
        sim: &RaceSim,
        ghost: Option<&crate::ghost::GhostRun>,
        alpha: f32,
    ) {
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

        // The ghost. It gets no pool light and casts no glow — it is a
        // translucent record of a lap, not a second car in the world.
        match ghost {
            Some(ghost) => {
                let ghost_boost = [0.0, 1.0][usize::from(ghost.boosting())];
                self.ghost_car
                    .pose(app, &ghost.car_pose(alpha), 0.0, ghost_boost);
            }
            None => self.ghost_car.hide(app),
        }

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
/// **The rig is a daylight rig.** The key is the sun: warm, near-white, and
/// strong enough that it — not the lamp over the car, not the ambient — is what
/// every surface in the frame is lit by. See [`KEY_INTENSITY`] for the exposure
/// arithmetic that sizes it against the reference's sunlit tarmac.
///
/// It arrived here from a night authorship in which all three of those terms
/// were the other way round, and the inversion is the whole change. On that rig
/// the key was held at roughly half power on the argument that a directional
/// light is *the same everywhere* — it lights the tarmac under the bumper and
/// the tarmac at the vanishing point to exactly the same value, so every unit of
/// it is a floor under the whole frame, which a night stage cannot afford. True,
/// and the reason the level kept being cut. Outdoors at noon that floor is not a
/// defect, it is the subject: the sun genuinely does land the same value on the
/// near lane and the far one, and what separates them is atmosphere
/// ([`FrameDepthFog`]), not falloff.
///
/// The positional light above the car survives the change, demoted. The backend
/// attenuates a point light by distance (`1/(1 + 0.09d + 0.032d²)`), which on
/// the night rig laid the brightest wash in the frame on the tarmac around the
/// car. At daylight levels that same wash reads as a spotlight following the
/// player and — worse — fills in the ground the car's own cast shadow has to
/// darken, so it drops to [`POOL_LIGHT_INTENSITY`] and becomes what it can
/// honestly be by day: a warm bounce off hot asphalt onto the car's sills.
///
/// What the level and the pool together still could not fix is that **the frame
/// had no shadow in it** — see [`KEY_DIRECTION`], which is the knob that decides
/// whether the shadow the engine already renders lands anywhere the camera can
/// see it.
fn install_lights(app: &mut RunningApp) -> Entity {
    app.add_light(
        DirectionalLight {
            direction: KEY_DIRECTION,
            // **Sunlight.** The cool `(0.72, 0.80, 1.0)` this replaces was
            // moonlight — sunlight reflected off bare rock — and a cool key is
            // the single most night-signalling term a rig has, because the eye
            // reads warm-key-against-cool-fill as *day* and the reverse as
            // *night* before it reads anything else in the frame.
            //
            // The reference is a high coastal sun: near-white, warm only by the
            // slight red-over-blue a short atmospheric path leaves. Pairing it
            // with the blue sky ambient above is what produces the reference's
            // defining split — warm sunlit tarmac against blue-grey shadow.
            color: Color::linear_rgb(
                palette::ratio(1.0),
                palette::ratio(0.955),
                palette::ratio(0.88),
            ),
            // NOT `palette::ratio`, which clamps to `0..=1`. That helper is a
            // sanitizer for *colour channels*, where above-one is meaningless,
            // and putting a sun through it silently pins the whole stage back at
            // the night rig's brightness with no error anywhere. A light's
            // intensity is a gain, not a channel — `palette::MOON` is authored
            // past one for the same reason.
            intensity: Ratio::finite_or_zero(KEY_INTENSITY),
        },
        Transform::IDENTITY,
    );
    // The pool that rides over the car. Under the daylight key it is demoted
    // from *the* light of the stage to a residual: at `1.0` it laid a lamp-lit
    // wash on the tarmac around the car, brighter than the sun on the same
    // surface, which in a daylight frame reads as a spotlight following the car
    // and — worse — fills in the very ground the car's own sun shadow is
    // supposed to darken. `0.16` keeps only what it is still good for: a warm
    // near-field bounce off the tarmac onto the car's sills and arches, gone
    // within a few metres. Warm now, because in daylight the bounce comes off
    // sunlit asphalt and sand, not off a cold moon.
    app.add_point_light(
        PointLight {
            color: Color::linear_rgb(
                palette::ratio(1.0),
                palette::ratio(0.94),
                palette::ratio(0.82),
            ),
            intensity: palette::ratio(POOL_LIGHT_INTENSITY),
        },
        Transform::from_translation(Vec3::new(0.0, POOL_LIGHT_HEIGHT, 0.0)),
    )
}

/// The colour grade laid over the finished frame.
///
/// This was [`FramePostProcess::low_key`] — a pure `0.16` black-point subtract
/// and nothing else, the correct grade for the moonlit stage this scene used to
/// be. A night raster's defect is a lifted *floor*: the hemisphere ambient, the
/// key and the fog each add a constant that cannot be driven to zero without
/// erasing something, so the blacks stall a tenth of the way up the range and the
/// eye reads the result as grey daylight, dimmed. One subtract fixes exactly
/// that and leaves the highlights alone.
///
/// The reference is now **midday**, and against a daylight frame that same
/// subtract is the defect rather than the cure. It is a hard clip, not a curve:
/// every pixel below byte 41 becomes exactly `0`. A sunlit frame's shadow side —
/// the shaded flank of the car, the underside of a palm crown, the dark half of
/// the tarmac — is *full of information* in the reference, and low-key deletes
/// all of it while dragging the sky and the sea down by the same 41 levels.
///
/// So the frame takes the daylight preset instead: exposure held near neutral so
/// the sky reads at the level it is authored at, a slight cool white balance
/// (the shade on a sunny day is lit by the blue sky, not by the sun), a gentle
/// contrast that separates the midtones without clipping the shadows, and a
/// saturation lift for the reference's vivid sea, foliage and paint. The black
/// point goes to zero: with a bright sky and a bright sea there is no lifted
/// floor left to remove, and removing one anyway would only mud the frame.
const GRADE: FramePostProcess = FramePostProcess::cinematic();

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

/// The direction **toward the light body in the sky** (world space,
/// un-normalized).
///
/// The name still says moon; the body is now the **daylight sun**, and the
/// direction survived the change unaltered because it was already right: the
/// reference's sun sits ~20° above the horizon and ~29° off the vanishing point
/// toward the road's right, which is exactly what this vector encodes. Only the
/// key's level and colour moved. The rename — and the disc's own colour, which
/// is `palette::MOON` and still cool — belong with the palette, not here.
///
/// The reasoning below is written for a moon and holds verbatim for a low sun:
/// every argument in it is about elevation, visibility and shadow length.
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

/// The key light's intensity — **the frame's exposure**.
///
/// The reference this course is scored against is a *daylight* frame: an open
/// coast road under a high sun, tarmac reading around byte 65, sand and cloud
/// near white, and every shadow a lifted blue rather than a hole. The rig being
/// scored against it was a moonlit one, and `0.88` was that stage's level. No
/// grade turns one into the other, because the difference is not a curve — it is
/// how much light is arriving.
///
/// The arithmetic that sets `3.6`, taken on the road, the largest surface in any
/// frame and the one every term lands on hardest:
///
/// * The key on flat ground is `intensity · N·L`. At [`MOON_DIRECTION`]'s 20°
///   elevation `N·L` is `0.345`, so the key contributes `1.242`.
/// * The sky ambient adds `0.267`, for globals of `1.509`.
/// * The tarmac's albedo is a deliberate `0.0886` luma, and it needed no change:
///   real asphalt *is* ~0.09 linear, so an albedo authored near-black for a
///   night stage is already the right albedo for a sunlit one. Only the light
///   was missing.
/// * `0.0886 · 1.509 = 0.1337` linear, which the backend's sRGB transfer writes
///   as byte **102**.
/// * [`FramePostProcess::low_key`] then subtracts `0.16` encoded and
///   renormalizes, landing the road at byte **73** — beside the reference's ~65,
///   and for the first time in the same decade as it.
///
/// Note the last step: this level is chosen to read correctly *through* the
/// existing low-key grade rather than by deleting it, because the grade is not
/// this constant's to spend. Retire that black point and the road lands at 102,
/// and this should come back to ~`2.6`.
///
/// **What this replaces, and why every word of it was true and still wrong:**
/// a directional light is by definition the same everywhere — it lights the
/// tarmac under the bumper, the tarmac at the vanishing point and the verge two
/// hundred metres out to exactly the same value. Every unit of it is a floor
/// under the *whole* frame, which is the one thing a night stage cannot afford,
/// and that is what drove this constant down and down. On a daylight stage the
/// sun *is* the frame and that floor is the subject; the near-to-far ramp the
/// old level was protecting belongs to the depth fog, not to a lamp on the car.
///
/// The history below is kept because it is the reasoning a future pass will
/// re-derive if the reference ever goes back to night.
///
/// All of that is true, and the level it produced — `0.30` — was still wrong,
/// because it removed the pedestal a **second** time. The frame's floor is taken
/// out twice: once here, by starving the globals, and once again downstream by
/// [`FramePostProcess::low_key`], which subtracts `0.16` **in display-encoded
/// space** (41/255) off the finished image and renormalizes. That subtract is a
/// hard floor, not a curve: every pixel the raster writes below byte 41 does not
/// get *deeper*, it becomes exactly `0`.
///
/// At `0.30` the globals on flat ground are `0.019 + 0.30·0.345 = 0.122`. The
/// tarmac's own albedo is a deliberate `0.0886` luma, so the road renders at byte
/// **27** and the verge at **35** — both under the subtract. Measured on the
/// champion, that is precisely what happened: off the pool and off the moon's
/// sheen, the whole ground plane reads `0.0`. The verge columns are `0.0–3.1`
/// against the reference's steady `6.5–9.6`, and the mid-field road left of the
/// car is `0.7–3.2` against the reference's `10.0–11.7`. The scene stopped being
/// a road at night and became lane paint floating in a void: no verge, no
/// shoulder, no surface between the car and the horizon.
///
/// So the rule this constant obeys has a second half. The globals must stay under
/// the pool — that is the ramp, and it still holds. But they must also land the
/// **road above the grade's black point**, or the grade clips the ground plane
/// away instead of deepening it. `0.88` puts the globals at `0.322`: the road
/// renders at byte 48 and survives the subtract at ~`7`, next to the reference's
/// `10`, while the pool beneath the car (`0.341`) still out-lights them.
///
/// And the subtract *sharpens* the ramp rather than flattening it, which is why
/// this costs the night nothing: road-beside-the-car goes to byte 69 in the
/// raster and mid-field road to 48 — 1.4x — but after the black point is removed
/// those are `33` and `7`, near 5x. The ramp the previous level was protecting is
/// produced by the grade acting on a raster that has something in it, not by
/// authoring the raster at zero.
///
/// The verticals come back with it, and that is the other half of the win: at
/// `0.30` a car flank facing the key got `0.135`, so the car was a black
/// silhouette with no lit side at all. At `3.6` it gets `1.6`, and the raking,
/// side-lit modelling [`MOON_DIRECTION`]'s low elevation was chosen for finally
/// reaches the geometry — with the sky fill under it, the *unlit* flank becomes
/// a readable cool shadow rather than a cutout.
///
/// **Reconciled by the foreman, and the arithmetic is the lighting lens's own.**
/// Two proposals in this pass moved the same pixels from opposite ends. Lighting
/// sized this key *through* `FramePostProcess::low_key()`, whose `0.16` black
/// point subtracts in display space: it picked `3.6` so the road would land near
/// byte 73 *after* that subtract. The colorist then retired `low_key()` entirely
/// for `cinematic()`, whose black point is zero — correct, because a black-point
/// subtract is the cure for a lifted night floor and a defect on a sunlit frame.
///
/// With the subtract gone, `3.6` is about 40% hot. The lighting proposal wrote
/// the contingency into its own caveat rather than leaving it to be rediscovered:
/// "if the colorist retires that black point in the same pass, my level should
/// come back to ~2.6". That is what this is. Neither lens is overruled — one of
/// them anticipated the other and left the correction behind.
const KEY_INTENSITY: f32 = 2.6;

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

/// How strong the car's pool light is.
///
/// On the night rig this was `1.0` and it was the brightest light in the frame —
/// the stage was lit *locally*, and the pool's falloff was the only thing
/// producing a near-to-far ramp. Under a daylight key that reading inverts: a
/// lamp that out-lights the sun on the tarmac beneath the car is a spotlight
/// following the player, and it fills in exactly the ground the car's own cast
/// shadow has to darken.
///
/// So it is demoted to a residual — the warm near-field bounce off sunlit
/// asphalt onto the car's sills and arches, a twentieth of the sun on the same
/// surface and gone within a few metres. Pinned against the key by
/// `the_sun_out_lights_every_other_term_in_the_frame`.
const POOL_LIGHT_INTENSITY: f32 = 0.16;

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
        scene.pose(&mut app, &sim, None, 0.0);
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

    /// **This is a daylight stage: the sun is the frame.** The three terms that
    /// reach the road — the directional key, the hemisphere sky ambient, and the
    /// pool light riding over the car — have a fixed order of precedence, and
    /// this pins it, because the rig arrived here from a *night* authorship in
    /// which the order was exactly inverted.
    ///
    /// Under that night rig the pool was the brightest thing on the tarmac and
    /// the globals were deliberately starved beneath it, so that only the pool's
    /// falloff produced any near-to-far ramp. Every one of those sentences is
    /// wrong outdoors at noon: a car does not out-light the sun, a lamp that
    /// does reads as a spotlight following the car, and — the reason this is a
    /// test and not a comment — a pool bright enough to beat the key fills in
    /// the very ground the car's own sun shadow is supposed to darken, which
    /// deletes the single most reference-defining feature of the frame.
    ///
    /// *Floor:* they must render the road **above the grade's black point**, for
    /// whatever [`GRADE`] currently is. A black point is subtracted in
    /// display-encoded space off the finished image, and that subtract is a hard
    /// clip rather than a curve: a road rendering below it does not get deeper, it
    /// becomes `0`. Starving the globals past that line deletes the verge, the
    /// shoulder and the mid-field tarmac outright — which is exactly what a
    /// measured champion once did (verge `0.0-3.1` against a reference `6.5-9.6`).
    /// The daylight grade spends nothing on a black point, so the wall sits at zero
    /// today; the rule is pinned against [`GRADE`] and not against a number,
    /// precisely so it survives the next change of grade — including a change back.
    ///
    /// Both walls are worth a test rather than a comment because passes keep
    /// walking into one while defending the other, and this pass walked into the
    /// seam between them: the key was sized through a black point that another
    /// proposal removed in the same round. The comparison is made on a
    /// **horizontal** surface — the road, the largest thing in any frame and the
    /// one every term lands on hardest.
    #[test]
    fn the_sun_out_lights_every_other_term_in_the_frame() {
        // The key on flat ground is `intensity * N·L`, with N = +Y.
        let len = (KEY_DIRECTION.x * KEY_DIRECTION.x
            + KEY_DIRECTION.y * KEY_DIRECTION.y
            + KEY_DIRECTION.z * KEY_DIRECTION.z)
            .sqrt();
        let n_dot_l = -KEY_DIRECTION.y / len;
        let key = KEY_INTENSITY * n_dot_l;

        // A daylight key is a gain past one, and `palette::ratio` clamps to
        // `0..=1`. Routing the intensity through that helper — the obvious thing
        // to do, and what every other value in this file does — pins the sun
        // back at the night rig's brightness and reports nothing. Pinned here
        // because the failure is invisible in the source and only shows up as a
        // frame that mysteriously refuses to get brighter.
        assert!(
            palette::ratio(KEY_INTENSITY).get() < KEY_INTENSITY,
            "the sanitizer no longer clamps the key — if that changed, the \
             comment at the `add_light` call site is stale"
        );

        // The hemisphere ambient's sky term is what an up-facing surface gets.
        let ambient = (0.19 + 0.25 + 0.36) / 3.0;

        // The backend's point-light falloff, mirrored: 1/(1 + 0.09d + 0.032d²),
        // times the pool's own intensity.
        let d = POOL_LIGHT_HEIGHT;
        let pool = POOL_LIGHT_INTENSITY / (1.0 + 0.09 * d + 0.032 * d * d);

        assert!(
            key > pool * 4.0,
            "the pool ({pool:.3}) is competing with the sun ({key:.3}) on the \
             tarmac beneath the car — that is a headlight at noon, and it erases \
             the car's cast shadow"
        );

        // The fill is a fill. It lights the lit face and the unlit face equally,
        // so every unit of it is contrast removed from every object in shot; a
        // sky term that catches the key flattens the frame into overcast.
        assert!(
            ambient < key * 0.30,
            "the sky fill ({ambient:.3}) has become a second key against \
             {key:.3} — the frame is going flat"
        );
        // But it is emphatically not zero. Under one directional key, every
        // surface turned away from the sun receives this and nothing else: at
        // the night rig's 0.019 the reference's blue-grey palm shadow renders as
        // a black hole, which is the defect this floor exists to prevent.
        assert!(
            ambient > 0.15,
            "the sky fill ({ambient:.3}) is back to a night residual — every \
             shadow in the frame is a hole again"
        );

        // The tarmac's luma albedo, and the sRGB transfer the backend writes it
        // through — the road as the display receives it, before grading.
        //
        // The band is the reference's own sunlit tarmac (~byte 65) with the
        // low-key grade's `0.16` subtract added back, since that stage still
        // sits downstream of this one: byte 87..=128 pre-grade.
        let road = 0.2126 * 0.085 + 0.7152 * 0.088 + 0.0722 * 0.105;
        let linear = road * (key + ambient);
        let encoded = 1.055 * linear.powf(1.0 / 2.4) - 0.055;
        let black_point = GRADE.black_point().get();
        assert!(
            encoded > black_point,
            "the globals put the road at {encoded:.3} encoded, under the grade's \
             black point of {black_point:.3} — the subtract clips the whole \
             ground plane to zero instead of deepening it"
        );
        assert!(
            (0.34..=0.50).contains(&encoded),
            "the road renders at {encoded:.3} encoded, outside the band that \
             lands it beside the reference's sunlit tarmac once the grade's \
             black point is taken off"
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
        scene.pose(&mut app, &sim, None, 0.5);
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
                scene.pose(&mut app, &sim, None, 0.0);
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
        scene.pose(&mut app, &sim, None, 1.0);
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
        scene.pose(&mut app, &sim, None, 0.0);
        let first = scene.view_projection();
        assert!(first.as_cols_array().iter().all(|v| v.is_finite()));
        for _ in 0..200 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        scene.pose(&mut app, &sim, None, 0.0);
        assert_ne!(scene.view_projection(), first, "it follows the car");
    }

    #[test]
    fn traffic_is_placed_on_the_road_and_retired_when_idle() {
        let (mut app, mut sim, mut scene) = fixture();
        for _ in 0..900 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        scene.pose(&mut app, &sim, None, 0.0);
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
        scene.pose(&mut app, &sim, None, 0.0);
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
        scene.pose(&mut app, &sim, None, 0.4);
        let first = app.tick(10);
        let first_draws = first.draws().len();
        let first_camera = first.camera_view_proj();

        scene.pose(&mut app, &sim, None, 0.4);
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
        scene.pose(&mut app, &sim, None, 0.0);
        assert!(scene
            .view_projection()
            .as_cols_array()
            .iter()
            .all(|v| v.is_finite()));
    }
}
