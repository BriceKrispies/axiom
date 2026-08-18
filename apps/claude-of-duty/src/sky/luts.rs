//! Ported from Claude-of-Duty `src/sky/luts.js`.
//!
//! The source bakes three atmosphere LUTs plus a 2x1 ambient probe as WebGL2
//! fragment shaders (`SkyLuts`, `luts.js:237-309`). This crate has no
//! GPU/WGSL path yet, so each `*_FRAG` shader body below is ported as an
//! ordinary `f64` function over the same texel grid the shader would
//! rasterize, producing the identical buffer a GPU bake would upload.
//! `uTransmittanceLut`/`uMultiScatterLut` (GLSL global sampler uniforms)
//! become explicit [`Lut2D`] sampling closures passed into
//! [`super::atmosphere::raymarch_sky`] — see that function's doc comment for
//! why.
//!
//! ## What this reference does not model
//!
//! No `f32`/fp16 storage quantization: the source's multiscatter, sky-view
//! and ambient render targets are `RGBA16F` (the transmittance target is
//! float32), so a real GPU bake rounds every value written to those three
//! textures to half precision before the next pass reads it back. This
//! reference stays `f64` throughout — higher precision than the eventual GPU
//! evaluation will have — matching `crate::materials::noise`'s own precedent
//! (see that module's doc comment). `docs/work-manifests/claude-of-duty-port/
//! notes/sky.md` records what a WGSL emission workstream would still need to
//! add on top of this.
//!
//! ## Resolution
//!
//! [`TRANSMITTANCE_WIDTH`]/[`MULTISCATTER_SIZE`]/[`SKYVIEW_WIDTH`] etc. name
//! the source's real production resolutions. The bake functions themselves
//! take `width`/`height` as parameters rather than hardcoding them, so a
//! sky-view bake (2.9M step-iterations at full 384x192x40) can run at a
//! smaller size in a fast unit test while the constants document what a real
//! bake (triggered by a future `system.rs`/GPU wiring, not part of this
//! slice) should actually request.

use super::atmosphere::{
    gl_sign, lut_uv, medium, ray_sphere, raymarch_sky, safe_acos, Vec3, ATMO, ISO_PHASE,
};

// ---------------------------------------------------------------------------
// Lut2D — CPU storage + the bilinear `texture()` sample every bake/lookup
// GLSL function in the source performs against a sampler2D.
// ---------------------------------------------------------------------------

/// A baked RGB LUT: `width * height` texels, row-major (`data[y*width+x]`),
/// sampled the way WebGL2 samples a `LINEAR`-filtered `sampler2D` — bilinear
/// with texel centres at `(i+0.5)/size`, clamped to the edge on `T` (`v`,
/// always) and, unless `wrap_s`, on `S` (`u`) too. The sky-view LUT sets
/// `wrapS = THREE.RepeatWrapping` (`luts.js:249`); the other three keep
/// three's default `ClampToEdgeWrapping`.
#[derive(Debug, Clone)]
pub struct Lut2D {
    pub width: usize,
    pub height: usize,
    pub wrap_s: bool,
    pub data: Vec<Vec3>,
}

impl Lut2D {
    fn new(width: usize, height: usize, wrap_s: bool) -> Self {
        Lut2D {
            width,
            height,
            wrap_s,
            data: vec![Vec3::splat(0.0); width * height],
        }
    }

    fn set(&mut self, x: usize, y: usize, v: Vec3) {
        self.data[y * self.width + x] = v;
    }

    /// Reads one texel, applying the LUT's addressing mode.
    fn texel(&self, x: i64, y: i64) -> Vec3 {
        let width = self.width as i64;
        let height = self.height as i64;
        let xi = if self.wrap_s {
            x.rem_euclid(width)
        } else {
            x.clamp(0, width - 1)
        };
        let yi = y.clamp(0, height - 1);
        self.data[(yi as usize) * self.width + (xi as usize)]
    }

    /// GLSL `texture(sampler, vec2(u, v))` — bilinear, texel-centred.
    pub fn sample(&self, u: f64, v: f64) -> Vec3 {
        let tx = u * self.width as f64 - 0.5;
        let ty = v * self.height as f64 - 0.5;
        let x0 = tx.floor();
        let y0 = ty.floor();
        let fx = tx - x0;
        let fy = ty - y0;
        let x0i = x0 as i64;
        let y0i = y0 as i64;
        let c00 = self.texel(x0i, y0i);
        let c10 = self.texel(x0i + 1, y0i);
        let c01 = self.texel(x0i, y0i + 1);
        let c11 = self.texel(x0i + 1, y0i + 1);
        let top = c00.scale(1.0 - fx).add(c10.scale(fx));
        let bottom = c01.scale(1.0 - fx).add(c11.scale(fx));
        top.scale(1.0 - fy).add(bottom.scale(fy))
    }
}

// ---------------------------------------------------------------------------
// Transmittance LUT — `TRANSMITTANCE_FRAG`, `luts.js:61-87`.
// ---------------------------------------------------------------------------

/// Production width — `floatTarget(256, 64, ...)`, `luts.js:241`.
pub const TRANSMITTANCE_WIDTH: usize = 256;
/// Production height.
pub const TRANSMITTANCE_HEIGHT: usize = 64;
/// `const float STEPS = 40.0;`, `luts.js:76`.
pub const TRANSMITTANCE_STEPS: u32 = 40;

/// `T(altitude, cos zenith)`: optical-depth-to-space, baked once (altitude/
/// aerosol dependent only). `TRANSMITTANCE_FRAG`, `luts.js:67-86`.
pub fn bake_transmittance(width: usize, height: usize, steps: u32, mie_scale: f64) -> Lut2D {
    let mut lut = Lut2D::new(width, height, false);
    for j in 0..height {
        for i in 0..width {
            let vu = (i as f64 + 0.5) / width as f64;
            let vv = (j as f64 + 0.5) / height as f64;
            let mu = vu * 2.0 - 1.0;
            let h = ATMO.ground_radius_mm + (ATMO.atmosphere_radius_mm - ATMO.ground_radius_mm) * vv;
            let pos = Vec3::new(0.0, h, 0.0);
            let dir = Vec3::new((1.0 - mu * mu).max(0.0).sqrt(), mu, 0.0);

            let t = ray_sphere(pos, dir, ATMO.atmosphere_radius_mm);
            if t <= 0.0 {
                lut.set(i, j, Vec3::splat(0.0));
                continue;
            }
            let dt = t / f64::from(steps);
            let mut od = Vec3::splat(0.0);
            for s in 0..steps {
                let p = pos.add(dir.scale((f64::from(s) + 0.5) * dt));
                let m = medium(p, mie_scale);
                od = od.add(m.extinction.scale(dt));
            }
            lut.set(i, j, Vec3::new((-od.x).exp(), (-od.y).exp(), (-od.z).exp()));
        }
    }
    lut
}

// ---------------------------------------------------------------------------
// Multiscatter LUT — `MULTISCATTER_FRAG`, `luts.js:89-161`.
// ---------------------------------------------------------------------------

/// Production width/height — `hdrTarget(32, 32, ...)`, `luts.js:242`.
pub const MULTISCATTER_SIZE: usize = 32;
/// `const float MS_STEPS = 20.0;`, `luts.js:96`.
pub const MULTISCATTER_STEPS: u32 = 20;
/// `const int SQRT_SAMPLES = 8;`, `luts.js:97`.
pub const MULTISCATTER_SQRT_SAMPLES: usize = 8;

/// `psi_ms(altitude, cos zenith)`: baked once, from the transmittance LUT.
/// `MULTISCATTER_FRAG`, `luts.js:99-160`.
pub fn bake_multiscatter(
    size: usize,
    steps: u32,
    sqrt_samples: usize,
    mie_scale: f64,
    transmittance: &Lut2D,
) -> Lut2D {
    let mut lut = Lut2D::new(size, size, false);
    let sample_transmittance = |p: Vec3, dir: Vec3| -> Vec3 {
        let (u, v) = lut_uv(p, dir);
        transmittance.sample(u, v)
    };

    for j in 0..size {
        for i in 0..size {
            let vu = (i as f64 + 0.5) / size as f64;
            let vv = (j as f64 + 0.5) / size as f64;
            let mu = vu * 2.0 - 1.0;
            let h_min = ATMO.ground_radius_mm + 1e-5;
            let h = h_min + (ATMO.atmosphere_radius_mm - h_min) * vv;
            let pos = Vec3::new(0.0, h, 0.0);
            let sun_dir = Vec3::new((1.0 - mu * mu).max(0.0).sqrt(), mu, 0.0).normalize();

            let mut lum_total = Vec3::splat(0.0);
            let mut fms_total = Vec3::splat(0.0);
            let inv_samples = 1.0 / (sqrt_samples * sqrt_samples) as f64;

            for si in 0..sqrt_samples {
                for sj in 0..sqrt_samples {
                    // Uniform on the sphere: theta linear, cos(phi) linear.
                    let theta = std::f64::consts::PI * (si as f64 + 0.5) / sqrt_samples as f64;
                    let phi = safe_acos(1.0 - 2.0 * (sj as f64 + 0.5) / sqrt_samples as f64);
                    let cp = phi.cos();
                    let sp = phi.sin();
                    let ray_dir = Vec3::new(sp * theta.sin(), cp, sp * theta.cos());

                    let top_t = ray_sphere(pos, ray_dir, ATMO.atmosphere_radius_mm);
                    let grn_t = ray_sphere(pos, ray_dir, ATMO.ground_radius_mm);
                    let t_max = if grn_t < 0.0 { top_t } else { grn_t };
                    if t_max <= 0.0 {
                        continue;
                    }

                    let mut lum = Vec3::splat(0.0);
                    let mut fms = Vec3::splat(0.0);
                    let mut trans = Vec3::splat(1.0);
                    let mut t = 0.0;
                    for step in 0..steps {
                        let nt = ((f64::from(step) + 0.5) / f64::from(steps)) * t_max;
                        let dt = nt - t;
                        t = nt;
                        let p = pos.add(ray_dir.scale(t));
                        let m = medium(p, mie_scale);
                        let sample_t = m.extinction.scale(-dt).exp();

                        // f_ms: the fraction of light that scatters at least
                        // once more.
                        let s_no_phase = m.rayleigh_s.add(Vec3::splat(m.mie_s));
                        fms = fms.add(
                            trans
                                .mul(s_no_phase.sub(s_no_phase.mul(sample_t)))
                                .div(m.extinction.max(Vec3::splat(1e-8))),
                        );

                        let t_sun = sample_transmittance(p, sun_dir);
                        let in_s = s_no_phase.scale(ISO_PHASE).mul(t_sun);
                        lum = lum.add(
                            trans
                                .mul(in_s.sub(in_s.mul(sample_t)))
                                .div(m.extinction.max(Vec3::splat(1e-8))),
                        );
                        trans = trans.mul(sample_t);
                    }

                    if grn_t > 0.0 {
                        let hit = pos
                            .add(ray_dir.scale(grn_t))
                            .normalize()
                            .scale(ATMO.ground_radius_mm);
                        if hit.dot(sun_dir) > 0.0 {
                            lum = lum.add(
                                trans
                                    .scale(ATMO.ground_albedo)
                                    .mul(sample_transmittance(hit, sun_dir)),
                            );
                        }
                    }

                    lum_total = lum_total.add(lum.scale(inv_samples));
                    fms_total = fms_total.add(fms.scale(inv_samples));
                }
            }

            // Infinite geometric series of scattering orders, collapsed.
            let psi = lum_total.div(Vec3::splat(1.0).sub(fms_total).max(Vec3::splat(1e-4)));
            lut.set(i, j, psi);
        }
    }
    lut
}

// ---------------------------------------------------------------------------
// Sky-view LUT — `SKYVIEW_FRAG`/`SKYVIEW_LOOKUP_GLSL`, `luts.js:32-55,
// 163-200`.
// ---------------------------------------------------------------------------

/// Production width — `hdrTarget(384, 192, ...)`, `luts.js:247`. 384x192
/// rather than Hillaire's 192x108 puts one texel under a degree of azimuth.
pub const SKYVIEW_WIDTH: usize = 384;
/// Production height.
pub const SKYVIEW_HEIGHT: usize = 192;
/// `40.0` steps passed to `skRaymarchSky`, `luts.js:197`.
pub const SKYVIEW_STEPS: u32 = 40;

/// The per-bake inputs `SKYVIEW_FRAG` reads as uniforms — `luts.js:172-176`.
pub struct SkyViewParams {
    pub sun_irradiance: Vec3,
    pub moon_irradiance: Vec3,
    /// Sun altitude, radians above the horizon.
    pub sun_altitude: f64,
    /// Moon azimuth relative to the sun, radians.
    pub moon_rel_az: f64,
    pub moon_altitude: f64,
    pub view_pos: Vec3,
    pub mie_scale: f64,
}

/// `L(azimuth, altitude)`: sun/moon dependent, rebaked when the sun moves.
/// `SKYVIEW_FRAG`, `luts.js:178-199`.
pub fn bake_sky_view(
    width: usize,
    height: usize,
    steps: u32,
    params: &SkyViewParams,
    transmittance: &Lut2D,
    multiscatter: &Lut2D,
) -> Lut2D {
    let mut lut = Lut2D::new(width, height, true);
    let sample_transmittance = |p: Vec3, dir: Vec3| -> Vec3 {
        let (u, v) = lut_uv(p, dir);
        transmittance.sample(u, v)
    };
    let sample_multiscatter = |p: Vec3, dir: Vec3| -> Vec3 {
        let (u, v) = lut_uv(p, dir);
        multiscatter.sample(u, v)
    };

    // The LUT frame puts the sun at azimuth 0 (along -Z).
    let sun_dir = Vec3::new(0.0, params.sun_altitude.sin(), -params.sun_altitude.cos());
    let cm = params.moon_altitude.cos();
    let moon_dir = Vec3::new(
        cm * params.moon_rel_az.sin(),
        params.moon_altitude.sin(),
        -cm * params.moon_rel_az.cos(),
    );

    for j in 0..height {
        for i in 0..width {
            let vu = (i as f64 + 0.5) / width as f64;
            let vv = (j as f64 + 0.5) / height as f64;
            let azimuth = (vu - 0.5) * 2.0 * std::f64::consts::PI;
            let adj_v = if vv < 0.5 {
                -(1.0 - 2.0 * vv) * (1.0 - 2.0 * vv)
            } else {
                (2.0 * vv - 1.0) * (2.0 * vv - 1.0)
            };

            let h = params.view_pos.length();
            let horizon = safe_acos((h * h - ATMO.ground_radius_mm * ATMO.ground_radius_mm).sqrt() / h)
                - 0.5 * std::f64::consts::PI;
            let altitude = adj_v * 0.5 * std::f64::consts::PI - horizon;
            let ca = altitude.cos();
            let ray_dir = Vec3::new(ca * azimuth.sin(), altitude.sin(), -ca * azimuth.cos());

            let lum = raymarch_sky(
                params.view_pos,
                ray_dir,
                sun_dir,
                params.sun_irradiance,
                moon_dir,
                params.moon_irradiance,
                steps,
                params.mie_scale,
                sample_transmittance,
                sample_multiscatter,
            );
            lut.set(i, j, lum);
        }
    }
    lut
}

/// `skSkyView`, `SKYVIEW_LOOKUP_GLSL`, `luts.js:37-53` — a *different*
/// parameterisation from [`super::atmosphere::lut_uv`]: azimuth is measured
/// **relative to the sun** (one 384x192 table serves every compass
/// direction), and the altitude axis is square-distributed about the
/// horizon, putting half the texels in the bottom 25 degrees.
pub fn sky_view_lookup(lut: &Lut2D, ray_dir: Vec3, sun_dir: Vec3, view_pos: Vec3) -> Vec3 {
    let h = view_pos.length();
    let up = view_pos.scale(1.0 / h);
    let horizon = safe_acos((h * h - ATMO.ground_radius_mm * ATMO.ground_radius_mm).sqrt() / h);
    let altitude = horizon - safe_acos(ray_dir.dot(up));

    let mut azimuth = 0.0;
    if altitude.abs() < (0.5 * std::f64::consts::PI - 1e-4) {
        let right = sun_dir.cross(up);
        let fwd = up.cross(right);
        let proj = ray_dir.sub(up.scale(ray_dir.dot(up))).normalize();
        azimuth = proj.dot(right).atan2(proj.dot(fwd)) + std::f64::consts::PI;
    }

    let v = 0.5 + 0.5 * gl_sign(altitude) * (altitude.abs() * 2.0 / std::f64::consts::PI).sqrt();
    lut.sample(azimuth / (2.0 * std::f64::consts::PI), v)
}

// ---------------------------------------------------------------------------
// Ambient probe — `AMBIENT_FRAG`, `luts.js:208-235`.
// ---------------------------------------------------------------------------

/// `const int N = 64;`, `luts.js:221`.
pub const AMBIENT_SAMPLES: usize = 64;

/// Average sky radiance: texel 0 is the cosine-weighted whole-sky average,
/// texel 1 is the horizon-band average — `AMBIENT_FRAG`, `luts.js:208-235`.
/// `bake_ambient` runs the fragment body twice (once per output texel),
/// exactly as the shader's `horizonBand = vUv.x > 0.5` selects between the
/// two texels of a 2x1 render target.
pub fn bake_ambient(sky_view: &Lut2D, sun_altitude: f64, view_pos: Vec3) -> [Vec3; 2] {
    let sun_dir = Vec3::new(0.0, sun_altitude.sin(), -sun_altitude.cos());
    [
        ambient_texel(sky_view, sun_dir, view_pos, false),
        ambient_texel(sky_view, sun_dir, view_pos, true),
    ]
}

fn ambient_texel(sky_view: &Lut2D, sun_dir: Vec3, view_pos: Vec3, horizon_band: bool) -> Vec3 {
    let mut sum = Vec3::splat(0.0);
    let mut wsum = 0.0;
    for i in 0..AMBIENT_SAMPLES {
        // Fibonacci hemisphere.
        let fi = (i as f64 + 0.5) / AMBIENT_SAMPLES as f64;
        let phi = i as f64 * 2.399_963_23;
        let ct = if horizon_band {
            gl_mix(-0.12, 0.35, fi)
        } else {
            (1.0 - fi).sqrt()
        };
        let st = (1.0 - ct * ct).max(0.0).sqrt();
        let d = Vec3::new(st * phi.cos(), ct, st * phi.sin());
        let w = if horizon_band { 1.0 } else { ct.max(0.0) };
        sum = sum.add(sky_view_lookup(sky_view, d, sun_dir, view_pos).scale(w));
        wsum += w;
    }
    sum.scale(1.0 / wsum.max(1e-4))
}

fn gl_mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
