//! The scene's authored curve paths, and the one place a curve failure becomes a
//! mesh failure.
//!
//! Three things in this scene ride a path — the road, the tunnel that arches
//! over a section of it, and every tree trunk — and all three want the same two
//! guarantees: the control points are authored literals (or a pure function of
//! an object index), and a curve that cannot be built is reported as a geometry
//! error rather than silently substituted. Both live here so the builders below
//! only ever see a `MeshResult<Curve>`.
//!
//! The tunnel path is **derived from the road path** rather than authored
//! separately: it is six positions read off the road curve in a fixed parameter
//! window and lifted. That is deliberate — an independently authored tunnel
//! would drift away from the road the moment either was edited, and the drift
//! would be invisible until somebody looked at the render.

use axiom_math::{Curve, Vec3};
use axiom_mesh::{MeshError, MeshErrorCode, MeshResult};

use crate::quantities::ratio;

/// The road's control points. Catmull-Rom interpolates through the interior
/// points, so the first and last are shaping handles the curve does not reach.
const ROAD_CONTROLS: [[f32; 3]; 8] = [
    [-34.0, 2.6, -92.0],
    [-26.0, 2.8, -66.0],
    [-6.0, 3.2, -36.0],
    [8.0, 3.6, -6.0],
    [3.0, 3.8, 22.0],
    [-12.0, 3.4, 48.0],
    [-2.0, 3.0, 72.0],
    [14.0, 2.8, 96.0],
];

/// The parameters along the road the tunnel's control points are read at. The
/// tunnel therefore spans the road's `0.42..0.66` window (Catmull-Rom reaches
/// only its interior controls).
const TUNNEL_STATIONS: [f32; 6] = [0.34, 0.42, 0.50, 0.58, 0.66, 0.74];

/// How far above the road surface the tunnel's path sits, so the arch springs
/// from the road edge rather than from inside the slab.
const TUNNEL_LIFT: f32 = 0.35;

/// The road: a Catmull-Rom spline that climbs, falls and changes direction, so
/// the sweep's rotation-minimising frames have something to actually minimise.
pub fn road_curve() -> MeshResult<Curve> {
    Curve::catmull_rom(
        ROAD_CONTROLS
            .iter()
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect(),
    )
    .map_err(|_| invalid_path("the authored road control points form a valid Catmull-Rom spline"))
}

/// The tunnel: six stations read off `road`, lifted, re-splined. The tunnel
/// therefore follows the road by construction.
pub fn tunnel_curve(road: &Curve) -> MeshResult<Curve> {
    Curve::catmull_rom(
        TUNNEL_STATIONS
            .iter()
            .map(|t| {
                road.position_at(ratio(*t))
                    .add(Vec3::new(0.0, TUNNEL_LIFT, 0.0))
            })
            .collect(),
    )
    .map_err(|_| invalid_path("six lifted road stations form a valid tunnel spline"))
}

/// A tree trunk's path: a short, slightly leaning, slightly curved spline whose
/// lean and height are a pure function of `index`, so four trees differ without
/// a single random number between them.
pub fn trunk_curve(index: u32, height: f32) -> MeshResult<Curve> {
    let lean = 0.35 + 0.22 * (index % 3) as f32;
    let bend = [1.0, -1.0][(index % 2) as usize];
    Curve::catmull_rom(vec![
        Vec3::new(0.0, -0.2 * height, 0.0),
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(bend * lean * 0.4, height * 0.38, lean * 0.25),
        Vec3::new(bend * lean, height * 0.74, lean * 0.1),
        Vec3::new(bend * lean * 1.15, height, -lean * 0.2),
        Vec3::new(bend * lean * 1.25, height * 1.25, -lean * 0.5),
    ])
    .map_err(|_| invalid_path("an authored trunk spline is valid"))
}

/// Read a position off a curve at an authored parameter.
pub fn point_on(curve: &Curve, t: f32) -> Vec3 {
    curve.position_at(ratio(t))
}

/// The unit tangent at an authored parameter, or `+Z` where the curve has none.
pub fn heading_on(curve: &Curve, t: f32) -> Vec3 {
    curve.tangent_at(ratio(t)).unwrap_or(Vec3::UNIT_Z)
}

/// A curve failure, reported in the mesh layer's vocabulary.
fn invalid_path(message: &'static str) -> MeshError {
    MeshError::new(MeshErrorCode::InvalidPath, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tunnel_follows_the_road_it_was_derived_from() {
        let road = road_curve().expect("the road curve builds");
        let tunnel = tunnel_curve(&road).expect("the tunnel curve builds");
        // Catmull-Rom reaches its interior controls, so the tunnel's own start
        // is exactly the road station it was read from, lifted.
        let mouth = point_on(&road, TUNNEL_STATIONS[1]);
        let tunnel_start = point_on(&tunnel, 0.0);
        let horizontal =
            (mouth.x - tunnel_start.x).abs() + (mouth.z - tunnel_start.z).abs();
        assert!(horizontal < 0.01, "tunnel mouth drifted {horizontal} from the road");
        assert!((tunnel_start.y - mouth.y - TUNNEL_LIFT).abs() < 0.01);
        // And the whole tunnel stays inside the road's own footprint.
        let road_end = point_on(&road, TUNNEL_STATIONS[4]);
        let far = point_on(&tunnel, 1.0);
        assert!((far.x - road_end.x).abs() + (far.z - road_end.z).abs() < 0.01);
        assert!(heading_on(&road, 0.5).length() > 0.5);
    }
}
