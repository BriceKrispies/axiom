//! `triangle_values`: average per-region elevation onto triangle faces.
//!
//! Each triangle is the dual of three regions; its elevation is the mean of its
//! three corner regions' elevations (0 when a corner index is out of range).
//! Branchless via [`crate::globe::corner_mean`].

use crate::globe::{corner_mean, PlanetGlobe};

pub(crate) fn triangle_values(globe: &mut PlanetGlobe) {
    let triangles = &globe.topology.triangles;
    let triangle_elevation: Vec<f32> = triangles
        .iter()
        .map(|&tri| corner_mean(&globe.region_elevation, tri))
        .collect();
    globe.triangle_elevation = triangle_elevation;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    fn tri_globe() -> PlanetGlobe {
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites: vec![Vec3::new(1.0, 0.0, 0.0); 4],
                triangles: vec![[0, 1, 2], [1, 2, 3]],
                subdivisions: 0,
            },
            graph: RegionGraph::default(),
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        g.region_elevation = vec![0.0, 3.0, 6.0, 9.0];
        g
    }

    #[test]
    fn averages_corner_regions() {
        let mut g = tri_globe();
        triangle_values(&mut g);
        assert_eq!(g.triangle_elevation[0], (0.0 + 3.0 + 6.0) / 3.0);
        assert_eq!(g.triangle_elevation[1], (3.0 + 6.0 + 9.0) / 3.0);
    }

    #[test]
    fn deterministic_same_input() {
        let mut a = tri_globe();
        let mut b = tri_globe();
        triangle_values(&mut a);
        triangle_values(&mut b);
        assert_eq!(a.triangle_elevation, b.triangle_elevation);
    }
}
