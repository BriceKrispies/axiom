//! [`BiomeApi`] — deterministic biome classification on the proc substrate.
//!
//! `classify` buckets an `(elevation, moisture)` pair into a biome with a
//! branchless Whittaker-style band lookup; `map` generates a whole field by
//! drawing per-cell elevation/moisture from an entropy stream keyed by a content
//! address. The **domain rules** — the thresholds that decide ocean vs forest vs
//! peak — live here, never in the generic `proc-validate` layer. Integer-only and
//! branchless; biome codes are small `u8`s with named constants.

use axiom_entropy::EntropyApi;
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
    fn types_are_debug() {
        let m = BiomeApi::map(7, &site(&[2, 5]), 2);
        assert!(!format!("{m:?}").is_empty());
        assert!(!format!("{:?}", BiomeApi).is_empty());
    }
}
