//! `tectonic_plates` stage: spherical-Voronoi partition of regions into plates.
//! Audit: worldgen `tectonic_plates`; OW deterministic overworld.
//!
//! Picks `ctx.plate_count` plate seed directions (deterministic unit vectors via
//! [`Rng`]) and assigns every region to the nearest seed by maximum dot product
//! against the region site direction. Fills `region_plate`.

use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};
use crate::growth::rng::Rng;

use axiom_math::Vec3;

pub struct TectonicPlatesStage;

impl Stage for TectonicPlatesStage {
    fn id(&self) -> &'static str {
        "tectonic_plates"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let plate_count = ctx.plate_count.max(1) as usize;
        let mut rng = Rng::seeded(ctx.seed).fork(0x_71A7_E5ED);

        // Deterministic plate seed directions on the sphere.
        let mut seeds: Vec<Vec3> = Vec::with_capacity(plate_count);
        for _ in 0..plate_count {
            seeds.push(rng.next_unit_vec3());
        }

        let region_count = globe.region_count();
        if globe.region_plate.len() != region_count {
            globe.region_plate.resize(region_count, 0);
        }

        // Nearest-seed assignment: spherical Voronoi by max dot.
        for r in 0..region_count {
            let site = globe.topology.sites[r];
            let mut best = 0usize;
            let mut best_dot = f32::NEG_INFINITY;
            for (p, seed) in seeds.iter().enumerate() {
                let d = site.dot(*seed);
                if d > best_dot {
                    best_dot = d;
                    best = p;
                }
            }
            globe.region_plate[r] = best as u32;
        }

        ctx.log
            .push(format!("tectonic_plates: {} plates", plate_count));
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

    fn ctx_with(plate_count: u32) -> GenContext {
        let mut c = GenContext::new(1234);
        c.plate_count = plate_count;
        c
    }

    #[test]
    fn partitions_all_regions_into_valid_plates() {
        let sites = vec![
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
        ];
        let mut g = globe_with_sites(sites);
        let mut ctx = ctx_with(4);
        TectonicPlatesStage.run(&mut g, &mut ctx);
        assert_eq!(g.region_plate.len(), 6);
        for &p in &g.region_plate {
            assert!(p < 4, "plate id {} out of range", p);
        }
    }

    #[test]
    fn deterministic_same_seed() {
        let sites: Vec<Vec3> = (0..32)
            .map(|i| {
                let a = i as f32;
                Vec3::new(a.sin(), a.cos(), (a * 0.5).sin())
                    .normalize()
                    .unwrap_or(Vec3::new(1.0, 0.0, 0.0))
            })
            .collect();
        let mut a = globe_with_sites(sites.clone());
        let mut b = globe_with_sites(sites);
        let mut ca = ctx_with(8);
        let mut cb = ctx_with(8);
        TectonicPlatesStage.run(&mut a, &mut ca);
        TectonicPlatesStage.run(&mut b, &mut cb);
        assert_eq!(a.region_plate, b.region_plate);
    }
}
