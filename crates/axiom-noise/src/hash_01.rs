//! The integer-hash lattice basis: a position folded to an unsigned unit sample.

use axiom_math::DVec3;

use crate::unit_noise::UnitNoise;

/// Round half-way values **up** — toward `+∞`, not away from zero.
///
/// `round_ties_up(-1.5) == -1.0`, where Rust's `f64::round` gives `-2.0`. Both
/// are legitimate tie-breaking rules and neither is IEEE's default
/// (`roundTiesToEven`); this one is the convention JavaScript's `Math.round`
/// uses, which is why a lattice basis transcribed from a browser reference must
/// state which it means rather than inherit the host language's.
///
/// The rule matters here specifically because it decides which lattice cell a
/// coordinate exactly on a half-boundary hashes into. Getting it wrong shifts a
/// measure-zero set of positions to a *completely unrelated* hash — a
/// discontinuity, not a rounding error.
///
/// Private, and staying that way until something outside this file needs it. It
/// was briefly public on the argument that a caller reproducing the basis in a
/// shader would have to round identically — true, and not yet a caller. A public
/// `fn(f64) -> f64` is also exactly the unitless-float surface the rulebook
/// bans, and the rulebook was right: "someone might need it" is how a layer
/// grows an API nothing calls.
fn round_ties_up(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// [`round_ties_up`], narrowed to the 32-bit word the hash mixes in.
///
/// Saturating rather than wrapping at the extremes: `f64 as i32` saturates in
/// Rust where a 32-bit-truncating language would wrap. The two agree for every
/// `|v| < 2^31`, which is every coordinate a world-space lattice is sampled at
/// by many orders of magnitude, so the divergence is unreachable rather than
/// handled — adding a wraparound arm would be dead code the coverage gate could
/// never reach honestly.
fn lattice_word(v: f64) -> u32 {
    round_ties_up(v) as i32 as u32
}

/// The three per-axis odd multipliers that spread a coordinate across the
/// 32-bit word before it is mixed. Distinct and mutually coprime so that two
/// different positions cannot collide by symmetry (`(a, b, c)` and its
/// permutations hash apart).
const AXIS_SPREAD: DVec3 = DVec3::new(1013.0, 1619.0, 31337.0);

/// The three mix constants, applied one per axis fold. These are the
/// widely-used MurmurHash3 finalizer constants; they are load-bearing (the
/// avalanche behaviour is theirs) and are not tunable knobs.
const MIX: [u32; 3] = [0x85eb_ca6b, 0xc2b2_ae35, 0x27d4_eb2f];

/// The seed the first axis is XORed against before its multiply, so a position
/// of all zeros does not hash to zero.
const SEED: u32 = 0x27d4_eb2d;

/// Deterministic 3D hash of a position to an unsigned unit sample.
///
/// A pure function of the position — no seed argument, no RNG stream, no
/// ambient state. That is the property the whole basis is chosen for: a caller
/// adding a draw to some *other* system's random stream cannot reshuffle this
/// one, because this one never consulted a stream. The wear pattern on a wall
/// stays where it is when an unrelated subsystem changes.
///
/// Every intermediate is 32-bit wrapping integer arithmetic, so the result is
/// exact and bit-identical on every platform — no transcendental, no rounding
/// beyond the tie rule in [`round_ties_up`], and no dependence on float
/// associativity.
pub fn hash_01(p: DVec3) -> UnitNoise {
    let spread = p.mul_componentwise(AXIS_SPREAD);
    let h = (lattice_word(spread.x) ^ SEED).wrapping_mul(MIX[0]);
    let h = (h ^ lattice_word(spread.y)).wrapping_mul(MIX[1]);
    let h = (h ^ lattice_word(spread.z)).wrapping_mul(MIX[2]);
    let h = h ^ (h >> 15);
    // `2^32`, written as the exact power it is. `f64::from(u32)` is exact and
    // the divisor is a power of two, so the quotient is exact too — every
    // representable output is a dyadic rational, never a rounded one.
    UnitNoise::from_signal(f64::from(h) / 4_294_967_296.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_ties_up_breaks_ties_toward_positive_infinity() {
        assert_eq!(round_ties_up(1.5), 2.0);
        assert_eq!(round_ties_up(0.5), 1.0);
        assert_eq!(round_ties_up(-0.5), 0.0);
        assert_eq!(round_ties_up(-1.5), -1.0);
        assert_eq!(round_ties_up(-2.5), -2.0);
    }

    /// The rule this function exists to *not* be. Rust rounds ties away from
    /// zero; if the basis used that, negative half-boundary coordinates would
    /// land in a different lattice cell entirely.
    #[test]
    fn round_ties_up_differs_from_rusts_round_on_negative_ties() {
        assert_eq!((-1.5_f64).round(), -2.0);
        assert_eq!(round_ties_up(-1.5), -1.0);
    }

    #[test]
    fn round_ties_up_leaves_non_ties_alone() {
        assert_eq!(round_ties_up(1.4), 1.0);
        assert_eq!(round_ties_up(1.6), 2.0);
        assert_eq!(round_ties_up(-1.4), -1.0);
        assert_eq!(round_ties_up(-1.6), -2.0);
    }

    #[test]
    fn hash_is_in_the_unit_interval() {
        (0..64).for_each(|i| {
            let t = f64::from(i) * 0.37 - 12.0;
            let v = hash_01(DVec3::new(t, -t * 0.5, t * 2.25)).get();
            assert!((0.0..1.0).contains(&v), "hash out of range: {v}");
        });
    }

    #[test]
    fn hash_is_a_pure_function_of_position() {
        let p = DVec3::new(3.5, -1.25, 9.0);
        assert_eq!(hash_01(p), hash_01(p));
    }

    /// Neighbouring cells must decorrelate — that is the whole job of the mix
    /// constants. A basis whose adjacent hashes tracked each other would show
    /// visible lattice banding rather than noise.
    ///
    /// The property is **statistical, not pointwise**: a good hash produces
    /// uniform outputs, so an individual adjacent pair landing close together
    /// is expected, not a defect. For two independent uniforms on `[0, 1)`,
    /// `E|U₁ - U₂| = 1/3`; a hash that merely incremented would sit near zero.
    /// The bound is loose enough to be a decorrelation test rather than a
    /// re-statement of these particular constants.
    #[test]
    fn adjacent_lattice_cells_decorrelate_on_average() {
        let mean_gap = |step: DVec3| {
            let total: f64 = (0..256)
                .map(|i| {
                    let base = DVec3::new(f64::from(i), f64::from(i % 7), f64::from(i % 13));
                    (hash_01(base).get() - hash_01(base.add(step)).get()).abs()
                })
                .sum();
            total / 256.0
        };
        [DVec3::UNIT_X, DVec3::UNIT_Y, DVec3::UNIT_Z]
            .into_iter()
            .for_each(|axis| {
                let gap = mean_gap(axis);
                assert!(
                    (0.25..0.42).contains(&gap),
                    "mean adjacent gap along {axis:?} was {gap}, expected near 1/3"
                );
            });
    }

    /// The axes are not interchangeable: permuting a coordinate triple must
    /// give an unrelated hash, which is what the distinct `AXIS_SPREAD`
    /// multipliers buy.
    #[test]
    fn permuting_the_axes_changes_the_hash() {
        let a = hash_01(DVec3::new(1.0, 2.0, 3.0)).get();
        let b = hash_01(DVec3::new(3.0, 1.0, 2.0)).get();
        assert_ne!(a, b);
    }

    /// The origin must not hash to zero — that is what `SEED` is for.
    #[test]
    fn the_origin_does_not_hash_to_zero() {
        assert_ne!(hash_01(DVec3::ZERO).get(), 0.0);
    }

    /// Large coordinates stay in range rather than saturating to a constant,
    /// covering the `as i32` narrowing on inputs far from the origin.
    #[test]
    fn large_coordinates_still_hash_into_range() {
        let v = hash_01(DVec3::new(1.0e6, -1.0e6, 1.0e6)).get();
        assert!((0.0..1.0).contains(&v));
    }
}
