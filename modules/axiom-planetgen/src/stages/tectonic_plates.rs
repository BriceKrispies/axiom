//! `tectonic_plates`: spherical-Voronoi partition of regions into plates.
//!
//! Picks `plate_count` deterministic uniform plate-seed directions and assigns
//! every region to the nearest seed by maximum dot against its site direction.
//! Fills `region_plate`. Branchless: a `fold` argmax per region.

use axiom_entropy::EntropyStream;
use axiom_math::Vec3;

use crate::globe::PlanetGlobe;

/// Uniform unit vector on the sphere from two unit draws (area-preserving via
/// [`axiom_math::unit_vec3`]). Draw order (u then v) is fixed for reproducibility.
fn unit_vec3(rng: &mut EntropyStream) -> Vec3 {
    let u = rng.unit();
    let v = rng.unit();
    axiom_math::unit_vec3(u, v)
}

/// Index of the plate seed with maximum dot against `site`; ties keep the lower
/// index (branchless table-select fold).
fn nearest_seed(site: Vec3, seeds: &[Vec3]) -> usize {
    seeds
        .iter()
        .enumerate()
        .fold((0usize, f32::NEG_INFINITY), |(bi, bd), (p, seed)| {
            let d = site.dot(*seed);
            [(bi, bd), (p, d)][usize::from(d > bd)]
        })
        .0
}

pub(crate) fn tectonic_plates(globe: &mut PlanetGlobe, plate_count: u32, root: &EntropyStream) {
    let plate_count = plate_count.max(1) as usize;
    let mut rng = root.fork(0x_71A7_E5ED);
    let seeds: Vec<Vec3> = (0..plate_count).map(|_| unit_vec3(&mut rng)).collect();
    let region_count = globe.region_count();
    let region_plate: Vec<u32> = (0..region_count)
        .map(|r| nearest_seed(globe.topology.sites[r], &seeds) as u32)
        .collect();
    globe.region_plate = region_plate;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_gen_api::worldgen_stream;
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
        tectonic_plates(&mut g, 4, &worldgen_stream(1234));
        assert_eq!(g.region_plate.len(), 6);
        assert!(g.region_plate.iter().all(|&p| p < 4));
    }

    #[test]
    fn zero_plate_count_is_clamped_to_one() {
        let mut g = globe_with_sites(vec![Vec3::new(1.0, 0.0, 0.0); 3]);
        tectonic_plates(&mut g, 0, &worldgen_stream(1));
        // Clamped to a single plate, so every region lands on plate 0.
        assert_eq!(g.region_plate, vec![0, 0, 0]);
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
        tectonic_plates(&mut a, 8, &worldgen_stream(7));
        tectonic_plates(&mut b, 8, &worldgen_stream(7));
        assert_eq!(a.region_plate, b.region_plate);
    }
}
