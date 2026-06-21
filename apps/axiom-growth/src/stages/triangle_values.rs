//! `triangle_values` stage: average per-region scalars onto triangle faces.
//! Audit: worldgen `triangle_values`; feeds terrain-mesh / river export.
//!
//! Each triangle is the dual of three regions; its elevation is the mean of its
//! three corner regions' elevations. (River flow is filled later by the river
//! stages.)

use crate::model_planet::PlanetGlobe;
use crate::pipeline::{GenContext, Stage};

pub struct TriangleValuesStage;

impl Stage for TriangleValuesStage {
    fn id(&self) -> &'static str {
        "triangle_values"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let tri_count = globe.topology.triangles.len();
        if globe.triangle_elevation.len() != tri_count {
            globe.triangle_elevation.resize(tri_count, 0.0);
        }
        let region_count = globe.region_count();

        for t in 0..tri_count {
            let [a, b, c] = globe.topology.triangles[t];
            let (a, b, c) = (a as usize, b as usize, c as usize);
            if a >= region_count || b >= region_count || c >= region_count {
                globe.triangle_elevation[t] = 0.0;
                continue;
            }
            let sum =
                globe.region_elevation[a] + globe.region_elevation[b] + globe.region_elevation[c];
            globe.triangle_elevation[t] = sum / 3.0;
        }

        ctx.log
            .push(format!("triangle_values: {} triangles", tri_count));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_planet::{Icosphere, RegionGraph};
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
        let mut ctx = GenContext::new(1);
        TriangleValuesStage.run(&mut g, &mut ctx);
        assert_eq!(g.triangle_elevation[0], (0.0 + 3.0 + 6.0) / 3.0);
        assert_eq!(g.triangle_elevation[1], (3.0 + 6.0 + 9.0) / 3.0);
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = tri_globe();
        let mut b = tri_globe();
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        TriangleValuesStage.run(&mut a, &mut ca);
        TriangleValuesStage.run(&mut b, &mut cb);
        assert_eq!(a.triangle_elevation, b.triangle_elevation);
    }
}
