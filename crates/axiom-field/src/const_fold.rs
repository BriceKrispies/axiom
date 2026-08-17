//! **Exact** constant folding: a node whose every input is already a constant is
//! replaced by the value **the evaluator would compute for it**.
//!
//! This file holds no arithmetic of its own. It answers one question — *may this
//! operator be evaluated ahead of time?* — and then calls
//! [`crate::dispatch::field_eval`], the same table the evaluator runs. That is
//! not a convenience: a second implementation of `Mix` or `Clamp` here would be a
//! second definition of what the language means, and the two would eventually
//! differ in a last bit and make `canonicalize` silently change what a graph
//! computes.
//!
//! ## What is deliberately not folded, and why
//!
//! Exactly the operators whose value is not a function of the node alone:
//!
//! * **`Point` / `Uv` / `Normal` / `Time`** — the context is supplied per
//!   evaluation, so they have no value at canonicalisation time.
//! * **`Param`** — its value lives in the parameter table. Folding it would move
//!   a *value* into *structure*, which is precisely what the split between
//!   [`crate::FieldGraph::serialize`] and [`crate::FieldGraph::digest`] exists to
//!   prevent: retuning a parameter would start changing the digest.
//! * **`Transform`** — its matrix lives in the parameter table too, for the same
//!   reason.
//!
//! **`Sin`, `Cos`, `Pow` and `Exp` are folded.** They are pure and total
//! functions of their inputs, so nothing about the transcendental tier's separate
//! CPU↔GPU tolerance argues against folding one. The consequence is worth
//! stating: a folded `Sin` is computed **on the CPU** and baked into a `Const`,
//! which every backend then reads verbatim — so a graph that folds is *more*
//! CPU-exact than the same graph unfolded, where the GPU would approximate the
//! sine itself. That is a strict improvement, never a divergence, because
//! folding only ever happens where the input was already a constant.
//!
//! **`Noise` and `Fbm` are folded.** They are pure functions of their seed
//! words, their knob words and their input point, and now that the CPU evaluator
//! is the semantic reference for what every backend must compute, folding one is
//! no longer fixing a value the backend might disagree with.
//!
//! ## And no algebra
//!
//! There is **no** rewriting beyond exact folding: no `x*1 -> x`, no `x+0 -> x`,
//! no reassociation, no strength reduction. Every one of those can change the
//! last bit of an `f32`, and the CPU/GPU parity budget cannot absorb that. A fold
//! whose result is not finite is refused outright, so a node that overflows
//! stays a node and overflows identically wherever it is evaluated.

use axiom_recipe::Param;

use crate::dispatch;
use crate::eval::FieldEvalStep;
use crate::eval_context::EvalContext;
use crate::field_op::{FieldOp, FIELD_OP_COUNT};
use crate::field_params::FieldParams;
use crate::field_value::FieldValue;

/// Which operators canonicalisation may evaluate ahead of time, indexed by the
/// operator code, in code order.
///
/// `false` means "the value is not a function of this node alone" — it reads the
/// evaluation context or the parameter table — never "the arithmetic is
/// unavailable here".
#[rustfmt::skip]
const FOLDABLE: [bool; FIELD_OP_COUNT] = [
    true,                                   // Const
    false, false, false, false,             // Point / Uv / Normal / Time
    false,                                  // Param
    true,  true,  true,  true,  true,       // Add / Sub / Mul / Min / Max
    true,                                   // Abs
    true,  true,  true,                     // Clamp / Mix / Smoothstep
    true,  true,  true,                     // Dot / Length / Normalize
    true,  true,                            // Compose / Component
    true,  true,                            // Noise / Fbm
    false,                                  // Transform
    true,  true,  true,  true,              // Sin / Cos / Pow / Exp
];

/// The value of the node `op` computes from `inputs`, or `None` when it cannot
/// be known at canonicalisation time.
///
/// `inputs` carries `None` for an input whose value is not constant, so an
/// operator folds only when **every** input folded. The value is whatever the
/// evaluator computes for the same operator, words and inputs — the context and
/// the parameter table handed to it are the neutral ones, which no foldable
/// operator reads.
pub(crate) fn fold_value(
    op: FieldOp,
    inputs: &[Option<FieldValue>],
    params: &[Param],
) -> Option<FieldValue> {
    FOLDABLE[op.code() as usize]
        .then(|| inputs.iter().copied().collect::<Option<Vec<FieldValue>>>())
        .flatten()
        .map(|values| {
            let context = EvalContext::ORIGIN;
            let table = FieldParams::new();
            dispatch::field_eval(
                op.code(),
                &FieldEvalStep::new(&values, params, &context, &table),
            )
        })
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3, Vec4};
    use axiom_noise::value_noise;
    use axiom_recipe::Scalar;

    use crate::field_type::FieldType;
    use crate::noise_words::seed_words;

    fn scalar(value: f32) -> Option<FieldValue> {
        Some(FieldValue::scalar(Scalar::new(value)))
    }

    fn vec3(x: f32, y: f32, z: f32) -> Option<FieldValue> {
        Some(FieldValue::vec3(Vec3::new(x, y, z)))
    }

    fn fold(op: FieldOp, inputs: &[Option<FieldValue>]) -> Option<FieldValue> {
        fold_value(op, inputs, &[])
    }

    #[test]
    fn the_binary_arithmetic_operators_fold_lane_by_lane() {
        let pairs = [
            (FieldOp::Add, 7.0_f32),
            (FieldOp::Sub, 1.0),
            (FieldOp::Mul, 12.0),
            (FieldOp::Min, 3.0),
            (FieldOp::Max, 4.0),
        ];
        pairs.iter().for_each(|(op, expected)| {
            assert_eq!(
                fold(*op, &[scalar(4.0), scalar(3.0)]),
                scalar(*expected),
                "{op:?} must fold exactly"
            );
        });
    }

    #[test]
    fn a_scalar_broadcasts_across_every_lane_of_the_vector_it_meets() {
        assert_eq!(
            fold(FieldOp::Mul, &[vec3(1.0, 2.0, 3.0), scalar(2.0)]),
            vec3(2.0, 4.0, 6.0)
        );
    }

    #[test]
    fn the_unary_and_ternary_shaping_operators_fold() {
        assert_eq!(fold(FieldOp::Abs, &[vec3(-1.0, 2.0, -3.0)]), vec3(1.0, 2.0, 3.0));
        assert_eq!(
            fold(FieldOp::Clamp, &[scalar(5.0), scalar(0.0), scalar(1.0)]),
            scalar(1.0)
        );
        assert_eq!(
            fold(FieldOp::Mix, &[scalar(0.0), scalar(10.0), scalar(0.25)]),
            scalar(2.5)
        );
        assert_eq!(
            fold(FieldOp::Smoothstep, &[scalar(0.0), scalar(2.0), scalar(1.0)]),
            scalar(0.5)
        );
    }

    #[test]
    fn a_degenerate_node_folds_to_the_total_value_the_evaluator_gives_it() {
        // These used to be refused because the old folder produced a NaN where
        // the operator's documented rule produces a value. Folding must agree
        // with evaluation everywhere, including the documented edges.
        assert_eq!(
            fold(FieldOp::Smoothstep, &[scalar(1.0), scalar(1.0), scalar(1.0)]),
            scalar(0.0)
        );
        assert_eq!(
            fold(FieldOp::Normalize, &[vec3(0.0, 0.0, 0.0)]),
            vec3(0.0, 1.0, 0.0)
        );
        assert_eq!(
            fold(FieldOp::Clamp, &[scalar(0.0), scalar(3.0), scalar(1.0)]),
            scalar(3.0)
        );
    }

    #[test]
    fn the_collapsing_operators_fold_to_a_scalar() {
        assert_eq!(
            fold(FieldOp::Dot, &[vec3(1.0, 2.0, 3.0), vec3(4.0, 5.0, 6.0)]),
            scalar(32.0)
        );
        assert_eq!(fold(FieldOp::Length, &[vec3(3.0, 4.0, 0.0)]), scalar(5.0));
        assert_eq!(
            fold(
                FieldOp::Length,
                &[Some(FieldValue::vec2(Vec2::new(3.0, 4.0)))]
            ),
            scalar(5.0)
        );
        assert_eq!(
            fold(FieldOp::Normalize, &[vec3(0.0, 5.0, 0.0)]),
            vec3(0.0, 1.0, 0.0)
        );
    }

    #[test]
    fn compose_takes_the_first_lane_of_each_input_and_component_takes_one_lane() {
        assert_eq!(
            fold_value(
                FieldOp::Compose,
                &[scalar(1.0), scalar(2.0), scalar(3.0)],
                &[Param::int(3)]
            ),
            vec3(1.0, 2.0, 3.0)
        );
        assert_eq!(
            fold_value(
                FieldOp::Compose,
                &[scalar(1.0), scalar(2.0), scalar(3.0), scalar(4.0)],
                &[Param::int(4)]
            ),
            Some(FieldValue::vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)))
        );
        assert_eq!(
            fold_value(FieldOp::Component, &[vec3(1.0, 2.0, 3.0)], &[Param::int(2)]),
            scalar(3.0)
        );
    }

    #[test]
    fn a_const_is_its_own_value_and_a_bad_type_code_folds_to_the_zero_default() {
        let value = FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(
            fold_value(FieldOp::Const, &[], &value.const_params()),
            Some(value)
        );
        assert_eq!(
            fold_value(
                FieldOp::Const,
                &[],
                &[Param::int(9), Param::int(0), Param::int(0), Param::int(0), Param::int(0)]
            ),
            Some(FieldValue::ZERO),
            "a type code naming no type is rejected by validation, not by folding"
        );
    }

    #[test]
    fn the_spatial_samplers_fold_now_that_the_evaluator_defines_them() {
        let point = vec3(0.37, 0.91, -0.22);
        let words: Vec<Param> = seed_words(4242).iter().copied().map(Param::from_bits).collect();
        assert_eq!(
            fold_value(FieldOp::Noise, &[point], &words),
            Some(FieldValue::scalar(Scalar::new(
                value_noise(4242, Vec3::new(0.37, 0.91, -0.22)).get()
            )))
        );
        let fbm_words: Vec<Param> = (0..6).map(Param::int).collect();
        assert_eq!(
            fold_value(FieldOp::Fbm, &[point], &fbm_words)
                .map(|value| value.ty()),
            Some(FieldType::Scalar)
        );
    }

    #[test]
    fn the_transcendental_tier_folds_on_the_cpu_and_bakes_the_exact_value() {
        assert_eq!(fold(FieldOp::Sin, &[scalar(0.5)]), scalar(0.5_f32.sin()));
        assert_eq!(fold(FieldOp::Cos, &[scalar(0.5)]), scalar(0.5_f32.cos()));
        assert_eq!(fold(FieldOp::Exp, &[scalar(2.0)]), scalar(2.0_f32.exp()));
        assert_eq!(fold(FieldOp::Pow, &[scalar(2.0), scalar(8.0)]), scalar(256.0));
        // The documented rule folds too — a negative base is a value, not a
        // refusal, so the node collapses rather than surviving as a NaN source.
        assert_eq!(fold(FieldOp::Pow, &[scalar(-2.0), scalar(0.5)]), scalar(0.0));
        assert_eq!(
            fold(FieldOp::Sin, &[vec3(0.0, 1.0, 2.0)]),
            vec3(0.0, 1.0_f32.sin(), 2.0_f32.sin())
        );
    }

    #[test]
    fn an_exp_that_overflows_is_refused_exactly_as_any_other_non_finite_fold_is() {
        // `NonFiniteConstant` is the validation rule this protects: a folded
        // infinity would be minted as a `Const` the checker then rejects, so the
        // folder refuses first and the node survives to overflow identically
        // wherever it is evaluated.
        assert_eq!(fold(FieldOp::Exp, &[scalar(1.0e6)]), None);
    }

    #[test]
    fn the_operators_whose_value_is_not_a_function_of_the_node_never_fold() {
        let opaque = [
            FieldOp::Point,
            FieldOp::Uv,
            FieldOp::Normal,
            FieldOp::Time,
            FieldOp::Param,
            FieldOp::Transform,
        ];
        opaque.iter().for_each(|op| {
            assert_eq!(
                fold(*op, &[vec3(1.0, 2.0, 3.0)]),
                None,
                "{op:?} must not fold"
            );
        });
    }

    #[test]
    fn one_unknown_input_stops_the_whole_fold() {
        assert_eq!(fold(FieldOp::Add, &[scalar(1.0), None]), None);
    }

    #[test]
    fn a_fold_that_would_produce_a_non_finite_lane_is_refused() {
        let huge = scalar(f32::MAX);
        assert_eq!(fold(FieldOp::Mul, &[huge, huge]), None);
    }
}
