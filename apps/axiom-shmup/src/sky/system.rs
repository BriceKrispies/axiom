//! Ported from Claude-of-Duty `src/sky/index.js:1-872` — the `SkySystem`
//! facade: weather and fog state, the shared uniform block, the sun/moon
//! ephemeris-to-lighting chain (`_updateCelestial`), the cloud-occlusion key
//! dimmer, and the per-frame drive.
//!
//! ## Everything here is CPU arithmetic, and it has a real oracle
//!
//! Unlike its siblings in this directory, `index.js` holds no GLSL at all.
//! Every number below comes out of plain JavaScript — `Math.sin`,
//! `transmittanceToSpace`, `MathUtils.lerp`/`smoothstep`/`clamp` — so
//! `tests/sky_system/capture.mjs` does not transcribe anything: it constructs
//! the **real** `SkySystem` against a stubbed WebGL surface and reads the real
//! fields back. That is the strongest oracle available anywhere in this
//! subsystem, and it is why this module is pinned value-for-value rather than
//! against a second reading of a shader.
//!
//! ## The photometric contract
//!
//! `1 unit = 25000 lx` — the source logs exactly that at the end of `init`
//! (`index.js:364-368`), and [`crate::sky::atmosphere::SCENE_LUX`] is that
//! number. Every intensity this module publishes is on that scale:
//! [`SkySystem::base_sun_intensity`] is
//! `SUN_ILLUMINANCE_TOP (= 128000/25000 = 5.12) * transmittance * discFraction
//! * beamGain`, and [`SkySystem::sun_light`]'s `intensity` is that times the
//! cloud dimmer times [`SUN_KEY_GAIN`]. **Nothing here applies an exposure
//! curve.** `exposure_bias` is published in EV and is *added to* whatever the
//! renderer's own bias is (`index.js:98-100`); it is a metering instruction,
//! not a multiplier this module has already applied. Any engine-side EV100
//! work must therefore treat these as scene-referred luminance/illuminance in
//! 25000-lx units and consume `exposure_bias` additively, or the sky will
//! double-count it. The atmosphere-side half of that contract, including the
//! stray-pi bug that once put the sky 1.65 stops over-bright, is in
//! [`crate::sky::atmosphere`]'s module doc.
//!
//! ## What is not ported
//!
//! The GPU object graph and nothing else:
//!
//! * `SkyLuts`/`SkyDome`/`Volumetrics`/`PMREMGenerator` construction and
//!   disposal, the equirect render target, and `render.setEnvMap`. The
//!   *maths* those objects run is already ported — [`crate::sky::luts`],
//!   [`crate::sky::dome`], [`crate::sky::volumetrics`] — so what is missing
//!   here is only the lifetime. [`SkySystem::bake_sky`] and
//!   [`SkySystem::bake_env`] keep the source's dirty/age bookkeeping exactly
//!   (that bookkeeping is what decides *when* a bake happens, and it is
//!   testable), and [`SkySystem::sky_view_params`] marshals the shared block
//!   into the [`crate::sky::luts::bake_sky_view`] argument the source's
//!   `SkyLuts.bakeSkyView` would read, so a caller that owns render targets
//!   has nothing left to re-derive.
//! * `ctx.scene.add(...)` / `render.addLight(...)` / `registerPass(...)`, and
//!   `dispose()`. Scene-graph membership, not computation.
//! * The `console.info` banner (`index.js:364-368`, and the `deterministic`
//!   trace at `index.js:406-417`).
//!
//! `fullscreen.js` — which `index.js` imports for `blit`/`hdrTarget` — is
//! audited separately in [`crate::sky::fullscreen`]; three pieces of it turned
//! out to be computation rather than plumbing.
//!
//! ## Events
//!
//! `index.js` emits `sky:changed` and `sky:env` on `ctx.events`. Payload types
//! are [`SkyChanged`] and [`SkyEnv`]. See [`SkyChanged`]'s doc for the
//! event-vocabulary note this port has to keep repeating.

use crate::config::Quality;
use crate::sky::atmosphere::{
    smoothstep, transmittance_to_space, Vec3, ATMO, MOON_ILLUMINANCE_NIGHT, SUN_ILLUMINANCE_TOP,
};
use crate::sky::celestial::{Celestial, Mat3, SITE};
use crate::sky::clouds::{cloud_sun_occlusion, SunOcclusionParams};
use crate::sky::luts::SkyViewParams;

/* ==================================================================== */
/* THREE.MathUtils                                                       */
/* ==================================================================== */

/// `THREE.MathUtils.RAD2DEG`. **`radians * (180 / PI)`, not
/// `radians * 180 / PI`** — the two differ in the last bits, and `altDeg`
/// feeds four `smoothstep` edges whose outputs multiply the key light.
const RAD2DEG: f64 = 180.0 / std::f64::consts::PI;

/// `THREE.MathUtils.lerp(x, y, t) = (1 - t) * x + t * y`.
///
/// **Not** [`crate::sky::atmosphere::gl_mix`], which is GLSL `mix`'s
/// `a + (b - a) * t`. Those are different floating-point expressions and this
/// module is pinned bit-for-bit against three's, so the two must not be
/// confused. `index.js` uses `MathUtils.lerp` six times and GLSL `mix` zero.
fn three_lerp(x: f64, y: f64, t: f64) -> f64 {
    (1.0 - t) * x + t * y
}

/// `THREE.MathUtils.clamp(v, min, max) = Math.max(min, Math.min(max, v))`.
fn three_clamp(v: f64, min: f64, max: f64) -> f64 {
    min.max(max.min(v))
}

/// `THREE.MathUtils.smoothstep(x, min, max)`.
///
/// Argument order is `(x, edge0, edge1)` — the *reverse* of GLSL's
/// `smoothstep(edge0, edge1, x)`. The body is otherwise identical (three's
/// early `<= min` / `>= max` returns give the same 0 and 1 the clamped
/// polynomial does), so this forwards to
/// [`crate::sky::atmosphere::smoothstep`] with the arguments flipped rather
/// than growing a second copy of the formula.
fn three_smoothstep(x: f64, min: f64, max: f64) -> f64 {
    smoothstep(min, max, x)
}

/// `Vector3.normalize()` — `divideScalar(length() || 1)`. The `|| 1` is why
/// this is not just `Vec3::normalize`: a zero vector normalizes to itself in
/// three and to `NaN` in the plain form.
fn three_normalize(v: Vec3) -> Vec3 {
    v.scale(1.0 / crate::jsmath::or_one(v.length()))
}

/* ==================================================================== */
/* module constants — index.js:17-55                                     */
/* ==================================================================== */

/// `SUN_LUM_FLOOR`, `index.js:24`. Floor on the beam's *luminous*
/// transmittance as a fraction of unity.
pub const SUN_LUM_FLOOR: f64 = 0.35;

/// `SUN_KEY_GAIN`, `index.js:45`. Gain on the sun's directional light only —
/// deliberately **not** on the irradiance the atmosphere scatters, and not on
/// the sky. See the source's comment: it is paying for level albedos that are
/// 1.1 stops darker than the photometric model assumes.
pub const SUN_KEY_GAIN: f64 = 1.55;

/// `SKY_AMBIENT_FRACTION`, `index.js:52`. Whole-sky diffuse illuminance as a
/// fraction of the beam.
pub const SKY_AMBIENT_FRACTION: f64 = 0.15;

/// `NIGHT_AMBIENT_HUE`, `index.js:55`. Moonlight after the Purkinje shift.
pub const NIGHT_AMBIENT_HUE: [f64; 3] = [0.35, 0.5, 1.0];

/// `const discRad = 4000`, `index.js:630`. The solar disc's true radiance
/// (E/omega ~ 75000 units) overflows a half-float, so it is clamped here.
pub const SUN_DISC_RADIANCE: f64 = 4000.0;

/// `const tint = [1.0, 0.975, 0.94]`, `index.js:565` — the solar spectrum is
/// a touch warm of D65 even before the atmosphere.
pub const SUN_TINT: [f64; 3] = [1.0, 0.975, 0.94];

/// `const cool = [0.66, 0.80, 1.0]`, `index.js:649`.
pub const MOON_COOL: [f64; 3] = [0.66, 0.80, 1.0];

/* ==================================================================== */
/* event payloads                                                        */
/* ==================================================================== */

// NOTE FOR THE INTEGRATION PASS, the same one `crate::ui::system` writes at
// its own payload block: `EventBus` payloads cross as `&dyn Any` and a handler
// downcasts to ONE concrete type, so there must be exactly one payload type
// per event name across the whole game. `sky:changed` and `sky:env` are new
// names — no other ported subsystem declares them, and neither
// `crate::audio::system` nor `crate::ui::system` (the two that have declared
// payloads so far, and which already fork six event names between them) has
// anything to reuse here. These two therefore add to the vocabulary rather
// than forking it.

/// `ctx.events.emit('sky:changed', {...})`, `index.js:400-405`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyChanged {
    pub hour: f64,
    pub sun_dir: Vec3,
    pub sun_intensity: f64,
    pub moon_intensity: f64,
}

/// `ctx.events.emit('sky:env', {...})`, `index.js:853`. The source's payload
/// carries `envMap` (a `THREE.Texture`); this port has no GPU texture, so the
/// payload is the sun direction plus a monotonically increasing bake counter,
/// which is what a listener actually keys off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyEnv {
    pub sun_dir: Vec3,
    /// How many times [`SkySystem::bake_env`] has run. `envMap` identity in
    /// the source; a generation number here.
    pub env_generation: u64,
}

/* ==================================================================== */
/* weather / fog                                                         */
/* ==================================================================== */

/// `this.weather`, `index.js:141-164`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weather {
    /// Aerosol multiplier. 1 clear, 2-3 hazy, 5 dust storm.
    pub turbidity: f64,
    pub cloud_coverage: f64,
    pub cloud_density: f64,
    pub cirrus_coverage: f64,
    pub cirrus_opacity: f64,
    /// km/s at the cloud deck.
    pub wind_speed: f64,
    pub wind_angle: f64,
    pub horizon_murk: f64,

    // ---- the `Object.assign` spill ------------------------------------
    // `setWeather` does `Object.assign(this.weather, patch)` FIRST
    // (`index.js:428`) and only then pulls `fogDensity`/`fogHeight`/
    // `shaftGain` out of the patch. So those three keys land on
    // `this.weather` as well, even though nothing ever reads them back off
    // it. They are modelled rather than dropped because they are observable
    // (`{...sky.weather}` shows them), and because dropping "a field nobody
    // reads" is how a port quietly stops being diffable.
    /// Present only once a patch has set it. Never read by the source.
    pub fog_density: Option<f64>,
    /// Present only once a patch has set it. Never read by the source.
    pub fog_height: Option<f64>,
    /// Present only once a patch has set it. Never read by the source.
    pub shaft_gain: Option<f64>,
}

impl Default for Weather {
    /// The literal at `index.js:141-164`.
    fn default() -> Self {
        Weather {
            turbidity: 1.35,
            cloud_coverage: 0.30,
            cloud_density: 1.9,
            cirrus_coverage: 0.21,
            cirrus_opacity: 0.30,
            wind_speed: 0.0042,
            wind_angle: 0.7,
            horizon_murk: 0.13,
            fog_density: None,
            fog_height: None,
            shaft_gain: None,
        }
    }
}

/// `setWeather`'s `patch` — every field optional, exactly as the JS object
/// literal is. `fog_density`/`fog_height`/`shaft_gain` are patch-only knobs
/// that rewrite [`Fog`] (`index.js:429-435`); the rest land on [`Weather`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeatherPatch {
    pub turbidity: Option<f64>,
    pub cloud_coverage: Option<f64>,
    pub cloud_density: Option<f64>,
    pub cirrus_coverage: Option<f64>,
    pub cirrus_opacity: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_angle: Option<f64>,
    pub horizon_murk: Option<f64>,
    pub fog_density: Option<f64>,
    pub fog_height: Option<f64>,
    pub shaft_gain: Option<f64>,
}

/// `this._fog`, `index.js:171-221`. `scatter` and `extinction` are
/// intentionally independent — see the source's note.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fog {
    /// 1/m at the fog base.
    pub scatter: f64,
    /// 1/m at the fog base.
    pub extinction: f64,
    pub height_scale: f64,
    pub base_y: f64,
    pub max_distance: f64,
    /// Inscatter gain on the key light; above 1 this is not physical and the
    /// source says so at length.
    pub shaft_gain: f64,
    pub ambient_gain: f64,
    pub noise: f64,
    pub noise_scale: f64,
    pub phase_forward: f64,
    pub phase_backward: f64,
    pub phase_back_weight: f64,
    /// Blue-biased so distant geometry loses red first, as Rayleigh does.
    pub extinction_tint: Vec3,
}

impl Default for Fog {
    /// The literal at `index.js:171-221`.
    fn default() -> Self {
        Fog {
            scatter: 3.6e-3,
            extinction: 1.45e-3,
            height_scale: 18.0,
            base_y: -2.0,
            max_distance: 900.0,
            shaft_gain: 2.6,
            ambient_gain: 0.22,
            noise: 0.55,
            noise_scale: 0.045,
            phase_forward: 0.76,
            phase_backward: -0.36,
            phase_back_weight: 0.34,
            extinction_tint: Vec3::new(0.94, 1.02, 1.24),
        }
    }
}

/* ==================================================================== */
/* the shared uniform block                                              */
/* ==================================================================== */

/// `this.shared`, `index.js:226-286` — the one object every pass and the dome
/// reference, so one write per frame updates the whole subsystem. Here it is
/// plain values; in the source each field is a `{ value }` box shared by
/// reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shared {
    pub mie_scale: f64,
    pub view_pos: Vec3,

    pub sun_dir: Vec3,
    pub moon_dir: Vec3,
    pub sun_irradiance: Vec3,
    pub moon_irradiance: Vec3,
    pub sun_disc_radiance: Vec3,
    pub moon_disc_radiance: Vec3,
    pub sun_altitude: f64,
    pub moon_altitude: f64,
    pub moon_rel_az: f64,
    /// x/y are the true angular radii of the sun and moon; z/w scale them up
    /// for readability (`index.js:245`).
    pub disc: [f64; 4],
    pub ground_albedo: Vec3,
    pub horizon_murk: f64,
    /// Sky highlight roll-off: `(knee in scene radiance, overshoot room)`.
    pub sky_rolloff: (f64, f64),

    pub star_params: [f64; 4],
    pub celestial: Mat3,

    pub cloud_params: [f64; 4],
    pub cloud_params2: [f64; 4],

    /// `uInvProj` — three's `Matrix4.elements`, **column-major**.
    pub inv_proj: [f64; 16],
    /// `uCamWorld` — three's `Matrix4.elements`, **column-major**.
    pub cam_world: [f64; 16],
    pub cam_pos: Vec3,
    pub fog: [f64; 4],
    pub fog2: [f64; 4],
    pub fog_ext: Vec3,
    pub phase: [f64; 4],
    pub key_dir: Vec3,
    pub key_irr: Vec3,
    pub fog_drift: Vec3,
}

impl Shared {
    /// The literal at `index.js:226-286`, with `weather` supplying the four
    /// fields the source reads out of it there.
    fn new(weather: &Weather) -> Self {
        let view_r = ATMO.ground_radius_mm + ATMO.view_altitude_mm;
        Shared {
            mie_scale: weather.turbidity,
            view_pos: Vec3::new(0.0, view_r, 0.0),
            sun_dir: Vec3::new(0.0, 1.0, 0.0),
            moon_dir: Vec3::new(0.0, -1.0, 0.0),
            sun_irradiance: Vec3::splat(0.0),
            moon_irradiance: Vec3::splat(0.0),
            sun_disc_radiance: Vec3::splat(0.0),
            moon_disc_radiance: Vec3::splat(0.0),
            sun_altitude: 0.0,
            moon_altitude: 0.0,
            moon_rel_az: 0.0,
            disc: [0.004654, 0.004516, 3.0, 4.2],
            ground_albedo: Vec3::new(0.33, 0.29, 0.225),
            horizon_murk: weather.horizon_murk,
            sky_rolloff: (0.30, 1.5),
            star_params: [0.0, 0.5, 0.0, 0.0],
            celestial: Mat3::identity(),
            cloud_params: [weather.cloud_coverage, weather.cloud_density, 1.0, 0.0],
            cloud_params2: [weather.cirrus_coverage, weather.cirrus_opacity, 0.004, 0.0016],
            inv_proj: IDENTITY4,
            cam_world: IDENTITY4,
            cam_pos: Vec3::splat(0.0),
            fog: [0.0; 4],
            fog2: [0.0; 4],
            fog_ext: Vec3::splat(0.0),
            phase: [0.0; 4],
            key_dir: Vec3::new(0.0, 1.0, 0.0),
            key_irr: Vec3::splat(0.0),
            fog_drift: Vec3::splat(0.0),
        }
    }
}

/// `new THREE.Matrix4()` — the identity, column-major.
const IDENTITY4: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/* ==================================================================== */
/* lights                                                                */
/* ==================================================================== */

/// The CPU-visible half of a `THREE.DirectionalLight`. `color` is linear
/// working-space RGB; `intensity` is on the [`crate::sky::atmosphere::SCENE_LUX`]
/// scale (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirLight {
    pub color: Vec3,
    pub intensity: f64,
    pub position: Vec3,
    /// `light.target.position` — always the origin here (`index.js:803`).
    pub target: Vec3,
}

/// Which light the renderer's cascades follow — `this.keyLight`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLight {
    Sun,
    Moon,
}

/* ==================================================================== */
/* the system                                                            */
/* ==================================================================== */

/// `class SkySystem`, `index.js:125-872`.
pub struct SkySystem {
    pub celestial: Celestial,
    /// `this.hour`, 0..24 local solar time.
    pub hour: f64,
    /// `this.timeRate` — hours of sky time per second of wall clock.
    pub time_rate: f64,

    pub weather: Weather,
    /// `this._fog`; the source exposes it as the `fog` getter.
    pub fog: Fog,
    pub shared: Shared,

    pub sun_light: DirLight,
    pub moon_light: DirLight,
    pub key_light: KeyLight,

    /// Approximate whole-sky tint AND level; the renderer scales its sky-fill
    /// band off it (`index.js:90-93`).
    pub ambient_color: Vec3,
    /// Indirect-light budget for this sun elevation: ~0.45 at golden hour, 1
    /// by day, 2.2 after dark.
    pub indirect_scale: f64,
    /// EV of metering compensation for this sun elevation; `+` is darker.
    pub exposure_bias: f64,

    /// `Math.max(8, steps)` — what `Volumetrics` is handed (`index.js:330-335`).
    pub volumetric_steps: u32,

    beam_gain: f64,
    beam_luminance: f64,
    base_sun_intensity: f64,
    sun_t: [f64; 3],
    moon_t: [f64; 3],
    env_sun_dir: Vec3,
    cloud_occlusion: f64,
    cloud_occ_target: f64,
    env_age: f64,
    sky_dirty: bool,
    env_dirty: bool,
    cloud_time: f64,
    sky_generation: u64,
    env_generation: u64,
}

impl SkySystem {
    /// `async init(ctx)`, `index.js:129-369`, minus the GPU object graph and
    /// the banner. `quality` is `ctx.config.quality`; its preset supplies the
    /// `volumetrics`/`ssr` flags the step count reads.
    pub fn new(quality: Quality) -> Self {
        let weather = Weather::default();
        let fog = Fog::default();
        let shared = Shared::new(&weather);

        // `const steps = q.volumetrics ? (quality === 'ultra' ? 56 : q.ssr ? 44
        // : 28) : 0;` then `steps: Math.max(8, steps)`. The 8 only ever applies
        // on the `volumetrics: false` branch, where the pass is analytic anyway.
        let q = quality.preset();
        let raw_steps = if q.volumetrics {
            if quality == Quality::Ultra {
                56
            } else if q.ssr {
                44
            } else {
                28
            }
        } else {
            0
        };

        let mut sky = SkySystem {
            celestial: Celestial::new(SITE),
            hour: 16.5,
            time_rate: 0.0,
            weather,
            fog,
            shared,
            // `new THREE.DirectionalLight(0xffffff, 4.0)`. Both colours are
            // overwritten by `_updateCelestial` before anything reads them —
            // `setTimeOfDay` runs at the end of `init` — so the moon's
            // `0x9fc0ff` (which three would sRGB-decode on the way in) never
            // reaches an observer and is not modelled.
            sun_light: DirLight {
                color: Vec3::new(1.0, 1.0, 1.0),
                intensity: 4.0,
                position: Vec3::splat(0.0),
                target: Vec3::splat(0.0),
            },
            moon_light: DirLight {
                color: Vec3::new(1.0, 1.0, 1.0),
                intensity: 0.0,
                position: Vec3::splat(0.0),
                target: Vec3::splat(0.0),
            },
            key_light: KeyLight::Sun,
            ambient_color: Vec3::splat(0.0),
            indirect_scale: 1.0,
            exposure_bias: 0.0,
            volumetric_steps: raw_steps.max(8),
            beam_gain: 1.0,
            beam_luminance: 0.0,
            base_sun_intensity: 0.0,
            sun_t: [0.0; 3],
            moon_t: [0.0; 3],
            env_sun_dir: Vec3::new(0.0, -1.0, 0.0),
            cloud_occlusion: 1.0,
            cloud_occ_target: 1.0,
            env_age: 1e9,
            sky_dirty: true,
            env_dirty: true,
            cloud_time: 0.0,
            sky_generation: 0,
            env_generation: 0,
        };

        sky.apply_weather();
        sky.apply_fog();
        sky.set_time_of_day(16.5);
        sky
    }

    /* ---------------------------------------------------------------- */
    /* public API — index.js:371-455                                     */
    /* ---------------------------------------------------------------- */

    /// `get timeOfDay`.
    pub fn time_of_day(&self) -> f64 {
        self.hour
    }
    /// `get sunDirection` — points AT the sun.
    pub fn sun_direction(&self) -> Vec3 {
        self.celestial.sun
    }
    /// `get moonDirection` — points AT the moon.
    pub fn moon_direction(&self) -> Vec3 {
        self.celestial.moon
    }
    /// `get sunAltitude` — radians above the horizon.
    pub fn sun_altitude(&self) -> f64 {
        self.celestial.sun_alt
    }
    /// `this._baseSunIntensity` — the key before the cloud dimmer and
    /// [`SUN_KEY_GAIN`]. Published because the ambient level and the rolloff
    /// knee are both derived from it.
    pub fn base_sun_intensity(&self) -> f64 {
        self.base_sun_intensity
    }
    /// `this._beamLuminance` — luminous beam level in scene units.
    pub fn beam_luminance(&self) -> f64 {
        self.beam_luminance
    }
    /// `this._beamGain`.
    pub fn beam_gain(&self) -> f64 {
        self.beam_gain
    }
    /// `this._sunT` — transmittance to space along the solar beam.
    pub fn sun_transmittance(&self) -> [f64; 3] {
        self.sun_t
    }
    /// `this._moonT`.
    pub fn moon_transmittance(&self) -> [f64; 3] {
        self.moon_t
    }
    /// `this._cloudOcclusion` — the eased cloud dimmer, 0..1.
    pub fn cloud_occlusion(&self) -> f64 {
        self.cloud_occlusion
    }
    /// `this._cloudTime`.
    pub fn cloud_time(&self) -> f64 {
        self.cloud_time
    }
    pub fn sky_dirty(&self) -> bool {
        self.sky_dirty
    }
    pub fn env_dirty(&self) -> bool {
        self.env_dirty
    }
    /// How many times [`SkySystem::bake_env`] has run.
    pub fn env_generation(&self) -> u64 {
        self.env_generation
    }
    /// How many times [`SkySystem::bake_sky`] has run.
    pub fn sky_generation(&self) -> u64 {
        self.sky_generation
    }

    /// `setTimeOfDay(hours)`, `index.js:392-419`. Hour of day, 0..24 local
    /// solar time; rebakes the sky and the IBL. Returns the `sky:changed`
    /// payload the source emits.
    pub fn set_time_of_day(&mut self, hours: f64) -> SkyChanged {
        // `((hours % 24) + 24) % 24` — the double modulo, because JS `%` (and
        // Rust `%`) keeps the sign of the dividend.
        self.hour = ((hours % 24.0) + 24.0) % 24.0;
        self.sky_dirty = true;
        self.env_dirty = true;
        self.update_celestial();
        self.bake_sky();
        self.bake_env();
        SkyChanged {
            hour: self.hour,
            sun_dir: self.celestial.sun,
            sun_intensity: self.sun_light.intensity,
            moon_intensity: self.moon_light.intensity,
        }
    }

    /// `setTimeRate(hoursPerSecond)`, `index.js:422-425`. 0 freezes the sun.
    pub fn set_time_rate(&mut self, hours_per_second: f64) {
        // `this.timeRate = hoursPerSecond || 0`. Both `±0` and `NaN` are falsy
        // in JS, so a NaN rate becomes a frozen sun rather than poisoning the
        // hour — the same quirk `crate::jsmath::or_one` documents for `|| 1`.
        self.time_rate = if hours_per_second == 0.0 || hours_per_second.is_nan() {
            0.0
        } else {
            hours_per_second
        };
    }

    /// `setWeather(patch)`, `index.js:427-443`. Returns `true` when the patch
    /// touched `turbidity`, i.e. when the source re-runs `luts.bakeStatic()`
    /// (turbidity is baked into all three LUTs).
    pub fn set_weather(&mut self, patch: &WeatherPatch) -> bool {
        // `Object.assign(this.weather, patch)` — every present key, including
        // the three fog-only ones. See `Weather`'s spill note.
        let w = &mut self.weather;
        if let Some(v) = patch.turbidity {
            w.turbidity = v;
        }
        if let Some(v) = patch.cloud_coverage {
            w.cloud_coverage = v;
        }
        if let Some(v) = patch.cloud_density {
            w.cloud_density = v;
        }
        if let Some(v) = patch.cirrus_coverage {
            w.cirrus_coverage = v;
        }
        if let Some(v) = patch.cirrus_opacity {
            w.cirrus_opacity = v;
        }
        if let Some(v) = patch.wind_speed {
            w.wind_speed = v;
        }
        if let Some(v) = patch.wind_angle {
            w.wind_angle = v;
        }
        if let Some(v) = patch.horizon_murk {
            w.horizon_murk = v;
        }
        if patch.fog_density.is_some() {
            w.fog_density = patch.fog_density;
        }
        if patch.fog_height.is_some() {
            w.fog_height = patch.fog_height;
        }
        if patch.shaft_gain.is_some() {
            w.shaft_gain = patch.shaft_gain;
        }

        if let Some(k) = patch.fog_density {
            self.fog.scatter = 3.6e-3 * k;
            self.fog.extinction = 1.45e-3 * k;
        }
        if let Some(v) = patch.fog_height {
            self.fog.height_scale = v;
        }
        if let Some(v) = patch.shaft_gain {
            self.fog.shaft_gain = v;
        }
        self.apply_weather();
        self.apply_fog();
        self.sky_dirty = true;
        self.env_dirty = true;
        patch.turbidity.is_some()
    }

    /// `cloudShadowAt(x, z)`, `index.js:446-455`. Fraction of direct sunlight
    /// reaching a ground point through the clouds.
    pub fn cloud_shadow_at(&self, x: f64, z: f64) -> f64 {
        let p = SunOcclusionParams {
            coverage: self.weather.cloud_coverage,
            density: self.weather.cloud_density,
            wind_x: self.shared.cloud_params2[2],
            wind_z: self.shared.cloud_params2[3],
            time: self.cloud_time,
        };
        cloud_sun_occlusion(x, z, self.celestial.sun, &p)
    }

    /* ---------------------------------------------------------------- */
    /* frame — index.js:457-506                                          */
    /* ---------------------------------------------------------------- */

    /// `update(dt, ctx)`, `index.js:461-494`. `elapsed` is `ctx.time.elapsed`
    /// and `camera_xz` is `(ctx.camera.position.x, ctx.camera.position.z)` —
    /// the only two things the source reads off the frame context here.
    pub fn update(&mut self, dt: f64, elapsed: f64, camera_xz: (f64, f64)) {
        // Cloud drift is deterministic (driven by elapsed) so capture mode
        // reproduces the exact same sky every run.
        self.cloud_time = elapsed;
        self.shared.cloud_params[3] = self.cloud_time;
        self.shared.star_params[2] = self.cloud_time;
        self.shared.fog_drift = Vec3::new(self.cloud_time * 0.09, self.cloud_time * 0.015, self.cloud_time * 0.045);

        if self.time_rate != 0.0 {
            self.hour = (self.hour + self.time_rate * dt) % 24.0;
            self.update_celestial();
        }

        self.cloud_occ_target = self.cloud_shadow_at(camera_xz.0, camera_xz.1);
        let k = (dt * 0.9).min(1.0);
        self.cloud_occlusion += (self.cloud_occ_target - self.cloud_occlusion) * k;
        self.apply_light_intensities();

        if self.sky_dirty {
            self.bake_sky();
        }

        self.env_age += dt;
        if self.env_dirty && self.env_age > 0.2 {
            self.bake_env();
        }
    }

    /// `lateUpdate(dt, ctx)`, `index.js:496-506`. Both matrices are three's
    /// `Matrix4.elements` — **column-major**, so the camera position is
    /// `[12], [13], [14]` (`setFromMatrixPosition`).
    pub fn late_update(&mut self, projection_matrix_inverse: [f64; 16], camera_matrix_world: [f64; 16]) {
        self.shared.inv_proj = projection_matrix_inverse;
        self.shared.cam_world = camera_matrix_world;
        self.shared.cam_pos = Vec3::new(camera_matrix_world[12], camera_matrix_world[13], camera_matrix_world[14]);
    }

    /* ---------------------------------------------------------------- */
    /* internals — index.js:508-854                                      */
    /* ---------------------------------------------------------------- */

    /// `_applyWeather()`, `index.js:512-524`.
    fn apply_weather(&mut self) {
        let w = self.weather;
        self.shared.mie_scale = w.turbidity;
        self.shared.horizon_murk = w.horizon_murk;
        self.shared.cloud_params[0] = w.cloud_coverage;
        self.shared.cloud_params[1] = w.cloud_density;
        self.shared.cloud_params2[0] = w.cirrus_coverage;
        self.shared.cloud_params2[1] = w.cirrus_opacity;
        self.shared.cloud_params2[2] = w.wind_angle.cos() * w.wind_speed;
        self.shared.cloud_params2[3] = w.wind_angle.sin() * w.wind_speed;
    }

    /// `_applyFog()`, `index.js:526-537`.
    fn apply_fog(&mut self) {
        let f = self.fog;
        self.shared.fog = [f.scatter, 1.0 / f.height_scale, f.base_y, f.max_distance];
        self.shared.fog2 = [f.extinction, f.shaft_gain, f.ambient_gain, f.noise];
        self.shared.fog_ext = f.extinction_tint.scale(f.extinction);
        self.shared.phase = [f.phase_forward, f.phase_backward, f.phase_back_weight, f.noise_scale];
    }

    /// `_updateCelestial()`, `index.js:540-794`. Sun/moon geometry, colours
    /// and intensities for the current hour.
    ///
    /// Transcribed statement for statement: every `lerp`/`smoothstep`/`clamp`
    /// keeps three's argument order and every product keeps the source's
    /// grouping, because float arithmetic is not associative and this whole
    /// function is pinned bit-for-bit.
    fn update_celestial(&mut self) {
        let hour = self.hour;
        self.celestial.set_hour(hour);
        let c = &self.celestial;
        let (sun, moon) = (c.sun, c.moon);
        let (sun_alt, moon_alt, sun_az, moon_az) = (c.sun_alt, c.moon_alt, c.sun_az, c.moon_az);
        let moon_phase = c.moon_phase;
        let celestial_matrix = c.celestial_matrix();

        self.shared.sun_dir = sun;
        self.shared.moon_dir = moon;
        self.shared.sun_altitude = sun_alt;
        self.shared.moon_altitude = moon_alt;
        // The sky-view LUT is baked with the sun at azimuth 0, so the moon
        // only needs its azimuth *relative* to the sun.
        let mut rel = moon_az - sun_az;
        while rel > std::f64::consts::PI {
            rel -= 2.0 * std::f64::consts::PI;
        }
        while rel < -std::f64::consts::PI {
            rel += 2.0 * std::f64::consts::PI;
        }
        self.shared.moon_rel_az = rel;
        self.shared.celestial = celestial_matrix;

        let mie = self.weather.turbidity;

        // ---- sun ------------------------------------------------------
        let mu_s = sun_alt.sin();
        // Fraction of the solar disc above the horizon.
        let disc_s = three_clamp(0.5 + mu_s / (2.0 * 0.004654), 0.0, 1.0);
        self.sun_t = transmittance_to_space(mu_s.max(0.0008), mie);
        let tint = SUN_TINT;
        let t = self.sun_t;
        // The key is the disc PLUS its aureole: raising the transmittance to a
        // power below one is the cheap monotonic way to say "disc convolved
        // with its aureole". Exponent 1 above 16 degrees.
        let aureole_p = three_lerp(0.55, 1.0, three_smoothstep(sun_alt * RAD2DEG, 0.0, 16.0));
        let sr = t[0].powf(aureole_p) * tint[0];
        let sg = t[1].powf(aureole_p) * tint[1];
        let sb = t[2].powf(aureole_p) * tint[2];
        let smax = 1e-6f64.max(sr).max(sg).max(sb);
        self.sun_light.color = Vec3::new(sr / smax, sg / smax, sb / smax);

        // ---- beam floor -----------------------------------------------
        // The hue stays exactly on the physical transmittance curve; only the
        // luminance is floored, for as long as any part of the disc can see
        // the scene.
        let lum_t = 0.2126 * sr + 0.7152 * sg + 0.0722 * sb;
        let alt_deg = sun_alt * RAD2DEG;
        // 1 while the disc still lights the street, 0 by 6 deg under.
        let beam_alive = three_smoothstep(alt_deg, -6.0, -1.0);
        let lum_floor = SUN_LUM_FLOOR * beam_alive;
        let beam_gain = (lum_floor / lum_t.max(1e-5)).max(1.0);
        self.beam_gain = beam_gain;
        self.base_sun_intensity = SUN_ILLUMINANCE_TOP * smax * disc_s * beam_gain;
        self.beam_luminance = SUN_ILLUMINANCE_TOP * (lum_t * beam_gain).max(1e-6) * disc_s;

        // Irradiance handed to the sky LUT is the *extraterrestrial* value.
        self.shared.sun_irradiance = Vec3::new(
            SUN_ILLUMINANCE_TOP * tint[0],
            SUN_ILLUMINANCE_TOP * tint[1],
            SUN_ILLUMINANCE_TOP * tint[2],
        );
        self.shared.sun_disc_radiance = Vec3::new(
            SUN_DISC_RADIANCE * tint[0],
            SUN_DISC_RADIANCE * tint[1],
            SUN_DISC_RADIANCE * tint[2],
        );

        // ---- night ramps ----------------------------------------------
        // Key handover, and the presentation ramp for stars/Milky Way/disc.
        let key_ramp = three_smoothstep(-alt_deg, -3.0, 5.0);
        let night_ramp = three_smoothstep(-alt_deg, 0.0, 9.0);

        // ---- moon ------------------------------------------------------
        let mu_m = moon_alt.sin();
        let disc_m = three_clamp(0.5 + mu_m / (2.0 * 0.004516), 0.0, 1.0);
        self.moon_t = transmittance_to_space(mu_m.max(0.0008), mie);
        let mt = self.moon_t;
        let cool = MOON_COOL;
        let mr = mt[0] * cool[0];
        let mg = mt[1] * cool[1];
        let mb = mt[2] * cool[2];
        let mmax = 1e-6f64.max(mr).max(mg).max(mb);
        self.moon_light.color = Vec3::new(mr / mmax, mg / mmax, mb / mmax);
        let mut moon_i = MOON_ILLUMINANCE_NIGHT * moon_phase * mmax * disc_m * key_ramp;
        // The renderer switches its own fallback sun back on if no foreign
        // directional light is brighter than 0.01, so keep a floor.
        if self.base_sun_intensity.max(moon_i) < 0.03 {
            moon_i = 0.03;
        }
        self.moon_light.intensity = moon_i;

        let moon_irr = MOON_ILLUMINANCE_NIGHT * moon_phase * key_ramp;
        self.shared.moon_irradiance = Vec3::new(moon_irr * cool[0], moon_irr * cool[1], moon_irr * cool[2]);

        let moon_disc = three_lerp(0.35, 3.5, night_ramp);
        self.shared.moon_disc_radiance = Vec3::new(moon_disc, moon_disc * 0.985, moon_disc * 0.95);

        // ---- ambient colour (published, not used for lighting) ---------
        // Must not go warm at night (normalising a dead beam's transmittance
        // gives pure sodium orange), so the warm swing is gated on beamAlive.
        let warm = (1.0 - three_smoothstep(alt_deg, 1.0, 22.0)) * beam_alive;
        let night = 1.0 - beam_alive;
        let nh = NIGHT_AMBIENT_HUE;
        let ar = three_lerp(three_lerp(0.36, nh[0], night), self.sun_light.color.x, warm);
        let ag = three_lerp(three_lerp(0.56, nh[1], night), self.sun_light.color.y, warm);
        let ab = three_lerp(three_lerp(1.0, nh[2], night), self.sun_light.color.z, warm);
        // The moon term is deliberately generous against the day term.
        let a_level = SKY_AMBIENT_FRACTION * self.base_sun_intensity + 0.9 * moon_i;
        self.ambient_color = Vec3::new(ar * a_level, ag * a_level, ab * a_level);

        // ---- sky shoulder ----------------------------------------------
        // The knee tracks the beam's luminance because autoexposure does, and
        // comes down as the sun does. Floored so the night sky is untouched.
        let knee_frac = three_lerp(0.045, 0.11, three_smoothstep(alt_deg, 2.0, 15.0));
        self.shared.sky_rolloff = ((knee_frac * self.beam_luminance).max(0.02 + 6.0 * moon_i), 0.34);

        // ---- exposure compensation for the time of day ------------------
        self.exposure_bias = 1.35 * (1.0 - three_smoothstep(alt_deg, 1.0, 13.0)) * beam_alive
            // ...and half a stop after dark.
            + 0.55 * (1.0 - beam_alive);

        // Released — and then some — once the beam is gone.
        self.indirect_scale = three_lerp(
            2.2,
            three_lerp(0.45, 1.0, three_smoothstep(alt_deg, 0.0, 14.0)),
            beam_alive,
        );

        // ---- stars -------------------------------------------------------
        self.shared.star_params[0] = 0.07 * night_ramp;
        self.shared.star_params[1] = 0.55;
        self.shared.star_params[3] = 0.16 * night_ramp;

        // ---- light transforms ---------------------------------------------
        // Clamp just above the horizon: a directional light at exactly 0
        // degrees degenerates the cascade fit.
        self.sun_light.position = place_light(sun, 0.006);
        self.sun_light.target = Vec3::splat(0.0);
        self.moon_light.position = place_light(moon, 0.026);
        self.moon_light.target = Vec3::splat(0.0);

        self.apply_light_intensities();
        self.sky_dirty = true;
        if self.env_sun_dir.dot(sun) < (0.35 * (std::f64::consts::PI / 180.0)).cos() {
            self.env_dirty = true;
        }
    }

    /// `_applyLightIntensities()`, `index.js:808-828`.
    fn apply_light_intensities(&mut self) {
        // A cloud crossing the sun dims the whole street, so the range stays
        // narrow: real broken cover swings about a stop, which 0.58..1.0 gives.
        let occ = 0.58 + 0.42 * self.cloud_occlusion;
        self.sun_light.intensity = self.base_sun_intensity * occ * SUN_KEY_GAIN;

        let sun_i = self.sun_light.intensity;
        let moon_i = self.moon_light.intensity;
        let moon_key = moon_i > sun_i;
        self.key_light = if moon_key { KeyLight::Moon } else { KeyLight::Sun };

        // The fog's key must be the light the renderer fitted its cascades to.
        let key = if moon_key { self.moon_light } else { self.sun_light };
        let dir = if moon_key { self.celestial.moon } else { self.celestial.sun };
        self.shared.key_dir = dir;
        let i = key.intensity;
        self.shared.key_irr = Vec3::new(key.color.x * i, key.color.y * i, key.color.z * i);
    }

    /// `_bakeSky()`, `index.js:830-834`. The LUT bake itself belongs to
    /// whoever owns render targets — see [`SkySystem::sky_view_params`] for
    /// the arguments it needs. What is ported is the dirty-flag bookkeeping,
    /// which is what decides *when* the bake runs.
    pub fn bake_sky(&mut self) {
        self.sky_dirty = false;
        self.sky_generation += 1;
    }

    /// `_bakeEnv()`, `index.js:836-854`. Returns the `sky:env` payload the
    /// source emits. As with [`SkySystem::bake_sky`], the equirect draw and
    /// the PMREM are the caller's; the bookkeeping is here.
    pub fn bake_env(&mut self) -> SkyEnv {
        self.env_generation += 1;
        self.env_sun_dir = self.celestial.sun;
        self.env_dirty = false;
        self.env_age = 0.0;
        SkyEnv {
            sun_dir: self.celestial.sun,
            env_generation: self.env_generation,
        }
    }

    /// The arguments `SkyLuts.bakeSkyView` reads out of the shared block
    /// (`luts.js:172-176`), marshalled for [`crate::sky::luts::bake_sky_view`].
    /// Not a function in the source — there the uniform objects are shared by
    /// reference, so the marshalling is free; here it has to be written down.
    pub fn sky_view_params(&self) -> SkyViewParams {
        SkyViewParams {
            sun_irradiance: self.shared.sun_irradiance,
            moon_irradiance: self.shared.moon_irradiance,
            sun_altitude: self.shared.sun_altitude,
            moon_rel_az: self.shared.moon_rel_az,
            moon_altitude: self.shared.moon_altitude,
            view_pos: self.shared.view_pos,
            mie_scale: self.shared.mie_scale,
        }
    }
}

/// `_placeLight(light, dir, minY)`, `index.js:796-806` — returns the light's
/// world position. `light.target.position` is always the origin, so the caller
/// sets that separately.
fn place_light(dir: Vec3, min_y: f64) -> Vec3 {
    let mut tmp = dir;
    if tmp.y < min_y {
        tmp.y = min_y;
        tmp = three_normalize(tmp);
    }
    tmp.scale(600.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_lands_on_the_documented_defaults() {
        let sky = SkySystem::new(Quality::High);
        assert_eq!(sky.hour, 16.5);
        assert_eq!(sky.time_rate, 0.0);
        assert_eq!(sky.weather.turbidity, 1.35);
        assert_eq!(sky.fog.scatter, 3.6e-3);
        // `setTimeOfDay` runs at the end of `init`, so both flags are clear.
        assert!(!sky.sky_dirty());
        assert!(!sky.env_dirty());
    }

    #[test]
    fn volumetric_steps_follow_the_quality_preset() {
        assert_eq!(SkySystem::new(Quality::Low).volumetric_steps, 8);
        assert_eq!(SkySystem::new(Quality::Medium).volumetric_steps, 28);
        assert_eq!(SkySystem::new(Quality::High).volumetric_steps, 44);
        assert_eq!(SkySystem::new(Quality::Ultra).volumetric_steps, 56);
    }

    #[test]
    fn set_time_of_day_wraps_both_ways() {
        let mut sky = SkySystem::new(Quality::High);
        sky.set_time_of_day(25.5);
        assert!((sky.hour - 1.5).abs() < 1e-12);
        sky.set_time_of_day(-3.25);
        assert!((sky.hour - 20.75).abs() < 1e-12);
    }

    #[test]
    fn the_moon_takes_over_as_key_after_dark() {
        let mut sky = SkySystem::new(Quality::High);
        sky.set_time_of_day(12.0);
        assert_eq!(sky.key_light, KeyLight::Sun);
        sky.set_time_of_day(0.0);
        assert_eq!(sky.key_light, KeyLight::Moon);
    }

    #[test]
    fn a_turbidity_patch_asks_for_a_static_rebake() {
        let mut sky = SkySystem::new(Quality::High);
        assert!(sky.set_weather(&WeatherPatch {
            turbidity: Some(3.0),
            ..Default::default()
        }));
        assert!(!sky.set_weather(&WeatherPatch {
            horizon_murk: Some(0.4),
            ..Default::default()
        }));
    }

    #[test]
    fn place_light_clamps_a_below_horizon_direction() {
        // Straight down, clamped to minY then renormalised and scaled by 600.
        let p = place_light(Vec3::new(0.0, -1.0, 0.0), 0.006);
        assert!(p.y > 0.0);
        assert!((p.length() - 600.0).abs() < 1e-9);
        // Already above the clamp: untouched, so no renormalisation happens.
        let q = place_light(Vec3::new(0.0, 1.0, 0.0), 0.006);
        assert_eq!(q, Vec3::new(0.0, 600.0, 0.0));
    }
}
