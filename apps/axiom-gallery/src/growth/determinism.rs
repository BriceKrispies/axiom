//! Determinism / QA hashing.
use crate::growth::model_planet::PlanetSurfaceAtlas;
use axiom_kernel::StableHash;

/// Stable digest over the generated atlas's elevation + moisture fields, via the
/// kernel's platform-stable FNV-1a [`StableHash`]. A diagnostic QA index, not the
/// proof of determinism — byte equality of the fields themselves remains the
/// source of truth. (These are the same bytes the transient globe carried before
/// `axiom_planetgen::build_atlas` copied them into the durable atlas.)
pub fn world_hash(atlas: &PlanetSurfaceAtlas) -> u64 {
    let bytes: Vec<u8> = atlas
        .region_elevation
        .iter()
        .chain(atlas.region_moisture.iter())
        .flat_map(|f| f.to_bits().to_le_bytes())
        .collect();
    StableHash::of_bytes(&bytes).raw()
}
