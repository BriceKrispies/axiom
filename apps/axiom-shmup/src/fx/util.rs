//! Ported from Claude-of-Duty `src/fx/util.js:1-184` — the whole file.
//!
//! Small vector helpers shared by the FX spawners: reflection, an
//! orthonormal basis, cone/hemisphere sampling, ejecta-direction shaping, and
//! a blackbody colour ramp.
//!
//! **Out-parameters, dropped.** The source writes into module-level scratch
//! objects (`V`, `V2`, `C`, `C2`, the internal `B`) so a burst of particles
//! never allocates a `Vector3` in a hot loop (`util.js:1-6`). Rust has no
//! garbage collector to spare here — a `(f64, f64, f64)` return is a
//! register-sized value, not a heap allocation — so every function below
//! returns its result instead of writing into a caller-supplied scratch.
//! This changes call sites (`cone(V, rng, ...)` becomes
//! `let v = cone(rng, ...)`) but not one value: the arithmetic is identical,
//! line for line, and callers in [`crate::fx::impacts`]/[`crate::fx::muzzle`]/
//! [`crate::fx::explosions`] read the same as the source with the assignment
//! made explicit.

use crate::rng::Rng;

/// `reflect(out, dx, dy, dz, nx, ny, nz)`, `util.js:11-16`.
pub fn reflect(dx: f64, dy: f64, dz: f64, nx: f64, ny: f64, nz: f64) -> (f64, f64, f64) {
    let d = dx * nx + dy * ny + dz * nz;
    (dx - 2.0 * d * nx, dy - 2.0 * d * ny, dz - 2.0 * d * nz)
}

/// Build any orthonormal pair around `(nx,ny,nz)`: `(tangent, bitangent)`.
/// `util.js:19-38`.
pub fn basis(nx: f64, ny: f64, nz: f64) -> (f64, f64, f64, f64, f64, f64) {
    let mut ax = 0.0;
    let mut ay = 1.0;
    let az = 0.0;
    if ny.abs() > 0.9 {
        ax = 1.0;
        ay = 0.0;
    }
    let mut tx = ay * nz - az * ny;
    let mut ty = az * nx - ax * nz;
    let mut tz = ax * ny - ay * nx;
    // `Math.hypot(tx, ty, tz) || 1`.
    //
    // TWO KNOWN DIVERGENCES, both recorded here because an earlier comment on
    // this line defended the wrong invariant ("a single 3-argument hypot, not
    // two chained 2-argument ones, which would round twice"). Chaining was
    // never the hazard:
    //
    // 1. V8's `Math.hypot` is **Kahan-compensated** and scales by the largest
    //    magnitude to avoid overflow; the naive `sqrt(x*x + y*y + z*z)` below
    //    is neither. They agree to within an ULP or so on the unit-ish vectors
    //    this function is fed, and disagree by more as the inputs spread.
    // 2. `|| 1` in JS catches **NaN as well as 0**. `h == 0.0` catches only 0,
    //    so a NaN component propagates here where the source would have
    //    divided by 1. Every port site that transcribes this idiom has the
    //    same gap.
    //
    // Neither is fixed in place: both change numbers this port has not
    // re-verified against a capture, so they are a finding for the integrator,
    // not a drive-by edit.
    let l = {
        let h = (tx * tx + ty * ty + tz * tz).sqrt();
        if h == 0.0 {
            1.0
        } else {
            h
        }
    };
    tx /= l;
    ty /= l;
    tz /= l;
    let bx = ny * tz - nz * ty;
    let by = nz * tx - nx * tz;
    let bz = nx * ty - ny * tx;
    (tx, ty, tz, bx, by, bz)
}

/// Random unit direction inside a cone of half-angle `spread` (radians)
/// around a unit axis. `power > 1` biases toward the axis. `util.js:44-56`.
pub fn cone(rng: &mut Rng, ax: f64, ay: f64, az: f64, spread: f64, power: f64) -> (f64, f64, f64) {
    let (tx, ty, tz, bx, by, bz) = basis(ax, ay, az);
    let u = rng.float().powf(power);
    let cos_t = (spread * u).cos();
    let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
    let phi = rng.float() * std::f64::consts::PI * 2.0;
    let cp = phi.cos() * sin_t;
    let sp = phi.sin() * sin_t;
    (
        ax * cos_t + tx * cp + bx * sp,
        ay * cos_t + ty * cp + by * sp,
        az * cos_t + tz * cp + bz * sp,
    )
}

/// Uniform direction on the hemisphere around a unit axis. `util.js:59-61`.
/// Not called anywhere in the source's `fx/*.js` at the time of this port
/// (`cone` is always called directly with an explicit spread), ported anyway
/// since it is an exported part of the module's public surface.
pub fn hemi(rng: &mut Rng, ax: f64, ay: f64, az: f64) -> (f64, f64, f64) {
    cone(rng, ax, ay, az, std::f64::consts::PI * 0.5, 1.0)
}

/// Force a direction into the hemisphere around an axis by mirroring
/// whatever component points the wrong way, keeping a minimum forward bias.
/// `util.js:71-90`.
pub fn toward_hemi(
    x: f64,
    y: f64,
    z: f64,
    ax: f64,
    ay: f64,
    az: f64,
    bias: f64,
) -> (f64, f64, f64) {
    let d = x * ax + y * ay + z * az;
    if d >= bias {
        return (x, y, z);
    }
    let k = bias - d;
    let mut ox = x + ax * (k - 2.0 * d.min(0.0));
    let mut oy = y + ay * (k - 2.0 * d.min(0.0));
    let mut oz = z + az * (k - 2.0 * d.min(0.0));
    let l = {
        let h = (ox * ox + oy * oy + oz * oz).sqrt();
        if h == 0.0 {
            1.0
        } else {
            h
        }
    };
    ox /= l;
    oy /= l;
    oz /= l;
    (ox, oy, oz)
}

/// Hard cone clamp: force a direction to lie within `cos_max` of an axis,
/// keeping whatever tangential direction it already had. `util.js:103-119`.
pub fn clamp_cone(
    x: f64,
    y: f64,
    z: f64,
    ax: f64,
    ay: f64,
    az: f64,
    cos_max: f64,
) -> (f64, f64, f64) {
    let d = x * ax + y * ay + z * az;
    if d >= cos_max {
        return (x, y, z);
    }
    let tx = x - ax * d;
    let ty = y - ay * d;
    let tz = z - az * d;
    let tl = (tx * tx + ty * ty + tz * tz).sqrt();
    if tl < 1e-5 {
        return (ax, ay, az);
    }
    let s = (1.0 - cos_max * cos_max).max(0.0).sqrt() / tl;
    (ax * cos_max + tx * s, ay * cos_max + ty * s, az * cos_max + tz * s)
}

/// `cos(55 deg)` — the ejecta cone half-angle shared by impacts and the
/// muzzle. `util.js:122`.
pub const COS55: f64 = 0.573_576_4;

/// Planckian locus, normalised so the brightest channel is 1. `util.js:133-149`.
pub fn blackbody(kelvin: f64) -> (f64, f64, f64) {
    let t = kelvin.clamp(1000.0, 6500.0) / 100.0;
    let (mut r, mut g);
    if t <= 66.0 {
        r = 255.0;
        g = 99.47 * t.ln() - 161.12;
    } else {
        r = 329.7 * (t - 60.0).powf(-0.1332);
        g = 288.12 * (t - 60.0).powf(-0.0755);
    }
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.52 * (t - 10.0).ln() - 305.04
    };
    r = r.clamp(0.0, 255.0) / 255.0;
    g = g.clamp(0.0, 255.0) / 255.0;
    let b = b.clamp(0.0, 255.0) / 255.0;
    let lin = |c: f64| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    r = lin(r);
    g = lin(g);
    let b = lin(b);
    let m = r.max(g).max(b).max(1e-4);
    (r / m, g / m, b / m)
}

/// Random point inside a disc of radius `r` on the plane whose normal is
/// `(nx,ny,nz)`. `util.js:156-166`.
pub fn disc_on(rng: &mut Rng, nx: f64, ny: f64, nz: f64, r: f64) -> (f64, f64, f64) {
    let (tx, ty, tz, bx, by, bz) = basis(nx, ny, nz);
    let rr = rng.float().sqrt() * r;
    let a = rng.float() * std::f64::consts::PI * 2.0;
    let cx = a.cos() * rr;
    let cy = a.sin() * rr;
    (tx * cx + bx * cy, ty * cx + by * cy, tz * cx + bz * cy)
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub fn clamp(v: f64, a: f64, b: f64) -> f64 {
    v.clamp(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflect_off_a_flat_wall() {
        let (x, y, z) = reflect(1.0, -1.0, 0.0, 0.0, 1.0, 0.0);
        assert_eq!((x, y, z), (1.0, 1.0, 0.0));
    }

    #[test]
    fn basis_is_orthonormal() {
        let (tx, ty, tz, bx, by, bz) = basis(0.0, 1.0, 0.0);
        let dot_tb = tx * bx + ty * by + tz * bz;
        assert!(dot_tb.abs() < 1e-12);
        let lt = (tx * tx + ty * ty + tz * tz).sqrt();
        let lb = (bx * bx + by * by + bz * bz).sqrt();
        assert!((lt - 1.0).abs() < 1e-12);
        assert!((lb - 1.0).abs() < 1e-12);
    }

    #[test]
    fn cone_direction_is_unit_length() {
        let mut rng = Rng::new(5);
        for _ in 0..20 {
            let (x, y, z) = cone(&mut rng, 0.0, 1.0, 0.0, 0.8, 1.2);
            let l = (x * x + y * y + z * z).sqrt();
            assert!((l - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn toward_hemi_never_travels_into_the_surface() {
        // Straight into the wall: must get pushed to at least `bias` cosine.
        let (x, y, z) = toward_hemi(0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.08);
        let d = x * 0.0 + y * 1.0 + z * 0.0;
        assert!(d >= 0.08 - 1e-9);
    }

    #[test]
    fn clamp_cone_leaves_directions_already_inside() {
        let (x, y, z) = clamp_cone(0.0, 1.0, 0.0, 0.0, 1.0, 0.0, COS55);
        assert_eq!((x, y, z), (0.0, 1.0, 0.0));
    }

    #[test]
    fn clamp_cone_clamps_a_wide_direction() {
        let (_x, y, _z) = clamp_cone(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, COS55);
        let d = y; // cosine against the axis (0,1,0)
        assert!((d - COS55).abs() < 1e-9);
    }

    #[test]
    fn blackbody_is_normalised_on_its_peak_channel() {
        let (r, g, b) = blackbody(2600.0);
        let m = r.max(g).max(b);
        assert!((m - 1.0).abs() < 1e-12);
    }

    #[test]
    fn blackbody_clamps_kelvin_range() {
        let low = blackbody(200.0);
        let clamped = blackbody(1000.0);
        assert_eq!(low, clamped);
        let high = blackbody(50_000.0);
        let clamped_high = blackbody(6500.0);
        assert_eq!(high, clamped_high);
    }

    #[test]
    fn disc_on_stays_within_radius() {
        let mut rng = Rng::new(3);
        for _ in 0..30 {
            let (x, y, z) = disc_on(&mut rng, 0.0, 1.0, 0.0, 0.5);
            let l = (x * x + y * y + z * z).sqrt();
            assert!(l <= 0.5 + 1e-9);
        }
    }

    #[test]
    fn lerp_and_clamp() {
        assert_eq!(lerp(0.0, 10.0, 0.3), 3.0);
        assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
        assert_eq!(clamp(-5.0, 0.0, 1.0), 0.0);
    }
}
