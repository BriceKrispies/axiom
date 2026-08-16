//! The six **arithmetic** operators, every one of them component-wise over the
//! node's operating width, with a `Scalar` input broadcasting across it.
//!
//! | Operator | Semantics |
//! |---|---|
//! | `Add(a, b)` | `a + b`, component-wise |
//! | `Sub(a, b)` | `a - b`, component-wise |
//! | `Mul(a, b)` | `a * b`, component-wise |
//! | `Min(a, b)` | `f32::min(a, b)`, component-wise |
//! | `Max(a, b)` | `f32::max(a, b)`, component-wise |
//! | `Abs(a)` | `f32::abs(a)`, component-wise |
//!
//! The output type is the node's operating width — the widest input — which is
//! exactly the type the checker's `WidthGeneric` rule derived. That is why the
//! evaluator needs no type table: every operator re-derives its own output type
//! from the same data the checker used, so no `Vec<FieldType>` is allocated per
//! evaluation.

use crate::eval::{typed, zip2, FieldEvalStep};
use crate::field_value::FieldValue;

/// `Add(a, b)` — **component-wise `a + b`**; a `Scalar` input broadcasts.
pub(crate) fn add(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(step.width(), zip2(step.lanes(0), step.lanes(1), sum))
}

/// `Sub(a, b)` — **component-wise `a - b`**; a `Scalar` input broadcasts.
pub(crate) fn subtract(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(step.width(), zip2(step.lanes(0), step.lanes(1), difference))
}

/// `Mul(a, b)` — **component-wise `a * b`**; a `Scalar` input broadcasts.
pub(crate) fn multiply(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(step.width(), zip2(step.lanes(0), step.lanes(1), product))
}

/// `Min(a, b)` — **component-wise `f32::min(a, b)`**; a `Scalar` input
/// broadcasts.
pub(crate) fn minimum(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(step.width(), zip2(step.lanes(0), step.lanes(1), f32::min))
}

/// `Max(a, b)` — **component-wise `f32::max(a, b)`**; a `Scalar` input
/// broadcasts.
pub(crate) fn maximum(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(step.width(), zip2(step.lanes(0), step.lanes(1), f32::max))
}

/// `Abs(a)` — **component-wise `f32::abs(a)`**.
pub(crate) fn absolute(step: &FieldEvalStep<'_>) -> FieldValue {
    let a = step.lanes(0);
    typed(step.width(), [a[0].abs(), a[1].abs(), a[2].abs(), a[3].abs()])
}

/// `a + b`. A named `fn` rather than a closure, so [`zip2`] stays a plain
/// function pointer.
fn sum(a: f32, b: f32) -> f32 {
    a + b
}

/// `a - b`.
fn difference(a: f32, b: f32) -> f32 {
    a - b
}

/// `a * b`.
fn product(a: f32, b: f32) -> f32 {
    a * b
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
    fn add_is_component_wise() {
        assert_eq!(run(add, &[scalar(4.0), scalar(3.0)]), scalar(7.0));
        assert_eq!(
            run(add, &[vec3(1.0, 2.0, 3.0), vec3(10.0, 20.0, 30.0)]),
            vec3(11.0, 22.0, 33.0)
        );
    }

    #[test]
    fn sub_is_component_wise_and_keeps_its_operand_order() {
        assert_eq!(run(subtract, &[scalar(4.0), scalar(3.0)]), scalar(1.0));
        assert_eq!(
            run(subtract, &[vec3(1.0, 2.0, 3.0), vec3(0.5, 0.5, 0.5)]),
            vec3(0.5, 1.5, 2.5)
        );
    }

    #[test]
    fn mul_is_component_wise() {
        assert_eq!(run(multiply, &[scalar(4.0), scalar(3.0)]), scalar(12.0));
        assert_eq!(
            run(multiply, &[vec3(1.0, 2.0, 3.0), vec3(2.0, 2.0, 2.0)]),
            vec3(2.0, 4.0, 6.0)
        );
    }

    #[test]
    fn min_is_component_wise() {
        assert_eq!(run(minimum, &[scalar(4.0), scalar(3.0)]), scalar(3.0));
        assert_eq!(
            run(minimum, &[vec3(1.0, 5.0, 3.0), vec3(4.0, 2.0, 3.0)]),
            vec3(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn max_is_component_wise() {
        assert_eq!(run(maximum, &[scalar(4.0), scalar(3.0)]), scalar(4.0));
        assert_eq!(
            run(maximum, &[vec3(1.0, 5.0, 3.0), vec3(4.0, 2.0, 3.0)]),
            vec3(4.0, 5.0, 3.0)
        );
    }

    #[test]
    fn abs_is_component_wise() {
        assert_eq!(run(absolute, &[scalar(-4.0)]), scalar(4.0));
        assert_eq!(
            run(absolute, &[vec3(-1.0, 2.0, -3.0)]),
            vec3(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn a_scalar_broadcasts_across_every_width_generic_operator() {
        let ops: [(fn(&FieldEvalStep<'_>) -> FieldValue, FieldValue); 5] = [
            (add, vec3(3.0, 4.0, 5.0)),
            (subtract, vec3(-1.0, 0.0, 1.0)),
            (multiply, vec3(2.0, 4.0, 6.0)),
            (minimum, vec3(1.0, 2.0, 2.0)),
            (maximum, vec3(2.0, 2.0, 3.0)),
        ];
        ops.iter().for_each(|(op, expected)| {
            let value = run(*op, &[vec3(1.0, 2.0, 3.0), scalar(2.0)]);
            assert_eq!(value.ty(), FieldType::Vec3);
            assert_eq!(value, *expected);
        });
        // The broadcast is symmetric: the scalar may sit in either slot.
        assert_eq!(
            run(add, &[scalar(2.0), vec3(1.0, 2.0, 3.0)]),
            vec3(3.0, 4.0, 5.0)
        );
    }

    #[test]
    fn the_lane_functions_are_plain_arithmetic() {
        assert_eq!(sum(1.5, 2.5), 4.0);
        assert_eq!(difference(1.5, 2.5), -1.0);
        assert_eq!(product(1.5, 2.5), 3.75);
    }
}
