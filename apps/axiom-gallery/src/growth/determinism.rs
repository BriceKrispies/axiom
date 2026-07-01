//! Determinism / QA hashing.
use crate::growth::model_planet::PlanetGlobe;
use axiom_kernel::StableHash;

/// Stable digest over elevation + moisture, via the kernel's platform-stable
/// FNV-1a [`StableHash`]. A diagnostic QA index, not the proof of determinism
/// — byte equality of the fields themselves remains the source of truth.
pub fn world_hash(globe: &PlanetGlobe) -> u64 {
    let bytes: Vec<u8> = globe
        .region_elevation
        .iter()
        .chain(globe.region_moisture.iter())
        .flat_map(|f| f.to_bits().to_le_bytes())
        .collect();
    StableHash::of_bytes(&bytes).raw()
}
