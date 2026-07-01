//! Determinism / QA hashing. Audit: SC-E3 (determinism hash of elevation +
//! moisture), "Correctness/QA requirements".
use crate::growth::model_planet::PlanetGlobe;
use axiom_kernel::StableHash;

/// Stable digest over the canonical scalar fields, using the kernel's
/// platform-stable FNV-1a [`StableHash`] instead of a hand-rolled copy. Each
/// field's `f32` bits are appended in little-endian order and the whole buffer is
/// folded — byte-for-byte the FNV-1a this used to compute inline, so the digest
/// value is unchanged. A diagnostic QA index, never the proof: byte equality
/// remains the source of truth for determinism.
pub fn world_hash(globe: &PlanetGlobe) -> u64 {
    let bytes: Vec<u8> = globe
        .region_elevation
        .iter()
        .chain(globe.region_moisture.iter())
        .flat_map(|f| f.to_bits().to_le_bytes())
        .collect();
    StableHash::of_bytes(&bytes).raw()
}
