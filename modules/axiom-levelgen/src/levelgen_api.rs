//! [`LevelGenApi`] — a composed world recipe over terrain + biome + placement.
//!
//! `generate` is the sanctioned multi-module composition: it reads each domain
//! module's facade and translates their contracts into one neutral [`World`]. It
//! builds a terrain elevation field, an independent terrain moisture field,
//! classifies the two into a biome map, and scatters objects — all keyed
//! deterministically by `(seed, address)`. Branchless.

use axiom_biome::BiomeApi;
use axiom_placement::PlacementApi;
use axiom_space::{Address, SpaceApi};
use axiom_terrain::TerrainApi;

use crate::world::World;

/// Salt deriving the moisture field's seed from the world seed, so elevation and
/// moisture are independent terrain fields.
const MOISTURE_SALT: u64 = 0x6d6f_6973_7475_7265;
/// One scattered object per this many cells.
const CELLS_PER_OBJECT: u32 = 16;

/// The composed world-recipe facade.
#[derive(Debug)]
pub struct LevelGenApi;

impl LevelGenApi {
    /// Generate a `width × height` world at `address` under `seed`, composing a
    /// terrain elevation field, an independent terrain moisture field, a biome map
    /// classified from the two, and a scatter of objects. Deterministic in
    /// `(seed, address)`.
    pub fn generate(seed: u64, address: &Address, width: u32, height: u32) -> World {
        let world_seed = seed ^ SpaceApi::digest(address).raw();
        let elevation = TerrainApi::heightfield(world_seed, 0, 0, width, height);
        let moisture = TerrainApi::heightfield(world_seed ^ MOISTURE_SALT, 0, 0, width, height);
        let biomes = elevation
            .heights()
            .iter()
            .zip(moisture.heights())
            .map(|(&e, &m)| BiomeApi::classify(e as u32, m as u32))
            .collect();
        let object_count = width * height / CELLS_PER_OBJECT;
        let placement = PlacementApi::scatter(world_seed, address, object_count, width, height);
        World::new(
            width,
            height,
            elevation.heights().to_vec(),
            biomes,
            placement.positions().to_vec(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(segments: &[u64]) -> Address {
        segments
            .iter()
            .fold(SpaceApi::root(), |a, &s| SpaceApi::child(&a, s))
    }

    #[test]
    fn generate_composes_a_consistent_world() {
        let w = LevelGenApi::generate(7, &site(&[1, 2]), 16, 16);
        assert_eq!(w.width(), 16);
        assert_eq!(w.height(), 16);
        assert_eq!(w.heights().len(), 256);
        assert_eq!(w.biomes().len(), 256);
        assert_eq!(w.objects().len(), 256 / 16);
        assert!(w.objects().iter().all(|&(x, y)| x < 16 && y < 16));
    }

    #[test]
    fn generation_is_deterministic() {
        let a = site(&[1, 2]);
        let w1 = LevelGenApi::generate(7, &a, 16, 16);
        let w2 = LevelGenApi::generate(7, &a, 16, 16);
        assert_eq!(w1, w2);
        assert_eq!(w1.to_bytes(), w2.to_bytes());
    }

    #[test]
    fn the_biome_map_reflects_the_terrain() {
        let w = LevelGenApi::generate(7, &site(&[1, 2]), 32, 32);
        let first = w.biomes()[0];
        assert!(
            w.biomes().iter().any(|&b| b != first),
            "terrain should yield varied biomes"
        );
        assert!(w.biomes().iter().all(|&b| b <= BiomeApi::PEAK));
    }

    #[test]
    fn distinct_seeds_or_addresses_yield_distinct_worlds() {
        let base = LevelGenApi::generate(7, &site(&[1, 2]), 16, 16);
        assert_ne!(base, LevelGenApi::generate(8, &site(&[1, 2]), 16, 16));
        assert_ne!(base, LevelGenApi::generate(7, &site(&[1, 3]), 16, 16));
    }

    #[test]
    fn golden_world_digest_is_stable() {
        let w = LevelGenApi::generate(7, &site(&[1, 2]), 16, 16);
        assert_eq!(w.digest().raw(), 18_209_658_541_754_841_967);
    }

    #[test]
    fn types_are_debug() {
        let w = LevelGenApi::generate(7, &site(&[1, 2]), 2, 2);
        assert!(!format!("{w:?}").is_empty());
        assert!(!format!("{:?}", LevelGenApi).is_empty());
    }
}
