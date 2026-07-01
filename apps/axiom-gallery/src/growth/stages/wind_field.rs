//! `wind_field` stage: prevailing wind direction per region from latitude
//! banding (trade winds / westerlies / polar easterlies).
//!
//! Each region's wind is a unit tangent vector built from the east/north basis
//! at its site direction. Idealised Earth bands set the zonal/meridional mix:
//! - |lat| < 30 deg  : trade winds, blowing toward the west (east-to-west).
//! - 30 .. 60 deg    : westerlies, blowing toward the east.
//! - > 60 deg        : polar easterlies, toward the west.
//!
//! Writes unit `region_wind` (the tangent direction the wind blows toward).

use axiom_math::{latitude, tangent_basis};

use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};

use axiom_math::Vec3;

const DEG: f32 = core::f32::consts::PI / 180.0;

pub struct WindFieldStage;

impl Stage for WindFieldStage {
    fn id(&self) -> &'static str {
        "wind_field"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let region_count = globe.region_count();
        if globe.region_wind.len() != region_count {
            globe
                .region_wind
                .resize(region_count, Vec3::new(1.0, 0.0, 0.0));
        }

        for r in 0..region_count {
            let site = globe.topology.sites[r];
            let lat = latitude(site).get();
            let abs_lat = lat.abs();
            let (east, north) = tangent_basis(site);

            // +east = blowing east; trade winds & polar easterlies blow west.
            let zonal = if abs_lat < 30.0 * DEG {
                -0.9 // trade winds (easterlies)
            } else if abs_lat < 60.0 * DEG {
                0.9 // westerlies
            } else {
                -0.6 // polar easterlies
            };
            // Small meridional drift toward the equator in the trade belt.
            let merid = if abs_lat < 30.0 * DEG {
                if lat >= 0.0 {
                    -0.3
                } else {
                    0.3
                }
            } else {
                0.0
            };

            let wind = east.mul_scalar(zonal).add(north.mul_scalar(merid));
            globe.region_wind[r] = wind.normalize().unwrap_or(east);
        }

        ctx.log
            .push("wind_field: latitude-banded winds".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::model_planet::{Icosphere, RegionGraph};

    fn globe_with_sites(sites: Vec<Vec3>) -> PlanetGlobe {
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites,
                triangles: Vec::new(),
                subdivisions: 0,
            },
            graph: RegionGraph::default(),
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        g
    }

    #[test]
    fn winds_are_unit_and_tangent() {
        let sites = vec![
            Vec3::new(1.0, 0.0, 0.0),  // equator
            Vec3::new(0.5, 0.7, 0.5),  // mid-lat north
            Vec3::new(0.2, -0.9, 0.2), // high-lat south
            Vec3::new(0.0, 0.0, 1.0),  // equator
        ];
        let normed: Vec<Vec3> = sites
            .into_iter()
            .map(|s| s.normalize().unwrap_or(Vec3::new(1.0, 0.0, 0.0)))
            .collect();
        let mut g = globe_with_sites(normed.clone());
        let mut ctx = GenContext::new(1);
        WindFieldStage.run(&mut g, &mut ctx);
        for (r, w) in g.region_wind.iter().enumerate() {
            assert!((w.length() - 1.0).abs() < 1.0e-3, "wind not unit at {}", r);
            assert!(w.dot(normed[r]).abs() < 1.0e-2, "wind not tangent at {}", r);
        }
    }

    #[test]
    fn deterministic_same_seed() {
        let sites: Vec<Vec3> = (0..16)
            .map(|i| {
                Vec3::new((i as f32).sin(), (i as f32 * 0.3).cos(), (i as f32).cos())
                    .normalize()
                    .unwrap_or(Vec3::new(1.0, 0.0, 0.0))
            })
            .collect();
        let mut a = globe_with_sites(sites.clone());
        let mut b = globe_with_sites(sites);
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        WindFieldStage.run(&mut a, &mut ca);
        WindFieldStage.run(&mut b, &mut cb);
        assert_eq!(a.region_wind, b.region_wind);
    }
}
