//! Ported from Claude-of-Duty `src/sky/stars.js:21-164` — `STARS_GLSL`, the
//! night sky (a procedural starfield plus a Milky Way band). No JavaScript
//! form exists anywhere (WebGL2 fragment-shader source only), so every
//! function here is hand-transcribed the same way `dome`/`clouds`/
//! `volumetrics` are, pinned against a second, independent hand-transcription
//! in `tests/sky_stars/capture.mjs` (read by `tests/sky_stars_port.rs`).
//!
//! **That pin cannot catch a mistake both transcriptions share**, which is
//! not a theoretical caveat: until this module was audited it *and* the
//! transcription in `tests/sky/capture.mjs` both wrote [`blackbody`]'s
//! `c / max(1e-4, dot(c, ...))` (`stars.js:54`) as a multiply by a rounded
//! reciprocal. No golden built from that pair could have found it. The
//! `sky_stars` transcription was therefore written from the GLSL text alone,
//! before this file was opened, and the two were diffed afterwards. Keep that
//! order if you revise either.
//!
//! Every `/` in the source is a `/` here, every multiply chain keeps the
//! source's left-to-right association, and a `vec * scalar * scalar` chain
//! stays two multiplies unless the source itself parenthesised the scalars
//! together — which, at `stars.js:95` and `:126`, it does.
//!
//! Three ideas the source's own module doc calls out, preserved here:
//!
//! - three density layers with a magnitude power law (`h^5.5`), so a handful
//!   of stars are conspicuous and thousands are barely there;
//! - a blackbody colour temperature with the *luminance normalised out* and
//!   then pulled 89% back toward white ([`SK_STAR_TINT`]) — a saturated
//!   two-pixel point reads as a dead sub-pixel, not "a red giant";
//! - Kasten-Young airmass extinction, so the sky loses stars toward the
//!   horizon instead of ending on a hard line.

use super::atmosphere::{gl_mix, smoothstep, Vec3};
use super::celestial::Mat3;
use super::noise::{fbm3, hash33};

/// How much of the blackbody hue survives — see the module doc.
/// `SK_STAR_TINT`, `stars.js:40`.
pub const STAR_TINT: f64 = 0.11;

/// Galactic pole direction, equatorial frame. `SK_GAL_POLE`, `stars.js:99`.
pub const GAL_POLE: Vec3 = Vec3::new(-0.4288, 0.7146, 0.5522);
/// Galactic-centre direction, equatorial frame. `SK_GAL_CORE`, `stars.js:100`.
pub const GAL_CORE: Vec3 = Vec3::new(0.7549, -0.2154, -0.6194);

/// The `uStarParams` uniform vec4, unpacked to named fields.
#[derive(Debug, Clone, Copy)]
pub struct StarParams {
    /// `uStarParams.x` — overall brightness.
    pub brightness: f64,
    /// `uStarParams.y` — twinkle amount.
    pub twinkle: f64,
    /// `uStarParams.z` — seconds; drives the twinkle phase.
    pub time: f64,
    /// `uStarParams.w` — Milky Way gain.
    pub milkyway_gain: f64,
}

/// Tanner Helland's blackbody fit, moved to linear light and normalised to
/// unit luminance. `skBlackbody`, `stars.js:43-55`.
pub fn blackbody(kelvin: f64) -> Vec3 {
    let t = kelvin.clamp(1200.0, 40000.0) / 100.0;
    let r = if t <= 66.0 {
        1.0
    } else {
        (1.292_936_19 * (t - 60.0).powf(-0.133_204_76)).clamp(0.0, 1.0)
    };
    let g = if t <= 66.0 {
        (0.390_081_58 * t.ln() - 0.631_841_44).clamp(0.0, 1.0)
    } else {
        (1.129_890_86 * (t - 60.0).powf(-0.075_514_85)).clamp(0.0, 1.0)
    };
    let b = if t >= 66.0 {
        1.0
    } else if t <= 19.0 {
        0.0
    } else {
        (0.543_206_79 * (t - 10.0).ln() - 1.196_254_09).clamp(0.0, 1.0)
    };
    let c = Vec3::new(r.powf(2.2), g.powf(2.2), b.powf(2.2));
    // `c / max( 1e-4, dot( c, ... ) )` (`stars.js:54`) is a real componentwise
    // DIVIDE. It was `c.scale(1.0 / ...)` here, and multiplying by a rounded
    // reciprocal is a different operation — the same last-bit faithfulness
    // defect the `dome`/`clouds` and `volumetrics` audits found ten of across
    // this subsystem, and invisible to any golden whose JS transcription was
    // written by reading this file.
    c.div(Vec3::splat(c.dot(Vec3::new(0.2126, 0.7152, 0.0722)).max(1e-4)))
}

/// Kasten-Young relative airmass: 1 overhead, ~38 at the horizon.
/// `skAirmass`, `stars.js:58-61`.
pub fn airmass(cos_zenith: f64) -> f64 {
    let z = cos_zenith.clamp(-1.0, 1.0).acos().to_degrees();
    1.0 / (cos_zenith.max(0.0) + 0.50572 * (96.07995 - z).max(0.0).powf(-1.6364))
}

/// One star per grid cell of a 3D lattice sampled on the unit sphere.
/// `skStarLayer`, `stars.js:68-96`.
#[allow(clippy::too_many_arguments)]
pub fn star_layer(dir: Vec3, n: f64, keep: f64, gain: f64, seed: f64, sigma: f64, twinkle: f64, band: f64, time: f64) -> Vec3 {
    let cell_floor = dir.scale(n).floor();
    let cell = cell_floor.add_scalar(seed);
    let h = hash33(cell);
    // `step(1.0-keep, h.x) < 0.5` <=> `h.x < 1.0-keep`.
    if h.x < 1.0 - keep {
        return Vec3::splat(0.0);
    }

    let h2 = hash33(cell.add_scalar(91.7));
    // `stars.js:76` recomputes `floor( dir * N )` here, without the seed;
    // `cell_floor` is that same value, bit for bit.
    //
    // `.normalize()` is `v * (1 / length(v))`, while GLSL's `normalize` is
    // reference-defined as `v / length(v)`. The two differ in the last bit
    // (and real hardware, which uses `rsqrt`, agrees with neither). Kept as
    // the shared `Vec3::normalize` that all five sky modules use rather than
    // diverging in one of them; `tests/sky_stars/capture.mjs` transcribes the
    // same convention, and the measured cost is 2.0e-14 on the worst golden
    // value, four orders under the pin. See the notes file.
    let star_dir = cell_floor
        .add_scalar(0.5)
        .add(h2.sub(Vec3::splat(0.5)).scale(0.94))
        .normalize();

    // sin(separation) — cheaper and better conditioned than acos near zero.
    let d = dir.cross(star_dir).length();

    // Magnitude power law: most cells hold something you would never notice.
    let mag = h.y.powf(5.5);
    let flux = gain * (mag + 0.0016) * (1.0 + band * 1.4);

    // Core plus a faint diffraction skirt.
    let core = (-(d * d) / (sigma * sigma)).exp();
    let skirt = 0.055 * (-d / (sigma * 3.4)).exp();

    let tw = 1.0
        + twinkle
            * ((time * (7.0 + 19.0 * h.z) + h2.x * 43.0).sin() + 0.6 * (time * (23.0 + 31.0 * h2.y)).sin());
    let kelvin = gl_mix(2600.0, 22000.0, h2.z.powf(1.9));
    let tint = Vec3::splat(1.0).mix(blackbody(kelvin), STAR_TINT);
    tint.scale(flux * (core + skirt) * tw.max(0.0))
}

/// Galactic plane: a tight bright spine inside a broad halo, dust lanes, and
/// a warm bulge toward the core. `skMilkyWay`, `stars.js:102-127`.
pub fn milky_way(eq: Vec3, gain: f64, oct: i32) -> Vec3 {
    let lat = eq.dot(GAL_POLE);
    let spine = (-(lat.abs() / 0.048).powf(1.55)).exp();
    let halo = (-(lat.abs() / 0.165).powf(1.30)).exp();
    let band = (0.78 * spine + 0.48 * halo).clamp(0.0, 1.4);
    if band < 0.002 {
        return Vec3::splat(0.0);
    }

    let to_core = eq.dot(GAL_CORE);
    let bulge = (-((1.0 - to_core).max(0.0) / 0.22).powf(1.1)).exp();

    let q = eq.scale(9.0);
    let clumps = fbm3(q, oct);
    let dust = fbm3(eq.scale(21.0).add_scalar(3.7), 2.max(oct - 1));
    let lane = smoothstep(0.36, 0.68, dust) * spine;

    let density = band * (0.20 + 1.35 * clumps * clumps) * (1.0 - 0.80 * lane);
    let density = density * (1.0 + 2.6 * bulge);

    let tint = Vec3::new(0.72, 0.80, 1.06).mix(Vec3::new(1.10, 0.86, 0.62), bulge * 0.85);
    tint.scale(density * gain)
}

/// Full night sky in scene radiance units. `dir` is world-space; `celestial`
/// (`Mat3::mul_vec3`, i.e. `uCelestial * dir`) rotates it to the equatorial
/// frame the star lattice and Milky Way are fixed in. `points = false`
/// (`quality <= 0`, the environment-map path) skips the star lattice
/// entirely, matching `skNightSky`'s `bool points` gate.
/// `skNightSky`, `stars.js:133-161`.
pub fn night_sky(dir: Vec3, mw_octaves: i32, points: bool, celestial: Mat3, star: StarParams) -> Vec3 {
    let eq = celestial.mul_vec3(dir);
    let am = airmass(dir.y);
    // Extinction ~0.16 mag/airmass in V, plus horizon murk.
    let ext = (-0.145 * am).exp() * smoothstep(-0.03, 0.10, dir.y);

    let mw = eq.dot(GAL_POLE).clamp(-1.0, 1.0);
    let band = (-(mw.abs() / 0.16).powf(1.4)).exp();

    let mut col = milky_way(eq, star.milkyway_gain, mw_octaves);

    if points {
        let tw = star.twinkle * ((am - 1.0) * 0.16).clamp(0.0, 0.85);
        col = col.add(star_layer(eq, 21.0, 0.30, 1.00, 0.0, 0.00165, tw, band, star.time));
        col = col.add(star_layer(eq, 43.0, 0.20, 0.34, 13.0, 0.00145, tw, band, star.time));
        col = col.add(star_layer(eq, 87.0, 0.10, 0.11, 47.0, 0.00125, tw * 0.5, band * 2.2, star.time));
    }

    // Airglow: real, faint, greenish — keeps the "empty" sky off zero.
    col = col.add(Vec3::new(0.55, 1.0, 0.78).scale(0.00030));

    col.scale(star.brightness * ext)
}
