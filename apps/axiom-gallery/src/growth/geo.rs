//! Spherical / geodesic math the planet substrate needs.
//! Audit: "Procedural-generation math" — lat/long, great-circle, tangent frames.
use axiom_math::Vec3;

/// Latitude in radians from a unit direction (y is the pole axis).
/// Range: [-pi/2, pi/2].
pub fn latitude(dir: Vec3) -> f32 {
    dir.y.clamp(-1.0, 1.0).asin()
}

/// Longitude in radians from a unit direction, measured in the x/z plane.
/// Range: (-pi, pi]. Returns 0 at the poles where longitude is undefined.
pub fn longitude(dir: Vec3) -> f32 {
    // atan2(z, x) gives a stable longitude that wraps at +/- pi. At the poles
    // x and z are ~0, where atan2(0, 0) == 0 — a well-defined fallback.
    dir.z.atan2(dir.x)
}

/// Unit direction from latitude/longitude (radians). y is the pole axis,
/// matching `latitude`/`longitude` above so the round trip is consistent.
pub fn unit_dir_from_lat_lon(lat: f32, lon: f32) -> Vec3 {
    let cl = lat.cos();
    Vec3::new(cl * lon.cos(), lat.sin(), cl * lon.sin())
}

/// Great-circle (angular) distance between two unit directions, radians.
pub fn great_circle_distance(a: Vec3, b: Vec3) -> f32 {
    a.dot(b).clamp(-1.0, 1.0).acos()
}

/// An east/north tangent basis at a unit direction. Hardened for pole cases:
/// near the poles `up x dir` degenerates, so we fall back to a stable axis.
/// Returns (east, north), both unit and orthogonal to `dir` and each other.
pub fn tangent_basis(dir: Vec3) -> (Vec3, Vec3) {
    let up = Vec3::new(0.0, 1.0, 0.0);
    let east_raw = up.cross(dir);
    let east = if east_raw.length() < 1.0e-5 {
        // At/near a pole, `dir` is parallel to `up`; choose a deterministic
        // east axis in the equatorial plane.
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        east_raw.normalize().unwrap_or(Vec3::new(1.0, 0.0, 0.0))
    };
    // north = dir x east completes a right-handed frame; it is already unit
    // when `dir` and `east` are unit and orthogonal, but normalize for safety.
    let north = dir
        .cross(east)
        .normalize()
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    (east, north)
}

/// Spherical linear interpolation between two unit directions.
/// `t` in [0,1]: t=0 -> a (normalized), t=1 -> b (normalized). Result is unit.
/// Falls back to normalized linear interpolation when the inputs are nearly
/// parallel (where slerp is numerically unstable).
pub fn slerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    let an = a.normalize().unwrap_or(Vec3::UNIT_X);
    let bn = b.normalize().unwrap_or(Vec3::UNIT_X);

    let dot = an.dot(bn).clamp(-1.0, 1.0);
    let theta = dot.acos();
    let sin_theta = theta.sin();

    if sin_theta.abs() < 1.0e-5 {
        // Nearly parallel (or antiparallel within tolerance): linear blend,
        // then renormalize. Endpoints are preserved exactly enough for t=0/1.
        let blended = an.mul_scalar(1.0 - t).add(bn.mul_scalar(t));
        return blended.normalize().unwrap_or(an);
    }

    let w0 = ((1.0 - t) * theta).sin() / sin_theta;
    let w1 = (t * theta).sin() / sin_theta;
    let out = an.mul_scalar(w0).add(bn.mul_scalar(w1));
    out.normalize().unwrap_or(an)
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

    #[test]
    fn latitude_poles_and_equator() {
        assert!(approx(latitude(Vec3::new(0.0, 1.0, 0.0)), PI / 2.0, 1.0e-5));
        assert!(approx(
            latitude(Vec3::new(0.0, -1.0, 0.0)),
            -PI / 2.0,
            1.0e-5
        ));
        assert!(approx(latitude(Vec3::new(1.0, 0.0, 0.0)), 0.0, 1.0e-5));
    }

    #[test]
    fn longitude_axes() {
        assert!(approx(longitude(Vec3::new(1.0, 0.0, 0.0)), 0.0, 1.0e-5));
        assert!(approx(
            longitude(Vec3::new(0.0, 0.0, 1.0)),
            PI / 2.0,
            1.0e-5
        ));
        assert!(approx(longitude(Vec3::new(-1.0, 0.0, 0.0)), PI, 1.0e-5));
    }

    #[test]
    fn lat_lon_round_trip() {
        let cases = [(0.0_f32, 0.0_f32), (0.5, 1.0), (-0.8, -2.0), (0.3, 3.0)];
        for (lat, lon) in cases {
            let d = unit_dir_from_lat_lon(lat, lon);
            assert!(approx(d.length(), 1.0, 1.0e-5), "must be unit length");
            assert!(approx(latitude(d), lat, 1.0e-5), "lat round trip");
            assert!(approx(longitude(d), lon, 1.0e-5), "lon round trip");
        }
    }

    #[test]
    fn great_circle_basics() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        assert!(approx(great_circle_distance(a, a), 0.0, 1.0e-5));
        assert!(approx(great_circle_distance(a, b), PI / 2.0, 1.0e-5));
        assert!(approx(
            great_circle_distance(a, Vec3::new(-1.0, 0.0, 0.0)),
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
        assert_orthonormal(Vec3::new(1.0, 0.0, 0.0));
        assert_orthonormal(Vec3::new(0.3, 0.4, 0.5).normalize().unwrap());
        assert_orthonormal(Vec3::new(-0.6, 0.2, 0.7).normalize().unwrap());
    }

    #[test]
    fn tangent_basis_orthonormal_at_poles() {
        // North pole and south pole are the degenerate cases.
        assert_orthonormal(Vec3::new(0.0, 1.0, 0.0));
        assert_orthonormal(Vec3::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn slerp_endpoints() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        assert!(vec_approx(slerp(a, b, 0.0), a, 1.0e-5), "t=0 -> a");
        assert!(vec_approx(slerp(a, b, 1.0), b, 1.0e-5), "t=1 -> b");
    }

    #[test]
    fn slerp_midpoint_is_unit_and_between() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        let m = slerp(a, b, 0.5);
        assert!(approx(m.length(), 1.0, 1.0e-5), "result unit");
        // Halfway along a 90-degree arc: 45 deg from each endpoint.
        assert!(approx(great_circle_distance(a, m), PI / 4.0, 1.0e-4));
        assert!(approx(great_circle_distance(b, m), PI / 4.0, 1.0e-4));
    }

    #[test]
    fn slerp_parallel_inputs_stable() {
        let a = Vec3::new(0.0, 0.0, 1.0);
        let m = slerp(a, a, 0.5);
        assert!(approx(m.length(), 1.0, 1.0e-5));
        assert!(vec_approx(m, a, 1.0e-5));
    }

    #[test]
    fn slerp_is_deterministic() {
        let a = Vec3::new(0.2, 0.9, -0.1);
        let b = Vec3::new(-0.5, 0.3, 0.8);
        assert_eq!(slerp(a, b, 0.37), slerp(a, b, 0.37));
    }
}
