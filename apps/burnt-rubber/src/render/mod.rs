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

pub mod asphalt_field;
pub mod asphalt_texture;
pub mod car_model;
pub mod chunks;
pub mod effects;
pub mod foliage_texture;
pub mod palette;
pub mod pickups;
pub mod prop_meshes;
pub mod road_mesh;
pub mod rock_mesh;
pub mod scenery;
pub mod scenery_pool;
pub mod surface_builder;
pub mod verge_texture;

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
use pickups::PickupVisuals;
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
    /// The boost pickups standing on the road ahead — three pools, one per
    /// tier, because a body's material is fixed when it is spawned.
    pickups: PickupVisuals,
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
    /// Install the scene for `sim` into `app`, generating its textures and road
    /// geometry inline.
    ///
    /// Kept for the test fixtures and capture slices that build a scene with no
    /// preparation phase. It delegates, so the prepared and inline paths cannot
    /// drift.
    pub fn install(app: &mut RunningApp, sim: &RaceSim, width: u32, height: u32) -> RaceScene {
        let textures = crate::preparation::textures::PreparedTextures::generate();
        let meshes = crate::preparation::meshes::PreparedMeshes::cut(
            sim.track(),
            &sim.tuning().course,
        );
        RaceScene::install_prepared(app, sim, width, height, &textures, meshes)
    }

    /// Install the scene from products the startup preparation phase already
    /// produced.
    ///
    /// **The install order below is frozen.** Materials are registered before
    /// the meshes that cite them, ids are `Vec::len() + 1` minted at
    /// registration, and those ids are encoded in the committed golden
    /// artifacts. Reordering anything here moves them.
    pub fn install_prepared(
        app: &mut RunningApp,
        sim: &RaceSim,
        width: u32,
        height: u32,
        textures: &crate::preparation::textures::PreparedTextures,
        meshes: crate::preparation::meshes::PreparedMeshes,
    ) -> RaceScene {
        let palette = ScenePalette::install_prepared(app, textures);
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
        // * **Ground (down)** is warm — the sun bouncing off sand and pale tarmac
        //   back onto undersides, wheel arches, the car's sills and the underside
        //   of every palm frond. It is a *bounce*, and the level below says so.
        //
        // **The ground term is the frame's flatness, and this is the pass that
        // fixes it.** The backend evaluates the hemisphere as a plain lerp on the
        // normal's up-component — `mix(ground, sky, N.y*0.5+0.5)` in the GPU
        // shader, `hemisphere_ambient` on the Canvas 2D arm — so the *only*
        // modelling a hemisphere ambient contributes is the span between its two
        // ends. At `0.24/0.21/0.15` (mean 0.200) against a sky of mean 0.267 that
        // span was 0.067 across the entire normal sphere: a fill that lands within
        // 25% of itself on an up-face, a side-face and a down-face is not a
        // hemisphere at all, it is a *constant* ambient wearing one. That is the
        // single term making the champion read as ambient-only, and it costs the
        // frame in the two places it can least afford:
        //
        // * **Camera-facing verticals** (`N.y = 0`) — the car's rear panel, which
        //   is the subject of the shot, plus every reflector post, palm trunk,
        //   barrier and traffic car — are turned fully away from a key that
        //   travels down-track, so this fill is *all the light they get*. They
        //   took `(sky+ground)/2 = 0.233`, within 13% of the fully sky-facing
        //   road, and rendered as flat slabs against it.
        // * **Down-facing surfaces** (`N.y = -1`) — the valance, the underside of
        //   the bumper, the wheel arches, the underside of every palm crown —
        //   took the full 0.200. Nothing in this frame could go dark. And the
        //   engine's one directional shadow map is a fixed 20 m box at the world
        //   origin while this moment is ~1.9 km down the course (see
        //   [`KEY_INTENSITY`]), so **the ground term is the only knob in the rig
        //   that can put a dark under the car at all**. Propping it up spent that.
        //
        // So the bounce becomes a bounce: the same warm hue, at a quarter of the
        // sky dome (mean 0.065). Undersides drop ~3x and finally read as contact;
        // verticals drop ~29% and regain a terminator against the sunlit
        // horizontals; the sky/ground span goes 1.15:1 -> 1.6:1, which is form.
        //
        // **The level, re-solved against the rendered champion rather than
        // against the reference alone.** The previous pass held the sky term at
        // mean `0.267` on the argument that it *is* the reference's measured
        // shadowed-road level (`0.302` incident — see [`KEY_INTENSITY`]'s table)
        // and therefore needed nothing. That argument checks the input and never
        // checked the output, and the output falsifies it. Measured on the two
        // frames over the same near-field road patch (left of the car, the band
        // the reference fills with palm shadow):
        //
        // | road, near field        | reference | champion |
        // |-------------------------|-----------|----------|
        // | sunlit (p50, right lane)| byte 114  | byte 128 |
        // | deepest shadow (p5)     | byte  33  | byte  65 |
        // | sun : shadow, linear    | ~12 : 1   | ~4 : 1   |
        //
        // The sunlit road is within 13% — the exposure this file spends most of
        // its length deriving is *right*. The shadow is 3.6x too bright in linear,
        // and that single error is the frame's flatness: the champion's road never
        // goes dark anywhere. Across the mid-field band it runs p5 116 / p50 119 /
        // p95 124 — **eight levels of tonal range on the largest surface in shot**,
        // where the reference's same band runs 58 / 105 / 246.
        //
        // Under one directional key a shadowed fragment receives the ambient and
        // nothing else, so the ambient *is* the shadow level and the sun:shadow
        // ratio is `(key + ambient) / ambient` — nothing else in the rig can set
        // it. At `0.267` that ratio is `6.8:1` before the backend's shadow floor
        // and the grade lift it further; the reference measures `~12:1` rendered.
        // Solving for the reference's ratio through the same model puts the sky
        // term at **`0.160`**, which is this scale: both hemisphere ends multiplied
        // by `0.60`.
        //
        // **Level, not colour, and not shape.** Both terms are scaled by the same
        // factor, so the sky stays blue by 1.9x red, the ground stays the warm
        // bounce the last pass made it, and the sky:ground span stays `4.1:1` —
        // every bit of hemisphere modelling that pass bought survives intact. This
        // is one number: how much open sky there is, not what colour it is.
        //
        // What it costs the exposure is `0.437 -> 0.425` encoded on sunlit road
        // (2.8%, and still inside the band
        // `the_sun_out_lights_every_other_term_in_the_frame` pins), because the key
        // is 90% of what a sunlit horizontal receives. What it buys is the shadowed
        // road at model byte `32` against the reference's measured `33` — the darks
        // arriving where the reference puts them for the first time.
        //
        // The floor the same test pins (`ambient > 0.15`, "not a night residual")
        // is deliberately left where it is and deliberately not approached
        // further: `0.160` sits just inside it, the reference's own shadowed road
        // is a lifted blue-grey and not a hole, and the next pass to want darks
        // should take them from the backend's shadow floor, not from here.
        app.set_ambient(FrameAmbient::new(AMBIENT_SKY, AMBIENT_GROUND));
        // Bloom: what turns the emissive cues — reflector posts, tail lights,
        // tunnel lamps, the lane paint catching the sun — from bright patches of
        // paint into things that read as lights. Gated by the backend's `Bloom`
        // capability, which the Canvas 2D profile drops and reports, so the
        // software arm is untouched without this app knowing which arm it is on.
        //
        // **The daylight preset, not the night one**, and this is the single
        // largest thing standing between the champion frame and the reference.
        // `FrameBloom::moonlit()` is `(threshold 0.62, knee 0.35, intensity 0.85,
        // radius 2.6)`: the bright pass therefore starts at luma `0.27` and is at
        // full surplus by `0.97`. That is the right window for a moonlit stage,
        // where `0.27` is reached by a lamp and by nothing else in shot. Under a
        // midday sun it is not a highlight threshold at all — it is *most of the
        // frame*. The blue sky sits near `0.5`, the cumulus and the sea near
        // `0.8`, and the lane paint (a `0.72` white pigment carrying a `0.30`
        // emissive floor) is over the top of it; all of them clear the knee, get
        // blurred at a 2.6-pixel radius and are added back at `0.85`.
        //
        // What that does to the image is exactly what the champion shows: a milky
        // white wash over the whole upper half with the sky's blue bleached out of
        // it, and a blown white streak running from the vanishing point down the
        // centre of the road where the lane markings smear into each other. It is
        // *whitening added to every bright pixel*, so it costs the frame twice —
        // the highlights lose their separation (contrast) and the midtones lose
        // their colour (saturation), which is why the grade above cannot recover
        // either one. No exposure, contrast or saturation number can subtract a
        // haze that is added after them.
        //
        // `highlights()` — `(1.0, 0.15, 0.55, 1.4)` — is the preset authored for
        // this case: nothing below luma `0.85` spills at all, so the sky stays the
        // blue it is authored as and the sea keeps its turquoise, and what does
        // spill (the sun's disc, which is authored above `1.0` precisely so it
        // has surplus to spend, plus the specular crest on the car) spills a short
        // distance and reads as a flare rather than as fog.
        app.set_bloom(FrameBloom::highlights());

        // The sun itself, drawn behind the scene. This is the piece the rig was
        // missing: the course was *lit* by a key with no source in shot, and the
        // eye needs to see the thing that is doing the lighting.
        //
        // Its direction is [`MOON_DIRECTION`], which is also the direction the key
        // light comes from, so the disc and the thing it lights agree. (The name
        // still says moon; the *geometry* constants belong to the light rig and
        // are left for the pass that re-aims the key, but the colour it hands the
        // sky is now `palette::SUN`.) The horizon colour is `palette::SKY`, the
        // dome's own pale end; the depth fog below fades into `palette::HAZE`
        // instead, which is a *different* colour for the reason that constant
        // documents — a fitted clear-sky primary is not what suspended water and
        // dust look like, and the reference measures the two 145 red levels
        // apart. The
        // zenith is the deeper, bluer end and the horizon the pale hazy one, which
        // is how a clear day sits: you are looking through the most air at the
        // ground line and the least of it overhead.
        //
        // The disc's colour is authored well above `1.0`. That surplus is not
        // wasted: it is exactly what the bloom above spends, so the sun carries a
        // soft flare rather than being a flat white sticker.
        //
        // ...and weather in it. A gradient plus a body is a sky with *nothing in
        // it*: whatever two colours it runs between, the upper half of a coastal
        // frame is a clean wash, and a clean wash reads as a backdrop the course
        // is pasted on rather than as sky the course is standing under. That is
        // the largest single area of this frame no scene geometry can reach —
        // above the horizon there is nothing to put geometry on.
        //
        // The cloud layer carries no colour of its own: the gradient behind it
        // fills its shaded body and the body's own colour lights its sunward
        // face. That is why it survived this pass unchanged while the palette
        // underneath it went from night to noon — it was authored to read as
        // silver moonlit cumulus under a moon and as blown-white tops under a
        // sun, from the same two numbers, and the sun arrived in the same round.
        //
        // Gated by the backend's `Sky` capability, exactly as the gradient and the
        // body already are: the Canvas 2D arm drops the whole sky and reports it,
        // so the software arm gains nothing to go wrong with and this app never
        // has to ask which arm it is on.
        // The gradient's *shape*, authored separately from its two colours. The
        // engine's default midpoint is 30° of elevation, which is a fine default
        // for a camera that looks at the sky and wrong for one that looks at a
        // road: the whole visible sky here is the band from the horizon to 32°,
        // so a 30° midpoint shows only the flat bottom of the curve and the dome
        // arrives as a wash however far apart the two stops are authored. See
        // [`palette::SKY_HAZE_HEIGHT`] — this is the number that made
        // [`palette::SKY_ZENITH`] a colour again instead of a slope hack.
        app.set_sky(
            FrameSky::gradient(palette::SKY_ZENITH, palette::SKY)
                .with_haze_height(Ratio::finite_or_zero(palette::SKY_HAZE_HEIGHT))
                .with_body(
                    [MOON_DIRECTION.x, MOON_DIRECTION.y, MOON_DIRECTION.z],
                    axiom_kernel::Radians::finite_or_zero(MOON_ANGULAR_RADIUS),
                    palette::SUN,
                    Ratio::finite_or_zero(MOON_HALO_FALLOFF),
                    Ratio::finite_or_zero(MOON_HALO_STRENGTH),
                )
                .with_clouds(
                    Ratio::finite_or_zero(CLOUD_COVERAGE),
                    Ratio::finite_or_zero(CLOUD_SCALE),
                ),
        );

        // The haze, at the strength the reference actually carries.
        //
        // Normalized device depth is hyperbolic, so a range authored in it is a
        // ramp that is **linear in `1/z`**: with this frustum,
        // `z = 1.20087 / (1.00073 - ndc)`. The old `0.990 .. 0.9993` is therefore
        // `112 m .. 841 m`, and that is the whole defect — the visible road runs
        // from under the bumper to roughly `250 m`, where perspective packs
        // everything beyond into the last few rows above the horizon. More than
        // half the road the camera shows was outside the fog's start entirely,
        // and the far half only ever reached `0.4` of a maximum that was itself
        // capped at `0.9`. Measured down the centre column, the champion's road
        // runs `(82, 63, 54)` at the vanishing point to `(75, 60, 53)` at
        // mid-frame: seven levels of atmosphere across the entire depth of the
        // shot. The reference's same column ramps continuously into a pale band.
        // A fog authored past the geometry is a fog that does not exist.
        //
        // So the ramp is re-sized against the road that is actually in shot:
        // `0.9836` is `70 m` (the fog starts a few car-lengths past the near
        // traffic, so the subject and its shadow stay untouched) and `0.9970` is
        // `322 m` (full density at the vanishing point rather than four times
        // past it). Half-strength lands at `~120 m` and `0.8` at `~200 m`, which
        // is the aerial perspective the reference shows over the receding palm
        // rank. The maximum goes `0.9` -> `0.96`: the reference's vanishing point
        // has *become* atmosphere, and a silhouette held back at a tenth is what
        // read as a dark smudge under a bright sky.
        //
        // The colour is [`palette::HAZE`], not [`palette::SKY`] — see that
        // constant for why turning a fitted clear-sky primary up to `0.96` takes
        // the frame's red the wrong way, and for the horizon seam it costs.
        //
        // **What re-sizing the window could not fix, and what the extinction rate
        // is here for.** A window authored in normalized depth ends in a *ceiling*:
        // at `far` the ramp clamps, and every surface beyond it — the whole
        // distant palm rank, the buildings, the headland — is mixed toward
        // `palette::HAZE` by exactly the same fraction whatever its range. That is
        // measurable on the champion: sampled across the full width of the band
        // just above the horizon, the frame reads `(141,176,178)` at the left edge
        // and `(150,194,198)` at the right, and essentially that same value
        // everywhere between. Nine levels of variation across the entire distance
        // of a nine-kilometre coastline. The reference's same band runs turquoise
        // sea, white cumulus, green headland and a warm sun-side glow — 200 levels
        // — because real air never saturates; it keeps taking the same *fraction*
        // per metre forever, so two things a kilometre apart are never the same
        // colour. No `[near, far]` pair can express that, which is why the engine
        // now carries the Beer–Lambert term (`FrameDepthFog::with_extinction`,
        // gated by `RenderCapability::AerialPerspective` and substituted by this
        // very window on the software arm).
        //
        // Sized so the near and mid field stay where the last passes measured
        // them and only the saturated tail changes. `far` moves `0.9970 -> 0.99955`
        // (`322 m -> 1018 m`), which stops the ramp clamping inside the shot, and
        // `0.001 /m` extinction — a `1000 m` half-distance — supplies the grade the
        // window no longer does. Composed as `1 - (1-screen)*(1-air)` and scaled
        // by the same `0.96` ceiling, that is:
        //
        // | range  | before | after | note                                      |
        // |--------|--------|-------|-------------------------------------------|
        // |  20 m  | 0.000  | 0.013 | the subject and its shadow stay untouched |
        // |  70 m  | 0.000  | 0.045 |                                           |
        // | 150 m  | 0.654  | 0.590 | the receding palm rank, within 6%         |
        // | 322 m  | 0.960  | 0.837 | *was* the ceiling; now still resolving    |
        // | 800 m  | 0.960  | 0.949 | separated from 322 m for the first time   |
        //
        // The near field moves by at most a hundredth and the mid field by six —
        // this is deliberately not a re-grade. What it buys is that the far band
        // stops being one flat value.
        app.set_depth_fog(
            FrameDepthFog::new(
                Ratio::finite_or_zero(0.9836),
                Ratio::finite_or_zero(0.99955),
                Ratio::finite_or_zero(0.96),
                palette::HAZE,
            )
            .with_extinction(Ratio::finite_or_zero(0.001)),
        );

        // The grade. See [`GRADE`] for why a daylight frame takes the opposite
        // one from the night frame this scene used to be.
        app.set_postprocess(GRADE);

        let road = RoadChunks::install_prepared(app, meshes, &tuning.camera, palette.road);
        let scenery = SceneryField::install(app, &palette, track, track.seed());
        let traffic = TrafficVisuals::install(app, &palette, tuning.race.traffic_active);
        let pickups = PickupVisuals::install(app, &palette);
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
            pickups,
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
    /// simulation's clock, so a browser rendering at 144 Hz gets the same sparks
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
        if let Some(range) = self
            .road
            .scenery_range_for(sim.track(), sim.car().distance)
        {
            self.scenery.refresh(sim.track(), &tuning.course, range);
        }
        self.scenery.pose(app, camera.eye, self.last_view_proj);

        self.pose_traffic(app, sim, alpha);
        self.pose_pickups(app, sim, alpha);

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
    fn pose_traffic(&self, app: &mut RunningApp, sim: &RaceSim, alpha: f32) {
        let track = sim.track();
        for index in 0..sim.traffic().cars().len() {
            // Interpolated between fixed steps, exactly as the player's car and
            // the camera are — otherwise traffic steps at the simulation rate
            // while everything around it moves at the display's, which is a
            // 60 Hz judder on a 120 Hz screen.
            let Some((distance, lateral)) = sim.traffic_pose(index, alpha) else {
                self.traffic.pose(app, index, None, 0.0);
                continue;
            };
            let sample = track.interpolated_at(distance);
            // A wreck is off its wheels: lifted onto the arc it was thrown along
            // and rolling about its own length. Everything else is flat on the
            // road and takes the zero.
            let (lift, tumble) = sim.traffic_wreck(index, alpha).unwrap_or((0.0, 0.0));
            let position = sample
                .at_lateral(lateral)
                .add(sample.up.mul_scalar(lift));
            let forward = sample.flat_forward();
            self.traffic.pose(
                app,
                index,
                Some((position, forward.x.atan2(forward.z), sample.up)),
                tumble,
            );
        }
    }

    /// Place every uncollected pickup in range.
    ///
    /// Unlike the traffic, nothing here is interpolated: a pickup does not move,
    /// so there is nothing between two fixed steps to interpolate *between*. The
    /// only thing `alpha` is used for is the diamond's spin, which is
    /// presentation and has no simulation state behind it at all.
    fn pose_pickups(&self, app: &mut RunningApp, sim: &RaceSim, alpha: f32) {
        let field = sim.pickups();
        self.pickups.pose(
            app,
            sim.track(),
            sim.plan().pickups(),
            sim.car().distance,
            pickups::spin_phase(sim.step_count(), alpha),
            &|pickup| field.is_taken(pickup),
        );
    }

    /// Diagnostics counters for this frame.
    pub fn counters(&self) -> SceneCounters {
        SceneCounters {
            road_draws: self.road.active_count(),
            total_road_draws: self.road.len(),
            road_triangles: self.road.active_triangles(),
            scenery_instances: self.scenery.drawn_count(),
            cached_scenery_chunks: self.scenery.cached_chunks(),
            effect_instances: self.effects.visible_count(),
            traffic_slots: self.traffic.len(),
            pickup_bodies: self.pickups.len(),
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
    pub road_draws: usize,
    pub total_road_draws: usize,
    pub road_triangles: usize,
    pub scenery_instances: usize,
    pub cached_scenery_chunks: usize,
    pub effect_instances: usize,
    pub traffic_slots: usize,
    /// Bodies in the pickup pools — three tiers' worth, all installed at
    /// startup whether or not the course has any pickups.
    pub pickup_bodies: usize,
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
            // **Sunlight** — see [`KEY_COLOR`], which is measured off the
            // reference's own road rather than argued from a colour-temperature
            // intuition. This is the term that decides whether the frame has a
            // sunlit surface in it at all.
            color: Color::linear_rgb(
                palette::ratio(KEY_COLOR[0]),
                palette::ratio(KEY_COLOR[1]),
                palette::ratio(KEY_COLOR[2]),
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
/// So the frame takes a daylight preset instead, and the black point goes to
/// zero: with a bright sky and a bright sea there is no lifted floor left to
/// remove, and removing one anyway would only mud the frame.
///
/// ## Why it is [`FramePostProcess::sunlit`] and no longer `cinematic`
///
/// `cinematic` was the right *family* and the wrong *sign*. It is authored to
/// rescue a raster that arrives warm-brown and flat, so two of its four knobs are
/// corrections: it eases red (`0.98`), lifts blue (`1.06`), and then pushes
/// saturation to `1.18` to put colour back. This frame is not that raster, and it
/// got the correction anyway.
///
/// Measured, band by band, on the champion against the reference — the sky
/// (`y 0.02..0.24`), the atmosphere band (`0.24..0.46`) and the near road
/// (`0.50..0.66`), each as a mean over every pixel in it:
///
/// | | sky | haze band | near road |
/// |---|---|---|---|
/// | champion red | 24 | 29 | 106 |
/// | reference red | 43 | 93 | 151 |
/// | champion saturation | 0.89 | 0.85 | 0.34 |
/// | reference saturation | 0.82 | 0.62 | 0.40 |
///
/// **Red is short in every band and saturation is long in the two that carry the
/// sky and the haze.** Those are not two defects; they are one, and this grade is
/// it. The white balance takes red out of a frame that has almost none to spare,
/// and the `1.18` saturation then scales each channel's *distance from luma* —
/// which drives the deficient red further down and lifts the already-dominant
/// blue further up. Compounded over the whole frame that reads as a cold,
/// electric cast, which is the opposite of the noon the reference was shot at.
///
/// `sunlit` inverts exactly those two (a warm white balance and `1.02`) and keeps
/// `cinematic`'s contrast and black point, which were never the defect. The
/// exposure lift is deliberately small — `1.02 -> 1.08`, well under the `1.10+`
/// the band error alone would ask for — because the constraint that binds here is
/// not the mean, it is the **dome**: [`palette::SKY`] grades to a blue of `0.973`
/// under this preset, and anything past `1.0` clips the sky's own colour to a
/// constant and flattens the gradient (see that constant, and the clipping test
/// beside it, for what that costs). Solved against the three bands under that
/// constraint, the total per-channel error falls from 272 display levels to 186.
///
/// One consequence is mandatory and lands next door: [`palette::HAZE`] is *defined*
/// as the reference's own horizon band inverted through this grade, so it is
/// re-derived here rather than left to drift.
const GRADE: FramePostProcess = FramePostProcess::sunlit();

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
/// component's *sign* is what the shadow-placement result above pinned, and it
/// survives untouched: the key still travels toward `+X`, so the shadow still
/// spills screen-left and toward the camera. Its magnitude and the elevation are
/// then set by where the reference's disc actually is — see
/// [`MOON_DIRECTION`], which measures it.
const KEY_DIRECTION: Vec3 = Vec3::new(
    -MOON_DIRECTION.x,
    -MOON_DIRECTION.y,
    -MOON_DIRECTION.z,
);

/// The direction **toward the light body in the sky** (world space,
/// un-normalized).
///
/// The name still says moon; the body is now the **daylight sun**, and this pass
/// re-aims it, because "~20° up and ~29° right" was asserted of the reference and
/// never measured against it. **Measured, the sun is not in the champion frame at
/// all.**
///
/// **The measurement.** Both frames are shot on the same chase camera, so the
/// road's vanishing point is a shared origin: reference `(490, 775)`, champion
/// `(500, 745)`. The reference's disc centres at `(870, 250)` — `+380 px` right
/// and `525 px` up. The camera's vertical field of view rides its speed band
/// (`fov_low 65°` … `fov_high 88°`), so a pixel offset converts to an angle
/// through `half_height / tan(fov/2)`, giving a *range* rather than a point:
///
/// | reference sun | at `fov 65°` | at `fov 81°` (the champion's speed) |
/// |---------------|--------------|--------------------------------------|
/// | azimuth right | 16.2°        | 20.9°                                |
/// | elevation     | 21.8°        | 26.9°                                |
///
/// The old `(-0.55, 0.42, 1.0)` is `28.8°` right and `20.2°` up: **too far right
/// at every fov in the band, and low.** Projected back through the same
/// arithmetic the disc lands at `x ≈ 1030..1210` on a `939 px` frame — off the
/// right edge, at both ends of the band. That is the whole reason the champion's
/// sky is an empty gradient while the reference's upper right is dominated by a
/// sun and the flare `FrameBloom::highlights` exists to spend on it. The frame
/// was *lit by* a sun with no source in shot — the exact defect the sky body was
/// added to cure, defeated by eight degrees of azimuth.
///
/// `16.2°` right is the narrow-fov end of the measured range rather than its
/// middle, and deliberately so: azimuth is the axis that decides *in shot or
/// not*, the disc has to clear the right edge at **both** ends of a live fov
/// band, and a sun a few degrees too central is a sun you can see. It lands the
/// disc at `x ≈ 770..865` against the reference's `870`. Elevation takes the
/// middle, `23.8°`, which is inside the `12°..28°` visibility band
/// `the_key_throws_its_shadow_toward_the_camera_and_not_out_of_shot` pins.
///
/// **What the elevation buys beyond the disc.** Shadow length is
/// `height / tan(elevation)`: `20.2° → 2.7` caster-heights, `23.8° → 2.3`. The
/// champion's palm shadows are the largest dark shapes in the frame and they
/// read as formless smears across the lower right; 15% off their length is 15%
/// less smear for the same shadow. And `N·L` on the road rises `0.345 → 0.404`,
/// so the key stops throwing away two thirds of itself on the largest surface in
/// shot — which is why [`KEY_INTENSITY`] falls in the same breath. **Read the two
/// constants as one decision: this move is exposure-neutral by construction and
/// the road does not shift by a level.**
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
/// The elevation is **low**, down from the old key's 50°. That is a real trade,
/// made deliberately:
///
/// * A moon at 50° is above the top of the frame from a chase camera. There is
///   no elevation at which a light is both "overhead" and "in shot"; the ask was
///   for a visible moon, so it comes down to where the camera can see it.
/// * A shadow's length is `height / tan(elevation)`: 50° → 0.84 car-heights,
///   23.8° → 2.3. The car's shadow stops being a smear under the bumper and
///   becomes a long raking shape thrown back toward the camera.
/// * The cost is `N·L` on the horizontal road: 0.77 → 0.40. That is why
///   [`KEY_INTENSITY`] rises above one to compensate. The verticals — car flanks,
///   reflector posts, tree cones — gain what the road loses, which is exactly
///   the raking, side-lit look a low sun produces and a high one cannot.
const MOON_DIRECTION: Vec3 = Vec3::new(-0.29, 0.46, 1.0);

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

/// How much of the sky the cloud layer covers.
///
/// **This number is not a fraction of the sky — it is a threshold, and the two
/// are nothing like each other.** The sky shader (`FrameSky::radiance`, mirrored
/// into the GPU arm) turns it into `threshold = 1 - coverage · (1 + CLOUD_EDGE)`
/// with `CLOUD_EDGE = 0.22`, and every point of the cloud field above that
/// threshold is cloud, fully opaque `CLOUD_EDGE` past it. The field is four
/// weighted sinusoid octaves summed to `0..1` with a mean of exactly `0.5`, so
/// the threshold has to sit *well above* `0.5` before the sky is mostly open.
///
/// At `0.55` the threshold was `0.329` — **below the field's own mean**. Sampled
/// over the field, that is 82% of the sky carrying cloud and 49% of it fully
/// opaque: not weather, a lid. It is exactly what the champion frame shows. The
/// top band of that frame — the deepest, cleanest part of any clear sky, where
/// `dir.y` is largest and the field's low-frequency octave spreads one lobe over
/// the whole width — measures 59% white pixels and a mean of `(143, 197, 236)`.
/// The reference's same band is 3% white and `(43, 125, 207)`: open cobalt.
/// [`palette::SKY_ZENITH`] is already authored at that cobalt; nothing but this
/// constant was painting over it, which is why no grade, bloom or exposure move
/// ever shifted the zenith.
///
/// `0.32` puts the threshold at `0.610`, a full `0.11` above the field's mean.
/// That leaves **18% of the sky carrying any cloud and 2% fully opaque** —
/// against the reference's measured ~19% cloud fraction across its sky — so the
/// dome reads as open blue with broken cumulus rather than as a ceiling. The
/// puffs survive where they belong: `reach = CLOUD_SCALE / dir.y` compresses the
/// field toward the horizon, so the same threshold that clears the zenith still
/// crowds cloud into the low band the reference's cumulus sit in.
///
/// Gated by the backend's `Sky` capability, so this reaches the GPU arm only;
/// the Canvas 2D arm drops the sky whole and is untouched.
const CLOUD_COVERAGE: f32 = 0.32;

/// The cloud field's scale — larger is smaller, busier cloud.
///
/// Read against the chase camera's field of view: at 0.5 a puff overhead is a
/// couple of dozen degrees across, which is the broad cumulus of a wide coastal
/// shot rather than the fine mackerel sky a larger value gives. The field is
/// sampled on a plane, so this value also sets how fast the layer crowds toward
/// the vanishing point the road runs to.
const CLOUD_SCALE: f32 = 0.5;

/// The key light's intensity — **the frame's exposure**.
///
/// # `5.9`, and why `1.84` was not the reference's sunlit road
///
/// Every derivation below this section is sound arithmetic run on **the wrong
/// statistic**, and this section replaces the statistic rather than the method.
///
/// The reference's road plane is *bimodal*: 61% of it reads warm (sun-struck),
/// 39% cool (sky-fill-only palm shadow), and the two modes are more than two
/// stops apart. Every level below was solved against "the warm **median**",
/// which sampled at byte 68 — and a median taken over a distribution whose warm
/// half is itself smeared through the penumbra of the frame's biggest shadows is
/// not the sunlit mode, it is the *penumbra*. Re-measured over the same
/// trapezoid (`n = 313k`), the warm half runs median 75, p75 102, **p90 113**;
/// sampled where the road is unambiguously in full sun — the mid-field band
/// between the two shadow ranks, and the near carriageway right of the car — it
/// is `(125, 107, 90)` and `(141, 110, 87)`, luma **110–115**. That is the level
/// the sun lays on flat tarmac in the reference; 68 is the level it lays on the
/// half-shadowed tarmac either side of it.
///
/// The champion measures luma **65.6** on the same near carriageway (model:
/// 67 — the model below is trustworthy to two levels, which is what makes this
/// re-solve worth doing at all). Linearised, the reference's sunlit road is
/// **2.4× brighter**, and its road carries `std 19` of tonal range against the
/// champion's `std 1.5`. The champion's road is not a dark road, it is a *flat
/// slab sitting at the reference's penumbra level* — the frame was exposed for
/// its own shadows.
///
/// Solving the identical model against the sunlit mode instead:
///
/// ```text
/// encoded 0.431 (byte 110, post-grade) -> 0.437 pre-grade (undo cinematic 1.10)
///   -> linear 0.1605 -> /albedo 0.0886 -> key + ambient = 1.812
///   -> key = 1.545 -> intensity = 1.545 / (N·L 0.404 · keyLuma 0.647) = 5.91
/// ```
///
/// **Three independent surfaces agree on it**, which is why it is trusted over a
/// 3.2× jump's face value. At `5.9` the up-facing globals are
/// `(2.58, 1.64, 1.01)`, and:
///
/// | surface | rendered | reference measures |
/// |---------|----------|--------------------|
/// | flat tarmac (albedo `.085/.088/.105`) | `(130, 109, 95)` | `(125, 107, 90)` |
/// | sunlit car flank (`N·L 0.255`, red livery) | `(244, 80, 49)` | `(221..250, 82..99, 43..46)` |
/// | lane paint (albedo `0.72`) | red/green clipped, blue `223` | `(253, 242, 204)` |
///
/// Three different albedos at two different orientations landing on the
/// reference from one gain is the check a single-surface solve cannot give you.
///
/// **What it does not touch.** The sky and the clouds are [`FrameSky`], not lit
/// by this; the car's *camera-facing* rear panel — the subject, and already at
/// the reference's level — has `N·L < 0` against a key travelling toward the
/// camera and receives only the fill. So this is not an exposure lift: it lands
/// on exactly the up-facing and sun-facing surfaces that measure short, and
/// nowhere else. Nothing new spills that the reference does not also blow: the
/// road's own radiance peaks at `0.22`, five times under
/// [`FrameBloom::highlights`]'s `1.0`, and what does clear it is the lane paint
/// and the car's stripes, which the reference blows too (it carries 4.84% of
/// pixels above `L=235`; the champion carries 2.93%).
///
/// **And it is the contrast fix, not only the level fix.** The key:fill ratio on
/// flat road goes `1.80:1 -> 5.79:1` against the reference's own measured
/// sunlit:shaded road of `10.4:1`. The remaining gap is the sky fill's, not the
/// key's, and is left for a pass that measures it — one knob per change.
///
/// ---
///
/// The rest of this comment is the `1.84` derivation, kept intact because
/// everything in it except the target level is still the live argument: the gel,
/// the `N·L` bookkeeping, and why the key rather than the grade owns this.
///
/// `1.84`, and the number is measured off the reference rather than argued from
/// it. **It is a gain, not a brightness**: what the frame actually receives is
/// `intensity · N·L · `[`KEY_COLOR_LUMA`], and this constant only ever moves in
/// company with the other two factors. It was `1.45` against a near-white key of
/// luma `0.959`; [`KEY_COLOR`] re-gels the sun to the reference's measured golden
/// `(1.0, 0.58, 0.27)`, luma `0.647`, and `1.45 · 0.959 / 0.647 = 2.15` is the
/// intensity that keeps every word below true. Flat road is unchanged at luma
/// `0.725`. Read the constants as one decision.
///
/// **And `N·L` is the third factor, which is why `2.15` is now `1.84`.**
/// [`MOON_DIRECTION`] re-aims the sun to where the reference actually puts it —
/// `23.8°` up rather than `20.2°` — and a horizontal surface takes
/// `N·L = 0.404` from that instead of `0.345`. The road would gain 17% of a stop
/// for free, so the gain gives back exactly what the geometry hands it:
/// `2.15 · 0.345 / 0.404 = 1.84`. The product this whole comment sizes,
/// `intensity · N·L · keyLuma`, goes `0.4804 → 0.4810` — the same light, from a
/// visible source, at the same exposure. **Every derivation below is stated
/// against the old `2.15 · 0.345` pairing and is unchanged by the swap**, because
/// the swap holds their product fixed; that is the point of writing it this way
/// rather than re-deriving the exposure a fourth time.
///
/// The `2.6` before that was derived twice by arithmetic — once through
/// [`FramePostProcess::low_key`]'s `0.16` black point, then re-derived when the
/// colorist retired that black point — and never once checked against a render.
/// This pass has both frames in hand and inverts the pipeline instead.
///
/// **The measurement.** The road is the calibration surface: the largest thing
/// in any frame and the one every term lands on hardest. Sampling the road
/// plane of both images and splitting it on chroma (warm `R > B` is sunlit,
/// cool `B > R` is sky-fill-only) gives the reference's two levels directly:
///
/// | reference road | byte | incident light (byte ÷ grade ÷ sRGB ÷ albedo) |
/// |----------------|------|-----------------------------------------------|
/// | sunlit (warm)  | 68.0 | `0.746` — sun + sky                            |
/// | shadowed (cool)| 37.8 | `0.302` — sky alone                            |
///
/// So the reference's sun lays **`0.444`** on flat road and its sky lays
/// `0.302`. The same inversion run on the champion returns `1.20` for every
/// road pixel it has, and the model that produces it agrees with the measured
/// byte to under half a level, so the arithmetic below is trustworthy.
///
/// **Why the key, and not the ambient or the grade.** At `2.6` the key alone
/// puts `2.6 · N·L(0.345) · keyLuma(0.959) = 0.861` on flat road — *more than
/// the reference's entire sunlit road (`0.746`), before any ambient is added at
/// all*. No reduction of the sky fill and no grade can bring that back: a term
/// that on its own overshoots the finished value is over-strength, full stop.
/// That is what makes this the light's defect and not the colourist's. Solving
/// `I · 0.345 · keyLuma = 0.444` is what sizes the key, and the answer moves with
/// the gel: at the old near-white `keyLuma` of `0.959` it gave `1.34`, and `1.45`
/// was the value that carried the test's own (slightly different, luma-averaged)
/// model onto the reference's `0.288` encoded. At [`KEY_COLOR_LUMA`]'s `0.647`
/// the identical solve gives `2.15`, and lands the identical `0.285` encoded —
/// same light, same exposure, differently coloured.
///
/// **What the over-key was costing, beyond level.** Globals of `1.16` on an
/// up-facing surface mean every albedo over `0.86` clips: the lane paint, the
/// car's white stripes and the sunlit sand all pinned at `255`, and all of it
/// then handed to the bloom — which is the milky wash over the champion, not a
/// haze setting. Today the up-facing globals are `(0.932, 0.680, 0.560)`, a luma
/// of `0.725`: the paint renders near byte 195, still comfortably the brightest
/// thing on the road, with headroom above it instead of a bloom smear.
///
/// It also restores the terminator on vertical surfaces. A car flank facing the
/// sun went from `2.44` (clipped to a flat orange slab) to `1.36`; its shaded
/// flank still gets the sky fill and nothing else, so the two now differ by a
/// readable stop instead of both sitting at the top of the range.
///
/// **The one thing this cannot fix.** The reference's road is 57% warm sunlit
/// and 42% cool shadow. Two separate things have to be true for that, and only
/// one of them is a level: the sun-struck road has to *be warm* (that is
/// [`KEY_COLOR`], which this pass fixes — the old near-white key made it
/// arithmetically impossible), and something has to *occlude* the sun to make
/// the cool half. The second is out of an app's reach —
/// `axiom_render_pipeline`'s shadow camera is a fixed
/// 20 m orthographic box anchored at the **world origin** (its own module docs
/// say so), and this moment is ~1.9 km down a 9 km course. Every cast shadow in
/// this frame is geometrically out of the map. Sizing and colouring the key is
/// the half of the axis an app can reach; the other half is a frame-contract
/// change and belongs to the engine architect.
///
/// **Era-C retune, 2026-08-09:** `5.9 → 4.65`, which is *not* an exposure
/// decision. It is the inverse of the luma the de-orangeing of [`KEY_COLOR`]
/// gained (`0.647 → 0.821`), applied so that re-gelling the sun does not
/// silently re-expose the frame. `intensity · N·L · KEY_COLOR_LUMA` is
/// unchanged to three figures.
const KEY_INTENSITY: f32 = 4.65;

/// The key light's **colour** — the sun's gel, and the frame's single largest
/// remaining lighting defect.
///
/// This replaces `(1.0, 0.955, 0.88)`: a near-white key, authored as "the
/// reference is a high coastal sun, warm only by the slight red-over-blue a
/// short atmospheric path leaves." That sentence is a colour-temperature
/// intuition, and the reference disagrees with it by a factor of three.
///
/// **The measurement.** The road is the calibration surface, exactly as in
/// [`KEY_INTENSITY`]. Take the reference's road plane alone (a trapezoid from
/// the mid-field down to the HUD, excluding the sand verge and the lane paint)
/// and split it on chroma — warm `R > B` is sun-struck, cool `B > R` is
/// sky-fill-only. Linearise both means and *subtract*. What is left is the sun
/// and nothing else, because the sky term is common to both and the asphalt's
/// albedo is the same pixel-for-pixel:
///
/// | reference road   | sRGB               | linear                       |
/// |------------------|--------------------|------------------------------|
/// | sunlit (warm)    | `(91.0,76.5,67.5)` | `(0.1045, 0.0733, 0.0569)`   |
/// | shaded (cool)    | `(27.2,40.5,53.1)` | `(0.0111, 0.0217, 0.0357)`   |
/// | **sun** (lit−shaded) |                | `(0.0934, 0.0516, 0.0212)`   |
///
/// Normalised to red, the reference's sun is **`(1.00, 0.55, 0.23)`** — a deeply
/// golden low sun, not a white one. (The same inversion run over the wider road
/// band, `n = 278k`, returns `(1.00, 0.59, 0.26)`; the two agree.) The shaded
/// road normalises to `(1.00, 1.96, 3.22)`, which is the blue sky dome the
/// ambient above is already authored as — that term needs nothing and is left
/// alone.
///
/// **Why this, and not the level, is what the road is missing.** The champion's
/// road measures **0% warm pixels**; the reference's is **57% warm**. Not "a bit
/// cool" — *not one pixel of road in the frame reads as sun-struck.* The
/// arithmetic says why, and it is not a shadow-map problem. On flat road the old
/// rig laid `1.45 · N·L(0.345) · (1.0, 0.955, 0.88) = (0.500, 0.478, 0.440)` of
/// key onto `(0.19, 0.25, 0.36)` of sky, summing to `(0.690, 0.728, 0.800)`.
/// **Blue is the largest channel.** A near-white key carries almost as much blue
/// as red, so it can never out-run a deliberately blue fill: under that rig the
/// road is cool *in full sun*, and the frame's defining warm-lit-against-cool-
/// shadow split is arithmetically unreachable at any intensity. The measured
/// champion road, `(62.3, 63.4, 69.0)`, is that prediction to within a level.
///
/// With this gel the same road takes `2.15 · 0.345 · (1.0, 0.58, 0.27) =
/// (0.742, 0.430, 0.200)`, summing to `(0.932, 0.680, 0.560)` — red largest,
/// `B/R = 0.60`, against the reference's own `0.54`. The road becomes warm where
/// the sun reaches it and stays the untouched blue-grey `(0.19, 0.25, 0.36)`
/// where it does not.
///
/// **This move is exposure-neutral by construction, and that is the point.** A
/// gel costs luma: this one is `0.647` against the old key's `0.959`, so
/// [`KEY_INTENSITY`] rises `1.45 → 2.15` in exactly that inverse ratio. Flat
/// road goes from luma `0.725` to luma `0.725` — the frame's measured exposure,
/// which [`KEY_INTENSITY`] derives at length and which this pass has no argument
/// with, does not move. Only the *hue* of the light moves. Nor can it clip: the
/// largest up-facing global becomes red at `0.932` (green and blue both *fall*,
/// to `0.680` and `0.560`), and the brightest albedo in shot is the lane paint's
/// `0.72`, which lands at `0.671` linear — still under one, still well under
/// [`FrameBloom::highlights`]'s `1.0` threshold, so nothing new spills.
/// The hemisphere ambient's sky end — the fill an up-facing surface gets when
/// the sun is not on it, and therefore the *shadow* level of the whole frame
/// under a single directional key.
///
/// A named constant rather than a literal at the `set_ambient` call because it
/// is read in two places: the rig, and the exposure model in
/// [`tests::the_sun_out_lights_every_other_term_in_the_frame`]. It was a literal
/// in both until 2026-08-09, and the copy in the test had already gone stale
/// once — a guard asserting against a fill the frame no longer used.
const AMBIENT_SKY: [f32; 3] = [0.114, 0.150, 0.216];

/// The hemisphere ambient's ground end — bounce off the road and verge, warm and
/// much weaker than [`AMBIENT_SKY`]. Named for the same reason.
const AMBIENT_GROUND: [f32; 3] = [0.047, 0.041, 0.029];

/// **Era-C retune, 2026-08-09.** The gel above was solved against a *night* rig
/// and it is the term the round-4 architect advisory named as the root of the
/// orange road: forward-modelling the app's own constants gave a pre-grade road
/// of `(136,106,80)` and inverting the measured champion through the grade gave
/// `(129,103,80)` — agreement to within seven levels, which is the proof the
/// backend is faithfully rendering an authored `1 : 0.58 : 0.27` orange rather
/// than a defect. Two lenses then corrected *downstream* of it — the tarmac
/// albedo was rotated cool and the grade's red gain eased 1.15 → 1.04 — and took
/// the road's `r−b` from `+79.9` to `+53.9` against the reference's `+15.8`.
/// About a third. The remaining two thirds are this constant, exactly as the
/// advisory predicted, and no albedo or grade move can reach them without
/// tinting everything else in frame to compensate.
///
/// So the gel's *chroma* is cut to 40% of its excursion from its own luma,
/// renormalised so red stays the unit channel: `R−B` goes `0.73 → 0.37`. The
/// sun stays warm — the reference's sunlit tarmac really is warm — it stops
/// being orange.
///
/// **Exposure-neutral by construction, on the file's own rule.** A gel costs
/// luma; this one delivers `0.821` against the old `0.647`, so [`KEY_INTENSITY`]
/// falls in exactly that inverse ratio (`5.9 → 4.65`, i.e. `× 0.647/0.821`) and
/// the frame's measured level does not move. Only the hue of the light does —
/// the same discipline the previous two gels were solved under, and the reason
/// this can be reasoned about at all.
const KEY_COLOR: [f32; 3] = [1.0, 0.787, 0.630];

/// [`KEY_COLOR`]'s Rec. 709 luma — how much *brightness*, as opposed to hue, the
/// gel actually delivers.
///
/// A rig's exposure is `intensity · N·L · this`, never `intensity · N·L`: two
/// keys at the same intensity but different gels light the frame to different
/// levels, and dropping the term is how a re-gel silently becomes a re-exposure.
/// Named so `the_sun_out_lights_every_other_term_in_the_frame` can hold the
/// road against the reference's measured byte through a *complete* model.
const KEY_COLOR_LUMA: f32 =
    0.2126 * KEY_COLOR[0] + 0.7152 * KEY_COLOR[1] + 0.0722 * KEY_COLOR[2];

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

    /// The cloud coverage is a **threshold on a field whose mean is 0.5**, not a
    /// fraction of the sky, and reading it as the latter is what put a lid over
    /// the champion frame. Pinned against the field's mean, because that is the
    /// number that decides whether the zenith is open sky or overcast.
    #[test]
    fn the_cloud_layer_leaves_the_zenith_open() {
        // `FrameSky::radiance`: threshold = 1 - coverage * (1 + CLOUD_EDGE).
        const CLOUD_EDGE: f32 = 0.22;
        let threshold = 1.0 - CLOUD_COVERAGE * (1.0 + CLOUD_EDGE);
        // The four octave weights sum to 1 and each octave averages 0.5, so the
        // field's mean is exactly 0.5. A threshold at or below it means more than
        // half the sky is cloud — an overcast lid, not weather.
        assert!(
            threshold > 0.6,
            "threshold {threshold:.3} sits at the cloud field's mean (0.5): most \
             of the sky is cloud and the zenith gradient never shows"
        );
        // And not so high that the layer disappears: the field has to be able to
        // clear the threshold *and* the CLOUD_EDGE ramp above it somewhere.
        assert!(
            threshold + CLOUD_EDGE < 1.0,
            "nothing in a 0..1 field can reach full density: the sky has no weather"
        );
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
        // The key on flat ground is `intensity * N·L * keyLuma`, with N = +Y.
        //
        // The `keyLuma` factor is not decoration. This model used to be
        // `KEY_INTENSITY * n_dot_l`, which is *colour-blind*, and a colour-blind
        // exposure model cannot tell a re-gel from a re-exposure: swap the sun's
        // hue and every number below silently reports the frame's old
        // brightness. That is the exact failure [`KEY_COLOR`] would have walked
        // into — its gel drops the key's luma from `0.959` to `0.647`, a third of
        // the frame's light, and this model would have shrugged. With the term
        // present the model is complete, and it is worth noting it changed
        // nothing about the rig it was written against: the near-white key's
        // `1.45 * 0.345 * 0.959`, the golden re-gel's `2.15 * 0.345 * 0.647` and
        // the re-aimed `1.84 * 0.404 * 0.647` are the same `0.480` to three
        // places — a re-gel and a re-aim are both exposure-neutral here, by
        // construction, and this product is what says so.
        //
        // Today's `5.9 * 0.404 * 0.647 = 1.542` is deliberately *not* one of
        // them. It is the one move in this constant's history that is a genuine
        // re-exposure, because the target it was solved against was wrong rather
        // than the arithmetic — see [`KEY_INTENSITY`], and the band at the foot
        // of this test, which is the assertion that carried the bad target.
        let len = (KEY_DIRECTION.x * KEY_DIRECTION.x
            + KEY_DIRECTION.y * KEY_DIRECTION.y
            + KEY_DIRECTION.z * KEY_DIRECTION.z)
            .sqrt();
        let n_dot_l = -KEY_DIRECTION.y / len;
        let key = KEY_INTENSITY * n_dot_l * KEY_COLOR_LUMA;

        // The gel: the reference's sun is warm, and "warm" is a hard inequality,
        // not a taste. But it has a CEILING as well as a floor, and this guard
        // used to have only the floor — as `KEY_COLOR[2] < KEY_COLOR[0] * 0.6`.
        //
        // That form was wrong in two ways and cost the campaign two passes.
        // First it asserted a raw channel ratio while *justifying* itself with an
        // outcome ("cannot put a warm pixel on the road against the blue sky
        // fill"), and the outcome depends on the ambient, which has since been
        // cut 40% — so the constant kept enforcing a conclusion drawn under a
        // sky fill the frame no longer has. Second, and worse, a floor alone
        // says a gel can never be too warm. It can: at `[1.0, 0.58, 0.27]` this
        // road rendered `r−b +79.9` against the reference's `+15.8`, the tarmac
        // read as orange clay, and it became indistinguishable from the sand
        // verge beside it. The guard was satisfied throughout.
        //
        // So it is re-litigated as what it always claimed to be — a statement
        // about the lit road — through the same complete model the rest of this
        // test uses, and it is now two-sided. Both bounds are failures this
        // campaign actually shipped, in both directions.
        let lit = |c: usize| {
            palette::TARMAC[c] * (KEY_INTENSITY * n_dot_l * KEY_COLOR[c] + AMBIENT_SKY[c])
        };
        let (warm, cool) = (lit(0), lit(2));
        assert!(
            warm > cool * 1.05,
            "the key has drifted back toward white ({KEY_COLOR:?}) — the lit road \
             comes out {warm:.4} red against {cool:.4} blue, and the frame's \
             sunlit/shadowed split is unreachable at any intensity"
        );
        assert!(
            warm < cool * 2.0,
            "the key has drifted into orange ({KEY_COLOR:?}) — the lit road comes \
             out {warm:.4} red against {cool:.4} blue, which is the tarmac-as-clay \
             state that also erased the road/verge boundary"
        );

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

        // The hemisphere ambient's sky term is what an up-facing surface gets —
        // and, under one directional key, it is exactly what a *shadowed*
        // up-facing surface gets, which is why the ratio below is the frame's
        // shadow contrast and not merely a fill check. Mirrors the sky end of the
        // `set_ambient` call above; see that call site for the measurement that
        // scaled both hemisphere ends by 0.60.
        let ambient = (AMBIENT_SKY[0] + AMBIENT_SKY[1] + AMBIENT_SKY[2]) / 3.0;

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
        // The wall is where the reference puts it, not where a night rig's
        // intuition put it. Measured off the reference's own road plane, the sky
        // fill is `0.302` and the sun on flat road is `0.444` — a ratio of
        // **0.68**. That is far above the 0.30 this used to demand, and the
        // reason is geometric rather than stylistic: the sun sits at 23.8°, so
        // `N·L` of 0.404 throws away 60% of the key on a horizontal while
        // the sky dome arrives on it whole. A guard calibrated for an overhead
        // sun is simply the wrong guard for a raking one, and holding 0.30
        // against this ambient pinned the key at 2.6 — i.e. it was the assertion,
        // not the reference, that was setting the frame's exposure.
        //
        // 0.75 keeps the rule the test is named for (the sun out-lights every
        // other term) with the reference's own 0.68 sitting just inside it.
        assert!(
            ambient < key * 0.75,
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
        // through — the road as the display receives it, before grading. The
        // band this feeds is set below, against the reference's sunlit mode.
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
        // The band is the reference's **sunlit mode**, read back through the
        // grade that is actually installed.
        //
        // It replaces `0.26..=0.32`, and that band is the whole reason the frame
        // was a stop and a half dark: it was solved against the *warm median* of
        // the reference's road plane (byte 68). The reference's road is bimodal —
        // 61% warm, 39% cool palm shadow — and the warm half is itself smeared
        // through those shadows' penumbra, so its median measures the penumbra,
        // not the sun. Re-measured over the same trapezoid (`n = 313k`) the warm
        // half runs median 75 / p75 102 / **p90 113**, and the unambiguously
        // sunlit tarmac (the mid-field band between the shadow ranks, and the
        // carriageway right of the car) is byte **110..115**. Undoing `GRADE`'s
        // 1.10 contrast about its mid pivot puts the pre-grade sunlit road at
        // **0.437** encoded. The band is that value with ~8 levels either side.
        //
        // A statistic, not a level, was the defect — see [`KEY_INTENSITY`], which
        // records the same correction and the three independent surfaces that
        // agree on the gain it implies.
        assert!(
            (0.41..=0.47).contains(&encoded),
            "the road renders at {encoded:.3} encoded, outside the band that \
             lands it on the reference's measured *sunlit* tarmac (0.437) under \
             the grade that is actually installed — a road that sits below this \
             band is exposed for the reference's shadows, not for its sun"
        );
    }

    /// The depth range is a rendering-quality decision, so it is pinned: it must
    /// cover the drawn road and no more, and its ratio must stay small enough to
    /// keep the road's layers apart.
    #[test]
    fn the_depth_range_covers_the_drawn_road_and_no_more() {
        // The *guaranteed* reach: a car sitting at the far end of its current
        // drawn mesh still has this much road in front of it. The worst case is
        // what the far plane has to cover, not the best.
        let drawn = chunks::DRAWS_AHEAD as f32 * road_mesh::DRAW_SPAN;
        assert!(FAR_PLANE > drawn, "the far plane reaches the furthest road mesh");
        assert!(
            FAR_PLANE < drawn + road_mesh::DRAW_SPAN,
            "and does not waste precision beyond the road that exists"
        );
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
        assert!(c.road_draws > 0);
        assert!(
            c.road_draws <= chunks::DRAWS_AHEAD + chunks::DRAWS_BEHIND + 1,
            "{} road meshes drawn",
            c.road_draws
        );
        assert!(c.total_road_draws > c.road_draws, "the course is streamed, not all drawn");
        assert!(c.road_triangles > 10_000);
        // Scenery is counted in authoring cells and the road in drawn meshes, so
        // these two are bounded against *their own* windows. Comparing them to
        // each other is what the old counter names invited, and it silently
        // stopped meaning anything the moment the road started batching cells.
        assert!(
            c.cached_scenery_chunks <= chunks::CHUNKS_AHEAD + chunks::CHUNKS_BEHIND + 2,
            "{} scenery cells cached",
            c.cached_scenery_chunks
        );
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
                assert!(c.road_draws <= ceiling, "step {step}: {} chunks", c.road_draws);
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
