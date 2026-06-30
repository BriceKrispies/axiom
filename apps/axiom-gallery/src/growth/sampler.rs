//! Overworld surface queries. Audit: OW-E3 locate_region/sample_surface,
//! "Query/API requirements" (spatial index), Climate (derived temperature/biome).
use axiom_math::Vec3;

use crate::growth::atlas::{lat_band, lon_band};
use crate::growth::geo;
use crate::growth::ids::{BiomeId, PlateId, RegionId};
use crate::growth::model_planet::{PlanetSurfaceAtlas, SurfaceSample};

/// Find the region whose site direction is closest to `dir` (max dot product).
///
/// Fast-path: when the atlas `RegionLocator` is populated, only the query's
/// lat/long cell plus its eight neighbours are checked (audit: perf P1). The
/// single neighbour ring guarantees the true nearest centre is considered even
/// when it sits just across a cell boundary, so the result is identical to the
/// brute-force scan. An empty locator (index not yet built) falls back to the
/// linear scan, so the query always works. Audit: OW-E3.
pub fn locate_region(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> RegionId {
    let bands = atlas.locator.bands;
    if bands == 0 || atlas.locator.cell_regions.is_empty() {
        return locate_region_linear(atlas, dir);
    }

    let qlat = lat_band(dir, bands) as i64;
    let qlon = lon_band(dir, bands) as i64;
    let bands_i = bands as i64;

    let mut best = usize::MAX;
    let mut best_dot = f32::NEG_INFINITY;

    // Check the query cell and its 8 lat/long neighbours: latitude clamps at the
    // poles, longitude wraps around the sphere.
    let mut dlat = -1i64;
    while dlat <= 1 {
        let la = qlat + dlat;
        dlat += 1;
        if la < 0 || la >= bands_i {
            continue;
        }
        let mut dlon = -1i64;
        while dlon <= 1 {
            let lo = ((qlon + dlon) % bands_i + bands_i) % bands_i;
            dlon += 1;
            let cell = (la * bands_i + lo) as usize;
            let Some(regions) = atlas.locator.cell_regions.get(cell) else {
                continue;
            };
            for &r in regions {
                let i = r as usize;
                if let Some(s) = atlas.sites.get(i) {
                    let d = s.dot(dir);
                    if d > best_dot {
                        best_dot = d;
                        best = i;
                    }
                }
            }
        }
    }

    // Degenerate fallback: if the neighbourhood happened to be empty (e.g. a
    // sparse pole cell), fall back to the exact scan rather than return junk.
    if best == usize::MAX {
        return locate_region_linear(atlas, dir);
    }
    RegionId(best as u32)
}

/// Exact O(R) nearest-site scan. The locator fast-path must agree with this.
fn locate_region_linear(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> RegionId {
    let mut best = 0usize;
    let mut best_dot = f32::NEG_INFINITY;
    for (i, s) in atlas.sites.iter().enumerate() {
        let d = s.dot(dir);
        if d > best_dot {
            best_dot = d;
            best = i;
        }
    }
    RegionId(best as u32)
}

/// Derive temperature from latitude + elevation. Audit: Climate requirements
/// (latitude cosine minus elevation lapse). Warmest at the equator (lat 0),
/// coldest at the poles; higher elevation is colder (lapse rate).
pub fn derive_temperature(latitude_rad: f32, elevation: f32) -> f32 {
    let latitudinal = latitude_rad.cos();
    let lapse = elevation.max(0.0) * 0.6;
    latitudinal - lapse
}

/// Threshold above which a region counts as "hot" for biome lookup.
const BIOME_HOT_THRESHOLD: f32 = 0.5;
/// Threshold above which a region counts as "wet" for biome lookup.
const BIOME_WET_THRESHOLD: f32 = 0.5;

/// Biome ids produced by [`derive_biome`]. Audit: OW-E3 derived biome, the
/// hot/cold x wet/dry lookup table (ocean when below sea level 0).
pub mod biome {
    pub const OCEAN: u32 = 0;
    pub const DESERT: u32 = 1; // hot + dry
    pub const RAINFOREST: u32 = 2; // hot + wet
    pub const TUNDRA: u32 = 3; // cold + dry
    pub const TAIGA: u32 = 4; // cold + wet
}

/// Map climate scalars to a biome id. Audit: Climate requirements biome lookup.
/// Ocean below sea level 0; otherwise a hot/cold x wet/dry table.
pub fn derive_biome(temperature: f32, moisture: f32, elevation: f32) -> BiomeId {
    if elevation < 0.0 {
        return BiomeId(biome::OCEAN);
    }
    let hot = temperature >= BIOME_HOT_THRESHOLD;
    let wet = moisture >= BIOME_WET_THRESHOLD;
    let id = match (hot, wet) {
        (true, false) => biome::DESERT,
        (true, true) => biome::RAINFOREST,
        (false, false) => biome::TUNDRA,
        (false, true) => biome::TAIGA,
    };
    BiomeId(id)
}

/// Sample overworld fields at a unit direction. Audit: OW-E3/E4.
pub fn sample_surface(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> SurfaceSample {
    let region = locate_region(atlas, dir);
    let mut sample = sample_region(atlas, region);
    // Temperature is latitude-dependent, so it is derived from the query
    // direction rather than the region centre. Audit: Climate (query-time temp).
    if sample.region == region {
        sample.temperature = derive_temperature(geo::latitude(dir), sample.elevation);
        sample.biome = derive_biome(sample.temperature, sample.moisture, sample.elevation);
    }
    sample
}

/// Sample overworld fields directly by region id (used by the game world, which
/// already knows the region from `locate_region`/`GameWorldLocalMap`). Audit:
/// OW-E3, GW-E2. Temperature uses the region centre's latitude.
pub fn sample_region(atlas: &PlanetSurfaceAtlas, region: RegionId) -> SurfaceSample {
    let i = region.index();
    if i >= atlas.region_count() {
        return SurfaceSample::default();
    }
    let elevation = atlas.region_elevation.get(i).copied().unwrap_or(0.0);
    let moisture = atlas.region_moisture.get(i).copied().unwrap_or(0.0);
    let latitude = atlas.sites.get(i).map(|&s| geo::latitude(s)).unwrap_or(0.0);
    let temperature = derive_temperature(latitude, elevation);
    SurfaceSample {
        region,
        plate: PlateId(atlas.region_plate.get(i).copied().unwrap_or(0)),
        elevation,
        moisture,
        temperature,
        biome: derive_biome(temperature, moisture, elevation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::atlas::build_locator;
    use crate::growth::model_planet::PlanetSurfaceAtlas;

    fn unit(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
            .normalize()
            .unwrap_or(Vec3::new(1.0, 0.0, 0.0))
    }

    /// Deterministic pseudo-random unit direction generator (no rng dep needed).
    fn pseudo_dir(seed: u32) -> Vec3 {
        let a = (seed.wrapping_mul(2654435761)) as f32 * 1.0e-9;
        let b = (seed.wrapping_mul(40503).wrapping_add(12345)) as f32 * 1.0e-9;
        unit(a.sin(), b.cos(), (a * 1.7 + b).sin())
    }

    /// Build a small synthetic atlas with a handful of sites + a real locator.
    fn synthetic_atlas(n: u32) -> PlanetSurfaceAtlas {
        let sites: Vec<Vec3> = (0..n).map(pseudo_dir).collect();
        let region_elevation: Vec<f32> = (0..n)
            .map(|i| (i as f32 % 5.0) - 2.0) // mix of below/above sea level
            .collect();
        let region_moisture: Vec<f32> = (0..n).map(|i| (i as f32 % 4.0) / 3.0).collect();
        let region_plate: Vec<u32> = (0..n).map(|i| i % 3).collect();
        let locator = build_locator(&sites);
        PlanetSurfaceAtlas {
            sites,
            region_plate,
            region_elevation,
            region_moisture,
            planet_radius_m: 6_371_000.0,
            locator,
            ..PlanetSurfaceAtlas::default()
        }
    }

    #[test]
    fn locator_matches_brute_force_for_many_queries() {
        // The key correctness property of the spatial index.
        let atlas = synthetic_atlas(137);
        assert!(atlas.locator.bands > 0, "locator must be populated");
        let mut checked = 0;
        for q in 0..2000u32 {
            let dir = pseudo_dir(q.wrapping_mul(7) + 3);
            let fast = locate_region(&atlas, dir);
            let brute = locate_region_linear(&atlas, dir);
            assert_eq!(
                fast, brute,
                "locator disagreed with brute force at query {q}: {fast:?} vs {brute:?}"
            );
            checked += 1;
        }
        assert_eq!(checked, 2000);
    }

    #[test]
    fn empty_locator_falls_back_to_linear() {
        let mut atlas = synthetic_atlas(40);
        atlas.locator = Default::default(); // simulate "index not built yet"
        for q in 0..200u32 {
            let dir = pseudo_dir(q + 100);
            let got = locate_region(&atlas, dir);
            let brute = locate_region_linear(&atlas, dir);
            assert_eq!(got, brute);
        }
    }

    #[test]
    fn sample_surface_returns_right_fields() {
        let atlas = synthetic_atlas(60);
        let dir = unit(0.3, 0.8, -0.2);
        let region = locate_region(&atlas, dir);
        let s = sample_surface(&atlas, dir);
        assert_eq!(s.region, region);
        let i = region.index();
        assert_eq!(s.elevation, atlas.region_elevation[i]);
        assert_eq!(s.moisture, atlas.region_moisture[i]);
        assert_eq!(s.plate, PlateId(atlas.region_plate[i]));
        // Biome derived from temperature/moisture/elevation, consistent.
        let expect_biome = derive_biome(s.temperature, s.moisture, s.elevation);
        assert_eq!(s.biome, expect_biome);
    }

    #[test]
    fn sample_region_by_id() {
        let atlas = synthetic_atlas(20);
        let s = sample_region(&atlas, RegionId(5));
        assert_eq!(s.region, RegionId(5));
        assert_eq!(s.elevation, atlas.region_elevation[5]);
        assert_eq!(s.moisture, atlas.region_moisture[5]);
        // Out-of-range id returns default.
        let oob = sample_region(&atlas, RegionId(9999));
        assert_eq!(oob.region, RegionId::default());
    }

    #[test]
    fn derive_biome_matches_table() {
        // Ocean dominates whenever below sea level.
        assert_eq!(derive_biome(1.0, 1.0, -0.1), BiomeId(biome::OCEAN));
        assert_eq!(derive_biome(0.0, 0.0, -5.0), BiomeId(biome::OCEAN));
        // hot/cold x wet/dry on land.
        assert_eq!(derive_biome(0.9, 0.1, 1.0), BiomeId(biome::DESERT));
        assert_eq!(derive_biome(0.9, 0.9, 1.0), BiomeId(biome::RAINFOREST));
        assert_eq!(derive_biome(0.1, 0.1, 1.0), BiomeId(biome::TUNDRA));
        assert_eq!(derive_biome(0.1, 0.9, 1.0), BiomeId(biome::TAIGA));
    }

    #[test]
    fn temperature_decreases_with_elevation() {
        let lat = 0.2_f32;
        let low = derive_temperature(lat, 0.0);
        let high = derive_temperature(lat, 2.0);
        assert!(
            high < low,
            "higher elevation must be colder: {high} !< {low}"
        );
    }

    #[test]
    fn temperature_warmest_at_equator() {
        let equator = derive_temperature(0.0, 0.0);
        let pole = derive_temperature(core::f32::consts::FRAC_PI_2, 0.0);
        assert!(equator > pole, "equator must be warmer than pole");
    }
}
