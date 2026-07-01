//! `plate_properties` stage: classify plates oceanic/continental and seed a base
//! elevation per region from its plate type.
//! Audit: worldgen `plate_properties`; OW elevation baseline.
//!
//! ~40% of plates are marked oceanic deterministically (a hashed-per-plate
//! threshold from the seed). Continental regions get a positive base elevation,
//! oceanic regions a negative one, giving `elevation` a tectonic floor before
//! the `elevation` stage adds boundary uplift + noise detail.

use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{worldgen_stream, GenContext, Stage};

/// Fraction of plates that are oceanic. Audit: ~40% oceanic plates.
const OCEANIC_FRACTION: f32 = 0.40;
/// Base elevation handed to continental regions (above sea level 0).
const CONTINENTAL_BASE: f32 = 0.30;
/// Base elevation handed to oceanic regions (below sea level 0).
const OCEANIC_BASE: f32 = -0.50;

pub struct PlatePropertiesStage;

impl Stage for PlatePropertiesStage {
    fn id(&self) -> &'static str {
        "plate_properties"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let region_count = globe.region_count();
        let plate_count = globe
            .region_plate
            .iter()
            .copied()
            .max()
            .map(|m| m as usize + 1)
            .unwrap_or(0);

        // Deterministic per-plate oceanic flag: hash the plate index off the seed
        // and threshold so ~OCEANIC_FRACTION of plates are oceanic.
        let base = worldgen_stream(ctx.seed).fork(0x_0CEA_0CEA);
        globe.plate_oceanic.clear();
        globe.plate_oceanic.resize(plate_count, false);
        let mut oceanic_count = 0usize;
        for p in 0..plate_count {
            let mut prng = base.fork(p as u64);
            let oceanic = prng.unit().get() < OCEANIC_FRACTION;
            globe.plate_oceanic[p] = oceanic;
            if oceanic {
                oceanic_count += 1;
            }
        }

        if globe.region_elevation.len() != region_count {
            globe.region_elevation.resize(region_count, 0.0);
        }

        for r in 0..region_count {
            let plate = globe.region_plate[r] as usize;
            let oceanic = globe.plate_oceanic.get(plate).copied().unwrap_or(false);
            globe.region_elevation[r] = if oceanic {
                OCEANIC_BASE
            } else {
                CONTINENTAL_BASE
            };
        }

        ctx.log.push(format!(
            "plate_properties: {}/{} plates oceanic",
            oceanic_count, plate_count
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::model_planet::{Icosphere, RegionGraph};
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
        let mut ctx = GenContext::new(77);
        PlatePropertiesStage.run(&mut g, &mut ctx);
        assert_eq!(g.plate_oceanic.len(), 3);
        for r in 0..g.region_count() {
            let plate = g.region_plate[r] as usize;
            if g.plate_oceanic[plate] {
                assert!(g.region_elevation[r] < 0.0);
            } else {
                assert!(g.region_elevation[r] >= 0.0);
            }
        }
    }

    #[test]
    fn roughly_forty_percent_oceanic() {
        let plates: Vec<u32> = (0..100).collect();
        let mut g = globe_with_plates(plates);
        let mut ctx = GenContext::new(2024);
        PlatePropertiesStage.run(&mut g, &mut ctx);
        let oceanic = g.plate_oceanic.iter().filter(|&&o| o).count();
        // 100 plates, target 40% — allow a generous band for the hash draw.
        assert!(
            (25..=55).contains(&oceanic),
            "oceanic count {} not near 40%",
            oceanic
        );
    }

    #[test]
    fn deterministic_same_seed() {
        let plates: Vec<u32> = (0..40).map(|i| i % 8).collect();
        let mut a = globe_with_plates(plates.clone());
        let mut b = globe_with_plates(plates);
        let mut ca = GenContext::new(9);
        let mut cb = GenContext::new(9);
        PlatePropertiesStage.run(&mut a, &mut ca);
        PlatePropertiesStage.run(&mut b, &mut cb);
        assert_eq!(a.plate_oceanic, b.plate_oceanic);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
