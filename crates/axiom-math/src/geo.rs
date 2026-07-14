//! Spherical / geodesic math over unit directions.
//! Latitude/longitude, great-circle (angular) distance, an east/north tangent
//! frame, spherical linear interpolation, and an area-preserving uniform
//! sphere-point sampler — the substrate a planetary / spherical-world generator
//! builds on. Every direction is a [`crate::Vec3`]; the lat/lon frame uses `y`
//! as the pole axis, while [`unit_vec3`] samples about `z` (the axis is free for
//! a uniform direction). Angles are dimensioned as [`Radians`] and the
//! interpolation / sampling parameters as [`Ratio`], so a caller cannot silently
//! hand a length where an angle belongs, nor a raw magnitude where a `[0, 1]`
//! blend or draw belongs. Pure and branchless; the pole and near-parallel
//! degeneracies are resolved by table selection, not control flow.

use axiom_kernel::{Radians, Ratio};

use crate::vec3::Vec3;

/// Latitude of a unit direction (`y` is the pole axis). Range `[-π/2, π/2]`.
/// The input `y` is clamped into `[-1, 1]` before `asin`, so the result is
/// always finite.
pub fn latitude(dir: Vec3) -> Radians {
    Radians::finite_or_zero(dir.y.clamp(-1.0, 1.0).asin())
}

/// Longitude of a unit direction, measured in the `x`/`z` plane. Range
/// `(-π, π]`. Returns `0` at the poles where longitude is undefined
/// (`atan2(0, 0) == 0`, a well-defined fallback).
pub fn longitude(dir: Vec3) -> Radians {
    Radians::finite_or_zero(dir.z.atan2(dir.x))
}

/// Unit direction from latitude/longitude, matching [`latitude`] / [`longitude`]
/// (with `y` as the pole axis) so the round trip is consistent.
pub fn unit_dir_from_lat_lon(lat: Radians, lon: Radians) -> Vec3 {
    let (lat, lon) = (lat.get(), lon.get());
    let cl = lat.cos();
    Vec3::new(cl * lon.cos(), lat.sin(), cl * lon.sin())
}

/// Great-circle (angular) distance between two unit directions. The dot product
/// is clamped into `[-1, 1]` before `acos`, so the result is always finite and
/// lies in `[0, π]`.
pub fn great_circle_distance(a: Vec3, b: Vec3) -> Radians {
    Radians::finite_or_zero(a.dot(b).clamp(-1.0, 1.0).acos())
}

/// An east/north tangent basis at a unit direction. Hardened for the poles,
/// where `up × dir` degenerates: there the equatorial `+X` axis is chosen as a
/// deterministic east instead. Returns `(east, north)`, both unit and orthogonal
/// to `dir` and each other.
pub fn tangent_basis(dir: Vec3) -> (Vec3, Vec3) {
    let east_raw = Vec3::UNIT_Y.cross(dir);
    // At/near a pole `dir` is parallel to `+Y`, so `east_raw` collapses toward
    // zero and its normalization is meaningless. Select a fixed equatorial east
    // there via a table index rather than a branch.
    let degenerate = east_raw.length() < 1.0e-5;
    let east =
        [east_raw.normalize().unwrap_or(Vec3::UNIT_X), Vec3::UNIT_X][usize::from(degenerate)];
    // north = dir × east completes a right-handed frame; already unit when `dir`
    // and `east` are unit and orthogonal, but normalize for numerical safety.
    let north = dir.cross(east).normalize().unwrap_or(Vec3::UNIT_Z);
    (east, north)
}

/// Spherical linear interpolation between two unit directions. `t` in `[0, 1]`:
/// `t = 0 → a` (normalized), `t = 1 → b` (normalized); the result is unit. Falls
/// back to a normalized linear blend when the inputs are nearly parallel (or
/// antiparallel), where the spherical form is numerically unstable.
pub fn slerp(a: Vec3, b: Vec3, t: Ratio) -> Vec3 {
    let t = t.get();
    let an = a.normalize().unwrap_or(Vec3::UNIT_X);
    let bn = b.normalize().unwrap_or(Vec3::UNIT_X);

    let dot = an.dot(bn).clamp(-1.0, 1.0);
    let theta = dot.acos();
    let sin_theta = theta.sin();

    // Nearly (anti)parallel fallback: linear blend then renormalize. Endpoints
    // are preserved exactly enough for `t = 0` / `t = 1`; an exactly antiparallel
    // midpoint blends to zero and renormalization falls back to `an`.
    let blended = an
        .mul_scalar(1.0 - t)
        .add(bn.mul_scalar(t))
        .normalize()
        .unwrap_or(an);

    // General spherical case. When `sin_theta ≈ 0` these weights divide by ~0 and
    // `arced` becomes non-finite, but that value is discarded by the select below
    // (and would itself fall back to `an` via `unwrap_or`), so it never escapes.
    let w0 = ((1.0 - t) * theta).sin() / sin_theta;
    let w1 = (t * theta).sin() / sin_theta;
    let arced = an
        .mul_scalar(w0)
        .add(bn.mul_scalar(w1))
        .normalize()
        .unwrap_or(an);

    let parallel = sin_theta.abs() < 1.0e-5;
    [arced, blended][usize::from(parallel)]
}

/// A uniformly-distributed point on the unit sphere from two uniform draws.
/// `u` selects the height `z = 2·u − 1` (uniform in `[-1, 1]`) and `v` selects the
/// azimuth `θ = 2π·v` about `z`; the ring radius is `r = √(max(0, 1 − z²))`. Because
/// `z` is uniform in height (Archimedes' hat-box theorem), the returned direction is
/// *area-preserving* — uniform over the sphere's surface — with no rejection loop. The
/// `max(0, …)` only absorbs a sub-ULP negative at the poles (`z = ±1`), where the ring
/// collapses to the axis point `(0, 0, ±1)`. Pure and branchless.
pub fn unit_vec3(u: Ratio, v: Ratio) -> Vec3 {
    let z = 2.0 * u.get() - 1.0;
    let theta = core::f32::consts::TAU * v.get();
    let r = (1.0 - z * z).max(0.0).sqrt();
    Vec3::new(r * theta.cos(), r * theta.sin(), z)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PI: f32 = std::f32::consts::PI;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    fn vec_approx(a: Vec3, b: Vec3, eps: f32) -> bool {
        approx(a.x, b.x, eps) && approx(a.y, b.y, eps) && approx(a.z, b.z, eps)
    }

    fn rad(v: f32) -> Radians {
        Radians::new(v).unwrap()
    }

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    #[test]
    fn latitude_poles_and_equator() {
        assert!(approx(
            latitude(Vec3::new(0.0, 1.0, 0.0)).get(),
            PI / 2.0,
            1.0e-5
        ));
        assert!(approx(
            latitude(Vec3::new(0.0, -1.0, 0.0)).get(),
            -PI / 2.0,
            1.0e-5
        ));
        assert!(approx(
            latitude(Vec3::new(1.0, 0.0, 0.0)).get(),
            0.0,
            1.0e-5
        ));
    }

    #[test]
    fn latitude_non_finite_input_collapses_to_zero() {
        // A NaN component drives `asin` non-finite; `finite_or_zero` sanitizes it.
        assert_eq!(latitude(Vec3::new(0.0, f32::NAN, 0.0)).get(), 0.0);
    }

    #[test]
    fn longitude_axes() {
        assert!(approx(
            longitude(Vec3::new(1.0, 0.0, 0.0)).get(),
            0.0,
            1.0e-5
        ));
        assert!(approx(
            longitude(Vec3::new(0.0, 0.0, 1.0)).get(),
            PI / 2.0,
            1.0e-5
        ));
        assert!(approx(
            longitude(Vec3::new(-1.0, 0.0, 0.0)).get(),
            PI,
            1.0e-5
        ));
    }

    #[test]
    fn lat_lon_round_trip() {
        let cases = [(0.0_f32, 0.0_f32), (0.5, 1.0), (-0.8, -2.0), (0.3, 3.0)];
        for (lat, lon) in cases {
            let d = unit_dir_from_lat_lon(rad(lat), rad(lon));
            assert!(approx(d.length(), 1.0, 1.0e-5), "must be unit length");
            assert!(approx(latitude(d).get(), lat, 1.0e-5), "lat round trip");
            assert!(approx(longitude(d).get(), lon, 1.0e-5), "lon round trip");
        }
    }

    #[test]
    fn great_circle_basics() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        assert!(approx(great_circle_distance(a, a).get(), 0.0, 1.0e-5));
        assert!(approx(great_circle_distance(a, b).get(), PI / 2.0, 1.0e-5));
        assert!(approx(
            great_circle_distance(a, Vec3::new(-1.0, 0.0, 0.0)).get(),
            PI,
            1.0e-5
        ));
    }

    fn assert_orthonormal(dir: Vec3) {
        let (east, north) = tangent_basis(dir);
        let d = dir.normalize().unwrap_or(dir);
        assert!(approx(east.length(), 1.0, 1.0e-4), "east unit");
        assert!(approx(north.length(), 1.0, 1.0e-4), "north unit");
        assert!(approx(east.dot(north), 0.0, 1.0e-4), "east _|_ north");
        assert!(approx(east.dot(d), 0.0, 1.0e-4), "east _|_ dir");
        assert!(approx(north.dot(d), 0.0, 1.0e-4), "north _|_ dir");
    }

    #[test]
    fn tangent_basis_orthonormal_general() {
        // General (non-degenerate) arm of the east table-select.
        assert_orthonormal(Vec3::new(1.0, 0.0, 0.0));
        assert_orthonormal(Vec3::new(0.3, 0.4, 0.5).normalize().unwrap());
        assert_orthonormal(Vec3::new(-0.6, 0.2, 0.7).normalize().unwrap());
    }

    #[test]
    fn tangent_basis_orthonormal_at_poles() {
        // Degenerate arm of the east table-select: both poles pick the equatorial
        // `+X` east explicitly, then complete an orthonormal frame.
        assert_orthonormal(Vec3::new(0.0, 1.0, 0.0));
        assert_orthonormal(Vec3::new(0.0, -1.0, 0.0));
        let (east_north, _) = tangent_basis(Vec3::new(0.0, 1.0, 0.0));
        assert!(
            vec_approx(east_north, Vec3::UNIT_X, 1.0e-6),
            "pole east is +X"
        );
    }

    #[test]
    fn slerp_endpoints() {
        // Non-parallel arm: `t = 0 -> a`, `t = 1 -> b`.
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        assert!(vec_approx(slerp(a, b, ratio(0.0)), a, 1.0e-5), "t=0 -> a");
        assert!(vec_approx(slerp(a, b, ratio(1.0)), b, 1.0e-5), "t=1 -> b");
    }

    #[test]
    fn slerp_midpoint_is_unit_and_between() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        let m = slerp(a, b, ratio(0.5));
        assert!(approx(m.length(), 1.0, 1.0e-5), "result unit");
        // Halfway along a 90-degree arc: 45 deg from each endpoint.
        assert!(approx(great_circle_distance(a, m).get(), PI / 4.0, 1.0e-4));
        assert!(approx(great_circle_distance(b, m).get(), PI / 4.0, 1.0e-4));
    }

    #[test]
    fn slerp_parallel_inputs_stable() {
        // Parallel arm of the select: identical inputs blend to themselves.
        let a = Vec3::new(0.0, 0.0, 1.0);
        let m = slerp(a, a, ratio(0.5));
        assert!(approx(m.length(), 1.0, 1.0e-5));
        assert!(vec_approx(m, a, 1.0e-5));
    }

    #[test]
    fn slerp_antiparallel_falls_back_to_a() {
        // Antiparallel (`theta = π`, `sin_theta ≈ 0`) selects the blend arm; the
        // midpoint blends to zero, so renormalization falls back to `a`.
        let a = Vec3::new(0.0, 0.0, 1.0);
        let b = Vec3::new(0.0, 0.0, -1.0);
        let m = slerp(a, b, ratio(0.5));
        assert!(vec_approx(m, a, 1.0e-5));
    }

    #[test]
    fn slerp_is_deterministic() {
        let a = Vec3::new(0.2, 0.9, -0.1);
        let b = Vec3::new(-0.5, 0.3, 0.8);
        assert_eq!(slerp(a, b, ratio(0.37)), slerp(a, b, ratio(0.37)));
    }

    #[test]
    fn unit_vec3_is_unit_length_across_the_square() {
        // Sweep the (u, v) unit square, including the poles u -> 0 and u -> 1 and
        // the azimuth wrap v -> 0 / v -> 1. Every draw must land on the unit sphere.
        let samples = [0.0f32, 0.001, 0.25, 0.5, 0.75, 0.999, 1.0];
        for &uu in &samples {
            for &vv in &samples {
                let p = unit_vec3(ratio(uu), ratio(vv));
                assert!(
                    approx(p.length(), 1.0, 1.0e-4),
                    "must be unit length on the sphere"
                );
            }
        }
    }

    #[test]
    fn unit_vec3_poles_are_the_axis() {
        // u = 1 -> z = +1, ring radius 0 -> the +Z pole regardless of azimuth;
        // u = 0 -> z = -1 -> the -Z pole. This exercises the `max(0, 1 - z^2)`
        // collapse at both extremes.
        assert!(vec_approx(
            unit_vec3(ratio(1.0), ratio(0.3)),
            Vec3::new(0.0, 0.0, 1.0),
            1.0e-5
        ));
        assert!(vec_approx(
            unit_vec3(ratio(0.0), ratio(0.7)),
            Vec3::new(0.0, 0.0, -1.0),
            1.0e-5
        ));
    }

    #[test]
    fn unit_vec3_maps_draws_to_height_and_azimuth() {
        // The equator (u = 0.5 -> z = 0) at azimuth 0 (v = 0) is +X; a quarter turn
        // (v = 0.25 -> θ = π/2) is +Y. Ties the two uniforms to z and θ concretely.
        assert!(vec_approx(
            unit_vec3(ratio(0.5), ratio(0.0)),
            Vec3::new(1.0, 0.0, 0.0),
            1.0e-5
        ));
        assert!(vec_approx(
            unit_vec3(ratio(0.5), ratio(0.25)),
            Vec3::new(0.0, 1.0, 0.0),
            1.0e-5
        ));
    }

    #[test]
    fn unit_vec3_is_deterministic() {
        assert_eq!(
            unit_vec3(ratio(0.31), ratio(0.62)),
            unit_vec3(ratio(0.31), ratio(0.62))
        );
    }
}
