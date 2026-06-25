//! [`Save`] — the compact, regenerable save: seed + version + address + deltas.

use axiom_kernel::{BinaryWriter, StableHash};
use axiom_space::{Address, SpaceApi};

/// Everything needed to reproduce a world *except* the world itself: the `seed`,
/// the generator `world_version`, the world's `address` and dimensions, and the
/// player's `overrides` (per-cell biome edits, as `(cell_index, biome_code)`).
/// Loading regenerates the world and replays the overrides, so the save stays tiny
/// no matter how large the world. Returned by [`crate::WorldSaveApi`].
#[derive(Debug)]
pub struct Save {
    seed: u64,
    world_version: u32,
    width: u32,
    height: u32,
    address: Address,
    overrides: Vec<(u32, u8)>,
}

impl Save {
    pub(crate) fn new(
        seed: u64,
        world_version: u32,
        width: u32,
        height: u32,
        address: Address,
        overrides: Vec<(u32, u8)>,
    ) -> Self {
        Save {
            seed,
            world_version,
            width,
            height,
            address,
            overrides,
        }
    }

    /// The world seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The generator version the world was made with. A caller restoring a save
    /// must check this against the current generator — a mismatch means the
    /// regenerated world will not match the original (versioning is explicit).
    pub fn world_version(&self) -> u32 {
        self.world_version
    }

    /// The world dimensions `(width, height)`.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The player's per-cell biome overrides: `(cell_index, biome_code)`.
    pub fn overrides(&self) -> &[(u32, u8)] {
        &self.overrides
    }

    pub(crate) fn address(&self) -> &Address {
        &self.address
    }

    /// The compact save bytes: seed, version, dimensions, the address, and the
    /// overrides. Far smaller than the world it regenerates.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u64(self.seed);
        writer.write_u32(self.world_version);
        writer.write_u32(self.width);
        writer.write_u32(self.height);
        let address_bytes = SpaceApi::to_bytes(&self.address);
        writer.write_u64(address_bytes.len() as u64);
        address_bytes.iter().for_each(|&byte| writer.write_u32(u32::from(byte)));
        writer.write_u64(self.overrides.len() as u64);
        self.overrides.iter().for_each(|&(index, code)| {
            writer.write_u32(index);
            writer.write_u32(u32::from(code));
        });
        writer.into_bytes()
    }

    /// The stable digest over [`Self::to_bytes`].
    pub fn digest(&self) -> StableHash {
        StableHash::of_bytes(&self.to_bytes())
    }
}
