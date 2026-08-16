//! The five **vector** operators: the ones that change a value's width.
//!
//! | Operator | Semantics |
//! |---|---|
//! | `Dot(a, b)` | the scalar dot product over the inputs' common width |
//! | `Length(v)` | `sqrt(dot(v, v))`, a `Scalar` |
//! | `Normalize(v)` | `v * (1.0 / length(v))`, a `Vec3`; a length below the math layer's [`Epsilon::DEFAULT`] yields **`+Y`** |
//! | `Compose(width)` | a vector of `width` lanes, taking the first lane of each input in slot order |
//! | `Component(i)` | lane `i` of the input, as a `Scalar` |
//!
//! **`Normalize`'s evaluation order is fixed and written down.** It is
//! `v * (1.0 / len)` — one reciprocal, then three multiplies — and *not*
//! `v / len`, which is three divisions and a different last bit. It is the only
//! reciprocal in the algebra, which is what makes CPU-to-CPU evaluation
//! bit-exact everywhere.
//!
//! The `+Y` fallback for a degenerate length is not invented here: it is the
//! deterministic default `crates/axiom-mesh-ops/src/implicit_surface.rs` already
//! uses where a sampled field's gradient vanishes.

use axiom_math::Epsilon;

use crate::eval::{dot as dot_lanes, typed, FieldEvalStep};
use crate::field_type::FieldType;
use crate::field_value::FieldValue;

/// The lanes of `+Y`, the documented direction a degenerate `Normalize` yields.
const UNIT_Y: [f32; 4] = [0.0, 1.0, 0.0, 0.0];

/// `Dot(a, b)` — **the scalar dot product over the inputs' common width**; a
/// `Scalar` input broadcasts, and lanes past the common width are zero, so the
/// four-lane sum is the dot product at any width.
pub(crate) fn dot(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(
        FieldType::Scalar,
        [dot_lanes(step.lanes(0), step.lanes(1)), 0.0, 0.0, 0.0],
    )
}

/// `Length(v)` — **`sqrt(dot(v, v))`**, a `Scalar`. `sqrt` is IEEE-754 exact, so
/// this carries no portability budget of its own.
pub(crate) fn length(step: &FieldEvalStep<'_>) -> FieldValue {
    let v = step.lanes(0);
    typed(
        FieldType::Scalar,
        [dot_lanes(v, v).sqrt(), 0.0, 0.0, 0.0],
    )
}

/// `Normalize(v)` — **`v * (1.0 / length(v))`**, a `Vec3`. A length below
/// [`Epsilon::DEFAULT`] yields `+Y`, the engine's existing deterministic default
/// for a direction that cannot be recovered.
pub(crate) fn normalize(step: &FieldEvalStep<'_>) -> FieldValue {
    let v = step.lanes(0);
    let length = dot_lanes(v, v).sqrt();
    let inverse = 1.0 / length;
    let scaled = [v[0] * inverse, v[1] * inverse, v[2] * inverse, 0.0];
    let degenerate = usize::from(length < Epsilon::DEFAULT.value());
    typed(FieldType::Vec3, [scaled, UNIT_Y][degenerate])
}

/// `Compose(width)` — **a vector of `width` lanes assembled from the first lane
/// of each input, in slot order**. The width rides in parameter word 0 and the
/// input count is that width, both already proved by validation.
pub(crate) fn compose(step: &FieldEvalStep<'_>) -> FieldValue {
    typed(
        FieldType::of_width(step.word(0)),
        [0_usize, 1, 2, 3].map(|slot| step.input(slot).as_scalar().get()),
    )
}

/// `Component(i)` — **lane `i` of the input, as a `Scalar`**. The lane index
/// rides in parameter word 0 and is already proved to be within the input's
/// width; an index past the fourth lane reads the fourth, which is total rather
/// than a panic.
pub(crate) fn component(step: &FieldEvalStep<'_>) -> FieldValue {
    let lanes = step.input(0).as_vec4();
    let lane = (step.word(0) as usize).min(3);
    typed(
        FieldType::Scalar,
        [[lanes.x, lanes.y, lanes.z, lanes.w][lane], 0.0, 0.0, 0.0],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3, Vec4};
    use axiom_recipe::{Param, Scalar};

    use crate::eval_context::EvalContext;
    use crate::field_params::FieldParams;

    fn scalar(value: f32) -> FieldValue {
        FieldValue::scalar(Scalar::new(value))
    }

    fn vec3(x: f32, y: f32, z: f32) -> FieldValue {
        FieldValue::vec3(Vec3::new(x, y, z))
    }

    fn run(op: fn(&FieldEvalStep<'_>) -> FieldValue, inputs: &[FieldValue]) -> FieldValue {
        run_with(op, inputs, &[])
    }

    fn run_with(
        op: fn(&FieldEvalStep<'_>) -> FieldValue,
        inputs: &[FieldValue],
        words: &[u32],
    ) -> FieldValue {
        let params: Vec<Param> = words.iter().copied().map(Param::int).collect();
        let table = FieldParams::new();
        op(&FieldEvalStep::new(
            inputs,
            &params,
            &EvalContext::ORIGIN,
            &table,
        ))
    }

    #[test]
    fn dot_sums_the_lane_products_of_the_common_width() {
        assert_eq!(
            run(dot, &[vec3(1.0, 2.0, 3.0), vec3(4.0, 5.0, 6.0)]),
            scalar(32.0)
        );
        assert_eq!(
            run(
                dot,
                &[
                    FieldValue::vec2(Vec2::new(3.0, 4.0)),
                    FieldValue::vec2(Vec2::new(3.0, 4.0))
                ]
            ),
            scalar(25.0)
        );
        // A scalar broadcasts across the vector's width: 1+2+3 lanes of one.
        assert_eq!(run(dot, &[vec3(1.0, 2.0, 3.0), scalar(1.0)]), scalar(6.0));
        assert_eq!(run(dot, &[scalar(3.0), scalar(3.0)]), scalar(9.0));
    }

    #[test]
    fn length_is_the_square_root_of_the_self_dot() {
        assert_eq!(run(length, &[vec3(3.0, 4.0, 0.0)]), scalar(5.0));
        assert_eq!(
            run(length, &[FieldValue::vec2(Vec2::new(3.0, 4.0))]),
            scalar(5.0)
        );
        assert_eq!(run(length, &[scalar(-2.0)]), scalar(2.0));
        assert_eq!(run(length, &[vec3(0.0, 0.0, 0.0)]), scalar(0.0));
    }

    #[test]
    fn normalize_scales_by_the_reciprocal_of_the_length() {
        assert_eq!(run(normalize, &[vec3(0.0, 5.0, 0.0)]), vec3(0.0, 1.0, 0.0));
        let v = Vec3::new(1.0, 2.0, 3.0);
        let inverse = 1.0 / (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
        assert_eq!(
            run(normalize, &[FieldValue::vec3(v)]),
            vec3(v.x * inverse, v.y * inverse, v.z * inverse),
            "the order is v * (1/len), not v/len"
        );
    }

    #[test]
    fn normalizing_a_vector_shorter_than_epsilon_yields_positive_y() {
        assert_eq!(run(normalize, &[vec3(0.0, 0.0, 0.0)]), vec3(0.0, 1.0, 0.0));
        assert_eq!(run(normalize, &[scalar(0.0)]), vec3(0.0, 1.0, 0.0));
        let tiny = Epsilon::DEFAULT.value() * 0.5;
        assert_eq!(
            run(normalize, &[vec3(tiny, 0.0, 0.0)]),
            vec3(0.0, 1.0, 0.0)
        );
        // A vector comfortably longer than the epsilon is normalized, not
        // replaced: the fallback is a floor, not the common path.
        assert_ne!(
            run(normalize, &[vec3(1.0e-3, 0.0, 0.0)]),
            vec3(0.0, 1.0, 0.0)
        );
        assert_eq!(run(normalize, &[vec3(2.0, 0.0, 0.0)]), vec3(1.0, 0.0, 0.0));
    }

    #[test]
    fn compose_takes_the_first_lane_of_each_input_at_its_declared_width() {
        assert_eq!(
            run_with(compose, &[scalar(1.0), scalar(2.0)], &[2]),
            FieldValue::vec2(Vec2::new(1.0, 2.0))
        );
        assert_eq!(
            run_with(compose, &[scalar(1.0), scalar(2.0), scalar(3.0)], &[3]),
            vec3(1.0, 2.0, 3.0)
        );
        assert_eq!(
            run_with(
                compose,
                &[scalar(1.0), scalar(2.0), scalar(3.0), scalar(4.0)],
                &[4]
            ),
            FieldValue::vec4(Vec4::new(1.0, 2.0, 3.0, 4.0))
        );
        // Only the first lane of a wider input contributes.
        assert_eq!(
            run_with(compose, &[vec3(7.0, 8.0, 9.0), scalar(2.0)], &[2]),
            FieldValue::vec2(Vec2::new(7.0, 2.0))
        );
    }

    #[test]
    fn component_extracts_every_lane_of_every_width() {
        let vec4 = FieldValue::vec4(Vec4::new(1.0, 2.0, 3.0, 4.0));
        (0..4).for_each(|lane| {
            assert_eq!(
                run_with(component, &[vec4], &[lane]),
                scalar(1.0 + lane as f32)
            );
        });
        (0..3).for_each(|lane| {
            assert_eq!(
                run_with(component, &[vec3(1.0, 2.0, 3.0)], &[lane]),
                scalar(1.0 + lane as f32)
            );
        });
        (0..2).for_each(|lane| {
            assert_eq!(
                run_with(component, &[FieldValue::vec2(Vec2::new(1.0, 2.0))], &[lane]),
                scalar(1.0 + lane as f32)
            );
        });
        assert_eq!(run_with(component, &[scalar(5.0)], &[0]), scalar(5.0));
        // A lane index past the fourth reads the fourth, which is total.
        assert_eq!(run_with(component, &[vec4], &[99]), scalar(4.0));
    }
}
