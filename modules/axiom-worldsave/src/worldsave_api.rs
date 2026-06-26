//! [`WorldSaveApi`] — capture and restore worlds as compact saves.
//!
//! `save` bundles only the regeneration inputs + the player's deltas; `restore`
//! regenerates the levelgen world from the seed and replays the deltas on top, so a
//! world reproduces from a tiny save byte-for-byte. The same `{seed, deltas}` shape
//! underlies lockstep multiplayer (a peer ships a seed + a command/delta stream,
//! never full state). Branchless.

use axiom_levelgen::LevelGenApi;
use axiom_space::Address;

use crate::save::Save;
use crate::saved_world::SavedWorld;

/// The generator version a save records, so a future generation change is a
/// deliberate, detectable mismatch — never silent.
const WORLD_VERSION: u32 = 1;

/// The save/delta facade.
#[derive(Debug)]
pub struct WorldSaveApi;

impl WorldSaveApi {
    /// Capture a save for the world at `address` under `seed`, with the player's
    /// per-cell biome `overrides`. The save stores only these inputs — never the
    /// generated world.
    pub fn save(
        seed: u64,
        address: &Address,
        width: u32,
        height: u32,
        overrides: &[(u32, u8)],
    ) -> Save {
        Save::new(
            seed,
            WORLD_VERSION,
            width,
            height,
            address.clone(),
            overrides.to_vec(),
        )
    }

    /// Restore the world from a save: regenerate the levelgen world from the seed,
    /// then replay the player's biome overrides on top. Byte-identical to the live
    /// world the save was captured from (an out-of-range override is a no-op).
    pub fn restore(save: &Save) -> SavedWorld {
        let (width, height) = save.dimensions();
        let world = LevelGenApi::generate(save.seed(), save.address(), width, height);
        let mut biomes = world.biomes().to_vec();
        save.overrides().iter().for_each(|&(index, code)| {
            biomes
                .get_mut(index as usize)
                .into_iter()
                .for_each(|cell| *cell = code);
        });
        SavedWorld::new(
            width,
            height,
            world.heights().to_vec(),
            biomes,
            world.objects().to_vec(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_space::SpaceApi;

    // Biome code 3 == FOREST in axiom-biome; worldsave does not depend on biome
    // (it stores opaque u8 codes), so the test uses the raw code.
    const FOREST: u8 = 3;

    fn site(segments: &[u64]) -> Address {
        segments
            .iter()
            .fold(SpaceApi::root(), |a, &s| SpaceApi::child(&a, s))
    }

    #[test]
    fn restore_is_deterministic_and_dimensioned() {
        let save = WorldSaveApi::save(7, &site(&[1, 2]), 16, 16, &[(5, FOREST)]);
        let a = WorldSaveApi::restore(&save);
        let b = WorldSaveApi::restore(&save);
        assert_eq!(a, b);
        assert_eq!(a.to_bytes(), b.to_bytes());
        assert_eq!(a.width(), 16);
        assert_eq!(a.height(), 16);
        assert_eq!(a.heights().len(), 256);
        assert_eq!(a.biomes().len(), 256);
        assert_eq!(save.world_version(), 1);
        assert_eq!(save.overrides(), &[(5, FOREST)]);
    }

    #[test]
    fn an_override_replaces_only_its_cells_biome() {
        let address = site(&[1, 2]);
        let base = WorldSaveApi::restore(&WorldSaveApi::save(7, &address, 16, 16, &[]));
        let edited =
            WorldSaveApi::restore(&WorldSaveApi::save(7, &address, 16, 16, &[(5, FOREST)]));
        assert_eq!(edited.biomes()[5], FOREST);
        assert_eq!(base.heights(), edited.heights()); // terrain unchanged
        assert_eq!(base.objects(), edited.objects()); // objects unchanged
        for i in 0..256 {
            if i != 5 {
                assert_eq!(base.biomes()[i], edited.biomes()[i]);
            }
        }
    }

    #[test]
    fn a_save_is_far_smaller_than_the_world_it_regenerates() {
        // The payoff: store seed + version + address + deltas, not the full world.
        let save = WorldSaveApi::save(7, &site(&[1, 2]), 32, 32, &[(5, 3), (100, 4)]);
        let world = WorldSaveApi::restore(&save);
        let save_len = save.to_bytes().len();
        let world_len = world.to_bytes().len();
        // A 56-byte save regenerates a >4KB world: at least a 4x reduction.
        assert!(
            save_len * 4 < world_len,
            "a save must be far smaller than its world"
        );
    }

    #[test]
    fn an_out_of_range_override_is_a_safe_noop() {
        let address = site(&[1, 2]);
        let base = WorldSaveApi::restore(&WorldSaveApi::save(7, &address, 4, 4, &[]));
        let edited = WorldSaveApi::restore(&WorldSaveApi::save(7, &address, 4, 4, &[(9999, 3)]));
        assert_eq!(base.biomes(), edited.biomes());
    }

    #[test]
    fn distinct_seeds_or_addresses_save_to_distinct_worlds() {
        let base = WorldSaveApi::restore(&WorldSaveApi::save(7, &site(&[1, 2]), 16, 16, &[]));
        assert_ne!(
            base,
            WorldSaveApi::restore(&WorldSaveApi::save(8, &site(&[1, 2]), 16, 16, &[]))
        );
        assert_ne!(
            base,
            WorldSaveApi::restore(&WorldSaveApi::save(7, &site(&[1, 3]), 16, 16, &[]))
        );
    }

    #[test]
    fn golden_save_and_world_digests_are_stable() {
        let save = WorldSaveApi::save(7, &site(&[1, 2]), 16, 16, &[(5, FOREST)]);
        let world = WorldSaveApi::restore(&save);
        assert_eq!(save.digest().raw(), 10_540_502_306_818_545_085);
        assert_eq!(world.digest().raw(), 8_248_762_692_994_712_636);
    }

    #[test]
    fn types_are_debug() {
        let save = WorldSaveApi::save(7, &site(&[1, 2]), 2, 2, &[]);
        let world = WorldSaveApi::restore(&save);
        assert!(!format!("{save:?}").is_empty());
        assert!(!format!("{world:?}").is_empty());
        assert!(!format!("{:?}", WorldSaveApi).is_empty());
    }
}
