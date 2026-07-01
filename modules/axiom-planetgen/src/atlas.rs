//! Build the durable [`PlanetSurfaceAtlas`] from a generated globe, and the
//! coarse [`RegionLocator`] band grid that accelerates `locate`.
//!
//! The atlas copies the compact per-region gameplay fields out of the transient
//! globe and builds a uniform lat/long band grid over the unit sphere: `bands ~=
//! sqrt(R)` so a populated cell holds O(1) regions on average, and each region's
//! site direction is binned into a `(lat_band, lon_band)` cell. The query side
//! (`crate::query`) checks a query cell plus its eight neighbours. Branchless.

use core::f32::consts::PI;

use axiom_kernel::Meters;
use axiom_math::Vec3;

use crate::globe::PlanetGlobe;
use crate::ids::{PlanetSurfaceAtlas, RegionLocator};

/// Copy the compact gameplay fields out of the globe into the durable atlas and
/// build the coarse [`RegionLocator`] spatial index.
pub(crate) fn build_atlas(globe: &PlanetGlobe, radius: Meters) -> PlanetSurfaceAtlas {
    let sites = globe.topology.sites.clone();
    let locator = build_locator(&sites);
    PlanetSurfaceAtlas {
        sites,
        graph: globe.graph.clone(),
        region_plate: globe.region_plate.clone(),
        plate_oceanic: globe.plate_oceanic.clone(),
        region_elevation: globe.region_elevation.clone(),
        region_moisture: globe.region_moisture.clone(),
        planet_radius_m: radius,
        locator,
    }
}

/// Band count for `region_count` sites: roughly `sqrt(R)` clamped to `[1, 4096]`,
/// or 0 for an empty atlas.
pub(crate) fn band_count(region_count: usize) -> u32 {
    let approx = (region_count as f32).sqrt().round() as u32;
    [0, approx.clamp(1, 4096)][usize::from(region_count > 0)]
}

/// Map a normalised coordinate `t` (intended `[0, 1)`) to a band index, clamping
/// the closed-interval edge case `t == 1.0` back into the last band. Safe for
/// `bands == 0` (yields 0, no underflow).
fn band_index(t: f32, bands: u32) -> u32 {
    let scaled = (t * bands as f32).floor().max(0.0) as u32;
    scaled.min(bands.saturating_sub(1))
}

/// Latitude band index in `[0, bands)` for a unit direction (y is the pole axis).
pub(crate) fn lat_band(dir: Vec3, bands: u32) -> u32 {
    let lat = dir.y.clamp(-1.0, 1.0).asin();
    band_index((lat + PI * 0.5) / PI, bands)
}

/// Longitude band index in `[0, bands)` for a unit direction (x/z plane).
pub(crate) fn lon_band(dir: Vec3, bands: u32) -> u32 {
    let lon = dir.z.atan2(dir.x);
    band_index((lon + PI) / (2.0 * PI), bands)
}

/// Flat cell index `lat_band * bands + lon_band` for a unit direction.
fn cell_of_dir(dir: Vec3, bands: u32) -> usize {
    let la = lat_band(dir, bands) as usize;
    let lo = lon_band(dir, bands) as usize;
    la * bands as usize + lo
}

/// Build the lat/long band grid: `bands*bands` cells, each holding the region
/// indices whose site direction falls in that cell. `cell_of_dir` is bounded by
/// construction — `lat_band` and `lon_band` each clamp/wrap into `[0, bands)`, so
/// `cell = la*bands + lo < bands*bands = cell_count` — hence the direct index (no
/// unreachable bounds guard).
pub(crate) fn build_locator(sites: &[Vec3]) -> RegionLocator {
    let bands = band_count(sites.len());
    let cell_count = (bands as usize) * (bands as usize);
    let mut cell_regions: Vec<Vec<u32>> = vec![Vec::new(); cell_count];
    sites.iter().enumerate().for_each(|(i, &site)| {
        cell_regions[cell_of_dir(site, bands)].push(i as u32);
    });
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
        assert_eq!(lat_band(dir(0.0, 1.0, 0.0), bands), bands - 1);
        assert_eq!(lat_band(dir(0.0, -1.0, 0.0), bands), 0);
        // The equator (y = 0 → lat 0 → t = 0.5) lands deterministically in the
        // middle band, `bands / 2`.
        assert_eq!(lat_band(dir(1.0, 0.0, 0.0), bands), bands / 2);
    }

    #[test]
    fn zero_bands_is_safe_and_binned_to_zero() {
        assert_eq!(lat_band(dir(0.0, 1.0, 0.0), 0), 0);
        assert_eq!(lon_band(dir(1.0, 0.0, 0.0), 0), 0);
        assert_eq!(cell_of_dir(dir(1.0, 0.0, 0.0), 0), 0);
    }

    #[test]
    fn longitude_wraps_into_range() {
        let bands = 8;
        (0..16).for_each(|k| {
            let a = (k as f32) * (2.0 * PI / 16.0) - PI;
            let lb = lon_band(dir(a.cos(), 0.0, a.sin()), bands);
            assert!(lb < bands);
        });
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
        assert_eq!(total, sites.len());
        assert_eq!(
            loc.cell_regions.len(),
            (loc.bands as usize) * (loc.bands as usize)
        );
    }

    #[test]
    fn empty_sites_yield_default_locator() {
        let loc = build_locator(&[]);
        assert_eq!(loc.bands, 0);
        assert!(loc.cell_regions.is_empty());
    }
}
