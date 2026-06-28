//! The host-seam numeric quantity: the single `f64` boundary of the embed seam.

/// A numeric value reported across the embed seam (SPEC-12).
///
/// `Score` carries a single `f64`. It is the **one sanctioned floating-point
/// boundary** of the host session/outcome contracts: an outcome's score, and
/// the numeric value of an opaque session parameter or metric, all enter and
/// leave as a `Score`. Its `new`/`get` are the boundary where a raw scalar
/// crosses — exactly the shape of the kernel's `Ratio` and the host's
/// [`crate::Pixels`], so a naked `f64` never appears anywhere else on the
/// public surface.
///
/// A score is **non-sim**: it is derived from deterministic final state and
/// reported, never fed back into a fixed update (SPEC-12 §6). Float
/// non-determinism across machines therefore cannot affect replay, so an
/// `f64` is the correct carrier here and the constructor is infallible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score(f64);

impl Score {
    /// Wrap a raw `f64` as a reported host-seam value. Infallible: a score is
    /// a reported output, not a validated sim input.
    pub const fn new(value: f64) -> Self {
        Score(value)
    }

    /// The underlying `f64`.
    pub const fn get(self) -> f64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_round_trips_through_get() {
        assert_eq!(Score::new(123.5).get(), 123.5);
    }

    #[test]
    fn equal_scores_compare_equal_and_unequal_scores_differ() {
        assert_eq!(Score::new(7.0), Score::new(7.0));
        assert_ne!(Score::new(7.0), Score::new(8.0));
    }
}
