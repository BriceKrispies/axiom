//! [`BiomeApi`] — deterministic biome classification on the proc substrate.
//!
//! `classify` buckets an `(elevation, moisture)` pair into a biome with a
//! branchless Whittaker-style band lookup; `map` generates a whole field by
//! drawing per-cell elevation/moisture from an entropy stream keyed by a content
//! address. The **domain rules** — the thresholds that decide ocean vs forest vs
//! peak — live here, never in the generic `proc-validate` layer. Integer-only and
//! branchless; biome codes are small `u8`s with named constants.
//!
//! Alongside the elevation×moisture band lens, `temperature` +
//! `classify_climate` add a complementary **climate lens**: a latitude/elevation
//! temperature (a dimensionless [`Ratio`]) feeding a temperature×moisture lookup
//! (ocean below sea level) over its own `CLIMATE_*` code vocabulary, so the two
//! classifications never collide.

use axiom_entropy::EntropyApi;
use axiom_kernel::{Meters, Radians, Ratio};
use axiom_space::Address;

use crate::biome_map::BiomeMap;

/// The value range elevation and moisture draws span, `[0, RANGE)`.
const RANGE: u64 = 1000;
/// Elevation band thresholds: below `LOW` is water-level, at/above `HIGH` is
/// highland; between them is midland.
const ELEV_LOW: u32 = 333;
const ELEV_HIGH: u32 = 666;
/// Moisture band threshold: below is dry, at/above is wet.
const MOIST_WET: u32 = 500;
/// The biome version. Bump to deliberately re-key generation (+ regolden).
const BIOME_VERSION: u32 = 1;

/// Climate lapse rate: fraction of temperature shed per metre of elevation above
/// sea level (used by [`BiomeApi::temperature`]).
const LAPSE_RATE: f32 = 0.6;
/// Temperature at or above which a climate cell counts as "hot".
const CLIMATE_HOT_THRESHOLD: f32 = 0.5;
/// Moisture at or above which a climate cell counts as "wet".
const CLIMATE_WET_THRESHOLD: f32 = 0.5;

/// The deterministic biome facade.
#[derive(Debug)]
pub struct BiomeApi;

impl BiomeApi {
    /// Biome codes — a small, stable vocabulary callers compare against.
    pub const OCEAN: u8 = 0;
    pub const BEACH: u8 = 1;
    pub const DESERT: u8 = 2;
    pub const FOREST: u8 = 3;
    pub const MOUNTAIN: u8 = 4;
    pub const PEAK: u8 = 5;

    /// Climate biome codes — a SECOND, climate-driven vocabulary (temperature ×
    /// moisture, with ocean below sea level), produced by
    /// [`BiomeApi::classify_climate`]. Kept as their own named constants so this
    /// climate universe never collides with the elevation-driven codes above:
    /// the two lenses answer different questions and are never mixed in one map.
    pub const CLIMATE_OCEAN: u8 = 0;
    pub const CLIMATE_DESERT: u8 = 1;
    pub const CLIMATE_RAINFOREST: u8 = 2;
    pub const CLIMATE_TUNDRA: u8 = 3;
    pub const CLIMATE_TAIGA: u8 = 4;

    /// Classify one `(elevation, moisture)` pair (each in `[0, 1000)`) into a
    /// biome code. Branchless: elevation and moisture are bucketed into bands
    /// whose product indexes the biome table.
    pub fn classify(elevation: u32, moisture: u32) -> u8 {
        // `(elev_band, moist_band)` -> biome, indexed by `elev_band * 2 + moist_band`.
        const BIOME_TABLE: [u8; 6] = [
            BiomeApi::BEACH,
            BiomeApi::OCEAN, // low:  dry, wet
            BiomeApi::DESERT,
            BiomeApi::FOREST, // mid:  dry, wet
            BiomeApi::PEAK,
            BiomeApi::MOUNTAIN, // high: dry, wet
        ];
        let elev_band = band(elevation, &[ELEV_LOW, ELEV_HIGH]);
        let moist_band = band(moisture, &[MOIST_WET]);
        BIOME_TABLE[elev_band * 2 + moist_band]
    }

    /// A biome map of `count` cells at `address` under `seed`: each cell draws an
    /// elevation then a moisture from the keyed entropy stream and classifies
    /// them. Deterministic in `(seed, address, count)`.
    pub fn map(seed: u64, address: &Address, count: u32) -> BiomeMap {
        let mut stream = EntropyApi::stream(seed, address, BIOME_VERSION);
        let codes = (0..count)
            .map(|_| {
                let elevation = stream.next_bounded(RANGE) as u32;
                let moisture = stream.next_bounded(RANGE) as u32;
                BiomeApi::classify(elevation, moisture)
            })
            .collect();
        BiomeMap::new(codes)
    }

    /// Derive a climate **temperature** from latitude and elevation: warmest at
    /// the equator (latitude 0, `cos 0 = 1`), coldest at the poles, and colder
    /// with elevation via a lapse rate (only elevation *above* sea level cools,
    /// `max(0.0)`). The result is a dimensionless normalized scalar — hence
    /// [`Ratio`], the kernel's finite-dimensionless quantity — consumed by
    /// [`BiomeApi::classify_climate`] against a hot/cold threshold. This is the
    /// climate lens; it is independent of the `(elevation, moisture)` band
    /// [`BiomeApi::classify`] uses. Branchless: `cos` of the already-finite angle
    /// minus the clamped-positive lapse, sanitized through the total
    /// [`Ratio::finite_or_zero`].
    pub fn temperature(latitude: Radians, elevation: Meters) -> Ratio {
        let latitudinal = latitude.get().cos();
        let lapse = elevation.get().max(0.0) * LAPSE_RATE;
        Ratio::finite_or_zero(latitudinal - lapse)
    }

    /// Classify a climate cell from `(temperature, moisture)` plus whether it
    /// sits below sea level, into a `CLIMATE_*` code. Ocean dominates below sea
    /// level; otherwise a hot/cold × wet/dry lookup selects desert / rainforest
    /// / tundra / taiga. Branchless in exactly the same shape as
    /// [`BiomeApi::classify`]: the hot and wet booleans index a table, and the
    /// below-sea-level flag selects ocean vs land through `usize::from(bool)`.
    pub fn classify_climate(temperature: Ratio, moisture: Ratio, below_sea_level: bool) -> u8 {
        // `(hot, wet)` -> climate, indexed by `hot * 2 + wet`.
        const CLIMATE_TABLE: [u8; 4] = [
            BiomeApi::CLIMATE_TUNDRA,     // cold, dry
            BiomeApi::CLIMATE_TAIGA,      // cold, wet
            BiomeApi::CLIMATE_DESERT,     // hot,  dry
            BiomeApi::CLIMATE_RAINFOREST, // hot,  wet
        ];
        let hot = usize::from(temperature.get() >= CLIMATE_HOT_THRESHOLD);
        let wet = usize::from(moisture.get() >= CLIMATE_WET_THRESHOLD);
        let land = CLIMATE_TABLE[hot * 2 + wet];
        [land, BiomeApi::CLIMATE_OCEAN][usize::from(below_sea_level)]
    }
}

/// How many thresholds `value` meets or exceeds — its band index. Branchless.
fn band(value: u32, thresholds: &[u32]) -> usize {
    thresholds.iter().filter(|&&t| value >= t).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_space::SpaceApi;

    fn site(segments: &[u64]) -> Address {
        segments
            .iter()
            .fold(SpaceApi::root(), |a, &s| SpaceApi::child(&a, s))
    }

    #[test]
    fn classify_maps_bands_to_the_expected_biomes() {
        // Low elevation: dry -> beach, wet -> ocean.
        assert_eq!(BiomeApi::classify(100, 100), BiomeApi::BEACH);
        assert_eq!(BiomeApi::classify(100, 800), BiomeApi::OCEAN);
        // Mid: dry -> desert, wet -> forest.
        assert_eq!(BiomeApi::classify(500, 100), BiomeApi::DESERT);
        assert_eq!(BiomeApi::classify(500, 800), BiomeApi::FOREST);
        // High: dry -> peak, wet -> mountain.
        assert_eq!(BiomeApi::classify(900, 100), BiomeApi::PEAK);
        assert_eq!(BiomeApi::classify(900, 800), BiomeApi::MOUNTAIN);
    }

    #[test]
    fn classification_is_epsilon_stable_except_at_a_boundary() {
        // Well inside the mid/dry band: a small change keeps the biome.
        assert_eq!(BiomeApi::classify(500, 100), BiomeApi::classify(501, 101));
        // Straddling the elevation HIGH boundary: it flips (mid -> high).
        assert_eq!(BiomeApi::classify(ELEV_HIGH - 1, 100), BiomeApi::DESERT);
        assert_eq!(BiomeApi::classify(ELEV_HIGH, 100), BiomeApi::PEAK);
    }

    #[test]
    fn map_is_deterministic() {
        let a = site(&[2, 5]);
        let m1 = BiomeApi::map(7, &a, 64);
        let m2 = BiomeApi::map(7, &a, 64);
        assert_eq!(m1, m2);
        assert_eq!(m1.to_bytes(), m2.to_bytes());
        assert_eq!(m1.len(), 64);
        assert!(!m1.is_empty());
        assert!(m1.codes().iter().all(|&c| c <= BiomeApi::PEAK));
    }

    #[test]
    fn distinct_seeds_or_sites_map_differently() {
        let base = BiomeApi::map(7, &site(&[2, 5]), 64);
        assert_ne!(base, BiomeApi::map(8, &site(&[2, 5]), 64)); // seed
        assert_ne!(base, BiomeApi::map(7, &site(&[2, 6]), 64)); // site
    }

    #[test]
    fn empty_map_is_empty() {
        let m = BiomeApi::map(7, &site(&[0]), 0);
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn golden_biome_map_digest_is_stable() {
        let m = BiomeApi::map(7, &site(&[2, 5]), 64);
        assert_eq!(m.digest().raw(), 9_396_021_443_120_572_672);
    }

    #[test]
    fn temperature_is_warmest_at_equator_sea_level_and_cools_with_latitude_and_elevation() {
        let equator = BiomeApi::temperature(Radians::new(0.0).unwrap(), Meters::new(0.0).unwrap());
        let pole = BiomeApi::temperature(
            Radians::new(core::f32::consts::FRAC_PI_2).unwrap(),
            Meters::new(0.0).unwrap(),
        );
        // Equator at sea level is the warmest: cos 0 = 1, no lapse.
        assert_eq!(equator.get(), 1.0);
        // The pole is colder than the equator (cos pi/2 ~ 0).
        assert!(pole.get() < equator.get());
        // Elevation above sea level cools via the lapse: 1.0 - 0.5 * 0.6 = 0.7.
        let highland = BiomeApi::temperature(Radians::new(0.0).unwrap(), Meters::new(0.5).unwrap());
        assert_eq!(highland.get(), 1.0 - 0.5 * LAPSE_RATE);
        // Below sea level (negative elevation) sheds no temperature: max(0.0).
        let below = BiomeApi::temperature(Radians::new(0.0).unwrap(), Meters::new(-3.0).unwrap());
        assert_eq!(below.get(), 1.0);
    }

    #[test]
    fn classify_climate_covers_ocean_and_every_land_quadrant() {
        let hot = Ratio::new(0.9).unwrap();
        let cold = Ratio::new(0.1).unwrap();
        let wet = Ratio::new(0.9).unwrap();
        let dry = Ratio::new(0.1).unwrap();
        // Ocean dominates below sea level regardless of temperature/moisture.
        assert_eq!(
            BiomeApi::classify_climate(hot, wet, true),
            BiomeApi::CLIMATE_OCEAN
        );
        // hot/cold x wet/dry on land, exercising all four table entries.
        assert_eq!(
            BiomeApi::classify_climate(hot, dry, false),
            BiomeApi::CLIMATE_DESERT
        );
        assert_eq!(
            BiomeApi::classify_climate(hot, wet, false),
            BiomeApi::CLIMATE_RAINFOREST
        );
        assert_eq!(
            BiomeApi::classify_climate(cold, dry, false),
            BiomeApi::CLIMATE_TUNDRA
        );
        assert_eq!(
            BiomeApi::classify_climate(cold, wet, false),
            BiomeApi::CLIMATE_TAIGA
        );
    }

    #[test]
    fn climate_and_elevation_code_universes_are_distinct_vocabularies() {
        // The two lenses are separate constant sets; asserting the climate codes
        // pins the app-facing colour numbering the gallery keys on.
        assert_eq!(BiomeApi::CLIMATE_OCEAN, 0);
        assert_eq!(BiomeApi::CLIMATE_DESERT, 1);
        assert_eq!(BiomeApi::CLIMATE_RAINFOREST, 2);
        assert_eq!(BiomeApi::CLIMATE_TUNDRA, 3);
        assert_eq!(BiomeApi::CLIMATE_TAIGA, 4);
    }

    #[test]
    fn types_are_debug() {
        let m = BiomeApi::map(7, &site(&[2, 5]), 2);
        assert!(!format!("{m:?}").is_empty());
        assert!(!format!("{:?}", BiomeApi).is_empty());
    }
}
