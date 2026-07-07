//! [`EaseCurve`] — the deterministic phase-progress shaping curves.
//!
//! A phase advances a normalized progress `t` in `[0, 1]`; its ease curve reshapes
//! that progress before it drives interpolation, so a `SmoothStep` phase eases in
//! and out while a `Linear` phase moves uniformly.

/// A shaping curve applied to a phase's normalized progress. The discriminant is
/// the dispatch index into [`EaseCurve::apply`]'s table, so the order is fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EaseCurve {
    /// Uniform: `t`.
    Linear,
    /// Hermite smoothstep: `t*t*(3 - 2t)` — eases in and out.
    SmoothStep,
    /// Quadratic ease-in: `t*t` — slow start, fast finish.
    EaseIn,
    /// Quadratic ease-out: `t*(2 - t)` — fast start, slow finish.
    EaseOut,
}

impl EaseCurve {
    /// Reshape a progress value into `[0, 1]`. Out-of-range inputs clamp first, so
    /// the curve is total.
    pub(crate) fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        // Table-indexed dispatch — every arm is a pure expression, no branch.
        [t, t * t * (3.0 - 2.0 * t), t * t, t * (2.0 - t)][self as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-6
    }

    #[test]
    fn endpoints_are_fixed_for_every_curve() {
        [EaseCurve::Linear, EaseCurve::SmoothStep, EaseCurve::EaseIn, EaseCurve::EaseOut]
            .iter()
            .for_each(|c| {
                assert!(close(c.apply(0.0), 0.0), "{c:?} at 0");
                assert!(close(c.apply(1.0), 1.0), "{c:?} at 1");
            });
    }

    #[test]
    fn midpoints_match_the_documented_shapes() {
        assert!(close(EaseCurve::Linear.apply(0.5), 0.5));
        assert!(close(EaseCurve::SmoothStep.apply(0.5), 0.5));
        assert!(close(EaseCurve::EaseIn.apply(0.5), 0.25));
        assert!(close(EaseCurve::EaseOut.apply(0.5), 0.75));
    }

    #[test]
    fn out_of_range_progress_is_clamped() {
        assert!(close(EaseCurve::Linear.apply(-1.0), 0.0));
        assert!(close(EaseCurve::EaseIn.apply(4.0), 1.0));
    }
}
