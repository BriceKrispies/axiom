//! Ported from Claude-of-Duty `src/sky/clouds.js` — `CLOUDS_GLSL` (the two
//! procedural cloud decks), plus `cloudMacro`/`cloudSunOcclusion`, the two
//! plain-JS CPU twins at the bottom of the file.
//!
//! Two decks, both intersected against the planet shell (via
//! [`super::atmosphere::ray_sphere`], not a flat plane:
//!
//! - **cumulus**, 1.5 km — coverage-eroded fbm with a fake vertical extent
//!   produced by parallax-shifting the sample along the view ray, self-
//!   shadowed with three taps toward the light, powder-darkened, silver-
//!   rimmed by a forward Henyey-Greenstein lobe.
//! - **cirrus**, 7.8 km — two decorrelated ridged-fbm families, each
//!   confined to an *isotropic* warped-fbm silhouette (so it can never
//!   streak, whorl, or converge on a vanishing point — see
//!   [`cirrus_band`]'s doc) and only *textured*, not *shaped*, by an
//!   anisotropic fibre field.
//!
//! [`cloud_macro`] is the one function with a genuine JavaScript oracle
//! (`clouds.js`'s exported `cloudMacro`/`cloudSunOcclusion`): it is
//! deliberately four analytic waves rather than noise, specifically so it
//! can be evaluated identically on the CPU (the sun's cloud-occlusion factor)
//! and the GPU (the shader's coverage field) without the two ever
//! disagreeing. Every other function here is GLSL with no JS form —
//! hand-transcribed the same way `dome`/`stars`/`volumetrics` are, pinned
//! against a second, independent hand-transcription in
//! `tests/sky/capture.mjs`.
//!
//! Radiance convention: `sun_low`/`sun_high`/`moon_low`/`moon_high` arrive as
//! *irradiance* in scene light units (already extinguished to each deck's own
//! altitude — see [`super::dome::sample`]), so every direct term here is
//! divided by `PI` to become framebuffer radiance, exactly as the
//! photometric-contract note at the top of `super::atmosphere` requires.

use std::f64::consts::PI;

use super::atmosphere::{gl_mix, hg_phase, ray_sphere, smoothstep, Vec3, ATMO};
use super::noise::{fbm2, ridge2, sk_rot, val2, Vec2};

/// `SK_CUMULUS_KM`, `clouds.js:49`.
pub const CUMULUS_KM: f64 = 1.5;
/// `SK_CIRRUS_KM`, `clouds.js:50`.
pub const CIRRUS_KM: f64 = 7.8;

/// The `uCloudParams`/`uCloudParams2` uniform vec4 pair, unpacked to named
/// fields. `detail_gain` (`uCloudParams.z`) is carried for fidelity to the
/// uniform's real shape but is never read by any function `CLOUDS_GLSL`
/// defines — a genuine unused-uniform-component in the source, not a
/// transcription gap; see the No-Shortcuts "dead computation" rule.
#[derive(Debug, Clone, Copy)]
pub struct CloudParams {
    /// `uCloudParams.x` — cumulus coverage, 0..1.
    pub coverage: f64,
    /// `uCloudParams.y` — cumulus density multiplier.
    pub density: f64,
    /// `uCloudParams.z` — unused by `CLOUDS_GLSL` (see struct doc).
    pub detail_gain: f64,
    /// `uCloudParams.w` — seconds; drives wind advection.
    pub time: f64,
    /// `uCloudParams2.x` — cirrus coverage, 0..1.
    pub cirrus_coverage: f64,
    /// `uCloudParams2.y` — cirrus opacity multiplier.
    pub cirrus_opacity: f64,
    /// `uCloudParams2.z` — wind, km/s, x.
    pub wind_x: f64,
    /// `uCloudParams2.w` — wind, km/s, z.
    pub wind_z: f64,
}

/// Weather-scale coverage field, in kilometres. Four analytic sine/cosine
/// waves rather than noise, *specifically* so it can be mirrored bit-for-bit
/// (modulo `f32`/`f64`) on the CPU — see the module doc. `skCloudMacro`,
/// `clouds.js:53-58`, and the plain-JS `cloudMacro`, `clouds.js:351-356`
/// (identical expression; this one function serves both roles).
pub fn cloud_macro(p: Vec2) -> f64 {
    let a = (p.x * 0.412 + 0.7).sin() * (p.y * 0.331 - 0.4).cos();
    let b = (p.x * 0.173 - p.y * 0.209 + 1.9).sin();
    let c = (p.x * 0.0871 + p.y * 0.1123 - 0.6).cos();
    (0.5 + 0.5 * (0.42 * a + 0.36 * b + 0.30 * c)).clamp(0.0, 1.0)
}

/// Ridged noise with a *parabolic* crest (`1 - (2v-1)^2`) instead of an
/// absolute-value one (`1 - |2v-1|`, [`super::noise::ridge2`]'s crease) — C1
/// across the crest, so a cirrus fibre has a soft shoulder instead of a
/// hairline. Two octaves only, deliberately: a third would land near the
/// pixel footprint of a deck sampled from 20 km away. `skSmoothRidge2`,
/// `clouds.js:74-84`.
pub fn smooth_ridge2(p: Vec2, oct: i32) -> f64 {
    let mut p = p;
    let mut a = 0.62;
    let mut s = 0.0;
    let mut n = 0.0;
    for _ in 0..oct {
        let v = val2(p) * 2.0 - 1.0;
        s += a * (1.0 - v * v);
        n += a;
        p = sk_rot(p).scale(2.17).add_scalar(3.71);
        a *= 0.45;
    }
    s / n.max(1e-4)
}

/// One family of cirrus, `p` in kilometres on the deck. See `clouds.js:86-149`
/// for the full "why this is shaped the way it is" essay (starburst ->
/// fingerprint -> brush-strokes, and how an isotropic silhouette gated by an
/// anisotropic *texture* answers all three); not reproduced here in full, but
/// the shape survives: silhouette is isotropic (cannot streak), fibre only
/// *modulates* density 0.35..1.4, bearing/rotation/patch-mask are per-family
/// so two families 75 degrees apart (see [`clouds`]) can never share a
/// vanishing point. `skCirrusBand`, `clouds.js:126-149`.
#[allow(clippy::too_many_arguments)]
pub fn cirrus_band(
    p: Vec2,
    cov: f64,
    seed: f64,
    base: f64,
    rot_km_inv: f64,
    len_km: f64,
    aniso: f64,
    oct: i32,
) -> f64 {
    // ---- silhouette: isotropic, so it can never streak --------------------
    let w = Vec2::new(
        val2(p.scale(0.30).add_scalar(seed)),
        val2(p.scale(0.30).add_scalar(seed).add_scalar(11.7)),
    )
    .add_scalar(-0.5);
    let n = fbm2(p.scale(0.78).add(w.scale(1.3)), oct + 1);
    let mut d = smoothstep(1.0 - cov * 1.65, 1.0 - cov * 0.60, n);
    if d <= 0.001 {
        return 0.0;
    }

    // ---- fronts -------------------------------------------------------------
    d *= smoothstep(0.36, 0.66, val2(p.scale(0.12).add_scalar(seed * 0.5)));
    if d <= 0.001 {
        return 0.0;
    }

    // ---- fibre texture inside the patch --------------------------------------
    let ang = base + (val2(p.scale(rot_km_inv).add_scalar(seed)) - 0.5) * 1.1;
    let (ca, sa) = (ang.cos(), ang.sin());
    let pr = Vec2::new(p.x * ca - p.y * sa, p.x * sa + p.y * ca);
    let fa = 1.0 / len_km.max(0.4);
    let q = Vec2::new(pr.x * fa, pr.y * fa * aniso);
    let f = smooth_ridge2(q.add_scalar(seed), oct);
    d * (0.35 + 1.05 * f)
}

/// Cumulus optical thickness at a point on the deck, `p` in kilometres.
/// `skCumulusDensity`, `clouds.js:152-172`.
pub fn cumulus_density(p: Vec2, oct: i32, coverage: f64) -> f64 {
    let macro_field = cloud_macro(p.scale(0.22));
    let cov = (coverage * (0.34 + 1.30 * macro_field)).clamp(0.0, 1.0);

    // Domain warp before the shape fbm — see clouds.js:158-161.
    let w = Vec2::new(val2(p.scale(0.42)), val2(p.scale(0.42).add_scalar(19.7))).add_scalar(-0.5);
    let n = fbm2(p.scale(1.25).add(w.scale(1.6)), oct);

    // Erode from below.
    let mut d = smoothstep(1.0 - cov, 1.0 - cov * 0.34 + 0.05, n);

    // Cauliflower the edges with a higher-frequency ridge.
    if d > 0.0 && d < 0.94 && oct > 3 {
        let e = ridge2(p.scale(5.3).add(w.scale(2.0)), 3);
        d = (d - (1.0 - d) * (0.50 - 0.50 * e)).clamp(0.0, 1.0);
    }
    d
}

/// Fraction of light reaching a point on the cumulus deck, marched along the
/// light's horizontal projection. `skCumulusLight`, `clouds.js:179-186`.
pub fn cumulus_light(p: Vec2, light_dir: Vec3, oct: i32, coverage: f64, density: f64) -> f64 {
    let step2 = Vec2::new(light_dir.x, light_dir.z)
        .add_scalar(1e-4)
        .normalize()
        .scale(0.20 / light_dir.y.abs().max(0.12));
    let mut tau = 0.0;
    tau += cumulus_density(p.add(step2.scale(1.0)), oct, coverage) * 1.0;
    tau += cumulus_density(p.add(step2.scale(2.4)), oct, coverage) * 0.7;
    tau += cumulus_density(p.add(step2.scale(4.6)), oct, coverage) * 0.4;
    (-tau * density * 2.1).exp()
}

/// Composite both decks for a view ray. Returns `(rgb radiance, alpha
/// coverage)` — `alpha == 0` lets the sky through untouched. `skClouds`,
/// `clouds.js:195-327`.
#[allow(clippy::too_many_arguments)]
pub fn clouds(
    ray_dir: Vec3,
    sun_dir: Vec3,
    sun_low: Vec3,
    sun_high: Vec3,
    moon_dir: Vec3,
    moon_low: Vec3,
    moon_high: Vec3,
    ambient: Vec3,
    quality: i32,
    params: &CloudParams,
    view_pos: Vec3,
) -> (Vec3, f64) {
    if ray_dir.y < -0.008 {
        return (Vec3::splat(0.0), 0.0);
    }

    let oct_d = if quality > 0 { 6 } else { 3 };
    let oct_l = if quality > 0 { 4 } else { 2 };
    // Cirrus always gets two octaves: this deck is 20 km away, where one
    // screen pixel covers thirty metres of it, so anything finer is aliasing.
    let oct_c = 2;
    let t = params.time;
    let wind = Vec2::new(params.wind_x, params.wind_z).scale(t);

    let cos_sun = ray_dir.dot(sun_dir);
    let cos_moon = ray_dir.dot(moon_dir);

    // ---- cirrus, 7.8 km -----------------------------------------------------
    let tc = ray_sphere(view_pos, ray_dir, ATMO.ground_radius_mm + CIRRUS_KM * 0.001);
    let mut cirrus_rgb = Vec3::splat(0.0);
    let mut cirrus_a = 0.0;
    if tc > 0.0 {
        let dist_km = tc * 1000.0;
        let mut fade = 1.0 - smoothstep(22.0, 90.0, dist_km);
        fade *= 1.0 - 0.66 * smoothstep(0.55, 0.85, ray_dir.y);

        if fade > 0.004 {
            let hit = view_pos.add(ray_dir.scale(tc));
            let p = Vec2::new(hit.x, hit.z).scale(1000.0).add(wind.scale(2.4));
            let cov = params.cirrus_coverage.clamp(0.0, 1.0);

            // Two decorrelated families, 75 degrees apart (0.24 vs 1.56 rad)
            // — see cirrus_band's doc for why that specific separation.
            let d1 = cirrus_band(p, cov, 0.0, 0.24, 0.135, 1.5, 4.0, oct_c);
            let d2 = cirrus_band(p.add_scalar(137.4), cov * 0.92, 4.7, 1.56, 0.098, 2.0, 3.4, oct_c);
            let d = 1.0 - (1.0 - d1) * (1.0 - d2 * 0.85);

            let a = (d * params.cirrus_opacity * fade).clamp(0.0, 0.70);

            let fwd = hg_phase(cos_sun, 0.74) * 3.2 + 0.60;
            let col = sun_high
                .scale(fwd)
                .add(moon_high.scale(hg_phase(cos_moon, 0.68) * 2.8 + 0.55))
                .scale(1.0 / PI)
                .add(ambient.scale(0.85));
            cirrus_rgb = col;
            cirrus_a = a;
        }
    }

    // ---- cumulus, 1.5 km -----------------------------------------------------
    let tk = ray_sphere(view_pos, ray_dir, ATMO.ground_radius_mm + CUMULUS_KM * 0.001);
    let mut cumulus_rgb = Vec3::splat(0.0);
    let mut cumulus_a = 0.0;
    if tk > 0.0 {
        let dist_km = tk * 1000.0;
        let fade = 1.0 - smoothstep(14.0, 130.0, dist_km);
        if fade > 0.004 {
            let hit0 = view_pos.add(ray_dir.scale(tk));
            let p0 = Vec2::new(hit0.x, hit0.z).scale(1000.0).add(wind);

            // Fake vertical extent by parallax: probe the base density, shift
            // along the view ray by the height the cloud would have there,
            // probe again. Tops lean away from the camera, bases toward it.
            let d_base = cumulus_density(p0, oct_d, params.coverage);
            let shear = Vec2::new(ray_dir.x, ray_dir.z).scale(0.85 * d_base / ray_dir.y.max(0.10));
            let d = cumulus_density(p0.add(shear), oct_d, params.coverage).max(d_base * 0.55);

            if d > 0.003 {
                let p = p0.add(shear);
                let lit = cumulus_light(p, sun_dir, oct_l, params.coverage, params.density);
                let lit_m = cumulus_light(p, moon_dir, oct_l, params.coverage, params.density);

                let graze = (0.09 / (ray_dir.y.abs() + 0.09)).clamp(0.0, 1.0);
                let thick = d * params.density * gl_mix(1.0, 1.7, graze);
                let a = (1.0 - (-thick * 3.4).exp()).clamp(0.0, 1.0) * fade;

                // Powder (dark-edge) term: darkens the thin lit edge, not the
                // base (that's `lit`'s own shadowed sun path).
                let powder = 1.0 - (-thick * 5.5).exp();
                let rim = (1.0 - d).clamp(0.0, 1.0).powf(2.0);

                let fwd_s = hg_phase(cos_sun, 0.62) * 4.0 + 0.62;
                let fwd_m = hg_phase(cos_moon, 0.60) * 3.4 + 0.55;

                let mut direct = sun_low.scale(lit * (0.55 + 0.45 * powder) * fwd_s + rim * lit * 0.9);
                direct = direct.add(moon_low.scale(lit_m * (0.55 + 0.45 * powder) * fwd_m + rim * lit_m * 0.9));
                let fill =
                    ambient.scale(gl_mix(0.50, 1.5, (d * 1.6).clamp(0.0, 1.0)) * (0.32 + 0.68 * lit));
                cumulus_rgb = direct.scale(1.0 / PI).add(fill);
                cumulus_a = a;
            }
        }
    }

    // Cumulus is below cirrus, so it composites on top.
    let out_a = cirrus_a + cumulus_a * (1.0 - cirrus_a);
    let mut out_c = cirrus_rgb.scale(cirrus_a).add(cumulus_rgb.scale(cumulus_a * (1.0 - cirrus_a)));
    if out_a > 1e-5 {
        out_c = out_c.scale(1.0 / out_a);
    }
    (out_c, out_a)
}

/// Sunlight reaching the ground through the cumulus deck, for a world XZ
/// point — the volumetric fog's shafts carry the cloud pattern via this.
/// `skCloudShadow`, `clouds.js:334-341`.
pub fn cloud_shadow(world_xz: Vec2, sun_dir: Vec3, params: &CloudParams) -> f64 {
    let p = world_xz
        .scale(0.001)
        .add(Vec2::new(sun_dir.x, sun_dir.z).scale(CUMULUS_KM / sun_dir.y.max(0.10)))
        .add(Vec2::new(params.wind_x, params.wind_z).scale(params.time));
    let d = cumulus_density(p, 4, params.coverage);
    (-d * params.density * 2.4).exp()
}

/// The `uCloudParams`-equivalent inputs [`cloud_sun_occlusion`] needs — a
/// distinct, smaller bundle from [`CloudParams`] because the plain-JS
/// `cloudSunOcclusion` (`clouds.js:364-374`) takes its own `params` object
/// with different field names (`windX`/`windZ`/`time`/`coverage`/`density`),
/// not the shader's `uCloudParams`/`uCloudParams2` vec4 pair.
#[derive(Debug, Clone, Copy)]
pub struct SunOcclusionParams {
    pub wind_x: f64,
    pub wind_z: f64,
    pub time: f64,
    pub coverage: f64,
    pub density: f64,
}

/// Approximate fraction of direct sunlight surviving the cumulus deck above a
/// world point, from the macro coverage field alone (weather-scale, not
/// per-cloud detail). Genuine JS oracle: `cloudSunOcclusion`, `clouds.js:364-374`.
pub fn cloud_sun_occlusion(world_x: f64, world_z: f64, sun_dir: Vec3, params: &SunOcclusionParams) -> f64 {
    let h = 1.5;
    let k = h / sun_dir.y.max(0.1);
    let px = world_x * 0.001 + sun_dir.x * k + params.wind_x * params.time;
    let pz = world_z * 0.001 + sun_dir.z * k + params.wind_z * params.time;
    let macro_field = cloud_macro(Vec2::new(px * 0.22, pz * 0.22));
    let cov = (params.coverage * (0.34 + 1.3 * macro_field)).clamp(0.0, 1.0);
    let d = ((cov - 0.42) / 0.62).clamp(0.0, 1.0);
    (-d * params.density * 1.55).exp()
}
