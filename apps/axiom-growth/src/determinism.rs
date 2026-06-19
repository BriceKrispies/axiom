//! Determinism / QA hashing. Audit: SC-E3 (determinism hash of elevation +
//! moisture), "Correctness/QA requirements".
use crate::model_planet::PlanetGlobe;

/// FNV-1a hash over the canonical scalar fields. Same inputs to the same hash.
pub fn world_hash(globe: &PlanetGlobe) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    fn mix(bits: u32, h: &mut u64) {
        for b in bits.to_le_bytes() {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    for &e in &globe.region_elevation {
        mix(e.to_bits(), &mut h);
    }
    for &m in &globe.region_moisture {
        mix(m.to_bits(), &mut h);
    }
    h
}
