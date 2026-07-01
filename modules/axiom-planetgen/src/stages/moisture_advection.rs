//! `moisture_advection`: advect moisture downwind along the region graph.
//!
//! For a fixed number of passes each land region pulls moisture from its most
//! *upwind* neighbour (the one the wind blows from); ocean regions are pinned wet
//! so they act as an infinite source. Branchless: each pass is a per-region `map`
//! and the passes are a `fold`; the upwind neighbour is a table-select fold.

use axiom_geosphere::RegionId;

use crate::globe::PlanetGlobe;

/// Advection passes (deterministic, fixed). More passes carry moisture further.
const PASSES: u32 = 6;
/// How strongly upwind moisture is blended in each pass.
const ADVECT: f32 = 0.5;

/// Moisture for land region `r` after one pass: blend from its most-upwind
/// neighbour (`None` when it has no neighbours → unchanged).
fn advect_region(globe: &PlanetGlobe, cur: &[f32], r: usize) -> f32 {
    let site = globe.topology.sites[r];
    let wind = globe.region_wind[r];
    let upwind = globe
        .graph
        .neighbours_of(RegionId(r as u32))
        .iter()
        .fold((None::<usize>, 0.0f32), |(best, best_score), &n| {
            let dir = globe.topology.sites[n as usize].subtract(site);
            let dir = dir.normalize().unwrap_or(dir);
            let score = dir.dot(wind.mul_scalar(-1.0));
            [(best, best_score), (Some(n as usize), score)][usize::from(score > best_score)]
        })
        .0;
    let here = cur[r];
    upwind.map_or(here, |up| {
        (here + ADVECT * (cur[up] - here)).clamp(0.0, 1.0)
    })
}

/// One advection pass: oceans stay wet sources, land regions blend upwind.
fn advect_pass(globe: &PlanetGlobe, cur: &[f32]) -> Vec<f32> {
    (0..cur.len())
        .map(|r| {
            let ocean = globe.region_elevation[r] < 0.0;
            [advect_region(globe, cur, r), 1.0][usize::from(ocean)]
        })
        .collect()
}

pub(crate) fn moisture_advection(globe: &mut PlanetGlobe) {
    let region_count = globe.region_count();
    // Pin oceans wet up front so they source moisture every pass.
    let pinned: Vec<f32> = (0..region_count)
        .map(|r| {
            let ocean = globe.region_elevation[r] < 0.0;
            [globe.region_moisture[r], 1.0][usize::from(ocean)]
        })
        .collect();
    let advected = (0..PASSES).fold(pinned, |cur, _| advect_pass(globe, &cur));
    globe.region_moisture = advected.iter().map(|m| m.clamp(0.0, 1.0)).collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line of regions along +x; region 0 ocean. Wind blows +x so moisture carries
    /// inland.
    fn line_globe(n: usize) -> PlanetGlobe {
        let sites: Vec<Vec3> = (0..n)
            .map(|i| Vec3::new(i as f32 + 1.0, 0.0, 0.0))
            .collect();
        let mut offsets = vec![0u32];
        let mut neighbours = Vec::new();
        (0..n).for_each(|i| {
            (i > 0).then(|| neighbours.push((i - 1) as u32));
            (i + 1 < n).then(|| neighbours.push((i + 1) as u32));
            offsets.push(neighbours.len() as u32);
        });
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites,
                triangles: Vec::new(),
                subdivisions: 0,
            },
            graph: RegionGraph {
                offsets,
                neighbours,
            },
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        g.region_elevation = (0..n)
            .map(|i| [0.5f32, -1.0][usize::from(i == 0)])
            .collect();
        g.region_wind = vec![Vec3::new(1.0, 0.0, 0.0); n];
        g.region_moisture = (0..n).map(|i| [0.0f32, 1.0][usize::from(i == 0)]).collect();
        g
    }

    #[test]
    fn moisture_stays_in_range_and_carries_inland() {
        let mut g = line_globe(5);
        moisture_advection(&mut g);
        assert!(g.region_moisture.iter().all(|&m| (0.0..=1.0).contains(&m)));
        assert!(g.region_moisture[1] > 0.0);
        assert!(g.region_moisture[1] >= g.region_moisture[4]);
    }

    #[test]
    fn ocean_stays_source() {
        let mut g = line_globe(5);
        moisture_advection(&mut g);
        assert_eq!(g.region_moisture[0], 1.0);
    }

    #[test]
    fn isolated_land_region_is_unchanged() {
        // One land region, no neighbours: advection leaves its moisture as-is.
        let mut g = line_globe(1);
        g.region_elevation = vec![0.5];
        g.region_moisture = vec![0.4];
        moisture_advection(&mut g);
        assert_eq!(g.region_moisture, vec![0.4]);
    }

    #[test]
    fn deterministic_same_input() {
        let mut a = line_globe(6);
        let mut b = line_globe(6);
        moisture_advection(&mut a);
        moisture_advection(&mut b);
        assert_eq!(a.region_moisture, b.region_moisture);
    }
}
