//! The three **shaping** operators. Every one is component-wise over the node's
//! operating width, with a `Scalar` input broadcasting across it.
//!
//! | Operator | Semantics |
//! |---|---|
//! | `Clamp(x, lo, hi)` | `max(min(x, hi), lo)` — `lo > hi` yields **`lo`**, documented, not undefined |
//! | `Mix(a, b, t)` | `a + (b - a) * t`, **`t` unclamped** |
//! | `Smoothstep(e0, e1, x)` | `t = clamp((x - e0) / (e1 - e0), 0, 1); t * t * (3 - 2 * t)`; `e0 == e1` yields `0` |
//!
//! The exact spellings matter and are not interchangeable with the algebraically
//! equal ones a mirror might reach for:
//!
//! * `Mix` is `a + (b - a) * t`, **not** `a * (1 - t) + b * t`. The two differ in
//!   the last `f32` bit, and CPU/GPU parity is budgeted in ulps.
//! * `Clamp` is `max(min(x, hi), lo)`, **not** `min(max(x, lo), hi)`. The two
//!   agree whenever `lo <= hi` and disagree exactly on the degenerate node — the
//!   first yields `lo`, the second `hi` — so the order is the documented rule.

use crate::eval::{typed, zip2, FieldEvalStep};
use crate::field_value::FieldValue;

/// `Clamp(x, lo, hi)` — **component-wise `max(min(x, hi), lo)`**. A node with
/// `lo > hi` yields `lo`: total and documented, never undefined.
pub(crate) fn clamp(step: &FieldEvalStep<'_>) -> FieldValue {
    let lowered = zip2(step.lanes(0), step.lanes(2), f32::min);
    typed(step.width(), zip2(lowered, step.lanes(1), f32::max))
}

/// `Mix(a, b, t)` — **component-wise `a + (b - a) * t`**, with `t` **unclamped**:
/// a `t` outside `0..=1` extrapolates, which is the language's only selection and
/// deliberately not a gate.
pub(crate) fn mix(step: &FieldEvalStep<'_>) -> FieldValue {
    let (a, b, t) = (step.lanes(0), step.lanes(1), step.lanes(2));
    typed(
        step.width(),
        [0_usize, 1, 2, 3].map(|lane| a[lane] + (b[lane] - a[lane]) * t[lane]),
    )
}

/// `Smoothstep(e0, e1, x)` — **component-wise
/// `t = clamp((x - e0) / (e1 - e0), 0, 1); t * t * (3 - 2 * t)`**.
///
/// A lane whose edges are equal divides by zero; that lane yields `0`, stated as
/// a rule rather than left to whatever `clamp` does with the resulting NaN or
/// infinity.
pub(crate) fn smoothstep(step: &FieldEvalStep<'_>) -> FieldValue {
    let (edge0, edge1, x) = (step.lanes(0), step.lanes(1), step.lanes(2));
    typed(
        step.width(),
        [0_usize, 1, 2, 3].map(|lane| {
            let t = ((x[lane] - edge0[lane]) / (edge1[lane] - edge0[lane])).clamp(0.0, 1.0);
            [t * t * (3.0 - 2.0 * t), 0.0][usize::from(edge0[lane] == edge1[lane])]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;
    use axiom_recipe::Scalar;

    use crate::eval_context::EvalContext;
    use crate::field_params::FieldParams;
    use crate::field_type::FieldType;

    fn scalar(value: f32) -> FieldValue {
        FieldValue::scalar(Scalar::new(value))
    }

    fn vec3(x: f32, y: f32, z: f32) -> FieldValue {
        FieldValue::vec3(Vec3::new(x, y, z))
    }

    fn run(op: fn(&FieldEvalStep<'_>) -> FieldValue, inputs: &[FieldValue]) -> FieldValue {
        let table = FieldParams::new();
        op(&FieldEvalStep::new(inputs, &[], &EvalContext::ORIGIN, &table))
    }

    #[test]
    fn clamp_bounds_each_lane_between_its_low_and_high() {
        assert_eq!(run(clamp, &[scalar(5.0), scalar(0.0), scalar(1.0)]), scalar(1.0));
        assert_eq!(run(clamp, &[scalar(-5.0), scalar(0.0), scalar(1.0)]), scalar(0.0));
        assert_eq!(run(clamp, &[scalar(0.25), scalar(0.0), scalar(1.0)]), scalar(0.25));
        assert_eq!(
            run(clamp, &[vec3(-1.0, 0.5, 9.0), scalar(0.0), scalar(1.0)]),
            vec3(0.0, 0.5, 1.0)
        );
    }

    #[test]
    fn clamp_with_a_low_above_its_high_yields_the_low() {
        // The documented degenerate rule: `max(min(x, hi), lo)` collapses to `lo`
        // whatever `x` is. The other spelling would yield `hi`.
        [(-9.0_f32), 0.0, 9.0].iter().for_each(|x| {
            assert_eq!(
                run(clamp, &[scalar(*x), scalar(3.0), scalar(1.0)]),
                scalar(3.0),
                "clamp({x}, lo=3, hi=1) must be the low"
            );
        });
    }

    #[test]
    fn mix_is_a_plus_b_minus_a_times_t() {
        assert_eq!(run(mix, &[scalar(0.0), scalar(10.0), scalar(0.25)]), scalar(2.5));
        assert_eq!(
            run(mix, &[vec3(0.0, 1.0, 2.0), vec3(2.0, 1.0, 0.0), scalar(0.5)]),
            vec3(1.0, 1.0, 1.0)
        );
        // The exact form, bit for bit: `a*(1-t) + b*t` is a different number here.
        let (a, b, t) = (0.1_f32, 0.7_f32, 0.3_f32);
        assert_eq!(
            run(mix, &[scalar(a), scalar(b), scalar(t)]).as_scalar().get(),
            a + (b - a) * t
        );
    }

    #[test]
    fn mix_does_not_clamp_its_parameter() {
        assert_eq!(run(mix, &[scalar(0.0), scalar(10.0), scalar(2.0)]), scalar(20.0));
        assert_eq!(run(mix, &[scalar(0.0), scalar(10.0), scalar(-1.0)]), scalar(-10.0));
    }

    #[test]
    fn smoothstep_shapes_between_its_edges_and_saturates_outside_them() {
        assert_eq!(
            run(smoothstep, &[scalar(0.0), scalar(2.0), scalar(1.0)]),
            scalar(0.5)
        );
        assert_eq!(
            run(smoothstep, &[scalar(0.0), scalar(2.0), scalar(-1.0)]),
            scalar(0.0)
        );
        assert_eq!(
            run(smoothstep, &[scalar(0.0), scalar(2.0), scalar(9.0)]),
            scalar(1.0)
        );
        let quarter = 0.25_f32;
        assert_eq!(
            run(smoothstep, &[scalar(0.0), scalar(1.0), scalar(quarter)])
                .as_scalar()
                .get(),
            quarter * quarter * (3.0 - 2.0 * quarter)
        );
    }

    #[test]
    fn smoothstep_with_equal_edges_is_zero_whatever_the_sample_is() {
        [-1.0_f32, 1.0, 5.0].iter().for_each(|x| {
            assert_eq!(
                run(smoothstep, &[scalar(1.0), scalar(1.0), scalar(*x)]),
                scalar(0.0),
                "equal edges must yield zero at x = {x}"
            );
        });
        // Only the degenerate lane collapses; the others still shape.
        assert_eq!(
            run(
                smoothstep,
                &[
                    vec3(0.0, 1.0, 0.0),
                    vec3(2.0, 1.0, 2.0),
                    vec3(1.0, 5.0, 3.0)
                ]
            ),
            vec3(0.5, 0.0, 1.0)
        );
    }

    #[test]
    fn a_scalar_broadcasts_across_every_shaping_operator() {
        let value = run(clamp, &[vec3(-1.0, 0.5, 9.0), scalar(0.0), scalar(1.0)]);
        assert_eq!(value.ty(), FieldType::Vec3);
        assert_eq!(
            run(mix, &[scalar(0.0), vec3(1.0, 2.0, 3.0), scalar(0.5)]),
            vec3(0.5, 1.0, 1.5)
        );
        assert_eq!(
            run(
                smoothstep,
                &[scalar(0.0), scalar(1.0), vec3(0.0, 0.5, 1.0)]
            ),
            vec3(0.0, 0.5, 1.0)
        );
    }
}
