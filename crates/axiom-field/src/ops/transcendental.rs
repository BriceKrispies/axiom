//! The four **transcendental** operators, every one of them component-wise over
//! the node's operating width, with a `Scalar` input broadcasting across it.
//!
//! | Operator | Semantics |
//! |---|---|
//! | `Sin(a)` | `f32::sin(a)`, component-wise |
//! | `Cos(a)` | `f32::cos(a)`, component-wise |
//! | `Pow(a, b)` | `f32::powf(a, b)` where `a > 0`, and **`0.0` everywhere else** |
//! | `Exp(a)` | `f32::exp(a)`, component-wise |
//!
//! ## This tier's CPU↔GPU budget is measured, not shared
//!
//! Every other operator in the algebra is exact arithmetic (`+`, `*`, `min`,
//! `sqrt`) whose CPU and GPU results differ only by the hardware's permitted
//! contraction. These four are *approximated* by both sides, with different
//! polynomials, so their parity budget is measured **per operator** rather than
//! sharing the algebra's `1e-4`. The measurement's finding, recorded in
//! `crates/axiom-field/ARCHITECTURE.md`, is worth knowing before assuming: on a
//! real adapter the tier agrees to about `1e-6` *relative*, so its budget came
//! out **tighter** than the algebra's default, not wider.
//!
//! CPU-to-CPU determinism is unaffected for a given target — `f32::sin` and
//! friends are deterministic per input — but they reach the platform's libm,
//! which Rust does not promise is bit-identical *across* targets. That is the one
//! documented limit of this tier, and it is scoped to these four operators.
//!
//! ## `Pow` yields `0.0` for every base at or below zero
//!
//! Not `NaN`, and not the CPU's `f32::powf` answer for a negative base with an
//! integral exponent. The reason is the mirror: WGSL's `pow(e1, e2)` is
//! **undefined** when `e1 < 0`, and undefined when `e1 == 0` with `e2 <= 0`, so a
//! CPU rule that produced `-8.0` for `Pow(-2, 3)` would be a rule the shader
//! cannot reproduce — a silent divergence rather than a documented one. One rule
//! covers all three hazards, is total, mirrors exactly, and can never produce a
//! `NaN` or an infinity from a finite base:
//!
//! * `a > 0` → `f32::powf(a, b)`, the ordinary answer.
//! * `a <= 0` → `0.0`, including `Pow(0, 0)` (which IEEE calls `1.0`) and every
//!   negative base.
//!
//! **A square is `Mul(x, x)`, not `Pow(x, 2)`** — the latter is `0` wherever `x`
//! is negative, by this rule. A reciprocal is `Pow(x, -1)`, `0` at and below
//! zero, which is exactly why the algebra still has no `Div`.

use crate::eval::{typed, zip2, FieldEvalStep};
use crate::field_value::FieldValue;

/// `Sin(a)` — **component-wise `f32::sin(a)`**, in radians.
pub(crate) fn sine(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(step.width(), step.lanes(0).map(f32::sin))
}

/// `Cos(a)` — **component-wise `f32::cos(a)`**, in radians.
pub(crate) fn cosine(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(step.width(), step.lanes(0).map(f32::cos))
}

/// `Pow(a, b)` — **component-wise `f32::powf(a, b)` where `a > 0`, and `0.0`
/// everywhere else**; a `Scalar` input broadcasts.
pub(crate) fn power(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(
        step.width(),
        zip2(step.lanes(0), step.lanes(1), positive_power),
    )
}

/// `Exp(a)` — **component-wise `f32::exp(a)`**. A large enough input overflows to
/// an infinity, exactly as `Mul` does, and constant folding refuses to bake a
/// non-finite value.
pub(crate) fn exponential(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(step.width(), step.lanes(0).map(f32::exp))
}

/// `a.powf(b)` for a strictly positive base, `0.0` otherwise. A named `fn` rather
/// than a closure, so [`zip2`] stays a plain function pointer.
fn positive_power(a: f32, b: f32) -> f32 {
    [0.0, a.max(0.0).powf(b)][usize::from(a > 0.0)]
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
    fn sin_is_component_wise_and_in_radians() {
        assert_eq!(run(sine, &[scalar(0.0)]), scalar(0.0));
        assert_eq!(
            run(sine, &[vec3(0.5, -1.25, 2.0)]),
            vec3(0.5_f32.sin(), (-1.25_f32).sin(), 2.0_f32.sin())
        );
    }

    #[test]
    fn cos_is_component_wise_and_in_radians() {
        assert_eq!(run(cosine, &[scalar(0.0)]), scalar(1.0));
        assert_eq!(
            run(cosine, &[vec3(0.5, -1.25, 2.0)]),
            vec3(0.5_f32.cos(), (-1.25_f32).cos(), 2.0_f32.cos())
        );
    }

    #[test]
    fn exp_is_component_wise() {
        assert_eq!(run(exponential, &[scalar(0.0)]), scalar(1.0));
        assert_eq!(
            run(exponential, &[vec3(1.0, -2.0, 0.25)]),
            vec3(1.0_f32.exp(), (-2.0_f32).exp(), 0.25_f32.exp())
        );
    }

    #[test]
    fn exp_overflows_to_an_infinity_rather_than_saturating() {
        assert!(run(exponential, &[scalar(1.0e6)])
            .as_scalar()
            .get()
            .is_infinite());
    }

    #[test]
    fn pow_is_the_ordinary_power_for_a_positive_base() {
        assert_eq!(run(power, &[scalar(2.0), scalar(10.0)]), scalar(1024.0));
        assert_eq!(run(power, &[scalar(4.0), scalar(0.5)]), scalar(2.0));
        assert_eq!(
            run(power, &[vec3(1.0, 2.0, 3.0), scalar(2.0)]),
            vec3(1.0, 4.0, 9.0)
        );
    }

    #[test]
    fn pow_of_anything_to_the_zero_is_one_for_a_positive_base() {
        assert_eq!(run(power, &[scalar(7.5), scalar(0.0)]), scalar(1.0));
        // …and zero at and below zero, where the documented rule takes over even
        // though IEEE would call `0^0` one.
        assert_eq!(run(power, &[scalar(0.0), scalar(0.0)]), scalar(0.0));
    }

    #[test]
    fn pow_yields_zero_for_every_base_at_or_below_zero_and_never_a_nan() {
        // The case the rule exists for: a negative base with a non-integral
        // exponent, which `f32::powf` calls NaN and WGSL calls undefined.
        assert!((-2.0_f32).powf(0.5).is_nan());
        assert_eq!(run(power, &[scalar(-2.0), scalar(0.5)]), scalar(0.0));
        // And the negative-base integral exponent, which `f32::powf` answers but
        // the shader cannot.
        assert_eq!((-2.0_f32).powf(3.0), -8.0);
        assert_eq!(run(power, &[scalar(-2.0), scalar(3.0)]), scalar(0.0));
        // A zero base with a negative exponent, which `f32::powf` calls infinite.
        assert_eq!(0.0_f32.powf(-1.0), f32::INFINITY);
        assert_eq!(run(power, &[scalar(0.0), scalar(-1.0)]), scalar(0.0));
        assert_eq!(
            run(power, &[vec3(-1.0, 0.0, 4.0), scalar(0.5)]),
            vec3(0.0, 0.0, 2.0)
        );
    }

    #[test]
    fn a_scalar_broadcasts_across_every_transcendental() {
        let ops: [fn(&FieldEvalStep<'_>) -> FieldValue; 3] = [sine, cosine, exponential];
        ops.iter().for_each(|op| {
            let value = run(*op, &[vec3(0.25, 0.5, 0.75)]);
            assert_eq!(value.ty(), FieldType::Vec3);
        });
        // `Pow`'s scalar may sit in either slot.
        assert_eq!(
            run(power, &[scalar(2.0), vec3(1.0, 2.0, 3.0)]),
            vec3(2.0, 4.0, 8.0)
        );
        assert_eq!(
            run(power, &[vec3(2.0, 3.0, 4.0), scalar(2.0)]).ty(),
            FieldType::Vec3
        );
    }

    #[test]
    fn sin_and_cos_at_a_large_argument_are_whatever_the_target_libm_says() {
        // Deliberately not a committed number: `f32::sin` is deterministic for a
        // given input on a given target but is not guaranteed bit-identical
        // across targets, and a GPU's range reduction is coarser still. What is
        // asserted is the bound every implementation must respect.
        let large = 1.0e7_f32;
        let s = run(sine, &[scalar(large)]).as_scalar().get();
        let c = run(cosine, &[scalar(large)]).as_scalar().get();
        assert!(s.abs() <= 1.0);
        assert!(c.abs() <= 1.0);
        assert!((s * s + c * c - 1.0).abs() < 1.0e-3);
    }

    #[test]
    fn the_lane_function_is_the_documented_rule() {
        assert_eq!(positive_power(3.0, 2.0), 9.0);
        assert_eq!(positive_power(-3.0, 2.0), 0.0);
        assert_eq!(positive_power(0.0, 1.0), 0.0);
    }
}
