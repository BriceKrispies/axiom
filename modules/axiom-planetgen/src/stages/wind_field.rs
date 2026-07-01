//! `wind_field`: prevailing wind direction per region from latitude banding
//! (trade winds / westerlies / polar easterlies).
//!
//! Each region's wind is a unit tangent vector built from the east/north basis at
//! its site. Idealised Earth bands set the zonal/meridional mix. Writes unit
//! `region_wind`. Branchless: table-selected band coefficients, per-region `map`.

use axiom_math::{latitude, tangent_basis, Vec3};

use crate::globe::PlanetGlobe;

const DEG: f32 = core::f32::consts::PI / 180.0;

/// Prevailing wind (unit tangent the wind blows toward) at a site direction.
fn wind_at(site: Vec3) -> Vec3 {
    let lat = latitude(site).get();
    let abs_lat = lat.abs();
    let (east, north) = tangent_basis(site);

    // Zonal band: |lat| < 30° trade (-0.9); < 60° westerly (0.9); else polar (-0.6).
    let trade = abs_lat < 30.0 * DEG;
    let temperate = abs_lat < 60.0 * DEG;
    let zonal = [[-0.6_f32, 0.9][usize::from(temperate)], -0.9][usize::from(trade)];

    // Small meridional drift toward the equator, only in the trade belt.
    let merid_trade = [0.3_f32, -0.3][usize::from(lat >= 0.0)];
    let merid = [0.0_f32, merid_trade][usize::from(trade)];

    let wind = east.mul_scalar(zonal).add(north.mul_scalar(merid));
    wind.normalize().unwrap_or(east)
}

pub(crate) fn wind_field(globe: &mut PlanetGlobe) {
    let region_count = globe.region_count();
    let region_wind: Vec<Vec3> = (0..region_count)
        .map(|r| wind_at(globe.topology.sites[r]))
        .collect();
    globe.region_wind = region_wind;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph};

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
        let normed: Vec<Vec3> = [
            Vec3::new(1.0, 0.0, 0.0),  // equator (trade, lat == 0 → north drift arm)
            Vec3::new(0.9, -0.2, 0.0), // southern trade belt (lat < 0 → south drift arm)
            Vec3::new(0.5, 0.7, 0.5),  // mid-lat north (westerly)
            Vec3::new(0.2, -0.9, 0.2), // high-lat south (polar easterly)
            Vec3::new(0.0, 0.0, 1.0),  // equator (trade)
        ]
        .into_iter()
        .map(|s| s.normalize().unwrap_or(Vec3::new(1.0, 0.0, 0.0)))
        .collect();
        let mut g = globe_with_sites(normed.clone());
        wind_field(&mut g);
        g.region_wind.iter().enumerate().for_each(|(r, w)| {
            assert!((w.length() - 1.0).abs() < 1.0e-3, "wind not unit at {r}");
            assert!(w.dot(normed[r]).abs() < 1.0e-2, "wind not tangent at {r}");
        });
    }

    #[test]
    fn deterministic_same_input() {
        let sites: Vec<Vec3> = (0..16)
            .map(|i| {
                Vec3::new((i as f32).sin(), (i as f32 * 0.3).cos(), (i as f32).cos())
                    .normalize()
                    .unwrap_or(Vec3::new(1.0, 0.0, 0.0))
            })
            .collect();
        let mut a = globe_with_sites(sites.clone());
        let mut b = globe_with_sites(sites);
        wind_field(&mut a);
        wind_field(&mut b);
        assert_eq!(a.region_wind, b.region_wind);
    }
}
