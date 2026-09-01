//! Rounding with ties broken toward `+∞`.

/// Round to the nearest integer, breaking ties toward **positive infinity**.
///
/// `round_ties_up(-2.5) == -2.0`, where [`f64::round`] gives `-3.0` (it breaks
/// ties *away from zero*) and IEEE's default `roundTiesToEven` gives `-2.0` for
/// `-2.5` but `-4.0` for `-3.5`. All three are legitimate; they are different
/// functions, and code that quantises with one and compares against another
/// disagrees on a set of inputs it will never think to test.
///
/// Reach for this wherever a tie has to fall the same way on both sides of
/// zero — quantising a coordinate onto a lattice, snapping a position to a
/// grid a lookup is keyed on. There the tie rule is not a rounding nicety: it
/// decides *which cell* a boundary value belongs to, and getting it wrong moves
/// a measure-zero set of inputs to a completely unrelated result. It is also
/// the rule JavaScript's `Math.round` uses, which matters when reproducing a
/// browser reference exactly.
///
/// ## `(x + 0.5).floor()` is not this function
///
/// That is the obvious implementation and it is wrong, at exactly one input per
/// binade. For `x = 0.49999999999999994` — the largest double below `0.5` —
/// adding `0.5` rounds *up* to exactly `1.0`, so the floor yields `1.0` where
/// the correct answer is `+0.0`. The addition rounds, and then the floor rounds
/// again; the first rounding is what makes the second one wrong.
///
/// The same double-rounding recurs at larger magnitudes wherever `x + 0.5`
/// carries across an integer boundary it should not have, which is why the
/// implementation below backs the carry out rather than trusting the floor. An
/// exact tie gives `floored - x == 0.5` and is deliberately left alone — that
/// tie landing on the upper integer is the behaviour this function exists for.
///
/// ## Sign of zero
///
/// Preserved. `round_ties_up(-0.2)` is `-0.0`, not `+0.0`, and
/// `round_ties_up(-0.5)` is `-0.0` too. That survives into anything that later
/// divides by the result or tests its sign.
///
/// Non-finite inputs pass through unchanged.
pub fn round_ties_up(x: f64) -> f64 {
    let passthrough = !x.is_finite() | (x == 0.0);

    // Stated before any flooring, deliberately: these two clauses are what make
    // the largest double below 0.5 round to +0 rather than to 1.
    let small_positive = (x > 0.0) & (x < 0.5);
    let small_negative = (x < 0.0) & (x >= -0.5);

    let floored = (x + 0.5).floor();
    let carried = usize::from(floored - x > 0.5);
    let corrected = [floored, floored - 1.0][carried];

    let with_small_negative = [corrected, -0.0][usize::from(small_negative)];
    let with_small_positive = [with_small_negative, 0.0][usize::from(small_positive)];
    [with_small_positive, x][usize::from(passthrough)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ties_break_toward_positive_infinity() {
        assert_eq!(round_ties_up(2.5), 3.0);
        assert_eq!(round_ties_up(1.5), 2.0);
        assert_eq!(round_ties_up(0.5), 1.0);
        assert_eq!(round_ties_up(-1.5), -1.0);
        assert_eq!(round_ties_up(-2.5), -2.0);
    }

    /// The divergence this function exists to prevent.
    #[test]
    fn differs_from_f64_round_on_every_negative_tie() {
        assert_eq!((-2.5_f64).round(), -3.0);
        assert_eq!(round_ties_up(-2.5), -2.0);
        assert_eq!((-0.5_f64).round(), -1.0);
        assert_eq!(round_ties_up(-0.5), 0.0);
    }

    #[test]
    fn non_ties_round_to_the_nearer_integer() {
        assert_eq!(round_ties_up(1.4), 1.0);
        assert_eq!(round_ties_up(1.6), 2.0);
        assert_eq!(round_ties_up(-1.4), -1.0);
        assert_eq!(round_ties_up(-1.6), -2.0);
        assert_eq!(round_ties_up(1_000_000.4), 1_000_000.0);
    }

    /// The input that makes `(x + 0.5).floor()` wrong. If this ever regresses,
    /// the implementation has quietly become the naive form.
    #[test]
    fn the_largest_double_below_a_half_rounds_to_zero_not_one() {
        let x = 0.499_999_999_999_999_94_f64;
        assert!(x < 0.5);
        assert_eq!((x + 0.5).floor(), 1.0, "the naive form really is wrong here");
        assert_eq!(round_ties_up(x), 0.0);
    }

    /// The same double-rounding at a larger magnitude, which the small-value
    /// clauses do not cover — this is what the carry correction is for.
    #[test]
    fn a_carry_across_an_integer_boundary_is_backed_out() {
        let x = 4_503_599_627_370_495.5_f64;
        assert_eq!(round_ties_up(x), 4_503_599_627_370_496.0);
        let just_below = 2.499_999_999_999_999_6_f64;
        assert!(just_below < 2.5);
        assert_eq!(round_ties_up(just_below), 2.0);
    }

    #[test]
    fn the_sign_of_zero_survives() {
        assert!(round_ties_up(-0.5).is_sign_negative());
        assert!(round_ties_up(-0.2).is_sign_negative());
        assert!(round_ties_up(0.2).is_sign_positive());
        assert!(round_ties_up(-0.0).is_sign_negative());
        assert!(round_ties_up(0.0).is_sign_positive());
    }

    #[test]
    fn non_finite_inputs_pass_through() {
        assert!(round_ties_up(f64::NAN).is_nan());
        assert_eq!(round_ties_up(f64::INFINITY), f64::INFINITY);
        assert_eq!(round_ties_up(f64::NEG_INFINITY), f64::NEG_INFINITY);
    }

    #[test]
    fn already_integral_values_are_unchanged() {
        assert_eq!(round_ties_up(3.0), 3.0);
        assert_eq!(round_ties_up(-3.0), -3.0);
    }
}
