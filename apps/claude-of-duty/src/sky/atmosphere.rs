//! Ported from Claude-of-Duty `src/sky/atmosphere.js:1-335`.
//!
//! Bruneton's scattering integral, evaluated the way Hillaire 2020 ("A
//! Scalable and Production Ready Sky and Atmosphere Rendering Technique")
//! does it. The source ships this as three pieces:
//!
//! - GLSL template strings (`ATMOSPHERE_GLSL`, `TRANSMITTANCE_LOOKUP_GLSL`,
//!   `MULTISCATTER_LOOKUP_GLSL`, `SCATTER_GLSL`) that every LUT-bake and
//!   sky-render shader in `src/sky/` concatenates in. This crate has no GPU
//!   path yet, so those bodies are ported here as CPU `f64` functions instead
//!   of shader source — see the crate-level note in `super` and
//!   `crate::materials::noise`'s doc comment for the precedent.
//! - `mediumJs`/`transmittanceToSpace`/`luminance`, a genuine CPU function
//!   with a real JavaScript oracle (`src/sky/atmosphere.js:275-334`), ported
//!   1:1 below.
//! - The `ATMO` media constants and the photometric-scale constants
//!   (`SCENE_LUX`, `SUN_ILLUMINANCE_TOP`, `MOON_ILLUMINANCE_NIGHT`), copied
//!   exactly.
//!
//! ## The photometric contract — read before changing a number here
//!
//! `atmosphere.js:20-57`: `1 light-intensity unit = SCENE_LUX (25000) lux`,
//! and `1 framebuffer radiance unit = SCENE_LUX cd/m^2`. three's Lambert BRDF
//! already carries the `1/pi` (a lit surface writes `b = I/pi`, whose physical
//! radiance is `L = I*SCENE_LUX/pi`, so `L = b*SCENE_LUX`); a scattering
//! integral that already evaluates a *radiance* — which `sigma_s * P(theta) *
//! E` is, once `E` is in scene light units — is therefore written to the
//! buffer **as-is**. [`raymarch_sky`] below must never be multiplied by `pi`
//! on the way out; the source's own history is that this exact mistake put
//! every daylight shot 1.65 stops over-bright (a sunlit wall darker than the
//! sky behind it, clouds darker than the gaps between them). Consequences
//! that fall out of the model rather than being dialled in, all order-of-
//! magnitude checks rather than exact pins (see `tests/sky_port.rs`):
//! extraterrestrial solar illuminance 128 klx -> [`SUN_ILLUMINANCE_TOP`]
//! (5.12); noon sun after atmospheric extinction -> ~3.9 units; clear zenith
//! sky ~1500 cd/m^2 -> ~0.06 radiance units.

/// A minimal `f64` 3-vector — the reference-implementation vocabulary this
/// module needs (`add`/`sub`/component `mul`/`div`, scalar `scale`, `dot`,
/// `cross`, `length`, `normalize`, componentwise `max`/`exp`). There is no
/// shared vector type across this crate (see `crate::materials::noise`'s
/// module doc); `axiom_math::Vec3` is `f32` and fallible on `normalize`,
/// which does not fit a "match the JS `number` exactly" reference port.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }

    pub const fn splat(s: f64) -> Self {
        Vec3 { x: s, y: s, z: s }
    }

    pub fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    pub fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    /// Componentwise `vec3 * vec3`.
    pub fn mul(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    /// Componentwise `vec3 / vec3`.
    pub fn div(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x / o.x, self.y / o.y, self.z / o.z)
    }

    /// `vec3 * float`.
    pub fn scale(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn normalize(self) -> Vec3 {
        self.scale(1.0 / self.length())
    }

    pub fn max(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }

    pub fn exp(self) -> Vec3 {
        Vec3::new(self.x.exp(), self.y.exp(), self.z.exp())
    }

    pub fn fract(self) -> Vec3 {
        Vec3::new(
            self.x - self.x.floor(),
            self.y - self.y.floor(),
            self.z - self.z.floor(),
        )
    }
}

/// `SCENE_LUX`, `atmosphere.js:54`.
pub const SCENE_LUX: f64 = 25000.0;

/// `SUN_ILLUMINANCE_TOP`, `atmosphere.js:57`. `128000.0 / SCENE_LUX == 5.12`.
pub const SUN_ILLUMINANCE_TOP: f64 = 128_000.0 / SCENE_LUX;

/// `MOON_ILLUMINANCE_NIGHT`, `atmosphere.js:74`.
pub const MOON_ILLUMINANCE_NIGHT: f64 = 0.30;

/// `1/(4*pi)` — `SK_ISO_PHASE`, `atmosphere.js:118`. Kept as the source's own
/// literal (not computed from `PI`) so a golden comparison never has to
/// account for a second, independently-rounded `1.0/(4.0*PI)`.
pub const ISO_PHASE: f64 = 0.079_577_471_545_947_67;

/// The media constants, `ATMO`, `atmosphere.js:76-90`. Hillaire units:
/// lengths in megametres, coefficients in Mm^-1.
#[derive(Debug, Clone, Copy)]
pub struct Atmo {
    pub ground_radius_mm: f64,
    pub atmosphere_radius_mm: f64,
    /// Viewer altitude. 200 m puts us above the thickest aerosol.
    pub view_altitude_mm: f64,
    pub rayleigh: [f64; 3],
    pub rayleigh_scale_height_km: f64,
    pub mie_scattering: f64,
    pub mie_absorption: f64,
    pub mie_scale_height_km: f64,
    pub ozone: [f64; 3],
    pub ozone_centre_km: f64,
    pub ozone_width_km: f64,
    pub ground_albedo: f64,
}

pub const ATMO: Atmo = Atmo {
    ground_radius_mm: 6.36,
    atmosphere_radius_mm: 6.46,
    view_altitude_mm: 0.0002,
    rayleigh: [5.802, 13.558, 33.1],
    rayleigh_scale_height_km: 8.0,
    mie_scattering: 3.996,
    mie_absorption: 4.4,
    mie_scale_height_km: 1.2,
    ozone: [0.65, 1.881, 0.085],
    ozone_centre_km: 25.0,
    ozone_width_km: 15.0,
    ground_albedo: 0.24,
};

/// `skSafeAcos`, `atmosphere.js:125`.
pub fn safe_acos(x: f64) -> f64 {
    x.clamp(-1.0, 1.0).acos()
}

/// GLSL `sign(x)`: `1` for `x>0`, `-1` for `x<0`, **exactly `0` for `x==0`**
/// — unlike Rust's `f64::signum`, which returns `+-1.0` even at `+-0.0` and
/// therefore cannot stand in for it. Used by [`super::luts::sky_view_lookup`].
pub fn gl_sign(x: f64) -> f64 {
    match x.partial_cmp(&0.0) {
        Some(std::cmp::Ordering::Greater) => 1.0,
        Some(std::cmp::Ordering::Less) => -1.0,
        _ => 0.0,
    }
}

/// Nearest positive hit of a ray against a sphere centred on the origin.
/// `skRaySphere`, `atmosphere.js:128-136`. The `d > b*b` branch (rather than
/// the more common `d > 0`) is the source's own condition, ported exactly.
pub fn ray_sphere(ro: Vec3, rd: Vec3, rad: f64) -> f64 {
    let b = ro.dot(rd);
    let c = ro.dot(ro) - rad * rad;
    if c > 0.0 && b > 0.0 {
        return -1.0;
    }
    let d = b * b - c;
    if d < 0.0 {
        return -1.0;
    }
    if d > b * b {
        -b + d.sqrt()
    } else {
        -b - d.sqrt()
    }
}

/// The medium sample `skMedium` evaluates at a point: separate Rayleigh
/// scattering, Mie scattering, and the total extinction coefficient (used to
/// attenuate transmittance).
#[derive(Debug, Clone, Copy)]
pub struct Medium {
    pub rayleigh_s: Vec3,
    pub mie_s: f64,
    pub extinction: Vec3,
}

/// `skMedium`, `atmosphere.js:138-148`.
pub fn medium(pos: Vec3, mie_scale: f64) -> Medium {
    let alt_km = (pos.length() - ATMO.ground_radius_mm) * 1000.0;
    let r_den = (-alt_km / ATMO.rayleigh_scale_height_km).exp();
    let m_den = (-alt_km / ATMO.mie_scale_height_km).exp();
    let rayleigh_s = Vec3::new(ATMO.rayleigh[0], ATMO.rayleigh[1], ATMO.rayleigh[2]).scale(r_den);
    let mie_s = ATMO.mie_scattering * mie_scale * m_den;
    let mie_a = ATMO.mie_absorption * mie_scale * m_den;
    let ozone_factor = (1.0 - (alt_km - ATMO.ozone_centre_km).abs() / ATMO.ozone_width_km).max(0.0);
    let ozone = Vec3::new(ATMO.ozone[0], ATMO.ozone[1], ATMO.ozone[2]).scale(ozone_factor);
    let extinction = rayleigh_s.add(Vec3::splat(mie_s + mie_a)).add(ozone);
    Medium {
        rayleigh_s,
        mie_s,
        extinction,
    }
}

/// Cornette-Shanks (`g = 0.8`), the well-behaved cousin of Henyey-Greenstein.
/// `skMiePhase`, `atmosphere.js:151-155`.
pub fn mie_phase(cos_theta: f64) -> f64 {
    const G: f64 = 0.8;
    let k = 3.0 / (8.0 * std::f64::consts::PI) * (1.0 - G * G) / (2.0 + G * G);
    k * (1.0 + cos_theta * cos_theta) / (1.0 + G * G - 2.0 * G * cos_theta).powf(1.5)
}

/// Analytic Rayleigh phase. `skRayleighPhase`, `atmosphere.js:157-159`.
pub fn rayleigh_phase(cos_theta: f64) -> f64 {
    3.0 / (16.0 * std::f64::consts::PI) * (1.0 + cos_theta * cos_theta)
}

/// Henyey-Greenstein — used by the (unported) ground fog pass, exposed here
/// so both would agree, exactly as the source keeps them side by side.
/// `skHG`, `atmosphere.js:162-166`.
pub fn hg_phase(cos_theta: f64, g: f64) -> f64 {
    let g2 = g * g;
    let d = (1.0 + g2 - 2.0 * g * cos_theta).max(1e-4);
    (1.0 - g2) / (4.0 * std::f64::consts::PI * d * d.sqrt())
}

/// The transmittance/multiscatter LUT parameterisation shared by the bake and
/// every lookup — `skLutUv`, `TRANSMITTANCE_LOOKUP_GLSL`,
/// `atmosphere.js:177-183`. Returns `(u, v)`.
pub fn lut_uv(pos: Vec3, dir: Vec3) -> (f64, f64) {
    let h = pos.length();
    let mu = dir.dot(pos.scale(1.0 / h));
    (
        (0.5 + 0.5 * mu).clamp(0.0, 1.0),
        ((h - ATMO.ground_radius_mm) / (ATMO.atmosphere_radius_mm - ATMO.ground_radius_mm))
            .clamp(0.0, 1.0),
    )
}

/// Single + multiple scattering along a view ray, for two light sources at
/// once (sun and moon) — `skRaymarchSky`, `SCATTER_GLSL`,
/// `atmosphere.js:212-269`. `sample_transmittance`/`sample_multiscatter`
/// stand in for the source's `uTransmittanceLut`/`uMultiScatterLut` global
/// sampler uniforms: GLSL resolves those implicitly through
/// `skTransmittance`/`skMultiScatter`, but a CPU port has no implicit global
/// texture binding, so the two lookups become explicit parameters — the one
/// deliberate shape divergence in this function.
///
/// Returns radiance in scene units (see the photometric-contract note at the
/// top of this file), **not** multiplied by `pi`. The direct solar/lunar disc
/// is deliberately excluded, exactly as the source excludes it (an unported
/// concern of the dome pass).
#[allow(clippy::too_many_arguments)]
pub fn raymarch_sky(
    pos: Vec3,
    ray_dir: Vec3,
    sun_dir: Vec3,
    sun_irr: Vec3,
    moon_dir: Vec3,
    moon_irr: Vec3,
    steps: u32,
    mie_scale: f64,
    sample_transmittance: impl Fn(Vec3, Vec3) -> Vec3,
    sample_multiscatter: impl Fn(Vec3, Vec3) -> Vec3,
) -> Vec3 {
    let top_t = ray_sphere(pos, ray_dir, ATMO.atmosphere_radius_mm);
    let ground_t = ray_sphere(pos, ray_dir, ATMO.ground_radius_mm);
    let t_max = if ground_t < 0.0 { top_t } else { ground_t };
    if t_max <= 0.0 {
        return Vec3::splat(0.0);
    }

    let c_s = ray_dir.dot(sun_dir);
    let c_m = ray_dir.dot(moon_dir);
    let mie_s_phase = mie_phase(c_s);
    let ray_s_phase = rayleigh_phase(c_s);
    let mie_m_phase = mie_phase(c_m);
    let ray_m_phase = rayleigh_phase(c_m);

    let mut lum = Vec3::splat(0.0);
    let mut trans = Vec3::splat(1.0);
    let mut t = 0.0;

    for i in 0..steps {
        // 0.3 rather than 0.5 biases samples toward the dense lower
        // atmosphere, which is where all the interesting colour is.
        let nt = ((f64::from(i) + 0.3) / f64::from(steps)) * t_max;
        let dt = nt - t;
        t = nt;
        let p = pos.add(ray_dir.scale(t));

        let m = medium(p, mie_scale);
        let sample_t = m.extinction.scale(-dt).exp();

        let t_sun = sample_transmittance(p, sun_dir);
        let psi_sun = sample_multiscatter(p, sun_dir);
        let mut in_scatter = m
            .rayleigh_s
            .mul(t_sun.scale(ray_s_phase).add(psi_sun))
            .add(Vec3::splat(m.mie_s).mul(t_sun.scale(mie_s_phase).add(psi_sun)))
            .mul(sun_irr);

        let t_moon = sample_transmittance(p, moon_dir);
        let psi_moon = sample_multiscatter(p, moon_dir);
        in_scatter = in_scatter.add(
            m.rayleigh_s
                .mul(t_moon.scale(ray_m_phase).add(psi_moon))
                .add(Vec3::splat(m.mie_s).mul(t_moon.scale(mie_m_phase).add(psi_moon)))
                .mul(moon_irr),
        );

        // Analytic integration of the segment (Hillaire eq. 8): exact for
        // constant media over dt, and unlike a midpoint sum it never
        // overshoots when the optical depth of a step is large.
        lum = lum.add(
            trans
                .mul(in_scatter.sub(in_scatter.mul(sample_t)))
                .div(m.extinction.max(Vec3::splat(1e-8))),
        );
        trans = trans.mul(sample_t);
    }
    // No pi here — see the photometric-contract note at the top of this file.
    lum
}

/// `mediumJs`, `atmosphere.js:275-284` — a distinct, simpler CPU-only medium
/// evaluation used only by [`transmittance_to_space`]: it lumps Mie
/// scattering and absorption into one `mie` term and returns extinction
/// directly, rather than the `{rayleigh_s, mie_s, extinction}` split
/// [`medium`] returns for the raymarch. Kept as its own function, exactly as
/// the source keeps `mediumJs` separate from `skMedium`, rather than
/// reprojected onto `medium`'s richer return shape.
fn extinction_at_altitude_km(alt_km: f64, mie_scale: f64) -> Vec3 {
    let r_den = (-alt_km / ATMO.rayleigh_scale_height_km).exp();
    let m_den = (-alt_km / ATMO.mie_scale_height_km).exp();
    let mie = (ATMO.mie_scattering + ATMO.mie_absorption) * mie_scale * m_den;
    let oz = (1.0 - (alt_km - ATMO.ozone_centre_km).abs() / ATMO.ozone_width_km).max(0.0);
    Vec3::new(
        ATMO.rayleigh[0] * r_den + mie + ATMO.ozone[0] * oz,
        ATMO.rayleigh[1] * r_den + mie + ATMO.ozone[1] * oz,
        ATMO.rayleigh[2] * r_den + mie + ATMO.ozone[2] * oz,
    )
}

/// Per-channel transmittance from the viewer to space along a direction whose
/// cosine with the local zenith is `mu`. Same integral as the GPU LUT bake,
/// so the sun's `DirectionalLight` colour and the sky it hangs in cannot
/// disagree. `transmittanceToSpace`, `atmosphere.js:294-329`. The source
/// defaults `mieScale` to `1`; Rust has no default arguments, so every caller
/// passes it explicitly.
pub fn transmittance_to_space(mu: f64, mie_scale: f64) -> [f64; 3] {
    let r = ATMO.ground_radius_mm + ATMO.view_altitude_mm;
    let top = ATMO.atmosphere_radius_mm;
    // Ray from (0,R,0) with vertical component mu. Path length to the top
    // shell.
    let disc = r * r * mu * mu - r * r + top * top;
    if disc <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    let t_top = -r * mu + disc.sqrt();
    // Below the horizon the ground blocks us entirely.
    let g_disc = r * r * mu * mu - r * r + ATMO.ground_radius_mm * ATMO.ground_radius_mm;
    if mu < 0.0 && g_disc > 0.0 {
        return [0.0, 0.0, 0.0];
    }
    const N: u32 = 48;
    let dt = t_top / f64::from(N);
    let mut od = Vec3::splat(0.0);
    for i in 0..N {
        let t = (f64::from(i) + 0.5) * dt;
        // |(0,R,0) + t*dir| with dir.y = mu
        let h = (r * r + t * t + 2.0 * r * t * mu).sqrt();
        let alt_km = ((h - ATMO.ground_radius_mm) * 1000.0).max(0.0);
        let ext = extinction_at_altitude_km(alt_km, mie_scale);
        od = od.add(ext.scale(dt));
    }
    [(-od.x).exp(), (-od.y).exp(), (-od.z).exp()]
}

/// Rec.709 luminance — used to split transmittance into colour + intensity.
/// `luminance`, `atmosphere.js:332-334`.
pub fn luminance(rgb: [f64; 3]) -> f64 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}
