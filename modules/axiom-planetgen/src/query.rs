//! Deterministic overworld surface queries over a [`PlanetSurfaceAtlas`].
//!
//! `locate` finds the region whose site direction is closest to a query
//! direction (max dot); `sample` / `sample_region` read the region's fields and
//! derive a query-time temperature + biome by composing the `biome` module's
//! climate lens. Branchless throughout: the locator fast-path folds over the
//! query cell + its eight neighbours, falling back to a linear scan when the
//! locator is empty or its neighbourhood holds no site.

use axiom_biome::BiomeApi;
use axiom_geosphere::RegionId;
use axiom_kernel::{Meters, Radians, Ratio};
use axiom_math::{latitude, Vec3};

use crate::atlas::{lat_band, lon_band};
use crate::ids::{BiomeId, PlanetSurfaceAtlas, PlateId, SurfaceSample};

/// The region whose site direction is closest to `dir` (max dot). Uses the
/// locator neighbourhood when populated, else an exact linear scan.
pub(crate) fn locate(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> RegionId {
    let idx = neighbourhood_best(atlas, dir).unwrap_or_else(|| linear_best(atlas, dir));
    RegionId(idx as u32)
}

/// Exact O(R) nearest-site (max dot) scan; returns 0 for an empty atlas.
fn linear_best(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> usize {
    atlas
        .sites
        .iter()
        .enumerate()
        .fold((0usize, f32::NEG_INFINITY), |(bi, bd), (i, s)| {
            let d = s.dot(dir);
            [(bi, bd), (i, d)][usize::from(d > bd)]
        })
        .0
}

/// Best site among the query's lat/long cell + its eight neighbours, or `None`
/// when the locator is empty or the neighbourhood holds no site. Latitude clamps
/// at the poles, longitude wraps.
fn neighbourhood_best(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> Option<usize> {
    let bands = atlas.locator.bands;
    let bands_i = bands as i64;
    let qlat = lat_band(dir, bands) as i64;
    let qlon = lon_band(dir, bands) as i64;
    (-1..=1)
        .flat_map(|dlat| (-1..=1).map(move |dlon| (dlat, dlon)))
        .filter(|&(dlat, _)| {
            let la = qlat + dlat;
            (la >= 0) & (la < bands_i)
        })
        .flat_map(|(dlat, dlon)| {
            let la = qlat + dlat;
            let lo = (qlon + dlon).rem_euclid(bands_i.max(1));
            let cell = (la * bands_i + lo) as usize;
            atlas
                .locator
                .cell_regions
                .get(cell)
                .into_iter()
                .flatten()
                .copied()
        })
        .filter_map(|r| {
            atlas
                .sites
                .get(r as usize)
                .map(|s| (r as usize, s.dot(dir)))
        })
        .fold(None, |best: Option<(usize, f32)>, (i, d)| {
            let take = best.map_or(true, |(_, bd)| d > bd);
            [best, Some((i, d))][usize::from(take)]
        })
        .map(|(i, _)| i)
}

/// Derive a biome id from climate scalars via the `biome` module's climate lens.
fn classify_biome(temperature: Ratio, moisture: f32, elevation: f32) -> BiomeId {
    BiomeId(BiomeApi::classify_climate(
        temperature,
        Ratio::finite_or_zero(moisture),
        elevation < 0.0,
    ) as u32)
}

/// Sample overworld fields directly by region id. Out-of-range ids return the
/// default sample. Temperature uses the region centre's latitude.
pub(crate) fn sample_region(atlas: &PlanetSurfaceAtlas, region: RegionId) -> SurfaceSample {
    let i = region.index();
    let in_range = i < atlas.region_count();
    let elevation = atlas.region_elevation.get(i).copied().unwrap_or(0.0);
    let moisture = atlas.region_moisture.get(i).copied().unwrap_or(0.0);
    let lat = atlas
        .sites
        .get(i)
        .map(|&s| latitude(s).get())
        .unwrap_or(0.0);
    let temperature = BiomeApi::temperature(
        Radians::finite_or_zero(lat),
        Meters::finite_or_zero(elevation),
    );
    let full = SurfaceSample {
        region,
        plate: PlateId(atlas.region_plate.get(i).copied().unwrap_or(0)),
        elevation: Meters::finite_or_zero(elevation),
        moisture: Ratio::finite_or_zero(moisture),
        temperature,
        biome: classify_biome(temperature, moisture, elevation),
    };
    [SurfaceSample::default(), full][usize::from(in_range)]
}

/// Sample overworld fields at a unit direction. Temperature is latitude-dependent
/// so it is derived from the *query* direction rather than the region centre.
pub(crate) fn sample(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> SurfaceSample {
    let region = locate(atlas, dir);
    let base = sample_region(atlas, region);
    let temperature =
        BiomeApi::temperature(Radians::finite_or_zero(latitude(dir).get()), base.elevation);
    let biome = classify_biome(temperature, base.moisture.get(), base.elevation.get());
    SurfaceSample {
        temperature,
        biome,
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::build_locator;

    fn unit(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
            .normalize()
            .unwrap_or(Vec3::new(1.0, 0.0, 0.0))
    }

    /// Deterministic pseudo-random unit direction.
    fn pseudo_dir(seed: u32) -> Vec3 {
        let a = (seed.wrapping_mul(2_654_435_761)) as f32 * 1.0e-9;
        let b = (seed.wrapping_mul(40503).wrapping_add(12345)) as f32 * 1.0e-9;
        unit(a.sin(), b.cos(), (a * 1.7 + b).sin())
    }

    fn synthetic_atlas(n: u32) -> PlanetSurfaceAtlas {
        let sites: Vec<Vec3> = (0..n).map(pseudo_dir).collect();
        let locator = build_locator(&sites);
        PlanetSurfaceAtlas {
            region_plate: (0..n).map(|i| i % 3).collect(),
            region_elevation: (0..n).map(|i| (i as f32 % 5.0) - 2.0).collect(),
            region_moisture: (0..n).map(|i| (i as f32 % 4.0) / 3.0).collect(),
            planet_radius_m: Meters::finite_or_zero(6_371_000.0),
            locator,
            sites,
            ..PlanetSurfaceAtlas::default()
        }
    }

    #[test]
    fn locator_matches_brute_force_for_many_queries() {
        let atlas = synthetic_atlas(137);
        assert!(atlas.locator.bands > 0);
        (0..2000u32).for_each(|q| {
            let dir = pseudo_dir(q.wrapping_mul(7) + 3);
            assert_eq!(
                locate(&atlas, dir),
                RegionId(linear_best(&atlas, dir) as u32)
            );
        });
    }

    #[test]
    fn empty_locator_falls_back_to_linear() {
        let mut atlas = synthetic_atlas(40);
        atlas.locator = Default::default();
        (0..200u32).for_each(|q| {
            let dir = pseudo_dir(q + 100);
            assert_eq!(
                locate(&atlas, dir),
                RegionId(linear_best(&atlas, dir) as u32)
            );
        });
    }

    #[test]
    fn locate_on_empty_atlas_is_region_zero() {
        let atlas = PlanetSurfaceAtlas::default();
        assert_eq!(locate(&atlas, unit(1.0, 0.0, 0.0)), RegionId(0));
    }

    #[test]
    fn sample_surface_returns_region_fields() {
        let atlas = synthetic_atlas(60);
        let dir = unit(0.3, 0.8, -0.2);
        let region = locate(&atlas, dir);
        let s = sample(&atlas, dir);
        assert_eq!(s.region, region);
        let i = region.index();
        assert_eq!(s.elevation.get(), atlas.region_elevation[i]);
        assert_eq!(s.moisture.get(), atlas.region_moisture[i]);
        assert_eq!(s.plate, PlateId(atlas.region_plate[i]));
        assert_eq!(
            s.biome,
            classify_biome(s.temperature, s.moisture.get(), s.elevation.get())
        );
    }

    #[test]
    fn sample_region_by_id_and_out_of_range() {
        let atlas = synthetic_atlas(20);
        let s = sample_region(&atlas, RegionId(5));
        assert_eq!(s.region, RegionId(5));
        assert_eq!(s.elevation.get(), atlas.region_elevation[5]);
        assert_eq!(s.moisture.get(), atlas.region_moisture[5]);
        let oob = sample_region(&atlas, RegionId(9999));
        assert_eq!(oob, SurfaceSample::default());
    }

    #[test]
    fn biome_and_temperature_track_climate() {
        // Ocean below sea level.
        assert_eq!(
            classify_biome(Ratio::finite_or_zero(1.0), 1.0, -0.1),
            BiomeId(BiomeApi::CLIMATE_OCEAN as u32)
        );
        // Hot + dry land → desert.
        assert_eq!(
            classify_biome(Ratio::finite_or_zero(0.9), 0.1, 1.0),
            BiomeId(BiomeApi::CLIMATE_DESERT as u32)
        );
        // Cold + wet land → taiga.
        assert_eq!(
            classify_biome(Ratio::finite_or_zero(0.1), 0.9, 1.0),
            BiomeId(BiomeApi::CLIMATE_TAIGA as u32)
        );
    }
}
