//! **The frame's look** — the composition step that turns two ported subsystem
//! facades ([`crate::sky::system::SkySystem`] and
//! [`crate::materials::system::MaterialSystem`]) into the lighting, atmosphere
//! and surface parameters the renderer already consumes.
//!
//! Nothing here is a port of a single source file. It is the wiring
//! `core/engine.js` performs across `sky/index.js` and `materials/index.js`,
//! and it is the only place in this crate that translates a ported facade's
//! output into an engine contract (`DirectionalLight`, `FrameAmbient`,
//! `FrameDepthFog`, `MaterialParams`).
//!
//! # 1. The sky: `SkySystem` supersedes `SkyLook`, except for one half
//!
//! `crate::scene::sky_look` and [`crate::sky::system::SkySystem`] are **not**
//! the same layer, and they are not wholly independent either. The overlap is
//! exact and it has to be named, because two things publishing a sun is how a
//! scene ends up lit twice:
//!
//! ```text
//!                          SkyLook          SkySystem
//! sun direction            yes              yes   <- SAME QUANTITY
//! sun colour               yes              yes   <- SAME QUANTITY
//! sun intensity            yes              yes   <- SAME QUANTITY
//! aureole exponent         no               yes
//! beam floor               no               yes
//! cloud-occlusion dimmer   no               yes
//! moon, key handover       no               yes
//! time of day moves        no (const 16.5)  yes (`time_rate`)
//! fog / volumetrics        no               yes
//! exposure bias            no               yes
//! clear colour             yes              NO
//! hemisphere ambient       yes              partial (`ambient_color`, sky only)
//! ```
//!
//! `sky_look.rs` says so itself: it drops "the aureole exponent and the beam
//! floor (both are presentation corrections applied to the *renderer's*
//! DirectionalLight intensity)". Those are not optional — they are
//! `_updateCelestial`'s own arithmetic. So for the key light, **`SkyLook` is a
//! lesser reimplementation of `SkySystem` and loses**: this module reads the
//! sun/moon straight off `SkySystem` and never re-derives them.
//!
//! What `SkySystem` genuinely does not publish is a **clear colour** and a
//! **ground-bounce ambient**, and it does not publish them for a structural
//! reason: in the source the sky is *drawn* (a dome pass) and the ambient comes
//! from a GPU env bake, neither of which this port has. `sky_look.rs`'s other
//! half — one `raymarch_sky` per direction against the two CPU-bakeable LUTs —
//! is the stand-in for exactly that, and it survives. It moves here
//! ([`SkyRadiance`]) so it can be fed from `SkySystem::shared` instead of its
//! own frozen constants, and so the LUTs are baked **once** and held rather
//! than re-baked per call.
//!
//! **Verdict: delete `apps/shmup/src/scene/sky_look.rs`.** Everything in it is
//! either superseded by `SkySystem` (the key light) or lives here now (the
//! raymarch). Leaving it constructed alongside a live `SkySystem` is the
//! two-things-fighting-over-the-sky case: `sky_look::HOUR` is a hard-coded hour
//! and `SkySystem`'s hour is settable and can move, so the sun in the frame and
//! the sun in the sky would drift apart the moment `time_rate` is non-zero.
//!
//! ## When the raymarch re-runs
//!
//! On the source's own gate. `SkySystem::update` re-bakes the environment when
//! the sun has moved 0.35 degrees *and* 0.2 s have passed (`index.js:836-854`),
//! which is precisely the question "has the sky changed enough to re-derive
//! what it lights the scene with". [`SkyDriver::frame`] watches
//! `env_generation()` and re-raymarches on the tick it increments. With the
//! default `time_rate == 0` that is exactly once, at build.
//!
//! # 2. The materials: already constructed, but transiently and half-read
//!
//! `MaterialSystem` is **not** unconstructed. `crate::materials::upload`
//! constructs one in `bake_albedo_maps` and one in `bake_library`, and
//! `scene::app::install_surface_textures` calls the first — so the cache, the
//! name resolution and the bake collapse all run today.
//!
//! What does not run is everything after the bake. The system is dropped on the
//! spot, so the per-key [`crate::materials::system::ResolvedParams`] it computed
//! — `{ ...DEFAULT_PARAMS, ...def.mat, ...opts }`, the merge the whole facade
//! exists to perform — is thrown away, and every batch in the level instead
//! shares one hand-authored `street_material()`. [`MaterialLook`] is the
//! persistent one: it resolves each palette key once and hands back that key's
//! own [`MaterialParams`], which costs **one pipeline** for all of them (a
//! runtime material's parameters are excluded from its surface digest — see
//! `axiom_surface::SurfaceKind`).
//!
//! Two parameters are forced rather than resolved, for the two reasons
//! `scene::app::street_material` already gives: `parallax = 0` (no height map is
//! bound, so a non-zero depth marches a flat field) and `detile = 0` (de-tiling
//! *is* a program permutation, so a key that turned it on would compile a second
//! pipeline to de-tile a 1x1 texture). Every other field is the resolved one.
//!
//! # 3. The HUD: `ui::system::UiSystem` is not missing wiring — `ui::Hud` is a
//! duplicate of it
//!
//! There is no [`crate::ui::system::UiSystem`] wiring in this file, deliberately.
//! `crate::ui::Hud` and `crate::ui::system::UiCore` are two ports of the same
//! file (`ui/index.js:1-613`) that already share `HudState`, `Blip`,
//! `PlayerPull` and `WeaponPull` but own **separate copies of all eleven
//! widgets** and **separate `late_update` frame drives**. `UiCore` is a strict
//! superset: it adds the seven event subscriptions, the effect journal, the
//! killfeed/banner/objective/match/blip API, the minimap gate, the menu host
//! and the `wasm32` DOM view. `Hud` is what `ui/mod.rs` calls "the source's
//! `UiSystem` minus the `Subsystem` impl and the `ctx.get`/`ctx.peek` reaches"
//! — and `UiCore` closes exactly those reaches with `set_links`/`set_camera`/
//! `set_input`/`set_clock`.
//!
//! **Verdict: `UiSystem`/`UiCore` survives, `Hud` is deleted.** Constructing a
//! second HUD next to the first is the one outcome that must not happen, so
//! nothing here constructs one. The migration is mechanical and is written out
//! in this slice's report.

use axiom::prelude::{
    runtime_material, Color, DirectionalLight, FrameAmbient, FrameDepthFog, FrameIndirect,
    MaterialParams, Ratio,
    Surface, UvMode, Vec3 as EngineVec3,
};
use axiom_kernel::StableHash;

use crate::config::{Config, Quality};
use crate::engine::Ctx;
use crate::registry::{Phase, Subsystem};
use crate::materials::system::{
    MaterialOpts, MaterialSystem, OptValue, RendererCaps, ResolvedParams,
};
use crate::sky::atmosphere::{lut_uv, raymarch_sky, Vec3, SUN_ILLUMINANCE_TOP};
use crate::sky::luts::{
    bake_multiscatter, bake_transmittance, Lut2D, MULTISCATTER_SIZE, MULTISCATTER_SQRT_SAMPLES,
    MULTISCATTER_STEPS, TRANSMITTANCE_HEIGHT, TRANSMITTANCE_STEPS, TRANSMITTANCE_WIDTH,
};
use crate::sky::system::{KeyLight, SkySystem, WeatherPatch, SUN_KEY_GAIN};
use crate::world::palette::{Palette, PaletteEntry};

/* ==================================================================== */
/* the sky                                                               */
/* ==================================================================== */

/// Hour of day the level is lit at — **the source's own**.
///
/// `sky/index.js:153-154` sets `this.hour = 16.5; this.timeRate = 0;` in
/// `SkySystem.init`, and nothing moves it: the level is lit by a frozen
/// late-afternoon sun. `SkySystem::new` starts at the same 16.5, so this
/// constant is now the identity rather than an override.
///
/// It was `9.5` — a value inherited from `crate::scene::sky_look`, which picked
/// a mid-morning hour of its own. Seven hours of sun position is a different
/// azimuth, a different elevation, a different sky gradient and a different
/// shadow direction, so no parity comparison against the source could survive
/// it however correct everything downstream was.
pub const HOUR: f64 = 16.5;

/// The raymarch step count for a single direction. The sky-view LUT bake uses
/// `SKYVIEW_STEPS` (40) per texel; this marches the same integral, so it uses
/// the same figure.
const DIRECTION_STEPS: u32 = 40;

/// The denominator that turns `SkySystem`'s scene-referred key intensity into
/// the engine's `Ratio`.
///
/// `DirLight::intensity` is in `SCENE_LUX` units (1 unit = 25000 lx) and peaks
/// at `SUN_ILLUMINANCE_TOP * SUN_KEY_GAIN` — a zenith sun through no atmosphere
/// with no cloud over it. `axiom`'s `DirectionalLight::intensity` is a `Ratio`,
/// so *something* has to divide. This is the one normalisation the engine's
/// light contract forces, and it is the largest value the source's own
/// arithmetic can produce rather than a number picked to look right.
///
/// It is a stand-in for the source's exposure path, not a replacement for it:
/// see [`SkyDriver::exposure_bias`].
pub const KEY_INTENSITY_FULL_SCALE: f64 = SUN_ILLUMINANCE_TOP * SUN_KEY_GAIN;

/// **The scene scale** — what one unit of the port's framebuffer radiance is
/// worth as an engine scene-referred value.
///
/// Two factors, and both are forced rather than chosen.
///
/// **`1 / KEY_INTENSITY_FULL_SCALE`.** [`SkyDriver::key_light`] divides the sun
/// by that constant because `DirectionalLight::intensity` is a `Ratio`, and
/// `scene::boot` hands the tone map an exposure of
/// `KEY_INTENSITY_FULL_SCALE * METERING_FIT`, which restores it. A radiance that
/// does *not* carry the same divisor is multiplied by a constant meant for one
/// that does, and lands about eight times over-bright relative to the key —
/// which is the sun-to-shadow ratio the source's blue shadows are made of.
///
/// **`PI`.** The unit-system conversion between three.js's surfaces and this
/// engine's. `crate::sky::atmosphere`'s photometric contract records that
/// three's Lambert BRDF carries the `1/PI` — a lit surface writes `b = I/PI` —
/// while [`crate::sky::atmosphere::raymarch_sky`] evaluates a radiance and is
/// written to the buffer as-is. Axiom's surfaces here are
/// `axiom_surface::LightingModel::LambertSpecular`, whose own doc is explicit
/// that it is **not** radiometrically scaled (`lit = base * light * N.L`, no
/// `1/PI`; only `LightingModel::Physical` carries one). So an engine-lit surface
/// is `PI` times brighter than the source's for the same light intensity, and
/// every radiance handed to the engine beside it has to be `PI` times brighter
/// too or the sky, the ambient and the fog all sink a stop and a half below the
/// street.
///
/// `PI / (5.12 * 1.55) = 0.3958662`.
pub const SCENE_RADIANCE_SCALE: f64 = std::f64::consts::PI / KEY_INTENSITY_FULL_SCALE;

/// `ln 2`, the base conversion between the source's `e`-based extinction
/// coefficient and `FrameDepthFog`'s `2`-based one. See [`SkyDriver::depth_fog`].
const LN_2: f64 = std::f64::consts::LN_2;

/// The sky's own radiance — the CPU stand-in for the dome pass and the
/// environment bake, both of which are GPU-only in the source.
///
/// Each field is one `raymarch_sky` against the two LUTs that *are* bakeable on
/// the CPU (transmittance and multiscatter), not a plausible constant.
///
/// **Scene-referred, linear and unbounded.** These are radiances on the engine's
/// own scale ([`SCENE_RADIANCE_SCALE`]), not display colours: a sun disc is a
/// four-figure value here and is *meant* to be. `Color` carries them because
/// `Ratio` does not clamp — nothing in the kernel bounds a colour channel at one
/// — and the frame's only tone map is the engine's AgX, downstream of every
/// field below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyRadiance {
    /// The clear colour: the sky a level-eye camera looks into.
    ///
    /// Scene-referred like the rest of this struct — the clear value is written
    /// into the `Rgba16Float` scene target and takes the composite's exposure
    /// and AgX with everything else, so a display-referred value here would be
    /// tone-mapped a second time.
    pub clear_color: Color,
    /// Hemisphere ambient, sky term — straight up.
    pub ambient_sky: Color,
    /// Hemisphere ambient, ground term — the sky reflected off the street.
    pub ambient_ground: Color,
}

/// Owns the ported [`SkySystem`], steps it, and publishes what a frame needs.
///
/// This is the seam: construct one, call [`SkyDriver::frame`] per frame, and
/// read [`SkyDriver::key_light`], [`SkyDriver::ambient`],
/// [`SkyDriver::depth_fog`] and [`SkyDriver::clear_color`].
pub struct SkyDriver {
    /// The ported facade. Public because the weather/time API on it
    /// (`set_time_rate`, `cloud_shadow_at`, `sky_view_params`) is the source's
    /// and has callers this module should not have to proxy.
    pub system: SkySystem,
    transmittance: Lut2D,
    multiscatter: Lut2D,
    /// The Mie scale the two LUTs above were baked at, so a weather change that
    /// moves it cannot leave them silently stale.
    lut_mie_scale: f64,
    radiance: SkyRadiance,
    /// `SkySystem::env_generation()` as of the last raymarch.
    radiance_generation: u64,
}

impl SkyDriver {
    /// Build the sky: construct the facade, set the hour, bake the two CPU
    /// LUTs, and resolve the first frame's radiance.
    ///
    /// This is the expensive call — the transmittance and multiscatter bakes —
    /// and it runs exactly once, at level build.
    pub fn new(quality: Quality, hour: f64) -> Self {
        let mut system = SkySystem::new(quality);
        // `sky:changed` — the caller of `new` has no bus to emit it on yet.
        let _ = system.set_time_of_day(hour);

        let mie = system.weather.turbidity;
        let (transmittance, multiscatter) = bake_luts(mie);
        let radiance = raymarch(&system, &transmittance, &multiscatter);
        SkyDriver {
            radiance_generation: system.env_generation(),
            system,
            transmittance,
            multiscatter,
            lut_mie_scale: mie,
            radiance,
        }
    }

    /// Move the clock. `SkyChanged` is the source's `sky:changed` payload
    /// (`index.js:400-405`); returning it rather than emitting keeps this type
    /// free of the game's event bus, and the caller emits if it has one.
    ///
    /// The radiance is **not** re-derived here — it follows the env-bake gate in
    /// [`SkyDriver::frame`], exactly as the source's env map does.
    pub fn set_hour(&mut self, hours: f64) -> crate::sky::system::SkyChanged {
        self.system.set_time_of_day(hours)
    }

    /// `setWeather(patch)` (`index.js:426-455`), plus the one thing the source
    /// gets for free and this port does not: a turbidity change invalidates the
    /// two LUTs, so they are re-baked here rather than left stale.
    ///
    /// Returns whether the patch changed anything, as the facade does.
    pub fn set_weather(&mut self, patch: &WeatherPatch) -> bool {
        let changed = self.system.set_weather(patch);
        let mie = self.system.weather.turbidity;
        let stale = mie != self.lut_mie_scale;
        stale.then(|| {
            let (transmittance, multiscatter) = bake_luts(mie);
            self.transmittance = transmittance;
            self.multiscatter = multiscatter;
            self.lut_mie_scale = mie;
            self.radiance = raymarch(&self.system, &self.transmittance, &self.multiscatter);
        })
        .unwrap_or_default();
        changed
    }

    /// Advance one frame — `SkySystem::update` (`index.js:461-494`), plus the
    /// CPU stand-in for the environment bake it gates.
    ///
    /// `elapsed` is `ctx.time.elapsed` and `camera_xz` is the camera's world
    /// `(x, z)`; both are the only things the source reads off the frame context
    /// here, and both are state `Game` already holds.
    ///
    /// Returns `true` when the raymarched terms were re-derived this frame,
    /// which is the caller's signal to re-push the ambient, the clear colour and
    /// the fog to the presentation arm. With the default `time_rate == 0` it is
    /// `true` on the first frame and never again.
    pub fn frame(&mut self, dt: f64, elapsed: f64, camera_xz: (f64, f64)) -> bool {
        self.system.update(dt, elapsed, camera_xz);
        let generation = self.system.env_generation();
        let baked = generation != self.radiance_generation;
        baked
            .then(|| {
                self.radiance_generation = generation;
                self.radiance = raymarch(&self.system, &self.transmittance, &self.multiscatter);
            })
            .unwrap_or_default();
        baked
    }

    /// `lateUpdate(dt, ctx)` (`index.js:496-506`) — the camera matrices the dome
    /// and volumetric passes read.
    ///
    /// Both are three's `Matrix4.elements`, **column-major**. `Game` holds
    /// neither, so they are explicit parameters: the caller takes them off the
    /// engine's `FrameOutcome` (`camera_projection`/`camera_view`, inverted).
    ///
    /// Nothing downstream of this port consumes `shared.inv_proj` /
    /// `shared.cam_world` / `shared.cam_pos` yet — the dome pass is unported —
    /// so this is here because the frame ordering is part of the port, not
    /// because a pixel currently depends on it.
    pub fn late_frame(
        &mut self,
        projection_matrix_inverse: [f64; 16],
        camera_matrix_world: [f64; 16],
    ) {
        self.system
            .late_update(projection_matrix_inverse, camera_matrix_world);
    }

    /// The frame's key light, as the engine's component.
    ///
    /// Whichever of sun and moon `SkySystem` fitted the cascades to
    /// (`_applyLightIntensities`, `index.js:808-828`). The ephemeris direction
    /// points *at* the body, so the light's direction is its negation.
    pub fn key_light(&self) -> DirectionalLight {
        let moon = self.system.key_light == KeyLight::Moon;
        let light = [self.system.sun_light, self.system.moon_light][usize::from(moon)];
        let toward = [self.system.sun_direction(), self.system.moon_direction()][usize::from(moon)];
        DirectionalLight {
            direction: EngineVec3::new(-toward.x as f32, -toward.y as f32, -toward.z as f32),
            color: normalized_color(light.color.x, light.color.y, light.color.z),
            intensity: ratio(light.intensity / KEY_INTENSITY_FULL_SCALE),
        }
    }

    /// Unit world direction **pointing at** the sun — what a shadow fit or a
    /// sun-visibility probe wants, as distinct from the light's travel
    /// direction in [`SkyDriver::key_light`].
    pub fn sun_direction(&self) -> EngineVec3 {
        let sun = self.system.sun_direction();
        EngineVec3::new(sun.x as f32, sun.y as f32, sun.z as f32)
    }

    /// The frame's hemisphere ambient.
    pub fn ambient(&self) -> FrameAmbient {
        let sky = self.radiance.ambient_sky.to_array();
        let ground = self.radiance.ambient_ground.to_array();
        FrameAmbient::new([sky[0], sky[1], sky[2]], [ground[0], ground[1], ground[2]])
    }

    /// The clear colour: the sky's own radiance in the band that fills most of a
    /// first-person frame. Scene-referred — see [`SkyRadiance::clear_color`].
    pub fn clear_color(&self) -> Color {
        self.radiance.clear_color
    }

    /// The raymarched terms, whole.
    pub const fn radiance(&self) -> SkyRadiance {
        self.radiance
    }

    /// The frame's **two-band indirect fill** — `render/index.js:1133-1147`.
    ///
    /// The term that stops everything the key light misses from collapsing
    /// toward black. Until this was authored the port's only fill was the
    /// hemisphere ambient, which is one `mix` between two colours by the
    /// normal's up-component — it cannot say that a vertical wall sees half the
    /// sky dome, and it carries no warm street bounce at all.
    ///
    /// The four tunables are the source's own (`render/index.js:405-425`), and
    /// its comment on them is worth keeping: `skyFill` is *"the frame's ONLY
    /// strongly chromatic indirect term… deliberately the biggest one now, and
    /// iblDiffuse below came down by the same amount to pay for it."* That is
    /// why this port can light a frame correctly with no environment probe at
    /// all: the dominant fill was never the probe.
    pub fn indirect_fill(&self) -> FrameIndirect {
        // `s.skyFill` / `s.groundFill` / `s.bounceFill` / `s.iblDiffuse` /
        // `s.interiorIndirect`.
        const SKY_FILL: f64 = 0.32;
        const GROUND_FILL: f64 = 0.013;
        const BOUNCE_FILL: f64 = 0.008;
        const IBL_DIFFUSE: f64 = 0.030;
        const INTERIOR_INDIRECT: f64 = 0.035;

        // `hue.divideScalar( max( hue.x, hue.y, hue.z, 1e-6 ) )` — the band
        // carries the sky's HUE at the band's own level, not the sky's level.
        let unit = |c: [f32; 4]| {
            let m = c[0].max(c[1]).max(c[2]).max(1e-6);
            [c[0] / m, c[1] / m, c[2] / m]
        };
        let sun = self.key_light().intensity.get() as f64;

        // `const skyRef = this._ambLevel / 0.15` with `_ambLevel = 0.15 * sunI`
        // (`index.js:1108`), so the reference IS the beam — which is the point
        // of the indirection: at night the key is a 0.05 moon and a band scaled
        // off it would be nothing.
        let sky_hue = unit(self.radiance.ambient_sky.to_array());
        let sky_level = (SKY_FILL * sun) as f32;

        // The lower band is sunlight off the road, so it takes the KEY's colour
        // through the ground albedo the sky dome itself uses — warm, not blue.
        let key = self.key_light().color.to_array();
        let ground_hue = unit([key[0] * 0.33, key[1] * 0.29, key[2] * 0.225, 1.0]);
        let ground_level = (GROUND_FILL * sun) as f32;

        FrameIndirect::new(
            [
                sky_hue[0] * sky_level,
                sky_hue[1] * sky_level,
                sky_hue[2] * sky_level,
            ],
            [
                ground_hue[0] * ground_level,
                ground_hue[1] * ground_level,
                ground_hue[2] * ground_level,
            ],
            // `u.owFillGain.value.set( 1, s.bounceFill / max( s.groundFill, 1e-6 ) )`.
            [1.0, (BOUNCE_FILL / GROUND_FILL.max(1e-6)) as f32],
            // Multiplies the image-based diffuse, which this engine's main pass
            // does not have — an exact zero times this is still zero, and the
            // lane is carried so it is already right when a probe lands.
            IBL_DIFFUSE as f32,
            INTERIOR_INDIRECT as f32,
        )
    }

    /// The two CPU-baked LUTs.
    ///
    /// `SkyDriver::new` pays for the transmittance and multiscatter bakes once,
    /// and they are the expensive call in the whole sky. Publishing them is what
    /// lets [`crate::scene::wiring::sky_draw::visible_sky`] measure the *visible*
    /// sky's gradient and halo against the **same** atmosphere that lights the
    /// scene, instead of baking a second identical pair beside it.
    pub const fn luts(&self) -> (&Lut2D, &Lut2D) {
        (&self.transmittance, &self.multiscatter)
    }

    /// The frame's atmospheric depth fog, from `SkySystem`'s `_fog` block.
    ///
    /// ## What maps, and what does not
    ///
    /// The source's fog is a **height-fogged, per-channel, phase-functioned**
    /// participating medium (`index.js:171-221`, evaluated by the unported
    /// volumetrics pass). `FrameDepthFog` is a scalar extinction plus a
    /// screen-space ramp. Three things therefore do not survive the boundary,
    /// and are named rather than quietly dropped:
    ///
    /// * **Height falloff.** `height_scale` (18 m) and `base_y` (-2 m) have no
    ///   counterpart — the engine's fog is uniform in `y`.
    /// * **Per-channel extinction.** `extinction_tint` (`[0.94, 1.02, 1.24]`,
    ///   blue-biased so distance loses red first, as Rayleigh does) collapses to
    ///   one rate. The rate used is the **luminance-weighted** mean of the three,
    ///   because the density a viewer reads is a luminance, and the hue the fog
    ///   pulls toward is carried by the colour instead.
    /// * **The phase function** and `shaft_gain` — inscatter, which needs the
    ///   volumetrics pass.
    ///
    /// The `[near, far]` ramp is deliberately a no-op (`strength = 0`):
    /// `FrameDepthFog`'s own module doc shows that a normalized-depth window over
    /// a ground plane running to the horizon is "a switch that flips at one
    /// screen row", and the physical term has no such defect. A backend without
    /// `RenderCapability::AerialPerspective` (the Canvas 2D software raster)
    /// therefore renders no fog rather than a seam.
    ///
    /// The colour distance recedes toward is the **clear colour** — the sky's own
    /// radiance in the view band, which is what aerial perspective is.
    pub fn depth_fog(&self) -> FrameDepthFog {
        let fog = self.system.fog;
        let tint = fog.extinction_tint;
        // Rec. 709 luminance of the per-channel extinction, then base e -> base 2:
        // the source's coefficient is Beer-Lambert in `e`, `FrameDepthFog`'s is
        // `1 - 2^(-rate * d)`.
        let luma = 0.2126 * tint.x + 0.7152 * tint.y + 0.0722 * tint.z;
        let rate = fog.extinction * luma / LN_2;
        let color = self.radiance.clear_color.to_array();
        FrameDepthFog::new(
            ratio(0.0),
            ratio(1.0),
            ratio(0.0),
            [color[0], color[1], color[2]],
        )
        .with_extinction(ratio(rate))
    }

    /// EV of metering compensation for this sun elevation; `+` is darker
    /// (`index.js:98-100`).
    ///
    /// This is a metering *instruction*, **additive** to whatever the renderer's
    /// own bias is — nothing in this module has applied it, and nothing
    /// downstream consumes it yet. Applying it here would double-count against
    /// the app's `FrameTonemap`, so it is published and left to whoever owns the
    /// exposure.
    pub const fn exposure_bias(&self) -> f64 {
        self.system.exposure_bias
    }
}

/// Bake the two CPU-reachable LUTs at a given Mie scale.
///
/// The sky-view LUT is not baked: it is 384x192 raymarches for a dome nothing
/// draws, and the three directions [`raymarch`] needs are cheaper computed
/// directly.
fn bake_luts(mie_scale: f64) -> (Lut2D, Lut2D) {
    let transmittance = bake_transmittance(
        TRANSMITTANCE_WIDTH,
        TRANSMITTANCE_HEIGHT,
        TRANSMITTANCE_STEPS,
        mie_scale,
    );
    let multiscatter = bake_multiscatter(
        MULTISCATTER_SIZE,
        MULTISCATTER_STEPS,
        MULTISCATTER_SQRT_SAMPLES,
        mie_scale,
        &transmittance,
    );
    (transmittance, multiscatter)
}

/// The CPU stand-in for the sky-view bake and the env bake, fed entirely from
/// `SkySystem::shared` — the same block `SkyLuts.bakeSkyView` reads
/// (`luts.js:172-176`).
///
/// Nothing here re-derives a celestial quantity: the irradiances, the view
/// position, the Mie scale and the ground albedo are all the facade's published
/// values, which is what stops this from becoming a second sky model.
fn raymarch(system: &SkySystem, transmittance: &Lut2D, multiscatter: &Lut2D) -> SkyRadiance {
    let shared = system.shared;
    let sun = system.sun_direction();
    let moon = system.moon_direction();

    let radiance = |dir: Vec3| -> Vec3 {
        raymarch_sky(
            shared.view_pos,
            dir,
            sun,
            shared.sun_irradiance,
            moon,
            shared.moon_irradiance,
            DIRECTION_STEPS,
            shared.mie_scale,
            |p, d| {
                let (u, v) = lut_uv(p, d);
                transmittance.sample(u, v)
            },
            |p, d| {
                let (u, v) = lut_uv(p, d);
                multiscatter.sample(u, v)
            },
        )
    };

    // The clear colour is the sky a level-eye camera actually looks into: 12
    // degrees above the horizon, on the far side of the sun's azimuth, which is
    // the band that fills most of a first-person frame.
    let horizon_az = Vec3::new(-sun.x, 0.0, -sun.z).normalize();
    let clear_dir = Vec3::new(
        horizon_az.x * 0.978,
        0.208, // sin(12 deg)
        horizon_az.z * 0.978,
    )
    .normalize();

    // Hemisphere ambient: straight up is the sky term; the down term is the same
    // sky reflected off the ground albedo the shared block publishes.
    let up = radiance(Vec3::new(0.0, 1.0, 0.0));
    SkyRadiance {
        clear_color: scene_radiance(radiance(clear_dir)),
        ambient_sky: scene_radiance(up),
        ambient_ground: scene_radiance(up.mul(shared.ground_albedo)),
    }
}

/// The port's framebuffer radiance, on the engine's scene scale — **linear and
/// unbounded**, which is exactly what every contract downstream of it asks for.
///
/// This replaced an invented Reinhard (`x / (1 + x)` at a fixed exposure of 1.0)
/// that was carried over from `scene::sky_look` and labelled at the time as a
/// stand-in for "an exposure and a tone map in the render graph, which is not
/// ported. A future render arm replaces it." **That arm landed** —
/// `scene::boot` authors a real `FrameTonemap`, the scene target is
/// `Rgba16Float` and the engine runs the ported AgX curve over it — so the
/// Reinhard had become a second tone map in front of the real one. It squashed
/// every sky, ambient and fog term into `0.3..0.6`, and the `Ratio` clamp behind
/// it flattened the sun disc's linear ~4000 to a 1.0. AgX then received a frame
/// whose whole dynamic range had already been spent.
///
/// The four consumers are `FrameSky`'s gradient stops and body colour
/// (`crate::scene::wiring::sky_draw`), [`SkyDriver::ambient`],
/// [`SkyDriver::depth_fog`]'s colour and [`SkyDriver::clear_color`]. All four
/// are documented linear and unbounded — `axiom_host::frame_sky`'s module doc
/// says so outright, and its own tests author a sun at `[3.0, 2.8, 2.4]`.
///
/// See [`SCENE_RADIANCE_SCALE`] for why the scale is what it is.
pub fn scene_radiance(radiance: Vec3) -> Color {
    hdr_color(
        radiance.x * SCENE_RADIANCE_SCALE,
        radiance.y * SCENE_RADIANCE_SCALE,
        radiance.z * SCENE_RADIANCE_SCALE,
    )
}

/// A [`Color`] from three **scene-referred** channels: finite-guarded and
/// floored at zero, but deliberately *not* clamped at one.
///
/// `Ratio` itself does not clamp — its own doc says "finite values (including
/// HDR magnitudes above `1.0`) pass through unchanged" — so the 0..1 ceiling in
/// this module was only ever [`ratio`]'s, and a scene value has no business
/// passing through it. `f64::max` returns the non-NaN operand, so the floor is
/// also the NaN guard [`normalized_color`] provides for the display-referred
/// side.
fn hdr_color(r: f64, g: f64, b: f64) -> Color {
    Color::linear_rgb(hdr(r), hdr(g), hdr(b))
}

/// One scene-referred channel as a `Ratio`: non-negative, finite, unbounded above.
fn hdr(value: f64) -> Ratio {
    Ratio::finite_or_zero(value.max(0.0) as f32)
}

/// A [`Color`] from three already-normalised channels, guarded so a NaN out of
/// the raymarch can never reach the renderer as an unwrap panic deep in a frame.
///
/// For values that genuinely live in `0..1` — a max-normalised light tint. A
/// scene radiance takes [`hdr_color`] instead.
fn normalized_color(r: f64, g: f64, b: f64) -> Color {
    Color::linear_rgb(ratio(r), ratio(g), ratio(b))
}

/// A `Ratio` from an `f64`, clamped to `0..1` and NaN-safe.
fn ratio(v: f64) -> Ratio {
    Ratio::new(v.clamp(0.0, 1.0) as f32).unwrap_or(Ratio::finite_or_zero(0.0))
}

/* ==================================================================== */
/* the materials                                                         */
/* ==================================================================== */

/// One palette key's resolved appearance, as the engine's contracts.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyLook {
    /// The library name the key's surface bakes from (`plaster`, `asphalt`, ...).
    pub library_name: &'static str,
    /// The runtime-material surface carrying this key's own resolved
    /// [`MaterialParams`]. Every key's surface shares one program — see the
    /// module doc.
    pub surface: Surface,
    /// `mat.emissive`, decoded and scaled by `emissiveIntensity`, or `None`
    /// where the key is not a practical.
    pub emissive: Option<Color>,
    /// `mat.opacity`.
    pub opacity: Ratio,
    /// `if (p.vertexMasks) mat.vertexColors = true` (`index.js:218`).
    pub vertex_colors: bool,
    /// `mat.transparent`, after `applyProps`.
    pub transparent: bool,
}

/// Owns the ported [`MaterialSystem`] for the life of the scene, and resolves
/// every palette key through it once.
///
/// See the module doc for why this is *not* "nothing constructs it": the facade
/// already runs at bake time and is dropped. This is the persistent one, so the
/// parameters it merges survive to the frame.
pub struct MaterialLook {
    /// The ported facade, public for the same reason [`SkyDriver::system`] is —
    /// `tune`, `set_ground_level`, `surface_of` and `names` are the source's
    /// public API and should not need a proxy per method.
    pub system: MaterialSystem,
    keys: Vec<(&'static str, KeyLook)>,
}

impl MaterialLook {
    /// Construct the facade at a quality and a ground height, and resolve every
    /// entry in [`Palette::ALL`].
    ///
    /// `ground_y` is the world height the weathering's ground-splash term
    /// measures up from; it is `set_ground_level`'s argument
    /// (`index.js:260-268`) and must be the level's real ground plane.
    ///
    /// The renderer's max anisotropy is the one capability `TextureForge` reads
    /// off a `WebGLRenderer` (`generator.js:147-150`); `8` is the source's own
    /// default and matches what `materials::upload` passes.
    pub fn new(quality: Quality, ground_y: f64) -> Self {
        let mut system = MaterialSystem::new(Some(RendererCaps {
            max_anisotropy: Some(8.0),
        }));
        // `configure` reports whether the quality moved; at construction it
        // always has, and nothing here acts on the answer.
        let _ = system.configure(quality, 8);
        system.set_ground_level(ground_y);

        let keys = Palette::ALL
            .iter()
            .map(|(key, entry)| (*key, resolve_key(&mut system, entry)))
            .collect();
        MaterialLook { system, keys }
    }

    /// This palette key's resolved look, or `None` for a key the palette does
    /// not carry.
    pub fn key(&self, palette_key: &str) -> Option<&KeyLook> {
        self.keys
            .iter()
            .find(|(key, _)| *key == palette_key)
            .map(|(_, look)| look)
    }

    /// Every resolved key, in [`Palette::ALL`] order.
    pub fn keys(&self) -> &[(&'static str, KeyLook)] {
        &self.keys
    }

    /// The distinct surfaces to declare at authoring time, deduplicated by
    /// content digest, so the preparation barrier compiles every program the
    /// frame will name **before** the first frame.
    ///
    /// With `detile` forced off this is exactly one surface; the deduplication
    /// is here so that turning de-tiling back on adds the second program to the
    /// barrier instead of rendering a fallback.
    pub fn surfaces(&self) -> Vec<Surface> {
        // By PARAMETER REGION, not by digest.
        //
        // Every runtime material shares one digest by construction — that is what
        // makes them one program — so deduplicating on it returned exactly ONE
        // surface for all forty-six palette keys, and the barrier prepared one
        // region for the whole street. `Surface::param_key` is the identity that
        // distinguishes concrete from brick from glass; this list is what the
        // preparation barrier compiles, so a key missing from it is a material
        // that silently renders as somebody else's.
        let mut seen: Vec<StableHash> = Vec::new();
        self.keys
            .iter()
            .filter_map(|(_, look)| {
                let key = look.surface.param_key();
                let fresh = !seen.contains(&key);
                fresh.then(|| {
                    seen.push(key);
                    look.surface.clone()
                })
            })
            .collect()
    }

    /// `update(dt)` (`index.js:166-172`) — the scratch-release idle timer. It is
    /// the only per-frame work the facade does, and it never ran before because
    /// nothing held the system long enough for five seconds to pass.
    pub fn frame(&mut self, dt: f64) {
        self.system.update(dt);
    }
}

/// Resolve one palette entry through the facade and translate the result into
/// the engine's contracts.
fn resolve_key(system: &mut MaterialSystem, entry: &PaletteEntry) -> KeyLook {
    let opts = palette_opts(entry);
    let def = system.get(entry.name, &opts);
    let emissive = def
        .three
        .num("emissive")
        .map(|hex| emissive_color(hex as u32, def.three.num("emissiveIntensity").unwrap_or(1.0)));
    KeyLook {
        library_name: entry.name,
        surface: runtime_material(engine_params(&def.params)),
        emissive,
        opacity: ratio(def.three.num("opacity").unwrap_or(1.0)),
        vertex_colors: def.vertex_colors,
        transparent: def.transparent(),
    }
}

/// `crate::world::palette::PaletteEntryOpts` as the facade's `opts` bag.
///
/// The key names are the source's camelCase ones, because they are what
/// `apply_to_params` and `stableKey` match on — a snake_case key would be
/// silently ignored by the first and would change the cache key in the second.
/// `three` is nested and **insertion-ordered**, exactly as the source's object
/// literal is (`stableKey` sorts only the top level).
fn palette_opts(entry: &PaletteEntry) -> MaterialOpts {
    let o = &entry.opts;
    let three = o.three.as_ref().map(|t| {
        let entries: Vec<(String, OptValue)> = [
            ("side", t.side.map(f64::from)),
            ("emissive", t.emissive.map(f64::from)),
            ("emissiveIntensity", t.emissive_intensity.map(f64::from)),
            ("opacity", t.opacity.map(f64::from)),
            ("envMapIntensity", t.env_map_intensity.map(f64::from)),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|n| (k.to_string(), OptValue::Num(n))))
        .chain(
            t.tone_mapped
                .map(|b| ("toneMapped".to_string(), OptValue::Bool(b))),
        )
        .collect();
        OptValue::Obj(entries)
    });

    // `scale` is not `Option` on the palette entry — every entry authors one.
    let authored: Vec<(&str, Option<OptValue>)> = vec![
        ("scale", Some(OptValue::Num(f64::from(o.scale)))),
        ("vertexMasks", o.vertex_masks.map(OptValue::Bool)),
        ("tint", o.tint.map(|v| OptValue::Num(f64::from(v)))),
        (
            "normalStrength",
            o.normal_strength.map(|v| OptValue::Num(f64::from(v))),
        ),
        ("weather", o.weather.map(|v| num_array(&v))),
        ("wear", o.wear.map(|v| num_array(&v))),
        ("detile", o.detile.map(|v| OptValue::Num(f64::from(v)))),
        ("roughness", o.roughness.map(|v| num_array(&v))),
        ("three", three),
    ];

    authored
        .into_iter()
        .filter_map(|(key, value)| value.map(|v| (key, v)))
        .fold(MaterialOpts::new(), |opts, (key, value)| {
            opts.with(key, value)
        })
}

/// A fixed-length `f32` array as a JS number array.
fn num_array<const N: usize>(values: &[f32; N]) -> OptValue {
    OptValue::Arr(
        values
            .iter()
            .map(|v| OptValue::Num(f64::from(*v)))
            .collect(),
    )
}

/// `new THREE.Color(hex)` scaled by `emissiveIntensity`, as a linear colour.
///
/// **The intensity clips.** `Color`'s channels are `Ratio`s and the practicals
/// author intensities of `12.0` and `1.1`; a value above one is exactly what
/// makes a lamp bloom, and it cannot cross this boundary. `Material::with_emissive`
/// takes a `Color`, so the surplus is lost here and the lamp reads as a flat
/// saturated warm rather than a light. That is a real limit of the engine's
/// emissive contract, stated rather than hidden — and it is still strictly
/// better than the current frame, where the emissive is dropped entirely.
fn emissive_color(hex: u32, intensity: f64) -> Color {
    let channel = |shift: u32| {
        let srgb = f64::from((hex >> shift) & 0xff) / 255.0;
        srgb_to_linear(srgb) * intensity
    };
    normalized_color(channel(16), channel(8), channel(0))
}

/// The sRGB electro-optical transfer function. Three.js decodes every hex colour
/// literal this way (`THREE.ColorManagement`, on by default in r180).
fn srgb_to_linear(c: f64) -> f64 {
    let low = c / 12.92;
    let high = ((c + 0.055) / 1.055).powf(2.4);
    [high, low][usize::from(c <= 0.04045)]
}

/// `crate::materials::system::ResolvedParams` as `axiom_surface::MaterialParams`.
///
/// The two are field-for-field ports of the same `DEFAULT_PARAMS`
/// (`shader.js:697-777`) — one in this app, one in the `surface` layer — so this
/// is a widening, not a translation. The `Vec<f64>` arrays absorb a short array
/// per element, which is the source's own `p.roughness[2] ?? DEFAULT.roughness[2]`.
///
/// Two fields are **forced**, not resolved; both for the reasons
/// `scene::app::street_material` already records:
///
/// * `parallax = 0` — no height map is bound, so a non-zero depth marches a flat
///   field and costs the loop for nothing.
/// * `detile = 0` — de-tiling is a structural permutation (`SurfaceKind::code`),
///   so a key that turned it on would compile a second pipeline to de-tile a 1x1
///   texture.
///
/// Delete both forcings the moment the ORM+height binding carries real texels.
pub fn engine_params(p: &ResolvedParams) -> MaterialParams {
    let d = MaterialParams::default();
    MaterialParams {
        uv_mode: uv_mode(&p.uv_mode),
        local_space: p.local_space,
        scale: p.scale as f32,
        offset: fill(&p.offset, d.offset),
        parallax: 0.0,
        parallax_fade: fill(&p.parallax_fade, d.parallax_fade),
        parallax_layers: p.parallax_layers as f32,
        detail: fill(&p.detail, d.detail),
        detail_world: p.detail_world as f32,
        macro_: fill(&p.macro_, d.macro_),
        macro_big: fill(&p.macro_big, d.macro_big),
        patch: fill(&p.patch, d.patch),
        cloth: fill(&p.cloth, d.cloth),
        macro_relief: p.macro_relief as f32,
        detile: 0.0,
        weather: fill(&p.weather, d.weather),
        ground_y: p.ground_y as f32,
        wear: fill(&p.wear, d.wear),
        wear_material: fill(&p.wear_material, d.wear_material),
        wear_color: p.wear_color,
        dust_color: p.dust_color,
        grime_color: p.grime_color,
        rust_color: p.rust_color,
        tint: p.tint,
        normal_strength: p.normal_strength as f32,
        roughness: fill(&p.roughness, d.roughness),
        ao_strength: p.ao_strength as f32,
        alpha_mask: p.alpha_mask,
        vertex_masks: p.vertex_masks,
        no_grad: p.no_grad,
    }
}

/// `DEFAULT_PARAMS.uvMode`. The source compares the string for equality and
/// treats anything unrecognised as planar, so this does the same rather than
/// failing on a typo the source would have absorbed.
fn uv_mode(name: &str) -> UvMode {
    [
        ("triplanar", UvMode::Triplanar),
        ("mesh", UvMode::Mesh),
    ]
    .into_iter()
    .find(|(candidate, _)| *candidate == name)
    .map_or(UvMode::Planar, |(_, mode)| mode)
}

/// Narrow a JS-shaped `Vec<f64>` into a fixed array, taking each missing element
/// from `fallback` — the source's `p.x[i] ?? DEFAULT_PARAMS.x[i]`.
fn fill<const N: usize>(values: &[f64], fallback: [f32; N]) -> [f32; N] {
    std::array::from_fn(|i| values.get(i).map_or(fallback[i], |v| *v as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The LUT bakes are the expensive part, so the sky is exercised through as
    /// few drivers as the assertions allow.
    fn authored_hour() -> SkyDriver {
        SkyDriver::new(Quality::High, HOUR)
    }

    /// **The defect this file was rewritten to fix, written down as a number.**
    ///
    /// The frame was tone mapped twice: `display` (as `scene_radiance` was then
    /// called) ran a Reinhard `x/(1+x)` and then clamped to `0..1`, in front of
    /// the engine's real AgX over an `Rgba16Float` target. Everything it touched
    /// — the sky's two stops, the hemisphere ambient, the fog colour — came out
    /// compressed and clipped, while [`SkyDriver::key_light`] went to the engine
    /// on the honest photometric scale. The *ratio* between them, which is what
    /// the source's blue shadows and its sunlit-facade contrast are made of, was
    /// destroyed before the renderer saw a pixel.
    ///
    /// So this pins the ratio, not the values: how far under a fully-lit white
    /// surface the sky's two gradient stops sit, in stops. The bands are wide —
    /// they are there to catch a second display transform reappearing, or the
    /// scale being dropped again, not to freeze the atmosphere's arithmetic.
    #[test]
    fn the_sky_sits_the_right_number_of_stops_under_the_key_light() {
        let sky = authored_hour();
        let key = sky.key_light();
        let luma = |c: [f32; 3]| {
            0.2126 * f64::from(c[0]) + 0.7152 * f64::from(c[1]) + 0.0722 * f64::from(c[2])
        };
        let rgb = |c: [f32; 4]| [c[0], c[1], c[2]];
        // What a white surface facing the key writes: the engine's non-physical
        // arm is `base * light_colour * light_intensity * N.L`, so at `N.L = 1`
        // and unit albedo this is the whole of it.
        let lit = f64::from(key.intensity.get()) * luma(rgb(key.color.to_array()));
        let under = |c: [f32; 4]| (lit / luma(rgb(c))).log2();

        let horizon = under(sky.radiance().clear_color.to_array());
        let zenith = under(sky.radiance().ambient_sky.to_array());
        assert!(
            (2.0..3.5).contains(&horizon),
            "the horizon band sits {horizon} stops under the key"
        );
        assert!(
            (4.0..5.5).contains(&zenith),
            "the zenith sits {zenith} stops under the key"
        );
        assert!(
            zenith > horizon,
            "the zenith is darker than the horizon at this hour"
        );

        // And the sun disc is scene-referred: a linear four-figure radiance that
        // the old `0..1` clamp flattened to display white before AgX could
        // decide where display white was.
        let body = crate::scene::wiring::sky_draw::visible_sky(&sky).body_color();
        assert!(
            luma(body) > 10.0,
            "the sun disc is clamped, not radiance: {body:?}"
        );
    }

    // ---- sky -------------------------------------------------------------

    #[test]
    fn the_driver_lights_the_scene_from_sky_system_not_from_a_second_model() {
        let sky = authored_hour();
        assert_eq!(sky.system.time_of_day(), HOUR);
        assert!(sky.system.sun_altitude() > 0.3, "the sun is well up");
        let light = sky.key_light();
        // The ephemeris points AT the sun; the light travels the other way.
        assert!(light.direction.y < 0.0, "the key comes down onto the street");
        assert_eq!(
            light.direction.x,
            -sky.sun_direction().x,
            "the light is exactly the negated ephemeris, not a re-derivation"
        );
        assert!(light.intensity.get() > 0.3, "got {}", light.intensity.get());
        assert!(light.intensity.get() <= 1.0);
        // Atmospheric extinction removes blue first, so a daytime sun is
        // red >= green >= blue after normalisation.
        let c = light.color.to_array();
        assert!(c[0] >= c[1]);
        assert!(c[1] >= c[2]);
    }

    #[test]
    fn the_key_intensity_normalisation_never_exceeds_one() {
        // Noon is the brightest the source's own arithmetic gets.
        let sky = SkyDriver::new(Quality::High, 12.0);
        assert!(sky.key_light().intensity.get() <= 1.0);
        assert!(sky.key_light().intensity.get() > 0.4);
    }

    #[test]
    fn the_clear_colour_is_a_daylight_blue_and_the_ambient_is_a_hemisphere() {
        let sky = authored_hour();
        let clear = sky.clear_color().to_array();
        assert!(clear[2] > clear[0], "the sky is blue: {clear:?}");
        assert!(clear[2] > 0.1, "and bright enough to see");

        let ambient = sky.ambient();
        // The ground term is the sky reflected off a dry-earth albedo: darker,
        // and less blue.
        assert!(ambient.ground()[2] < ambient.sky()[2]);
        assert!(ambient.ground()[0] > 0.0, "and not black");
    }

    #[test]
    fn midnight_hands_the_key_to_the_moon_and_puts_the_sun_below_the_horizon() {
        let sky = SkyDriver::new(Quality::High, 0.0);
        assert!(sky.system.sun_altitude() < 0.0);
        assert_eq!(sky.system.key_light, KeyLight::Moon);
        // The moon key still lights something — the source floors it at 0.03 so
        // the renderer does not switch its fallback sun back on.
        assert!(sky.key_light().intensity.get() > 0.0);
    }

    /// A still sky raymarches **once, at construction**, and never again.
    ///
    /// This test used to assert the opposite of its own name: that the env-bake
    /// gate *fired* on one of the first thirty frames. It did fire, but only as
    /// an artefact — `HOUR` was `9.5` while `SkySystem::new` starts at the
    /// source's `16.5`, so `SkyDriver::new`'s `set_time_of_day` bumped
    /// `env_generation` and left the driver stale-by-construction for the first
    /// frame to clear. `SkyDriver::new` **already** raymarches eagerly, so that
    /// clearing re-derived a radiance that was correct the moment it was built.
    ///
    /// With `HOUR` back on the source's own hour the hour never moves, the
    /// generation never moves, and the gate correctly never fires. That is the
    /// real invariant and it is the one worth pinning: a frozen sky must not pay
    /// a raymarch per frame. `moving_the_clock_re_derives_the_radiance_on_the_env_gate`
    /// covers the other half — that the gate *does* fire when the clock moves.
    #[test]
    fn a_still_sky_raymarches_once_at_construction_and_never_again() {
        let mut sky = authored_hour();
        let before = sky.radiance();
        // Built, not blank: `new` resolved the radiance before any frame ran.
        assert!(
            before.clear_color.to_array()[2] > 0.0,
            "the radiance was left for the first frame to derive: {:?}",
            before.clear_color.to_array()
        );
        // Well past the 0.2 s `env_age` the gate waits on, twice over.
        let baked = (0..150).fold(false, |seen, i| {
            seen | sky.frame(1.0 / 60.0, f64::from(i) / 60.0, (0.0, 0.0))
        });
        assert!(
            !baked,
            "a still sky re-baked, which would raymarch every frame"
        );
        assert_eq!(before.clear_color, sky.radiance().clear_color);
    }

    #[test]
    fn moving_the_clock_re_derives_the_radiance_on_the_env_gate() {
        let mut sky = authored_hour();
        let day_clear = sky.clear_color().to_array();
        sky.set_hour(19.6);
        // One frame past the 0.2 s env age, so the gate can fire.
        let baked = (0..30).fold(false, |seen, _| {
            seen | sky.frame(1.0 / 60.0, 0.0, (0.0, 0.0))
        });
        assert!(baked);
        let dusk_clear = sky.clear_color().to_array();
        assert_ne!(day_clear, dusk_clear, "the sky did not follow the sun");
        // 45 N at the solstice: 19.6 h puts the sun within a degree of the
        // horizon, so the sky is dimmer than it was at a 53-degree elevation.
        assert!(sky.system.sun_altitude() < 0.05);
        let sum = |c: [f32; 4]| c[0] + c[1] + c[2];
        assert!(sum(dusk_clear) < sum(day_clear), "the sky did not dim");
    }

    #[test]
    fn a_turbidity_change_re_bakes_the_luts_rather_than_leaving_them_stale() {
        let mut sky = authored_hour();
        let clear = sky.clear_color();
        let changed = sky.set_weather(&WeatherPatch {
            turbidity: Some(4.0),
            ..WeatherPatch::default()
        });
        assert!(changed);
        assert_eq!(sky.system.weather.turbidity, 4.0);
        assert_ne!(clear, sky.clear_color(), "a dust storm looks the same");
    }

    #[test]
    fn a_weather_patch_that_leaves_turbidity_alone_keeps_the_luts() {
        let mut sky = authored_hour();
        let clear = sky.clear_color();
        sky.set_weather(&WeatherPatch {
            cloud_coverage: Some(0.6),
            ..WeatherPatch::default()
        });
        assert_eq!(clear, sky.clear_color());
    }

    #[test]
    fn the_fog_is_the_physical_term_only_and_recedes_toward_the_sky() {
        let sky = authored_hour();
        let fog = sky.depth_fog();
        assert_eq!(fog.strength().get(), 0.0, "the NDC ramp is a deliberate no-op");
        let rate = fog.extinction().get();
        // `1.45e-3 /m` in base e is ~2.1e-3 in base 2, luminance-weighted a
        // touch above that by the blue-biased tint.
        assert!(rate > 2.0e-3 && rate < 2.3e-3, "got {rate}");
        let clear = sky.clear_color().to_array();
        assert_eq!(fog.color(), [clear[0], clear[1], clear[2]]);
        // Half the haze is in at ~470 m, which is the street's whole depth.
        assert!(fog.extinction().get() * 470.0 > 0.9);
    }

    #[test]
    fn late_frame_writes_the_camera_into_the_shared_block() {
        let mut sky = authored_hour();
        let mut world = [0.0f64; 16];
        world[12] = 3.0;
        world[13] = 1.7;
        world[14] = -8.0;
        sky.late_frame([0.0; 16], world);
        assert_eq!(sky.system.shared.cam_pos.x, 3.0);
        assert_eq!(sky.system.shared.cam_pos.y, 1.7);
        assert_eq!(sky.system.shared.cam_pos.z, -8.0);
    }

    #[test]
    fn the_exposure_bias_is_published_and_unapplied() {
        let day = authored_hour();
        let dusk = SkyDriver::new(Quality::High, 19.8);
        assert_eq!(day.exposure_bias(), day.system.exposure_bias);
        assert!(dusk.exposure_bias() > day.exposure_bias(), "dusk meters darker");
    }

    /// The conversion is a **linear scale**, not a tone map.
    ///
    /// This test used to assert `bright[0] < 1.0` — "Reinhard never reaches
    /// one" — and that assertion was the defect written down: the frame is tone
    /// mapped by the engine's AgX over an `Rgba16Float` target, so a second
    /// compressive curve in front of it spent the whole dynamic range before
    /// the real one saw the frame. What has to hold now is that the transform
    /// is exactly `SCENE_RADIANCE_SCALE`, that it is therefore ratio-preserving
    /// (which is what makes the sky comparable to the key light at all), and
    /// that it is still floored and NaN-safe.
    #[test]
    fn the_scene_transform_is_a_linear_unbounded_scale_and_nan_safe() {
        let dark = scene_radiance(Vec3::splat(0.01)).to_array();
        let bright = scene_radiance(Vec3::splat(100.0)).to_array();
        assert!(dark[0] < bright[0]);
        assert!(
            bright[0] > 1.0,
            "a scene value above display white has to survive: {}",
            bright[0]
        );
        // Exactly the scale, on both probes — no shoulder anywhere in between.
        // The tolerances are `f32` round-trip slack, not modelling slack: the
        // channels are stored as `f32`, so the ratio of two of them carries
        // about `1e-3` of absolute error at `10_000`.
        assert!((f64::from(dark[0]) - 0.01 * SCENE_RADIANCE_SCALE).abs() < 1.0e-8);
        assert!((f64::from(bright[0]) / f64::from(dark[0]) - 10_000.0).abs() < 0.1);
        assert_eq!(scene_radiance(Vec3::splat(0.0)).to_array()[0], 0.0);
        assert_eq!(scene_radiance(Vec3::splat(-5.0)).to_array()[0], 0.0);
        assert_eq!(scene_radiance(Vec3::splat(f64::NAN)).to_array()[0], 0.0);
        assert_eq!(normalized_color(f64::NAN, 0.5, 0.5).to_array()[0], 0.0);
    }

    // ---- materials -------------------------------------------------------

    fn look() -> MaterialLook {
        MaterialLook::new(Quality::High, 0.0)
    }

    #[test]
    fn every_palette_key_resolves_to_its_own_parameters() {
        let look = look();
        assert_eq!(look.keys().len(), Palette::ALL.len());

        let cream = look.key("plaster_cream").expect("the palette carries it");
        let sand = look.key("plaster_sand").expect("the palette carries it");
        assert_eq!(cream.library_name, "plaster");
        assert_eq!(sand.library_name, "plaster");

        let cream_params = cream
            .surface
            .kind()
            .material_params()
            .expect("a runtime material");
        let sand_params = sand
            .surface
            .kind()
            .material_params()
            .expect("a runtime material");
        // One bake, two materials: the whole point of the facade's two keys.
        assert_ne!(cream_params.tint, sand_params.tint);
        assert_eq!(cream_params.scale, 2.35, "the palette's own metres-per-tile");
        assert_eq!(sand_params.scale, 2.1);
        assert_eq!(cream_params.ground_y, 0.0);
        assert!(cream.vertex_colors, "the palette asks for vertex masks");
        assert!(look.key("no_such_key").is_none());
    }

    /// **One pipeline, many parameter regions.**
    ///
    /// This asserted `surfaces().len() == 1` and called it "a runtime material's
    /// parameters are not in its digest". The premise was right and the
    /// conclusion was the bug: the digest is the PROGRAM key and correctly
    /// excludes parameter values, but it was being used for the PARAMETER
    /// REGION too, so all forty-six palette keys collapsed onto one block and
    /// concrete, brick, metal, glass and asphalt every one of them shaded as
    /// whichever survived.
    ///
    /// `Surface::param_key` separates the two. Both halves are pinned here,
    /// because either alone is satisfiable by a regression: many digests would
    /// mean many pipelines, and one region would mean the original bug.
    #[test]
    fn the_whole_palette_costs_one_pipeline_and_a_region_per_key() {
        let look = look();
        let digests: std::collections::BTreeSet<u64> = look
            .keys()
            .iter()
            .map(|(_, k)| k.surface.digest().raw())
            .collect();
        assert_eq!(digests.len(), 1, "every runtime material is ONE program");
        assert!(
            look.surfaces().len() > 40,
            "{} parameter regions for {} palette keys — the regions collapsed",
            look.surfaces().len(),
            look.keys().len()
        );
    }

    #[test]
    fn parallax_and_detiling_are_forced_off_for_every_key() {
        let look = look();
        look.keys().iter().for_each(|(key, entry)| {
            let params = entry
                .surface
                .kind()
                .material_params()
                .expect("a runtime material");
            assert_eq!(params.parallax, 0.0, "{key} marches an unbound height map");
            assert!(!params.detile_enabled(), "{key} compiles a second pipeline");
        });
    }

    #[test]
    fn the_practicals_carry_the_emissive_the_level_currently_drops() {
        let look = look();
        // `window_glow`: 0xffb066 at intensity 1.1. Only the red channel
        // saturates, so the warm hue survives the `Ratio` clamp.
        let glow = look
            .key("window_glow")
            .expect("the palette carries it")
            .emissive
            .expect("a practical emits")
            .to_array();
        assert_eq!(glow[0], 1.0, "0xff * 1.1 clips");
        assert!(glow[0] > glow[1] && glow[1] > glow[2], "warm: {glow:?}");

        // `emissive_warm`: 0xffd39a at intensity 12.0. Every channel clips, so
        // the practical reads as flat white instead of a bloomed warm lamp —
        // the limit this module's `emissive_color` doc names.
        let warm = look
            .key("emissive_warm")
            .expect("the palette carries it")
            .emissive
            .expect("a practical emits")
            .to_array();
        assert_eq!([warm[0], warm[1], warm[2]], [1.0, 1.0, 1.0]);

        // A key with no `three` block emits nothing and is fully opaque.
        let cream = look.key("plaster_cream").expect("the palette carries it");
        assert_eq!(cream.emissive, None);
        assert_eq!(cream.opacity.get(), 1.0);
        assert!(!cream.transparent);
    }

    #[test]
    fn a_zero_intensity_practical_is_black_rather_than_absent() {
        let look = look();
        // `lamp_lens` sets `emissiveIntensity: 0.0`: the emissive is still
        // THERE, multiplied to black, rather than dropped.
        let lens = look.key("lamp_lens").expect("the palette carries it");
        let emissive = lens.emissive.expect("the key has an emissive").to_array();
        assert_eq!([emissive[0], emissive[1], emissive[2]], [0.0, 0.0, 0.0]);
        assert_eq!(lens.opacity.get(), 0.5);
        // `glass`'s library entry carries `three.transparent: true`.
        assert!(lens.transparent);
    }

    #[test]
    fn the_scratch_release_timer_finally_runs() {
        let mut look = look();
        assert!(!look.system.scratch_freed());
        // `index.js:166-172` frees the scratch height targets after five idle
        // seconds. Nothing held the system that long before.
        (0..360).for_each(|_| look.frame(1.0 / 60.0));
        assert!(look.system.scratch_freed());
    }

    #[test]
    fn a_short_parameter_array_falls_back_element_by_element() {
        // `window_glass` passes `roughness: [0.3, 0.06]`, two elements, and the
        // source reads `p.roughness[2] ?? DEFAULT_PARAMS.roughness[2]`.
        let d = MaterialParams::default();
        assert_eq!(fill::<3>(&[0.3, 0.06], d.roughness), [0.3, 0.06, d.roughness[2]]);
        assert_eq!(fill::<3>(&[], d.roughness), d.roughness);
        assert_eq!(fill::<2>(&[1.0, 2.0, 3.0], [9.0, 9.0]), [1.0, 2.0]);
    }

    #[test]
    fn an_unrecognised_uv_mode_is_planar_as_the_source_treats_it() {
        assert_eq!(uv_mode("planar"), UvMode::Planar);
        assert_eq!(uv_mode("triplanar"), UvMode::Triplanar);
        assert_eq!(uv_mode("mesh"), UvMode::Mesh);
        assert_eq!(uv_mode("Triplanar"), UvMode::Planar);
    }

    #[test]
    fn the_palette_opts_use_the_source_s_camel_case_keys() {
        let entry = Palette::ALL
            .iter()
            .find(|(key, _)| *key == "plaster_cream")
            .map(|(_, entry)| *entry)
            .expect("the palette carries it");
        let opts = palette_opts(entry);
        // A snake_case key would be silently ignored by `apply_to_params`.
        assert_eq!(opts.get("vertexMasks"), Some(&OptValue::Bool(true)));
        assert!(opts.get("vertex_masks").is_none());
        assert_eq!(opts.get("tint"), Some(&OptValue::Num(f64::from(0xcf_c0a4u32))));
        assert!(matches!(opts.get("weather"), Some(OptValue::Arr(v)) if v.len() == 4));
    }

    #[test]
    fn srgb_decoding_matches_three_at_both_ends_of_the_knee() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-12);
        // Below the knee the transfer is the linear segment.
        assert!((srgb_to_linear(0.04) - 0.04 / 12.92).abs() < 1e-12);
        assert!(srgb_to_linear(0.5) < 0.5, "sRGB decoding darkens midtones");
    }
}

/// The registry face of [`SkyDriver`] — `sky/index.js:126`.
///
/// Same two-phase shape as [`crate::world::system::WorldSubsystem`], and for the
/// same reason: [`Subsystem::init`] is where a system may touch `ctx.rng`, so
/// construction has to be cheap and empty. See that type for the full argument.
///
/// **This one takes no fork**, and the emptiness is the point rather than an
/// oversight: `scene::game`'s init-order comment records that `materials` and
/// `sky` draw nothing from the root stream, which is why the pinned sequence
/// runs `world, weapons, fx, ai, ui, audio` with no slot between `start` and
/// `world`. A fork added here moves every subsequent slot and changes the level;
/// `crate::scene::game::tests::the_root_stream_is_consumed_in_the_registrys_order`
/// is what says so.
pub struct SkySubsystem {
    built: Option<SkyDriver>,
    quality: Quality,
    hour: f64,
}

impl SkySubsystem {
    /// An unbuilt sky at `quality` and time of day `hour`.
    pub const fn new(quality: Quality, hour: f64) -> Self {
        SkySubsystem {
            built: None,
            quality,
            hour,
        }
    }

    /// The built driver, or `None` before the registry has run `init`.
    pub const fn get(&self) -> Option<&SkyDriver> {
        self.built.as_ref()
    }

    /// The built driver, mutably.
    pub const fn get_mut(&mut self) -> Option<&mut SkyDriver> {
        self.built.as_mut()
    }
}

impl Subsystem for SkySubsystem {
    fn id(&self) -> &'static str {
        "sky"
    }

    /// `static deps = ['render', 'materials']` (`sky/index.js:127`).
    fn deps(&self) -> &'static [&'static str] {
        &["render", "materials"]
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Update]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// No `ctx.rng` here. See the type doc: the sky is one of the two slots the
    /// source's order includes and this port's root sequence does not.
    fn init(&mut self, _ctx: &Ctx<'_>) -> Result<(), crate::error::CoreError> {
        self.built = Some(SkyDriver::new(self.quality, self.hour));
        Ok(())
    }

    fn update(&mut self, dt: axiom_kernel::Seconds, _ctx: &Ctx<'_>) {
        let step = f64::from(dt.get());
        self.built
            .as_mut()
            .map(|sky| sky.frame(step, 0.0, (0.0, 0.0)))
            .unwrap_or_default();
    }
}

#[cfg(test)]
mod sky_subsystem_tests {
    use super::*;

    #[test]
    fn it_answers_to_the_id_and_the_sources_deps() {
        let sky = SkySubsystem::new(Quality::High, HOUR);
        assert_eq!(sky.id(), "sky");
        assert_eq!(sky.deps(), &["render", "materials"]);
        assert!(sky.get().is_none(), "an uninitialised sky is not built");
    }

    /// **The sky must not start forking.** The pinned root sequence has no sky
    /// slot; adding one moves every slot after it and rebuilds the world.
    #[test]
    fn construction_and_init_draw_nothing_from_the_root_stream() {
        let registry = crate::registry::Registry::new();
        let events = crate::events::EventBus::new();
        let time = crate::engine::Time::default();
        let config = Config::default();
        let rng = std::cell::RefCell::new(crate::rng::Rng::new(7));
        let input = std::cell::RefCell::new(crate::input::Input::new());
        let before = rng.borrow().state();
        let ctx = Ctx::over(&config, &events, &time, &rng, &input, &registry);
        let mut sky = SkySubsystem::new(Quality::High, HOUR);
        sky.init(&ctx).expect("the sky needs nothing to initialise");
        assert_eq!(
            rng.borrow().state(),
            before,
            "the sky drew from the root stream — every later slot just moved"
        );
    }
}
