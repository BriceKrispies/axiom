//! Build the durable PlanetSurfaceAtlas from a generated globe.
//! Audit: OW-E1 (atlas from globe), OW-E20 (drop transient globe after),
//! perf P1 ("Query/API requirements" spatial index for `locate_region`).
use axiom_math::Vec3;

use crate::growth::genome::PlanetGenome;
use crate::growth::model_planet::{PlanetGlobe, PlanetSurfaceAtlas, RegionLocator};

use core::f32::consts::PI;

/// Copy compact gameplay fields out of the globe into the session atlas and
/// build the coarse `RegionLocator` spatial index. Audit: OW-E1, perf P1.
pub fn build_atlas(globe: &PlanetGlobe, genome: &PlanetGenome) -> PlanetSurfaceAtlas {
    let sites = globe.topology.sites.clone();
    let locator = build_locator(&sites);
    PlanetSurfaceAtlas {
        sites,
        graph: globe.graph.clone(),
        region_plate: globe.region_plate.clone(),
        plate_oceanic: globe.plate_oceanic.clone(),
        region_elevation: globe.region_elevation.clone(),
        region_moisture: globe.region_moisture.clone(),
        planet_radius_m: genome.radius_m,
        locator,
    }
}

// --- spatial index ------------------------------------------------------------
//
// Design (audit: perf P1, "Query/API requirements" spatial index):
//
// A `locate_region(dir)` is "which site direction has max dot with `dir`",
// i.e. nearest region centre on the sphere. A linear scan is O(R) per query;
// with per-chunk `sample_macro` doing many queries this dominates.
//
// The `RegionLocator` is a uniform lat/long band grid over the unit sphere:
//   * `bands` ~= sqrt(R) so a populated cell holds O(1) regions on average
//     (R sites spread over bands*bands cells -> ~R/bands^2 = ~1 per cell).
//   * Each region's site direction is binned into a `(lat_band, lon_band)`
//     cell; the cell stores the region indices that fall in it.
//   * A query hashes `dir` into its cell, then checks that cell plus its 8
//     lat/long neighbours (with longitude wrap and pole clamping), taking the
//     max-dot region among the candidates. The single ring of neighbours
//     covers the small chance that the true nearest centre sits just across a
//     cell boundary from the query, so the result matches a brute-force scan.
//
// Binning is by *band index*, not by raw great-circle distance, so it is cheap
// and deterministic. The sampler owns the query side and reuses these helpers.

/// Choose the band count for `region_count` sites: roughly `sqrt(R)`, clamped so
/// tiny atlases still get a 1x1 grid and huge ones do not explode the cell table.
pub fn band_count(region_count: usize) -> u32 {
    if region_count == 0 {
        return 0;
    }
    let approx = (region_count as f32).sqrt().round() as u32;
    approx.clamp(1, 4096)
}

/// Latitude band index in `[0, bands)` for a unit direction (y is the pole axis).
pub fn lat_band(dir: Vec3, bands: u32) -> u32 {
    if bands == 0 {
        return 0;
    }
    // latitude in [-PI/2, PI/2] -> normalised [0, 1).
    let lat = dir.y.clamp(-1.0, 1.0).asin();
    let t = (lat + PI * 0.5) / PI;
    band_index(t, bands)
}

/// Longitude band index in `[0, bands)` for a unit direction (x/z plane).
pub fn lon_band(dir: Vec3, bands: u32) -> u32 {
    if bands == 0 {
        return 0;
    }
    // longitude in [-PI, PI] -> normalised [0, 1) with wrap.
    let lon = dir.z.atan2(dir.x);
    let t = (lon + PI) / (2.0 * PI);
    band_index(t, bands)
}

/// Flat cell index `lat_band * bands + lon_band` for a unit direction.
pub fn cell_of_dir(dir: Vec3, bands: u32) -> usize {
    if bands == 0 {
        return 0;
    }
    let la = lat_band(dir, bands) as usize;
    let lo = lon_band(dir, bands) as usize;
    la * bands as usize + lo
}

/// Map a normalised coordinate `t` (intended `[0, 1)`) to a band index, clamping
/// the closed-interval edge case `t == 1.0` back into the last band.
fn band_index(t: f32, bands: u32) -> u32 {
    let scaled = (t * bands as f32).floor();
    let idx = if scaled < 0.0 { 0 } else { scaled as u32 };
    idx.min(bands - 1)
}

/// Build the lat/long band grid: `bands*bands` cells, each holding the region
/// indices whose site direction falls in that cell.
pub fn build_locator(sites: &[Vec3]) -> RegionLocator {
    let bands = band_count(sites.len());
    if bands == 0 {
        return RegionLocator::default();
    }
    let cell_count = (bands as usize) * (bands as usize);
    let mut cell_regions: Vec<Vec<u32>> = vec![Vec::new(); cell_count];
    for (i, &site) in sites.iter().enumerate() {
        let cell = cell_of_dir(site, bands);
        if cell < cell_regions.len() {
            cell_regions[cell].push(i as u32);
        }
    }
    RegionLocator {
        cell_regions,
        bands,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
            .normalize()
            .unwrap_or(Vec3::new(1.0, 0.0, 0.0))
    }

    #[test]
    fn band_count_is_about_sqrt() {
        assert_eq!(band_count(0), 0);
        assert_eq!(band_count(100), 10);
        assert_eq!(band_count(1), 1);
        assert!(band_count(4096) <= 4096);
    }

    #[test]
    fn poles_and_equator_land_in_expected_lat_bands() {
        let bands = 8;
        // North pole -> highest lat band; south pole -> lowest.
        assert_eq!(lat_band(dir(0.0, 1.0, 0.0), bands), bands - 1);
        assert_eq!(lat_band(dir(0.0, -1.0, 0.0), bands), 0);
        // Equator sits in a middle band.
        let eq = lat_band(dir(1.0, 0.0, 0.0), bands);
        assert!(eq == bands / 2 || eq == bands / 2 - 1);
    }

    #[test]
    fn longitude_wraps_into_range() {
        let bands = 8;
        for k in 0..16 {
            let a = (k as f32) * (2.0 * PI / 16.0) - PI;
            let d = dir(a.cos(), 0.0, a.sin());
            let lb = lon_band(d, bands);
            assert!(lb < bands);
        }
    }

    #[test]
    fn every_site_bins_into_a_valid_cell() {
        let sites: Vec<Vec3> = (0..200)
            .map(|i| {
                let f = i as f32;
                dir(f.sin(), (f * 0.7).cos(), (f * 1.3).sin())
            })
            .collect();
        let loc = build_locator(&sites);
        let total: usize = loc.cell_regions.iter().map(|c| c.len()).sum();
        assert_eq!(total, sites.len(), "every site must be binned exactly once");
        assert_eq!(
            loc.cell_regions.len(),
            (loc.bands as usize) * (loc.bands as usize)
        );
    }
}
