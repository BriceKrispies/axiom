//! Ported from Claude-of-Duty `src/sky/volumetrics.js` — the physics inside
//! the volumetric fog/light-shaft pass: the phase functions, the density
//! field, the closed-form height integral, and the per-sample raymarch loop
//! (`MARCH_FRAG`'s `main()`, `volumetrics.js:211-274`) and its analytic
//! single-scattering fallback (`COMPOSITE_FRAG`'s `VOL_ANALYTIC` branch,
//! `volumetrics.js:370-383`). No JavaScript form exists anywhere (WebGL2
//! fragment-shader source only), so every function here is hand-transcribed
//! the same way `dome`/`clouds`/`stars` are, pinned against a second,
//! independent hand-transcription in `tests/sky/capture.mjs`.
//!
//! ## What is deliberately not ported
//!
//! `volumetrics.js` also defines: `SkyPass`/render-target/uniform wiring
//! (the `Volumetrics` class) — pure GPU/host plumbing, same category as
//! `fullscreen.js` (see the `sky` module doc); `skRayFor` — screen-space ray
//! reconstruction from `uInvProj`/`uCamWorld`, which needs a real camera
//! projection/view matrix this crate has no type for yet (out of scope for a
//! CPU physics reference, the same boundary `super::dome`'s module doc draws
//! around `fwidth`); the `RESOLVE_FRAG` temporal-accumulation pass (history
//! buffer + velocity reprojection + neighbourhood clamp) — stateful,
//! frame-to-frame GPU buffer logic with nothing to port as a pure function;
//! and `skSunVisibility`/`CSM_GLSL`'s cascade-shadow-map sampling — there is
//! no CPU representation of a shadow-map texture atlas anywhere in this
//! crate. [`raymarch_fog`] takes sun visibility as a closure instead (the
//! same shape [`super::atmosphere::raymarch_sky`] already uses for
//! `uTransmittanceLut`/`uMultiScatterLut`), so a caller with real shadow data
//! can supply it later without reshaping this function. [`vogel`] (the pure
//! vector math `skSunVisibility` builds its 4 taps from) is ported on its
//! own, since it has no such dependency.
//!
//! ## Scattering vs extinction
//!
//! Deliberately separate uniforms, not tied by a single-scattering albedo —
//! see `volumetrics.js:29-35`. Extinction sets ground-level visibility;
//! inscatter gain sets shaft readability. Tying them either hides the shafts
//! outdoors or turns 200 m of street to milk.

use super::atmosphere::{gl_mix, hg_phase, Vec3};
use super::clouds::{cloud_shadow, CloudParams};
use super::noise::{val3, Vec2};

/// Colour of the haze as a function of angle to the key light — the two
/// texels of the ambient probe (cool whole-sky average, warm horizon-band
/// average) split the haze hue by direction instead of averaging it to grey.
/// `skFogAmbient`, `volumetrics.js:70-79`.
pub fn fog_ambient(cos_key: f64, ambient: [Vec3; 2], key_irr: Vec3) -> Vec3 {
    let cool = ambient[0];
    let hor = ambient[1];
    let max_c = key_irr.x.max(key_irr.y.max(key_irr.z)).max(1.0e-4);
    let key_hue = key_irr.scale(1.0 / max_c);
    let f = 0.5 + 0.5 * cos_key.clamp(-1.0, 1.0);
    let warm = hor.mul(Vec3::splat(1.0).mix(key_hue, 0.55)).scale(1.3);
    cool.mix(warm, f * f)
}

/// Dual-lobe Henyey-Greenstein: a forward peak for the shafts, a broad back
/// lobe so the fog is visible with the sun behind you. `skFogPhase`,
/// `volumetrics.js:83-85`.
pub fn fog_phase(cos_theta: f64, g_fwd: f64, g_back: f64, back_weight: f64) -> f64 {
    gl_mix(hg_phase(cos_theta, g_fwd), hg_phase(cos_theta, g_back), back_weight)
}

/// The shaft gain applied to the *anisotropic excess only*
/// (`p + max(0, p - iso) * (gain - 1)`), not the whole phase function —
/// multiplying the whole phase also multiplies its isotropic floor, which
/// covers every pixel regardless of where the sun is. See `volumetrics.js:87-107`
/// for the full derivation, including why the excess form (rather than
/// `mix(iso, p, gain)`, which goes negative in the side lobes) is used.
/// `skFogInscatterPhase`, `volumetrics.js:103-107`.
pub fn fog_inscatter_phase(cos_theta: f64, g_fwd: f64, g_back: f64, back_weight: f64, shaft_gain: f64) -> f64 {
    const ISO: f64 = 1.0 / (4.0 * std::f64::consts::PI);
    let p = fog_phase(cos_theta, g_fwd, g_back, back_weight);
    p + (p - ISO).max(0.0) * (shaft_gain - 1.0)
}

/// Near-field scattering ramp: fades the first 12 m of fog in, so an
/// exponential-height density tuned for 200 m of street doesn't wash the
/// weapon/hands/near geometry. `skFogNearRamp`, `volumetrics.js:118-120`.
pub fn fog_near_ramp(t: f64) -> f64 {
    super::atmosphere::smoothstep(0.0, 12.0, t)
}

/// Normalised density: 1 at the fog base, exponential above, optionally
/// wind-torn by a two-octave value-noise field. `skFogDensity`,
/// `volumetrics.js:123-129`.
pub fn fog_density(p: Vec3, base_y: f64, inv_height_scale: f64, noise_scale: f64, drift: Vec3, noise_amount: f64) -> f64 {
    let h = (-(p.y - base_y) * inv_height_scale).exp();
    if noise_amount <= 0.001 {
        return h;
    }
    let q = p.scale(noise_scale).add(drift);
    let n = val3(q) * 0.63 + val3(q.scale(2.71).add_scalar(5.1)) * 0.37;
    h * gl_mix(1.0, 0.30 + 1.55 * n, noise_amount)
}

/// Closed form of `integral(0..t) exp(-(y-b)/H) ds` along a ray — exact, so
/// the transmittance applied to geometry is smooth at full resolution.
/// `skHeightIntegral`, `volumetrics.js:136-141`.
pub fn height_integral(y0: f64, dy: f64, t: f64, base_y: f64, inv_height_scale: f64) -> f64 {
    let d0 = (-(y0 - base_y) * inv_height_scale).exp();
    let x = dy * inv_height_scale * t;
    if x.abs() < 1.0e-4 {
        return d0 * t;
    }
    d0 * (1.0 - (-x).exp()) / (dy * inv_height_scale)
}

/// A single Vogel-disc tap. `skVogel`, `volumetrics.js:163-167`.
pub fn vogel(i: i32, n: i32, phi: f64) -> Vec2 {
    let r = ((f64::from(i) + 0.5) / f64::from(n)).sqrt();
    let theta = f64::from(i) * 2.399_963_23 + phi;
    Vec2::new(theta.cos(), theta.sin()).scale(r)
}

/// The `uFog`/`uFog2`/`uPhase`/... uniforms the march and composite passes
/// share, unpacked. See each field's comment for the uniform it replaces.
#[derive(Debug, Clone, Copy)]
pub struct FogUniforms {
    /// `uFog.x` — scattering coefficient.
    pub sigma_s: f64,
    /// `uFog.y` — `1 / height scale`.
    pub inv_height_scale: f64,
    /// `uFog.z` — base Y.
    pub base_y: f64,
    /// `uFog2.x` — extinction coefficient (monochrome; see [`super`]'s
    /// scattering-vs-extinction note).
    pub sigma_e: f64,
    /// `uFog2.y` — shaft gain, applied via [`fog_inscatter_phase`].
    pub shaft_gain: f64,
    /// `uFog2.z` — ambient boost.
    pub ambient_boost: f64,
    /// `uFog2.w` — wind-torn noise amount.
    pub noise_amount: f64,
    /// `uPhase.x` — forward HG `g`.
    pub g_fwd: f64,
    /// `uPhase.y` — back HG `g`.
    pub g_back: f64,
    /// `uPhase.z` — back-lobe weight.
    pub back_weight: f64,
    /// `uPhase.w` — density-noise spatial scale.
    pub noise_scale: f64,
    pub key_dir: Vec3,
    pub key_irr: Vec3,
    pub fog_drift: Vec3,
    /// The two-texel ambient probe (`super::luts::bake_ambient`'s output).
    pub ambient: [Vec3; 2],
}

/// The per-pixel raymarch: `MARCH_FRAG`'s `main()`, minus the screen-space
/// ray reconstruction and the shadow-map texture read (see the module doc).
/// Returns `(L, T)` — inscattered radiance and residual transmittance,
/// exactly `fragColor`'s `.rgb`/`.a`. Steps are distributed exponentially
/// (`f*f*(3-2f)*0.35 + f*f*f*0.65`, `volumetrics.js:242`): centimetres near
/// the camera, tens of metres at the far plane. `volumetrics.js:211-274`.
#[allow(clippy::too_many_arguments)]
pub fn raymarch_fog(
    dir: Vec3,
    ray_len: f64,
    max_t: f64,
    dith: f64,
    cam_pos: Vec3,
    u: &FogUniforms,
    cloud: &CloudParams,
    steps: u32,
    sun_visibility: impl Fn(Vec3, f64, f64) -> f64,
) -> (Vec3, f64) {
    let cos_key = dir.dot(u.key_dir);
    let phase = fog_inscatter_phase(cos_key, u.g_fwd, u.g_back, u.back_weight, u.shaft_gain);
    let ambient = fog_ambient(cos_key, u.ambient, u.key_irr).scale(u.ambient_boost);

    // Cloud shadow, twice per ray rather than once per step (~25x cheaper,
    // visually indistinguishable — see volumetrics.js:226-230).
    let cloud_near = cloud_shadow(Vec2::new(cam_pos.x, cam_pos.z), u.key_dir, cloud);
    let far_pos = cam_pos.add(dir.scale(max_t));
    let cloud_far = cloud_shadow(Vec2::new(far_pos.x, far_pos.z), u.key_dir, cloud);

    let mut l = Vec3::splat(0.0);
    let mut t_trans = 1.0;
    let mut prev = 0.0;

    for i in 0..steps {
        let f = (f64::from(i) + dith) / f64::from(steps);
        let t = max_t * f * f * (3.0 - 2.0 * f) * 0.35 + max_t * f * f * f * 0.65;
        let dt = t - prev;
        prev = t;
        if dt <= 1.0e-5 {
            continue;
        }

        let wp = cam_pos.add(dir.scale(t));
        let dens = fog_density(wp, u.base_y, u.inv_height_scale, u.noise_scale, u.fog_drift, u.noise_amount);
        if dens <= 1.0e-4 {
            continue;
        }

        let sigma_s = u.sigma_s * dens * fog_near_ramp(t);
        let sigma_e = (u.sigma_e * dens).max(1.0e-7);

        let mut vis = sun_visibility(wp, t / ray_len, dith);
        vis *= gl_mix(cloud_near, cloud_far, f);

        // Ambient-occlusion proxy: a shadowed sample sees far less sky.
        let amb_occ = 0.42 + 0.58 * vis;

        // `phase` already carries the shaft gain on its anisotropic part
        // only, so this is the forward lobe lifted and nothing else.
        let j = u.key_irr.scale(vis * phase).add(ambient.scale(amb_occ));

        let a_t = (-sigma_e * dt).exp();
        let contrib = j.scale(t_trans).scale(sigma_s).scale(1.0 - a_t).scale(1.0 / sigma_e);
        l = l.add(contrib);
        t_trans *= a_t;
        if t_trans < 0.004 {
            break;
        }
    }

    (l, t_trans)
}

/// The analytic single-scattering fallback used when the marched pass is
/// disabled (`VOL_ANALYTIC`, `low-end machines still get the correct
/// distance falloff` — `volumetrics.js:24-27`). Same phase split and
/// near-field ramp as [`raymarch_fog`], folded in closed-form via
/// [`height_integral`]. `COMPOSITE_FRAG`, `volumetrics.js:367-388`.
pub fn composite_analytic(color: Vec3, dir: Vec3, dist: f64, cam_pos_y: f64, fog_ext: Vec3, u: &FogUniforms) -> Vec3 {
    // This IS applied to sky pixels too — this layer is the ground haze, and
    // it sits between the camera and the sky just as much as a wall.
    let od = height_integral(cam_pos_y, dir.y, dist, u.base_y, u.inv_height_scale);
    let trans = fog_ext.scale(-od).exp();

    // The ramp folded in analytically: smoothstep(0,12,t) averages 0.5 over
    // [0,12] and 1 past it, so subtracting half the near optical depth
    // reproduces the marched integral to within a few percent.
    let od_near = height_integral(cam_pos_y, dir.y, dist.min(12.0), u.base_y, u.inv_height_scale);
    let od_s = (od - od_near * 0.5).max(0.0);
    let mono = 1.0 - (-u.sigma_e * od_s).exp();
    let cos_key = dir.dot(u.key_dir);
    let inscatter = u
        .key_irr
        .scale(fog_inscatter_phase(cos_key, u.g_fwd, u.g_back, u.back_weight, u.shaft_gain) * 0.55)
        .add(fog_ambient(cos_key, u.ambient, u.key_irr).scale(u.ambient_boost))
        .scale((u.sigma_s / u.sigma_e.max(1.0e-6)) * mono);

    color.mul(trans).add(inscatter)
}
