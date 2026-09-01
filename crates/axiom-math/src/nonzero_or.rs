//! The degenerate-divisor guard: substitute for zero and NaN, clamp nothing.

/// `value`, unless it is zero or NaN, in which case `fallback`.
///
/// The guard for a divisor that must not be degenerate — normalising a vector
/// whose length may be zero, dividing by a distance that may be zero. The
/// degenerate case collapses to the fallback and the geometry stays finite,
/// instead of producing an infinity or a NaN that surfaces three subsystems
/// later with no trace of where it came from.
///
/// ## This is a substitution, not a clamp, and they are different functions
///
/// `f64::max` raises *every* value below the threshold; this one replaces only
/// the degenerate ones:
///
/// ```text
/// nonzero_or(5e-5, 1e-4) == 5e-5     // a small distance is still a distance
/// 5e-5_f64.max(1e-4)     == 1e-4     // ...and the clamp has invented one
/// ```
///
/// They agree at exactly zero and above the fallback, and disagree across the
/// whole interval between — which is precisely the range a "guard against tiny
/// divisors" is reached for. A radial impulse applied within a tenth of a
/// millimetre of a blast centre is the case that separates them: the
/// substitution leaves it alone, the clamp silently strengthens it.
///
/// NaN counts as degenerate. A NaN divisor is not a small number, it is the
/// absence of one, and propagating it defeats the point of guarding at all.
pub fn nonzero_or(value: f64, fallback: f64) -> f64 {
    let degenerate = (value == 0.0) | value.is_nan();
    [value, fallback][usize::from(degenerate)]
}

/// [`nonzero_or`] with a fallback of `1.0` — the unit divisor, for normalising
/// a possibly-zero-length vector without a special case at the call site.
pub fn nonzero_or_one(value: f64) -> f64 {
    nonzero_or(value, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nonzero_value_passes_through() {
        assert_eq!(nonzero_or(2.5, 1e-4), 2.5);
        assert_eq!(nonzero_or(-2.5, 1e-4), -2.5);
        assert_eq!(nonzero_or_one(2.5), 2.5);
    }

    #[test]
    fn both_signed_zeroes_are_degenerate() {
        assert_eq!(nonzero_or(0.0, 7.0), 7.0);
        assert_eq!(nonzero_or(-0.0, 7.0), 7.0);
        assert_eq!(nonzero_or_one(0.0), 1.0);
        assert_eq!(nonzero_or_one(-0.0), 1.0);
    }

    #[test]
    fn nan_is_degenerate_rather_than_propagated() {
        assert_eq!(nonzero_or(f64::NAN, 7.0), 7.0);
        assert_eq!(nonzero_or_one(f64::NAN), 1.0);
    }

    /// The distinction the doc comment turns on, asserted so it cannot be
    /// "simplified" into a clamp.
    #[test]
    fn is_a_substitution_and_not_a_clamp() {
        assert_eq!(nonzero_or(5e-5, 1e-4), 5e-5);
        assert_eq!(5e-5_f64.max(1e-4), 1e-4, "the clamp this is not");
        // They agree only at zero and at or above the fallback.
        assert_eq!(nonzero_or(0.0, 1e-4), 0.0_f64.max(1e-4));
        assert_eq!(nonzero_or(1.0, 1e-4), 1.0_f64.max(1e-4));
    }

    #[test]
    fn infinity_is_not_degenerate() {
        assert_eq!(nonzero_or(f64::INFINITY, 7.0), f64::INFINITY);
    }

    /// The reason it exists: normalising a zero-length vector stays finite.
    #[test]
    fn dividing_by_a_guarded_zero_length_stays_finite() {
        let length = 0.0_f64;
        let normalised = 0.0 / nonzero_or_one(length);
        assert_eq!(normalised, 0.0);
        assert!((0.0_f64 / length).is_nan(), "unguarded, this is NaN");
    }
}
