//! Ported from Claude-of-Duty `src/sky/dome.js` — the layered sky sample
//! (`skSample`, `dome.js:194-273`) and its helpers. No JavaScript form exists
//! (WebGL2 fragment-shader source only), so every function here is
//! hand-transcribed the same way `clouds`/`stars`/`volumetrics` are, pinned
//! against a second, independent hand-transcription in
//! `tests/sky/capture.mjs`.
//!
//! `dome.js` itself also defines `SkyDome`, a `THREE.ShaderMaterial` +
//! full-screen-triangle wrapper (uniform wiring, `onBeforeRender`, PMREM env
//! bake material) — pure GPU/host plumbing with no portable computation, the
//! same category `fullscreen.js` is entirely made of (see this crate's
//! `sky` module doc). Not ported; a future WGSL/render-integration slice
//! needs it, not this CPU reference.
//!
//! The three shader **`main`s** around it are a different matter, and an
//! earlier draft of this module wrongly filed them under the same
//! justification. `DOME_VERT` and `ENV_FRAG` each carry real, portable,
//! testable arithmetic — the map from a screen pixel (or an equirectangular
//! texel) to the world ray [`sample`] wants — with the matrices as their only
//! GPU-shaped input. They are ported here as [`screen_ray`] and
//! [`equirect_direction`]. `DOME_FRAG`'s `main` is the one that really is
//! trivial: `fragColor = vec4(skSample(normalize(vRay), 1), 1.0)`, i.e.
//! [`sample`] of a normalised [`screen_ray`], and `ENV_FRAG`'s is the same
//! with `normalize(dir)` and `quality = 0`.
//!
//! [`sample`] composites, in the source's own order (`dome.js:194-273`):
//! sky-view LUT lookup -> [`aureole`] x2 (sun, moon) -> [`super::clouds`] ->
//! night sky ([`super::stars::night_sky`]), occluded by cloud alpha -> ground
//! bounce below the horizon -> horizon murk -> [`rolloff`] -> [`sun_disc`]
//! and [`moon_disc`] (**after** the roll-off — they are meant to clip and
//! bloom, and are the only things in the sky that are).
//!
//! ## `fwidth` — a GPU-only input, made explicit
//!
//! `skSunDisc`/`skMoonDisc` anti-alias their disc edge with GLSL's `fwidth`,
//! the screen-space derivative between neighbouring pixels — a quantity a
//! single CPU sample has no way to reconstruct (there is no "neighbouring
//! pixel"). Rather than fake it, [`sun_disc`]/[`moon_disc`]/[`sample`] take
//! the derivative as an explicit parameter, the same shape divergence
//! `super::atmosphere::raymarch_sky` already establishes for
//! `uTransmittanceLut`/`uMultiScatterLut`: a GLSL *implicit* input becomes an
//! explicit Rust parameter, because a CPU port has no implicit binding to
//! resolve it through.
//!
//! That parameter is not a dead end, though, and treating it as one would be
//! the dodge. [`fwidth`] is GLSL's own definition of the quantity
//! (`abs(dFdx(v)) + abs(dFdy(v))`), and with [`screen_ray`] a caller can
//! produce the two neighbouring-pixel rays a GPU quad would have, evaluate
//! `theta`/`r2` at all three, and feed the *real* derivative in — which is
//! exactly what `tests/sky_port.rs` does, so the disc anti-aliasing is
//! pinned end to end rather than at an invented value.

use std::f64::consts::PI;

use super::atmosphere::{gl_mix, luminance, lut_uv, mie_phase, safe_acos, smoothstep, Vec3, ATMO};
use super::celestial::Mat3;
use super::clouds::{clouds, CloudParams};
use super::luts::{sky_view_lookup, Lut2D};
use super::noise::fbm3;
use super::stars::{night_sky, StarParams};

/// `CUT`, the aureole's angular cutoff — `cos(24 degrees)`. `dome.js:100`.
const AUREOLE_CUT: f64 = 0.9135;

/// GLSL `fwidth(v)` — by definition `abs(dFdx(v)) + abs(dFdy(v))`, and on a
/// 2x2 fragment quad `dFdx(v) = v(x+1) - v(x)`, `dFdy(v) = v(y+1) - v(y)`.
/// Given the value at a pixel and at its two quad neighbours, this is the
/// exact quantity `skSunDisc`/`skMoonDisc` pass to their `aa` term
/// (`dome.js:72`, `dome.js:170`).
///
/// The hardware picks the quad, not the shader, so which two neighbours a
/// given fragment sees is a rasteriser detail a CPU sample cannot know — that
/// choice stays the caller's, which is the whole reason
/// [`sun_disc`]/[`moon_disc`] take the derivative as a parameter rather than
/// computing it. This function is the *arithmetic* of `fwidth`, nothing more.
pub fn fwidth(v: f64, v_at_x_plus_1: f64, v_at_y_plus_1: f64) -> f64 {
    (v_at_x_plus_1 - v).abs() + (v_at_y_plus_1 - v).abs()
}

/// The screen-pixel -> world-ray map: `DOME_VERT`'s `main`, `dome.js:276-290`.
///
/// `inv_proj` is `uInvProj` (the camera's `projectionMatrixInverse`, kept in
/// sync with the renderer's TAA jitter — which is *why* the sky is jittered
/// with the rest of the frame) and `cam_world` is `uCamWorld`
/// (`camera.matrixWorld`); both in `THREE.Matrix4.elements` **column-major**
/// order, the same convention `crate::physics::math::ray_obb` takes its
/// `inv`. GLSL `mat4 * vec4` is `col0*v.x + col1*v.y + col2*v.z + col3*v.w`,
/// so the element indices below are columns, not rows — writing this
/// row-major is the silent matrix-storage-order trap and would transpose the
/// projection.
///
/// The shader runs this at the three vertices of a full-screen triangle and
/// lets the rasteriser interpolate `vRay`. That is exact rather than
/// approximate, and the source says why at `dome.js:284-286`: dividing by
/// `-vd.z` puts the direction on the `z = -1` plane, where it is *linear* in
/// screen space. Evaluating it per-pixel here therefore reproduces the
/// interpolated value, not merely something close to it.
///
/// Returns `vRay` **unnormalised**, exactly as the varying carries it;
/// `DOME_FRAG`'s `main` is `skSample(normalize(vRay), 1)`, so a caller
/// normalises before handing it to [`sample`].
pub fn screen_ray(ndc_x: f64, ndc_y: f64, inv_proj: &[f64; 16], cam_world: &[f64; 16]) -> Vec3 {
    // vec4 h = uInvProj * vec4( ndc, 1.0, 1.0 );
    let hx = inv_proj[0] * ndc_x + inv_proj[4] * ndc_y + inv_proj[8] + inv_proj[12];
    let hy = inv_proj[1] * ndc_x + inv_proj[5] * ndc_y + inv_proj[9] + inv_proj[13];
    let hz = inv_proj[2] * ndc_x + inv_proj[6] * ndc_y + inv_proj[10] + inv_proj[14];
    let hw = inv_proj[3] * ndc_x + inv_proj[7] * ndc_y + inv_proj[11] + inv_proj[15];

    // vec3 vd = h.xyz / h.w;
    let vd = Vec3::new(hx / hw, hy / hw, hz / hw);
    // vd /= max( 1.0e-6, -vd.z );  (componentwise, as GLSL `vec3 /= float`)
    let s = (-vd.z).max(1.0e-6);
    let vd = Vec3::new(vd.x / s, vd.y / s, vd.z / s);

    // vRay = mat3( uCamWorld ) * vd;  — mat3(mat4) is the upper-left 3x3,
    // i.e. columns (0,1,2), (4,5,6), (8,9,10) of the column-major elements.
    Vec3::new(
        cam_world[0] * vd.x + cam_world[4] * vd.y + cam_world[8] * vd.z,
        cam_world[1] * vd.x + cam_world[5] * vd.y + cam_world[9] * vd.z,
        cam_world[2] * vd.x + cam_world[6] * vd.y + cam_world[10] * vd.z,
    )
}

/// The equirectangular-texel -> world-ray map for the PMREM environment
/// bake: `ENV_FRAG`'s `main`, `dome.js:303-315`. The source's own comment is
/// "Matches three's `equirectUv` exactly", so the `az`/`lat` parameterisation
/// is a compatibility contract with three, not a free choice.
///
/// Returns `dir` **unnormalised**, exactly as the shader's local `vec3 dir`
/// holds it; the `main` immediately below it does
/// `skSample( normalize( dir ), 0 )` — note `quality = 0`, the fewer-octaves,
/// no-star-points path.
pub fn equirect_direction(uv_x: f64, uv_y: f64) -> Vec3 {
    let az = (uv_x - 0.5) * 2.0 * PI;
    let lat = (uv_y - 0.5) * PI;
    let cl = lat.cos();
    Vec3::new(cl * az.cos(), lat.sin(), cl * az.sin())
}

/// Circumsolar/circumlunar aureole — the Mie forward peak a one-degree-texel
/// sky-view LUT destroys, restored analytically. See `dome.js:84-117` for
/// the full derivation of the `4.2` coefficient (the one number in the whole
/// sky chosen by eye, because it corrects a *sampling* error, not a physical
/// quantity).
///
/// `dome.js`'s `skAureole(rayDir, lightDir, irradiance, cosTheta)` takes
/// `lightDir` as a parameter but never reads it in the body (every other use
/// of the light direction is already folded into `cosTheta`, computed by the
/// caller) — a genuine unused GLSL parameter. Rust has no silent-unused-arg
/// convention for a public function, so `light_dir` is simply not in this
/// signature; the source's own dead parameter, not a Rust simplification, is
/// documented here instead of reproduced as an ignored argument.
/// `skAureole`, `dome.js:99-117`.
pub fn aureole(ray_dir_y: f64, irradiance: Vec3, transmittance_along_ray: Vec3, cos_theta: f64, mie_scale: f64) -> Vec3 {
    if cos_theta <= AUREOLE_CUT {
        return Vec3::splat(0.0);
    }
    let mie_od = ATMO.mie_scattering * mie_scale * 0.0012 / (ray_dir_y + 0.055).max(0.055);
    let excess = (mie_phase(cos_theta) - mie_phase(AUREOLE_CUT)).max(0.0);
    irradiance.mul(transmittance_along_ray).scale(excess * mie_od * 4.2)
}

/// Highlight roll-off for the sky, and only the sky — a power compressor on
/// luminance with chromaticity carried through unchanged (**not** Reinhard;
/// see `dome.js:119-154` for why Reinhard's hard asymptote is exactly the
/// artefact this exists to prevent). `skRolloff`, `dome.js:140-154`.
///
/// `dome.js` declares its own `owSkLum` (`dome.js:57`) —
/// `dot(c, vec3(0.2126, 0.7152, 0.0722))` — rather than reusing
/// `atmosphere.js`'s `luminance`, but the two are the same Rec.709 sum in
/// the same term order, so [`super::atmosphere::luminance`] stands in for it
/// exactly (GLSL `dot` on a `vec3` is `x*x' + y*y' + z*z'`, left to right).
/// `owSkLum` has no other caller in the source.
pub fn rolloff(col: Vec3, knee: f64, exponent: f64) -> Vec3 {
    if knee <= 0.0 {
        return col;
    }
    let l = luminance([col.x, col.y, col.z]).max(1.0e-6);
    if l <= knee {
        return col;
    }
    col.scale((l / knee).powf(exponent) * knee / l)
}

/// Radiance of the solar disc, limb darkened per-channel
/// (`pow(mu, (0.32, 0.44, 0.58))` — blue falls off fastest, so a low sun's
/// rim reads orange), `fwidth`-antialiased, and divided by `draw_scale^2` so
/// enlarging the disc for readability adds no energy. `skSunDisc`,
/// `dome.js:70-82`.
#[allow(clippy::too_many_arguments)]
pub fn sun_disc(
    theta: f64,
    fwidth_theta: f64,
    ang_radius: f64,
    draw_scale: f64,
    disc_radiance: Vec3,
    transmittance_to_sun: Vec3,
) -> Vec3 {
    let r_edge = ang_radius * draw_scale;
    let aa = fwidth_theta.max(1.0e-6);
    let cover = smoothstep(r_edge + aa, r_edge - aa, theta);
    if cover <= 0.0 {
        return Vec3::splat(0.0);
    }
    let r = (theta / r_edge).clamp(0.0, 1.0);
    let mu = (1.0 - r * r).max(0.0).sqrt();
    let limb = Vec3::new(mu.powf(0.32), mu.powf(0.44), mu.powf(0.58));
    // `uSunDiscRadiance * limb * cover * skTransmittance(...) / ( uDisc.z *
    // uDisc.z )` — a left-to-right chain ending in a componentwise DIVIDE.
    // Not `* (1 / z^2)`: multiplying by a rounded reciprocal is a different
    // operation, and folding any two of these steps together re-associates a
    // float product. Transcribe the chain literally.
    disc_radiance
        .mul(limb)
        .scale(cover)
        .mul(transmittance_to_sun)
        .div(Vec3::splat(draw_scale * draw_scale))
}

/// The moon disc: gnomonic projection, procedural albedo (maria vs
/// highlands), a real terminator with regolith backscatter
/// (`pow(NdL, 0.42)`, not Lambert) plus earthshine. `skMoonDisc`,
/// `dome.js:156-188`.
#[allow(clippy::too_many_arguments)]
pub fn moon_disc(
    ray_dir: Vec3,
    theta: f64,
    oct: i32,
    moon_dir: Vec3,
    sun_dir: Vec3,
    ang_radius: f64,
    draw_scale: f64,
    fwidth_r2: f64,
    disc_radiance: Vec3,
) -> Vec3 {
    let r_edge = ang_radius * draw_scale;
    if theta > r_edge * 1.6 {
        return Vec3::splat(0.0);
    }

    let reference = if moon_dir.y.abs() > 0.97 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let mr = reference.cross(moon_dir).normalize();
    let mu3 = moon_dir.cross(mr);

    let px = ray_dir.dot(mr) / r_edge;
    let py = ray_dir.dot(mu3) / r_edge;
    let r2 = px * px + py * py;
    let aa = (1.9 * fwidth_r2).max(1.0e-4);
    let cover = smoothstep(1.0 + aa, 1.0 - aa, r2);
    if cover <= 0.0 {
        return Vec3::splat(0.0);
    }

    let n = mr
        .scale(px)
        .add(mu3.scale(py))
        .sub(moon_dir.scale((1.0 - r2.min(1.0)).max(0.0).sqrt()))
        .normalize();

    // Maria are basalt floods over anorthositic highlands: albedo 0.06 vs 0.14.
    let highlands = fbm3(n.scale(6.5), oct);
    let maria = smoothstep(0.44, 0.63, fbm3(n.scale(2.1).add_scalar(5.0), 2.max(oct - 1)));
    let albedo = gl_mix(0.105, 0.155, highlands) * gl_mix(1.0, 0.52, maria);

    let n_dl = n.dot(sun_dir).max(0.0);
    // Lunar regolith backscatters hard: nearly flat right up to the terminator.
    let shade = n_dl.powf(0.42);
    let earthshine = 0.014;

    // `uMoonDiscRadiance * ( albedo / 0.13 ) * ( shade + earthshine ) *
    // cover` — three separate vector-by-scalar multiplies, left to right.
    // Folding the three scalars into one product first re-associates it and
    // changes the last bits.
    disc_radiance
        .scale(albedo / 0.13)
        .scale(shade + earthshine)
        .scale(cover)
}

/// The `uSunDir`/`uMoonDir`/... uniforms `skSample` reads, unpacked. Named
/// per the GLSL uniform each field replaces; see each field's comment.
#[derive(Debug, Clone, Copy)]
pub struct DomeUniforms {
    pub sun_dir: Vec3,
    pub moon_dir: Vec3,
    /// `uSunIrradiance` — scene light units, at the ground.
    pub sun_irradiance: Vec3,
    pub moon_irradiance: Vec3,
    /// `uSunDiscRadiance` — radiance of the disc before extinction.
    pub sun_disc_radiance: Vec3,
    pub moon_disc_radiance: Vec3,
    /// `uDisc.x` — sun angular radius.
    pub sun_ang_radius: f64,
    /// `uDisc.y` — moon angular radius.
    pub moon_ang_radius: f64,
    /// `uDisc.z` — sun draw scale.
    pub sun_draw_scale: f64,
    /// `uDisc.w` — moon draw scale.
    pub moon_draw_scale: f64,
    pub ground_albedo: Vec3,
    /// City haze piled up at eye level.
    pub horizon_murk: f64,
    /// `uSkyRolloff.x` — knee, in scene radiance units.
    pub rolloff_knee: f64,
    /// `uSkyRolloff.y` — compression exponent above the knee.
    pub rolloff_exponent: f64,
    pub mie_scale: f64,
    /// `vec3(0, groundRadius + viewAltitude, 0)`.
    pub view_pos: Vec3,
    pub cloud: CloudParams,
    pub star: StarParams,
    /// `uCelestial` — equatorial -> world rotation for the starfield.
    pub celestial: Mat3,
}

/// The full layered sky sample. `quality` is `1` for the on-screen dome
/// pass, `0` for the (fewer-octaves, no star points) environment-map bake —
/// matching `skSample`'s `int quality` parameter exactly (GLSL has no bool
/// overload here, so this stays `i32` rather than becoming a Rust `bool`).
/// `sky_view`/`transmittance` are the two baked LUTs from `super::luts`;
/// `ambient` is the two-texel probe `super::luts::bake_ambient` produces
/// (`[cosine-weighted-sky, horizon-band]`, exactly `skAmbientSky`/
/// `skAmbientHorizon`'s two fixed-uv texture reads). `skSample`, `dome.js:194-273`.
#[allow(clippy::too_many_arguments)]
pub fn sample(
    ray_dir: Vec3,
    quality: i32,
    u: &DomeUniforms,
    sky_view: &Lut2D,
    transmittance: &Lut2D,
    ambient: [Vec3; 2],
    fwidth_sun_theta: f64,
    fwidth_moon_r2: f64,
) -> Vec3 {
    let amb_sky = ambient[0];
    let amb_hor = ambient[1];

    let mut col = sky_view_lookup(sky_view, ray_dir, u.sun_dir, u.view_pos);

    let cos_s = ray_dir.dot(u.sun_dir);
    let cos_m = ray_dir.dot(u.moon_dir);
    let theta_s = safe_acos(cos_s);
    let theta_m = safe_acos(cos_m);

    let transmittance_at = |p: Vec3, dir: Vec3| -> Vec3 {
        let (uu, vv) = lut_uv(p, dir);
        transmittance.sample(uu, vv)
    };

    // Aureoles go in before the discs so the discs sit inside their own glow.
    let trans_along_ray = transmittance_at(u.view_pos, ray_dir);
    col = col.add(aureole(ray_dir.y, u.sun_irradiance, trans_along_ray, cos_s, u.mie_scale));
    col = col.add(aureole(ray_dir.y, u.moon_irradiance, trans_along_ray, cos_m, u.mie_scale));

    // ---- clouds -------------------------------------------------------------
    let p_low = Vec3::new(0.0, ATMO.ground_radius_mm + 0.0015, 0.0);
    let p_high = Vec3::new(0.0, ATMO.ground_radius_mm + 0.0078, 0.0);
    let sun_low = u.sun_irradiance.mul(transmittance_at(p_low, u.sun_dir));
    let sun_high = u.sun_irradiance.mul(transmittance_at(p_high, u.sun_dir));
    let moon_low = u.moon_irradiance.mul(transmittance_at(p_low, u.moon_dir));
    let moon_high = u.moon_irradiance.mul(transmittance_at(p_high, u.moon_dir));
    let (cloud_rgb, cloud_a) = clouds(
        ray_dir, u.sun_dir, sun_low, sun_high, u.moon_dir, moon_low, moon_high, amb_sky, quality, &u.cloud,
        u.view_pos,
    );

    // ---- night sky, behind the decks -----------------------------------------
    let night = night_sky(ray_dir, if quality > 0 { 5 } else { 3 }, quality > 0, u.celestial, u.star);
    col = col.add(night.scale(1.0 - (cloud_a * 1.9).clamp(0.0, 1.0)));

    if cloud_a > 1.0e-4 {
        // Aerial perspective on the decks: fades the *colour* toward the sky
        // in front of it, keyed off view elevation (which sets path length).
        let bleed = 1.0 - smoothstep(0.0, 0.22, ray_dir.y);
        col = col.mix(cloud_rgb.mix(col, bleed * 0.82), cloud_a);
    }

    // ---- ground / below the horizon --------------------------------------------
    if ray_dir.y < 0.0 {
        // `uSunIrradiance * max( 0.0, uSunDir.y ) / SK_PI` groups as
        // `(irradiance * cosine) / pi`, componentwise — NOT
        // `irradiance * (cosine / pi)`. Float multiplication is not
        // associative, so keep the source's grouping (and its divide, rather
        // than a multiply by a precomputed reciprocal).
        let ground = u.ground_albedo.mul(
            amb_hor
                .add(u.sun_irradiance.scale(u.sun_dir.y.max(0.0)).div(Vec3::splat(PI)))
                .add(u.moon_irradiance.scale(u.moon_dir.y.max(0.0)).div(Vec3::splat(PI))),
        );
        col = col.mix(ground, smoothstep(0.0, -0.22, ray_dir.y));
    }

    // A real city horizon is never clean: dust and exhaust pile up.
    let murk = u.horizon_murk * (-ray_dir.y.abs() * 26.0).exp();
    col = col.mix(amb_hor.scale(1.15), murk.clamp(0.0, 0.85));

    // ---- horizon roll-off -------------------------------------------------------
    col = rolloff(col, u.rolloff_knee, u.rolloff_exponent);

    // The discs go in AFTER the roll-off: they are supposed to clip and bloom.
    if quality > 0 {
        let trans_to_sun = transmittance_at(u.view_pos, u.sun_dir);
        col = col.add(sun_disc(
            theta_s,
            fwidth_sun_theta,
            u.sun_ang_radius,
            u.sun_draw_scale,
            u.sun_disc_radiance,
            trans_to_sun,
        ));
    }
    col = col.add(moon_disc(
        ray_dir,
        theta_m,
        if quality > 0 { 4 } else { 2 },
        u.moon_dir,
        u.sun_dir,
        u.moon_ang_radius,
        u.moon_draw_scale,
        fwidth_moon_r2,
        u.moon_disc_radiance,
    ));

    col.max(Vec3::splat(0.0))
}
