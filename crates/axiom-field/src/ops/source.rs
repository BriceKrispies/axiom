//! The six **source** operators: the ones that read something rather than
//! compute something.
//!
//! | Operator | Semantics |
//! |---|---|
//! | `Const` | the parameter words, typed by the declared `FieldType` |
//! | `Point` | the evaluation context's `point`, a `Vec3` |
//! | `Uv` | the evaluation context's `uv`, a `Vec2` |
//! | `Normal` | the evaluation context's `normal`, a `Vec3` |
//! | `Time` | the evaluation context's `time`, a `Scalar` |
//! | `Param` | `FieldParams[slot]` |

use axiom_recipe::Scalar;

use crate::eval::FieldEvalStep;
use crate::field_type::FieldType;
use crate::field_value::FieldValue;
use crate::ids::FieldParamSlot;

/// `Const` — **the parameter words, typed by the declared `FieldType`**: word 0
/// is the type code and words 1..5 are the four lanes, exactly the encoding
/// [`FieldValue::const_params`] writes.
///
/// A type code naming no type reads as [`FieldValue::ZERO`]; validation rejects
/// such a node (`UnknownType`) long before evaluation.
pub(crate) fn constant(step: &FieldEvalStep<'_>) -> FieldValue {
    u16::try_from(step.word(0))
        .ok()
        .and_then(FieldType::from_code)
        .map_or(FieldValue::ZERO, |ty| {
            FieldValue::from_words(
                ty,
                [step.word(1), step.word(2), step.word(3), step.word(4)],
            )
        })
}

/// `Point` — **the evaluation context's `point`**, in whatever space the caller
/// supplied, as a `Vec3`.
pub(crate) fn point(step: &FieldEvalStep<'_>) -> FieldValue {
    FieldValue::vec3(step.context().point())
}

/// `Uv` — **the evaluation context's `uv`**, as a `Vec2`.
pub(crate) fn uv(step: &FieldEvalStep<'_>) -> FieldValue {
    FieldValue::vec2(step.context().uv())
}

/// `Normal` — **the evaluation context's `normal`**, as a `Vec3`.
pub(crate) fn normal(step: &FieldEvalStep<'_>) -> FieldValue {
    FieldValue::vec3(step.context().normal())
}

/// `Time` — **the evaluation context's `time`**, as a `Scalar`.
pub(crate) fn time(step: &FieldEvalStep<'_>) -> FieldValue {
    FieldValue::scalar(Scalar::new(step.context().time().get()))
}

/// `Param` — **the value the parameter table holds in the slot named by word
/// 0**, whatever type that slot carries.
///
/// A slot the table does not have reads as [`FieldValue::ZERO`], the same fill
/// [`crate::FieldParams`] itself uses for a gap; validation rejects such a node
/// (`UnknownParamSlot`) long before evaluation.
pub(crate) fn parameter(step: &FieldEvalStep<'_>) -> FieldValue {
    u16::try_from(step.word(0))
        .ok()
        .map(FieldParamSlot::from_raw)
        .and_then(|slot| step.table().get(slot))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Seconds;
    use axiom_math::{Vec2, Vec3};
    use axiom_recipe::Param;

    use crate::eval_context::EvalContext;
    use crate::field_params::FieldParams;

    fn context() -> EvalContext {
        EvalContext::new(
            Vec3::new(1.0, 2.0, 3.0),
            Vec2::new(0.25, 0.75),
            Vec3::UNIT_X,
            Seconds::finite_or_zero(1.5),
        )
    }

    fn table() -> FieldParams {
        FieldParams::new().with(
            FieldParamSlot::from_raw(0),
            FieldValue::vec2(Vec2::new(9.0, 8.0)),
        )
    }

    fn run(
        op: fn(&FieldEvalStep<'_>) -> FieldValue,
        words: &[u32],
        table: &FieldParams,
    ) -> FieldValue {
        let params: Vec<Param> = words.iter().copied().map(Param::from_bits).collect();
        let context = context();
        op(&FieldEvalStep::new(&[], &params, &context, table))
    }

    #[test]
    fn const_is_its_declared_type_and_its_four_lane_words() {
        let value = FieldValue::vec3(Vec3::new(1.5, -2.5, 3.5));
        let words: Vec<u32> = value.const_params().iter().map(|p| p.bits()).collect();
        assert_eq!(run(constant, &words, &FieldParams::new()), value);
    }

    #[test]
    fn const_with_a_type_code_naming_no_type_reads_as_zero() {
        assert_eq!(
            run(constant, &[9, 1, 1, 1, 1], &FieldParams::new()),
            FieldValue::ZERO
        );
        assert_eq!(
            run(constant, &[u32::MAX, 0, 0, 0, 0], &FieldParams::new()),
            FieldValue::ZERO
        );
    }

    #[test]
    fn point_is_the_contexts_point() {
        assert_eq!(
            run(point, &[], &FieldParams::new()),
            FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0))
        );
    }

    #[test]
    fn uv_is_the_contexts_uv() {
        assert_eq!(
            run(uv, &[], &FieldParams::new()),
            FieldValue::vec2(Vec2::new(0.25, 0.75))
        );
    }

    #[test]
    fn normal_is_the_contexts_normal() {
        assert_eq!(
            run(normal, &[], &FieldParams::new()),
            FieldValue::vec3(Vec3::UNIT_X)
        );
    }

    #[test]
    fn time_is_the_contexts_time_as_a_scalar() {
        let value = run(time, &[], &FieldParams::new());
        assert_eq!(value.ty(), FieldType::Scalar);
        assert_eq!(value.as_scalar().get(), 1.5);
    }

    #[test]
    fn param_reads_the_slot_word_zero_names() {
        assert_eq!(
            run(parameter, &[0], &table()),
            FieldValue::vec2(Vec2::new(9.0, 8.0))
        );
    }

    #[test]
    fn param_reading_a_slot_the_table_lacks_is_the_zero_default() {
        assert_eq!(run(parameter, &[7], &table()), FieldValue::ZERO);
        // A slot index past `u16` cannot name a slot at all.
        assert_eq!(run(parameter, &[u32::MAX], &table()), FieldValue::ZERO);
    }
}
