//! [`ProcAnimApi`] — deterministic procedural animation: `(seed, address, tick)`
//! → an [`AnimatedTransform`].
//!
//! Each entity's motion is keyed by its `space` [`Address`]: an `entropy` stream
//! over that address draws the entity's animation parameters (phase, bob, spin,
//! pulse, period) once, and those drive a fixed-point sine **oscillation** and a
//! continuous **ramp** evaluated at `tick`. So the same entity bobs, spins, and
//! pulses the same way on every run and platform, and two entities at different
//! addresses animate differently. Integer-only and branchless.

use axiom_entropy::EntropyApi;
use axiom_space::Address;

use crate::animated_transform::AnimatedTransform;

/// A fixed-point sine over one full turn: 16 samples, scaled to ±[`SINE_SCALE`].
/// A table read is branchless where `sin` would be a transcendental call.
const SINE: [i32; 16] =
    [0, 383, 707, 924, 1000, 924, 707, 383, 0, -383, -707, -924, -1000, -924, -707, -383];

/// The fixed-point scale of [`SINE`] (`1000` = ×1).
const SINE_SCALE: i32 = 1000;

/// One full turn in milliradians (2π × 1000), the modulus a yaw ramp wraps at.
const FULL_TURN: i32 = 6283;

/// The animation version keying the entropy stream — a future change to the
/// parameter draw is a deliberate, detectable reseed, never silent drift.
const ANIM_VERSION: u32 = 1;

/// The procedural-animation facade.
#[derive(Debug)]
pub struct ProcAnimApi;

impl ProcAnimApi {
    /// The animated transform for the entity at `address`, under `seed`, at `tick`.
    /// Deterministic: identical `(seed, address, tick)` always yields identical
    /// output; the entity bobs (Y offset), spins (yaw), and pulses (scale).
    pub fn animate(seed: u64, address: &Address, tick: u64) -> AnimatedTransform {
        let mut stream = EntropyApi::stream(seed, address, ANIM_VERSION);
        let phase = stream.next_bounded(SINE.len() as u64);
        let bob = stream.next_bounded(400) as i32 + 100;
        let spin = stream.next_bounded(20) as i32 + 1;
        let pulse = stream.next_bounded(150) as i32;
        let period = stream.next_bounded(48) + 16;
        let bob_offset = oscillate(tick + phase, period, bob);
        let pulse_offset = oscillate(tick + phase, period, pulse);
        let scale = SINE_SCALE + pulse_offset;
        AnimatedTransform::new([0, bob_offset, 0], ramp(tick, spin), [scale, scale, scale])
    }
}

/// Fixed-point sine oscillation in `[-amplitude, amplitude]`, one cycle every
/// `period` ticks (`period >= 1`). Branchless: a table index, not a branch.
fn oscillate(tick: u64, period: u64, amplitude: i32) -> i32 {
    let index = (tick * SINE.len() as u64 / period % SINE.len() as u64) as usize;
    SINE[index] * amplitude / SINE_SCALE
}

/// A continuous yaw ramp: `rate` milliradians per tick, wrapped to a full turn.
fn ramp(tick: u64, rate: i32) -> i32 {
    (tick * rate as u64 % FULL_TURN as u64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_space::SpaceApi;

    fn site(segments: &[u64]) -> Address {
        segments.iter().fold(SpaceApi::root(), |a, &s| SpaceApi::child(&a, s))
    }

    #[test]
    fn animation_is_deterministic() {
        let a = site(&[3]);
        let t = ProcAnimApi::animate(7, &a, 42);
        assert_eq!(t, ProcAnimApi::animate(7, &a, 42));
        assert_eq!(t.to_bytes(), ProcAnimApi::animate(7, &a, 42).to_bytes());
        assert_eq!(t, t.clone());
    }

    #[test]
    fn animation_evolves_over_time() {
        let a = site(&[3]);
        let first = ProcAnimApi::animate(7, &a, 0);
        // It actually moves: at least one later tick differs from the first.
        let moves = (1..40u64).any(|tick| ProcAnimApi::animate(7, &a, tick) != first);
        assert!(moves, "the entity must animate over time");
    }

    #[test]
    fn distinct_entities_animate_differently() {
        // Two addresses draw different parameters, so they diverge at some tick.
        let differ = (0..16u64)
            .any(|tick| ProcAnimApi::animate(7, &site(&[1]), tick) != ProcAnimApi::animate(7, &site(&[2]), tick));
        assert!(differ, "different entities must animate differently");
    }

    #[test]
    fn the_transform_stays_in_its_documented_bounds() {
        let a = site(&[3]);
        (0..128u64).for_each(|tick| {
            let t = ProcAnimApi::animate(7, &a, tick);
            // Bob is on Y only; X and Z offsets are zero.
            assert_eq!(t.offset()[0], 0);
            assert_eq!(t.offset()[2], 0);
            assert!(t.offset()[1].unsigned_abs() <= 500, "bob within +-500 milliunits");
            // Scale stays positive around 1000 (pulse < 150).
            assert!(t.scale().iter().all(|&s| (851..=1149).contains(&s)), "scale near 1.0");
            // Yaw stays within one full turn.
            assert!((0..FULL_TURN).contains(&t.yaw()), "yaw within a full turn");
        });
    }

    #[test]
    fn a_still_entity_at_tick_zero_has_no_yaw() {
        // At tick 0 the ramp is 0 for every spin rate.
        assert_eq!(ProcAnimApi::animate(7, &site(&[5]), 0).yaw(), 0);
    }

    #[test]
    fn golden_animation_digest_is_stable() {
        let t = ProcAnimApi::animate(7, &site(&[3]), 42);
        assert_eq!(t.digest().raw(), 16_551_335_488_770_522_567);
    }

    #[test]
    fn types_are_debug() {
        let t = ProcAnimApi::animate(7, &site(&[3]), 1);
        assert!(!format!("{t:?}").is_empty());
        assert!(!format!("{:?}", ProcAnimApi).is_empty());
    }
}
