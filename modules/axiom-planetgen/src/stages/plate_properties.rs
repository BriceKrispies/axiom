//! `plate_properties`: classify plates oceanic/continental and seed a base
//! elevation per region from its plate type.
//!
//! ~40% of plates are marked oceanic deterministically (a per-plate hashed fork
//! of the seed). Continental regions get a positive base elevation, oceanic ones
//! a negative one — the tectonic floor `elevation` then builds on. Branchless:
//! per-plate and per-region `map`s with a table-select.

use axiom_entropy::EntropyStream;

use crate::globe::PlanetGlobe;

/// Fraction of plates that are oceanic.
const OCEANIC_FRACTION: f32 = 0.40;
/// Base elevation handed to continental regions (above sea level 0).
const CONTINENTAL_BASE: f32 = 0.30;
/// Base elevation handed to oceanic regions (below sea level 0).
const OCEANIC_BASE: f32 = -0.50;

pub(crate) fn plate_properties(globe: &mut PlanetGlobe, root: &EntropyStream) {
    let region_count = globe.region_count();
    let plate_count = globe
        .region_plate
        .iter()
        .copied()
        .max()
        .map(|m| m as usize + 1)
        .unwrap_or(0);

    let base = root.fork(0x_0CEA_0CEA);
    let plate_oceanic: Vec<bool> = (0..plate_count)
        .map(|p| base.fork(p as u64).unit().get() < OCEANIC_FRACTION)
        .collect();

    let region_elevation: Vec<f32> = (0..region_count)
        .map(|r| {
            let plate = globe.region_plate[r] as usize;
            let oceanic = plate_oceanic.get(plate).copied().unwrap_or(false);
            [CONTINENTAL_BASE, OCEANIC_BASE][usize::from(oceanic)]
        })
        .collect();

    globe.plate_oceanic = plate_oceanic;
    globe.region_elevation = region_elevation;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_gen_api::worldgen_stream;
    use axiom_geosphere::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    fn globe_with_plates(plates: Vec<u32>) -> PlanetGlobe {
        let n = plates.len();
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites: vec![Vec3::new(1.0, 0.0, 0.0); n],
                triangles: Vec::new(),
                subdivisions: 0,
            },
            graph: RegionGraph::default(),
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        g.region_plate = plates;
        g
    }

    #[test]
    fn elevation_sign_matches_plate_type() {
        let mut g = globe_with_plates(vec![0, 0, 1, 1, 2, 2]);
        plate_properties(&mut g, &worldgen_stream(77));
        assert_eq!(g.plate_oceanic.len(), 3);
        (0..g.region_count()).for_each(|r| {
            let plate = g.region_plate[r] as usize;
            let expect_land = !g.plate_oceanic[plate];
            assert_eq!(g.region_elevation[r] >= 0.0, expect_land);
        });
    }

    #[test]
    fn empty_region_plate_yields_no_plates() {
        let mut g = globe_with_plates(Vec::new());
        plate_properties(&mut g, &worldgen_stream(1));
        assert!(g.plate_oceanic.is_empty());
        assert!(g.region_elevation.is_empty());
    }

    #[test]
    fn roughly_forty_percent_oceanic() {
        let plates: Vec<u32> = (0..100).collect();
        let mut g = globe_with_plates(plates);
        plate_properties(&mut g, &worldgen_stream(2024));
        let oceanic = g.plate_oceanic.iter().filter(|&&o| o).count();
        assert!((25..=55).contains(&oceanic), "oceanic count {oceanic}");
    }

    #[test]
    fn deterministic_same_seed() {
        let plates: Vec<u32> = (0..40).map(|i| i % 8).collect();
        let mut a = globe_with_plates(plates.clone());
        let mut b = globe_with_plates(plates);
        plate_properties(&mut a, &worldgen_stream(9));
        plate_properties(&mut b, &worldgen_stream(9));
        assert_eq!(a.plate_oceanic, b.plate_oceanic);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
