//! The three **spatial** operators: the ones that read a point rather than a
//! number.
//!
//! | Operator | Semantics |
//! |---|---|
//! | `Noise(seed)` | `axiom_noise::value_noise(seed, point)`, a `Scalar` in `[-1, 1]` |
//! | `Fbm(seed, cfg…)` | `axiom_noise::Fbm::new(seed, cfg).sample(point)`, a `Scalar` in `[-1, 1]` |
//! | `Transform` | the `Mat4` whose four columns the parameter table holds, applied to the input as a **point** (`w = 1`) |
//!
//! `Noise` and `Fbm` are the algebra's only stochastic-looking operators and
//! neither is random: both are pure functions of `(seed, config, point)`, where
//! the seed and the config are parameter words of the node. Their word encoding
//! is [`crate::noise_words`], stated once so the authoring surface and the
//! evaluator cannot drift.
//!
//! The domain warp `axiom_noise::Fbm::sample_warped` offers is **not** reachable
//! from this operator: `FbmConfig` does not carry a warp strength, so the `Fbm`
//! row carries no word for one. A warped field is `Fbm` composed with an
//! explicit displacement in the graph, which is the same value and one the
//! backend can see.

use axiom_math::{Mat4, Vec4};
use axiom_noise::{value_noise, Fbm};
use axiom_recipe::Scalar;

use crate::eval::FieldEvalStep;
use crate::field_value::FieldValue;
use crate::ids::FieldParamSlot;
use crate::noise_words::{fbm_config, seed};

/// `Noise(seed)` — **single-octave coherent noise at the input point**, a
/// `Scalar` in `[-1, 1]`. The seed's two words are parameter words 0 and 1, low
/// half first.
pub(crate) fn noise(step: &FieldEvalStep<'_>) -> FieldValue {
    FieldValue::scalar(Scalar::new(
        value_noise(seed(step.words()), step.input(0).as_vec3()).get(),
    ))
}

/// `Fbm(seed, cfg…)` — **fractal Brownian motion at the input point**, a
/// `Scalar` in `[-1, 1]`. The seed's two words come first, then the four
/// `FbmConfig` knob words: `octaves`, `frequency`, `lacunarity`, `gain`.
pub(crate) fn fbm(step: &FieldEvalStep<'_>) -> FieldValue {
    let words = step.words();
    FieldValue::scalar(Scalar::new(
        Fbm::new(seed(words), fbm_config(words))
            .sample(step.input(0).as_vec3())
            .get(),
    ))
}

/// `Transform` — **the input through a `Mat4`, as a point (`w = 1`)**, a `Vec3`.
///
/// The matrix's four columns live in the parameter table, one `Vec4` slot per
/// column, named by parameter words 0..4 in column order. Keeping the matrix in
/// the table rather than in the node's words is what lets a field be re-posed
/// without moving its structural digest.
///
/// The application is the math layer's own [`Mat4::transform_point`] — `m *
/// vec4(p, 1)`, followed by the perspective divide that method defines when the
/// resulting `w` is neither `0` nor `1`. For the affine matrices a field carries
/// that divide is the identity, and reusing the layer's definition is what keeps
/// "a point through a matrix" meaning one thing engine-wide.
pub(crate) fn transform(step: &FieldEvalStep<'_>) -> FieldValue {
    FieldValue::vec3(matrix(step).transform_point(step.input(0).as_vec3()))
}

/// The matrix a `Transform` node's four parameter words name, column by column.
fn matrix(step: &FieldEvalStep<'_>) -> Mat4 {
    let columns = [0_usize, 1, 2, 3].map(|column| column_value(step, column));
    Mat4::from_cols_array([
        columns[0].x,
        columns[0].y,
        columns[0].z,
        columns[0].w,
        columns[1].x,
        columns[1].y,
        columns[1].z,
        columns[1].w,
        columns[2].x,
        columns[2].y,
        columns[2].z,
        columns[2].w,
        columns[3].x,
        columns[3].y,
        columns[3].z,
        columns[3].w,
    ])
}

/// One column of a `Transform`'s matrix: the parameter slot word `index` names,
/// read as four lanes. A slot the table does not have reads as the zero column —
/// the same [`FieldValue::ZERO`] fill the parameter table itself uses for a gap.
fn column_value(step: &FieldEvalStep<'_>, index: usize) -> Vec4 {
    u16::try_from(step.word(index))
        .ok()
        .map(FieldParamSlot::from_raw)
        .and_then(|slot| step.table().get(slot))
        .unwrap_or_default()
        .as_vec4()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Ratio;
    use axiom_math::Vec3;
    use axiom_noise::{FbmConfig, Frequency, Lacunarity};
    use axiom_recipe::Param;

    use crate::eval_context::EvalContext;
    use crate::field_params::FieldParams;
    use crate::field_type::FieldType;
    use crate::noise_words::{fbm_words, seed_words};

    fn point() -> FieldValue {
        FieldValue::vec3(Vec3::new(0.37, 0.91, -0.22))
    }

    fn run(
        op: fn(&FieldEvalStep<'_>) -> FieldValue,
        inputs: &[FieldValue],
        words: &[u32],
        table: &FieldParams,
    ) -> FieldValue {
        let params: Vec<Param> = words.iter().copied().map(Param::from_bits).collect();
        op(&FieldEvalStep::new(
            inputs,
            &params,
            &EvalContext::ORIGIN,
            table,
        ))
    }

    fn config() -> FbmConfig {
        FbmConfig {
            gain: Ratio::finite_or_zero(0.5),
            lacunarity: Lacunarity::DOUBLING,
            ..FbmConfig::new(4, Frequency::finite_or_zero(1.5))
        }
    }

    /// A parameter table holding the four columns of `matrix` in slots 0..4.
    fn columns(matrix: Mat4) -> FieldParams {
        let raw = matrix.as_cols_array();
        (0..4).fold(FieldParams::new(), |table, column| {
            table.with(
                FieldParamSlot::from_raw(column as u16),
                FieldValue::vec4(Vec4::new(
                    raw[column * 4],
                    raw[column * 4 + 1],
                    raw[column * 4 + 2],
                    raw[column * 4 + 3],
                )),
            )
        })
    }

    #[test]
    fn noise_is_the_noise_layers_value_noise_at_the_input_point() {
        let table = FieldParams::new();
        let value = run(noise, &[point()], &seed_words(4242), &table);
        assert_eq!(value.ty(), FieldType::Scalar);
        assert_eq!(
            value.as_scalar().get(),
            value_noise(4242, Vec3::new(0.37, 0.91, -0.22)).get()
        );
        // The seed is genuinely read: a different seed is a different field.
        assert_ne!(
            run(noise, &[point()], &seed_words(1), &table),
            run(noise, &[point()], &seed_words(2), &table)
        );
    }

    #[test]
    fn fbm_is_the_noise_layers_fbm_at_the_input_point() {
        let table = FieldParams::new();
        let words: Vec<u32> = seed_words(77)
            .iter()
            .copied()
            .chain(fbm_words(config()))
            .collect();
        let value = run(fbm, &[point()], &words, &table);
        assert_eq!(value.ty(), FieldType::Scalar);
        assert_eq!(
            value.as_scalar().get(),
            Fbm::new(77, config())
                .sample(Vec3::new(0.37, 0.91, -0.22))
                .get()
        );
        // The knobs are genuinely read: one octave is not four.
        let single: Vec<u32> = seed_words(77)
            .iter()
            .copied()
            .chain(fbm_words(FbmConfig {
                octaves: 1,
                ..config()
            }))
            .collect();
        assert_ne!(run(fbm, &[point()], &single, &table), value);
    }

    #[test]
    fn transform_puts_the_input_through_the_matrix_the_table_holds() {
        let matrix = Mat4::translation(Vec3::new(10.0, 20.0, 30.0));
        let table = columns(matrix);
        assert_eq!(
            run(transform, &[point()], &[0, 1, 2, 3], &table),
            FieldValue::vec3(matrix.transform_point(Vec3::new(0.37, 0.91, -0.22)))
        );
        assert_eq!(
            run(transform, &[point()], &[0, 1, 2, 3], &columns(Mat4::IDENTITY)),
            point()
        );
    }

    #[test]
    fn a_transform_column_naming_no_slot_reads_as_the_zero_column() {
        // No slots at all: every column is zero, so the matrix collapses the
        // point to the origin rather than panicking.
        assert_eq!(
            run(
                transform,
                &[point()],
                &[9, 9, 9, 9],
                &FieldParams::new()
            ),
            FieldValue::vec3(Vec3::ZERO)
        );
        assert_eq!(
            run(
                transform,
                &[point()],
                &[u32::MAX, u32::MAX, u32::MAX, u32::MAX],
                &columns(Mat4::IDENTITY)
            ),
            FieldValue::vec3(Vec3::ZERO)
        );
    }
}
