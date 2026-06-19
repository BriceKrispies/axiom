//! Deterministic, float-capable RNG for the Growth substrate.
//!
//! The Axiom kernel's `DeterministicRng` (splitmix64) deliberately exposes only
//! integer sampling. Worldgen needs floats, ranges, and unit vectors on the
//! sphere everywhere, so this app-level wrapper adds them on top of the same
//! splitmix64 mixing — keeping every generation stage replayable from a seed.
//!
//! Audit: "Determinism requirements", "Float / distribution sampling" gap.
//! This is a candidate to graduate into the kernel/math layer once proven.

use axiom_math::Vec3;

/// A seeded deterministic generator. Same seed → identical stream, every run,
/// every platform. Pure splitmix64; no global state, no wall clock.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Construct from a 64-bit seed.
    pub fn seeded(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Derive an independent sub-stream from a string-ish salt (e.g. a stage id
    /// or chunk coordinate), so different subsystems do not share a sequence.
    pub fn fork(&self, salt: u64) -> Self {
        Self {
            state: self.state ^ splitmix64(salt.wrapping_add(0x9E37_79B9_7F4A_7C15)),
        }
    }

    /// Next raw 64-bit value (splitmix64 step).
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        splitmix64(self.state)
    }

    /// Uniform `u32`.
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform integer in `[0, bound)` (bound > 0).
    pub fn next_bounded(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        // Lemire's multiply-high reduction.
        ((self.next_u64() as u128 * bound as u128) >> 64) as u64
    }

    /// Uniform `f32` in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        // 24 high bits → [0,1) with full float mantissa precision.
        ((self.next_u64() >> 40) as f32) * (1.0 / 16_777_216.0)
    }

    /// Uniform `f32` in `[min, max)`.
    pub fn next_range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }

    /// A roughly standard-normal `f32` via Box–Muller (deterministic).
    pub fn next_normal(&mut self) -> f32 {
        let u1 = (self.next_f32()).max(1.0e-7);
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (core::f32::consts::TAU * u2).cos()
    }

    /// A uniformly-distributed unit vector on the sphere. Audit: tectonic plate
    /// seeds, spherical sampling.
    pub fn next_unit_vec3(&mut self) -> Vec3 {
        // z uniform in [-1,1], theta uniform — area-preserving on the sphere.
        let z = self.next_range(-1.0, 1.0);
        let theta = self.next_range(0.0, core::f32::consts::TAU);
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * theta.cos(), r * theta.sin(), z)
    }
}

fn splitmix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::seeded(42);
        let mut b = Rng::seeded(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn floats_in_unit_range() {
        let mut r = Rng::seeded(7);
        for _ in 0..10_000 {
            let f = r.next_f32();
            assert!((0.0..1.0).contains(&f));
        }
    }

    #[test]
    fn unit_vectors_are_unit_length() {
        let mut r = Rng::seeded(99);
        for _ in 0..1000 {
            let v = r.next_unit_vec3();
            assert!((v.length() - 1.0).abs() < 1.0e-3);
        }
    }

    #[test]
    fn fork_diverges() {
        let base = Rng::seeded(1);
        let mut a = base.fork(10);
        let mut b = base.fork(20);
        assert_ne!(a.next_u64(), b.next_u64());
    }
}
