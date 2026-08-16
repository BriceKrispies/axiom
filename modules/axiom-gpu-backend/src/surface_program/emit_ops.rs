//! One WGSL emitter per field operator, in a `const` table indexed by the
//! operator code.
//!
//! This table is the deliberate mirror image of `axiom-field`'s `OPS` dispatch
//! table: same length, same order, same `fn`-pointer discipline, and the same
//! per-operator doc sentence. The two are meant to be read side by side, because
//! that adjacency is the only thing that makes a semantic drift between the CPU
//! evaluator and the emitted shader visible in review.
//!
//! ## Every SSA value is a `vec4<f32>`
//!
//! The CPU evaluator works on `Lanes = [f32; 4]` with every lane past a value's
//! width held at zero, and stamps the result with a type that scrubs the lanes
//! past *its* width. The emitter carries exactly that representation: one
//! `let n = vec4<f32>(…)` per node, lanes past the node's type always zero. That
//! is what lets the twenty-three emitters be one-liners with no type dispatch —
//! the type is a property the emitter *knows*, never one the shader carries.
//!
//! The invariant every emitter must preserve: **the lanes past the node's own
//! type are zero**. Each one below preserves it by construction (an operand
//! broadcast to the operating width already has them zero, and an operator that
//! narrows writes the zeroes out explicitly), which is why no generic masking
//! step exists.
//!
//! ## The two that are not one-liners
//!
//! `Noise` and `Fbm` call the fixed helpers in
//! [`crate::surface_program::wgsl_template::SURFACE_PRELUDE_WGSL`], passing their
//! seed and knob words as literals — the words are node parameters, so they are
//! known at emission and never ride a uniform.

use axiom_field::{FieldType, FieldValue};
use axiom_math::Epsilon;

use crate::surface_program::params::MAX_SURFACE_PARAMS;

/// A field operator's emitter: one node's step in, the right-hand side of its
/// `let` out. The peer of `axiom-field`'s `fn(&FieldEvalStep<'_>) -> FieldValue`.
pub(crate) type EmitFn = fn(&EmitStep<'_>) -> String;

/// The emission table. Its order mirrors `axiom_field::FieldOp` exactly, so the
/// operator code **is** the row index.
#[rustfmt::skip]
pub(crate) const EMIT: [EmitFn; 23] = [
    constant,
    point, uv, normal, time,
    parameter,
    add, subtract, multiply, minimum, maximum,
    absolute,
    clamp, mix, smoothstep,
    dot, length, normalize,
    compose, component,
    noise, fbm, transform,
];

/// The four lane selectors, indexed by lane number.
const LANE: [&str; 4] = ["x", "y", "z", "w"];

/// The WGSL spelling of `FieldValue::ZERO` read as three lanes.
const ZERO_VEC3: &str = "vec3<f32>(0.0, 0.0, 0.0)";

/// The WGSL spelling of `FieldValue::ZERO` read as four lanes.
const ZERO_VEC4: &str = "vec4<f32>(0.0, 0.0, 0.0, 0.0)";

/// One already-emitted node as an operand: the SSA name that holds it, and the
/// type whose lanes that name carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmitOperand {
    name: String,
    ty: FieldType,
}

impl EmitOperand {
    /// An operand from its SSA name and its derived type.
    pub(crate) fn new(name: String, ty: FieldType) -> EmitOperand {
        EmitOperand { name, ty }
    }

    /// The derived type whose lanes this operand's name carries.
    pub(crate) const fn ty(&self) -> FieldType {
        self.ty
    }
}

/// One node under the emitter's eye: the names of its already-emitted inputs,
/// its raw parameter words, its operating width, and where its channel's
/// parameters sit in the shared uniform region.
///
/// Borrow-only and allocation-free to build — the mirror of `FieldEvalStep`,
/// with the two runtime tables replaced by the two facts an emitter needs about
/// them at generation time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EmitStep<'a> {
    inputs: &'a [EmitOperand],
    words: &'a [u32],
    width: FieldType,
    param_base: u32,
    param_count: u32,
}

impl<'a> EmitStep<'a> {
    /// One step from its five parts.
    pub(crate) fn new(
        inputs: &'a [EmitOperand],
        words: &'a [u32],
        width: FieldType,
        param_base: u32,
        param_count: u32,
    ) -> EmitStep<'a> {
        EmitStep {
            inputs,
            words,
            width,
            param_base,
            param_count,
        }
    }

    /// Parameter word `slot`, or `0` when the node carries no such word.
    pub(crate) fn word(&self, slot: usize) -> u32 {
        self.words.get(slot).copied().unwrap_or_default()
    }

    /// Input slot `slot`'s lanes, broadcast to the node's operating width: a
    /// `Scalar` replicates across every lane of the width, and every lane past
    /// the width is zero. The emission peer of `eval::broadcast`.
    pub(crate) fn lanes(&self, slot: usize) -> String {
        let operand = self.inputs.get(slot);
        let width = usize::from(self.width.lanes());
        let sources = [0_usize, 1, 2, 3].map(|lane| {
            let present = operand.map_or_else(
                || String::from("0.0"),
                |operand| {
                    let scalar = usize::from(operand.ty == FieldType::Scalar);
                    [
                        format!("{}.{}", operand.name, LANE[lane]),
                        format!("{}.x", operand.name),
                    ][scalar]
                        .clone()
                },
            );
            [String::from("0.0"), present][usize::from(lane < width)].clone()
        });
        format!(
            "vec4<f32>({}, {}, {}, {})",
            sources[0], sources[1], sources[2], sources[3]
        )
    }

    /// Input slot `slot`'s lane `lane`, **unbroadcast** — the peer of
    /// `step.input(slot).as_scalar()` / `.as_vec4()[lane]`, which read the value
    /// as it stands rather than at the node's width. A slot the node does not
    /// have reads `0.0`, the first lane of `FieldValue::ZERO`.
    pub(crate) fn input_lane(&self, slot: usize, lane: usize) -> String {
        self.inputs.get(slot).map_or_else(
            || String::from("0.0"),
            |operand| format!("{}.{}", operand.name, LANE[lane.min(3)]),
        )
    }

    /// Input slot `slot`'s first three lanes, **unbroadcast** — the peer of
    /// `step.input(slot).as_vec3()`.
    pub(crate) fn input_vec3(&self, slot: usize) -> String {
        self.inputs.get(slot).map_or_else(
            || String::from(ZERO_VEC3),
            |operand| format!("{}.xyz", operand.name),
        )
    }

    /// The WGSL expression for the parameter-table slot named by word `slot`.
    ///
    /// Three ways it can fail to name a readable slot, and all three read as
    /// `FieldValue::ZERO`, exactly as the evaluator's `.and_then(get)
    /// .unwrap_or_default()` chain does: a word past `u16`, a slot past the
    /// graph's own table, and a slot past the shared region's fixed size. The
    /// third is the emitter's own — a shader cannot index outside a fixed-length
    /// array — and it can only be reached by a surface the capability gate has
    /// already rejected.
    pub(crate) fn param_slot(&self, slot: usize) -> String {
        let local = self.word(slot);
        let global = self.param_base.saturating_add(local);
        let readable = (local <= u32::from(u16::MAX))
            & (local < self.param_count)
            & (global < u32::from(MAX_SURFACE_PARAMS));
        [
            String::from(ZERO_VEC4),
            format!("params.slots[{global}u]"),
        ][usize::from(readable)]
            .clone()
    }

    /// The `vec2<u32>` a node's first two words spell as a `u64` seed, high half
    /// first — the order `axiom_hash_cell` folds them in.
    fn seed(&self) -> String {
        format!("vec2<u32>({}u, {}u)", self.word(1), self.word(0))
    }
}

/// Emit one node: select its emitter by code and run it.
///
/// An operator code outside the table names no operator. That is impossible on a
/// graph that type-checks — the signature table rejects it — so the guard yields
/// the WGSL spelling of the documented `FieldValue::ZERO` default, exactly as
/// `axiom-field`'s own dispatcher does, rather than inventing an error path the
/// emitter promises not to have.
pub(crate) fn emit_node(code: u16, step: &EmitStep<'_>) -> String {
    EMIT.get(code as usize)
        .map_or_else(|| String::from(ZERO_VEC4), |emit| emit(step))
}

/// A finite `f32` as a WGSL literal, or its exact bit pattern when it is not
/// finite.
///
/// Rust's `Debug` for `f32` prints the shortest decimal that round-trips through
/// `f32`, and it always carries a `.` or an exponent — both of which WGSL's
/// float-literal grammar requires. WGSL has **no** spelling for an infinity or a
/// NaN, so those go through `bitcast`, which is exact and reaches the same bits
/// the CPU evaluator would have read.
pub(crate) fn wgsl_float(value: f32) -> String {
    [
        format!("bitcast<f32>({}u)", value.to_bits()),
        format!("{value:?}"),
    ][usize::from(value.is_finite())]
    .clone()
}

/// `Const` — **the parameter words, typed by the declared `FieldType`**: word 0
/// is the type code and words 1..5 are the four lanes. A type code naming no
/// type reads as `FieldValue::ZERO`.
fn constant(step: &EmitStep<'_>) -> String {
    let lanes = u16::try_from(step.word(0))
        .ok()
        .and_then(FieldType::from_code)
        .map_or(1_u8, FieldType::lanes);
    let words = [0_usize, 1, 2, 3].map(|lane| {
        [0_u32, step.word(lane + 1)][usize::from((lane as u8) < lanes)]
    });
    format!(
        "vec4<f32>({}, {}, {}, {})",
        wgsl_float(f32::from_bits(words[0])),
        wgsl_float(f32::from_bits(words[1])),
        wgsl_float(f32::from_bits(words[2])),
        wgsl_float(f32::from_bits(words[3]))
    )
}

/// `Point` — **the object-space sample position**, a `Vec3`.
fn point(_step: &EmitStep<'_>) -> String {
    String::from("vec4<f32>(in.object_pos, 0.0)")
}

/// `Uv` — **the interpolated surface parameterisation**, a `Vec2`.
fn uv(_step: &EmitStep<'_>) -> String {
    String::from("vec4<f32>(in.uv, 0.0, 0.0)")
}

/// `Normal` — **the object-space surface normal**, a `Vec3`.
fn normal(_step: &EmitStep<'_>) -> String {
    String::from("vec4<f32>(in.object_normal, 0.0)")
}

/// `Time` — **the presentation time**, a `Scalar`.
fn time(_step: &EmitStep<'_>) -> String {
    String::from("vec4<f32>(in.time, 0.0, 0.0, 0.0)")
}

/// `Param` — **the value the shared uniform region holds in the slot word 0
/// names**, at this channel's offset into that region.
fn parameter(step: &EmitStep<'_>) -> String {
    step.param_slot(0)
}

/// `Add(a, b)` — **component-wise `a + b`**; a `Scalar` input broadcasts.
fn add(step: &EmitStep<'_>) -> String {
    format!("({}) + ({})", step.lanes(0), step.lanes(1))
}

/// `Sub(a, b)` — **component-wise `a - b`**; a `Scalar` input broadcasts.
fn subtract(step: &EmitStep<'_>) -> String {
    format!("({}) - ({})", step.lanes(0), step.lanes(1))
}

/// `Mul(a, b)` — **component-wise `a * b`**; a `Scalar` input broadcasts.
fn multiply(step: &EmitStep<'_>) -> String {
    format!("({}) * ({})", step.lanes(0), step.lanes(1))
}

/// `Min(a, b)` — **component-wise minimum**; a `Scalar` input broadcasts.
fn minimum(step: &EmitStep<'_>) -> String {
    format!("min({}, {})", step.lanes(0), step.lanes(1))
}

/// `Max(a, b)` — **component-wise maximum**; a `Scalar` input broadcasts.
fn maximum(step: &EmitStep<'_>) -> String {
    format!("max({}, {})", step.lanes(0), step.lanes(1))
}

/// `Abs(a)` — **component-wise absolute value**.
fn absolute(step: &EmitStep<'_>) -> String {
    format!("abs({})", step.lanes(0))
}

/// `Clamp(x, lo, hi)` — **component-wise `max(min(x, hi), lo)`**, in that order:
/// a node with `lo > hi` yields `lo`, which is the documented degenerate rule and
/// the opposite of what the other spelling gives.
fn clamp(step: &EmitStep<'_>) -> String {
    format!(
        "max(min({}, {}), {})",
        step.lanes(0),
        step.lanes(2),
        step.lanes(1)
    )
}

/// `Mix(a, b, t)` — **component-wise `a + (b - a) * t`**, `t` unclamped. Written
/// out rather than handed to the `mix` builtin, whose factoring is unspecified.
fn mix(step: &EmitStep<'_>) -> String {
    let (a, b, t) = (step.lanes(0), step.lanes(1), step.lanes(2));
    format!("({a}) + ((({b}) - ({a})) * ({t}))")
}

/// `Smoothstep(e0, e1, x)` — **component-wise
/// `t = clamp((x - e0) / (e1 - e0), 0, 1); t * t * (3 - 2t)`**, with a lane whose
/// edges are equal yielding `0` rather than whatever the division's NaN would
/// produce. Written out rather than handed to the `smoothstep` builtin, whose
/// degenerate-edge behaviour is not the documented rule.
fn smoothstep(step: &EmitStep<'_>) -> String {
    let (edge0, edge1, x) = (step.lanes(0), step.lanes(1), step.lanes(2));
    format!(
        "select(\
         (clamp((({x}) - ({edge0})) / (({edge1}) - ({edge0})), vec4<f32>(0.0), vec4<f32>(1.0)) \
         * clamp((({x}) - ({edge0})) / (({edge1}) - ({edge0})), vec4<f32>(0.0), vec4<f32>(1.0)) \
         * (vec4<f32>(3.0) - 2.0 \
         * clamp((({x}) - ({edge0})) / (({edge1}) - ({edge0})), vec4<f32>(0.0), vec4<f32>(1.0)))), \
         vec4<f32>(0.0), ({edge0}) == ({edge1}))"
    )
}

/// `Dot(a, b)` — **the four-lane sum of products**, a `Scalar`. Written out
/// rather than handed to the `dot` builtin, whose summation order is
/// unspecified; lanes past the common width are zero, so the four-lane sum is
/// the dot product at any width.
fn dot(step: &EmitStep<'_>) -> String {
    let (a, b) = (step.lanes(0), step.lanes(1));
    format!("vec4<f32>({}, 0.0, 0.0, 0.0)", lane_sum(&a, &b))
}

/// `Length(v)` — **`sqrt(dot(v, v))`**, a `Scalar`.
fn length(step: &EmitStep<'_>) -> String {
    let v = step.lanes(0);
    format!("vec4<f32>(sqrt({}), 0.0, 0.0, 0.0)", lane_sum(&v, &v))
}

/// `Normalize(v)` — **`v * (1.0 / length(v))`**, a `Vec3`; a length below the
/// math layer's default epsilon yields `+Y`.
///
/// The reciprocal is spelled out. `inverseSqrt` is a lower-precision GPU
/// builtin, and the CPU evaluator's order is one reciprocal then three
/// multiplies — matching it is worth the extra divide.
fn normalize(step: &EmitStep<'_>) -> String {
    let v = step.lanes(0);
    let epsilon = wgsl_float(Epsilon::DEFAULT.value());
    format!(
        "select(\
         vec4<f32>(({v}).xyz * (1.0 / sqrt({sum})), 0.0), \
         vec4<f32>(0.0, 1.0, 0.0, 0.0), \
         sqrt({sum}) < {epsilon})",
        sum = lane_sum(&v, &v)
    )
}

/// `Compose(width)` — **a vector of `width` lanes assembled from the first lane
/// of each input, in slot order**; lanes past the declared width are zero.
fn compose(step: &EmitStep<'_>) -> String {
    let width = (step.word(0) as usize).min(4);
    let sources = [0_usize, 1, 2, 3].map(|slot| {
        [String::from("0.0"), step.input_lane(slot, 0)][usize::from(slot < width)].clone()
    });
    format!(
        "vec4<f32>({}, {}, {}, {})",
        sources[0], sources[1], sources[2], sources[3]
    )
}

/// `Component(i)` — **lane `i` of the input**, a `Scalar`. An index past the
/// fourth lane reads the fourth, which is total rather than a fault.
fn component(step: &EmitStep<'_>) -> String {
    format!(
        "vec4<f32>({}, 0.0, 0.0, 0.0)",
        step.input_lane(0, step.word(0) as usize)
    )
}

/// `Noise(seed)` — **single-octave coherent noise at the input point**, a
/// `Scalar` in `[-1, 1]`. The seed's two words are node parameters, so it is a
/// literal in the emitted call rather than a uniform.
fn noise(step: &EmitStep<'_>) -> String {
    format!(
        "vec4<f32>(axiom_noise({}, {}), 0.0, 0.0, 0.0)",
        step.seed(),
        step.input_vec3(0)
    )
}

/// `Fbm(seed, cfg…)` — **fractal Brownian motion at the input point**, a
/// `Scalar` in `[-1, 1]`.
///
/// The four knob words decode exactly as `axiom-field`'s `noise_words::fbm_config`
/// decodes them: a non-finite frequency or gain reads as `0.0`, and a non-finite
/// lacunarity reads as the canonical octave-doubling `2.0`. That decode is
/// mirrored here rather than shared, because this module may not name
/// `axiom-noise`; the mirror is pinned by the CPU/GPU parity sweep, which drives
/// hostile knob words through both sides.
fn fbm(step: &EmitStep<'_>) -> String {
    let frequency = finite_or(f32::from_bits(step.word(3)), 0.0);
    let lacunarity = finite_or(f32::from_bits(step.word(4)), 2.0);
    let gain = finite_or(f32::from_bits(step.word(5)), 0.0);
    format!(
        "vec4<f32>(axiom_fbm({}, {}u, {}, {}, {}, {}), 0.0, 0.0, 0.0)",
        step.seed(),
        step.word(2),
        wgsl_float(frequency),
        wgsl_float(lacunarity),
        wgsl_float(gain),
        step.input_vec3(0)
    )
}

/// `Transform` — **the input point through the `Mat4` whose four columns the
/// parameter slots named by words 0..4 hold**, a `Vec3`.
///
/// The application is `axiom_math::Mat4::transform_point`: the column-weighted
/// sum in that method's exact order, then the perspective divide it performs when
/// the resulting `w` is neither `0` nor `1`.
fn transform(step: &EmitStep<'_>) -> String {
    let columns = [0_usize, 1, 2, 3].map(|column| step.param_slot(column));
    let p = step.input_vec3(0);
    format!(
        "vec4<f32>((({c0}) * ({p}).x + ({c1}) * ({p}).y + ({c2}) * ({p}).z + ({c3})).xyz \
         / select(1.0, (({c0}) * ({p}).x + ({c1}) * ({p}).y + ({c2}) * ({p}).z + ({c3})).w, \
         ((({c0}) * ({p}).x + ({c1}) * ({p}).y + ({c2}) * ({p}).z + ({c3})).w != 0.0) \
         & ((({c0}) * ({p}).x + ({c1}) * ({p}).y + ({c2}) * ({p}).z + ({c3})).w != 1.0)), 0.0)",
        c0 = columns[0],
        c1 = columns[1],
        c2 = columns[2],
        c3 = columns[3]
    )
}

/// The four-lane sum of products of two `vec4` expressions, left-associated —
/// `eval::dot`'s exact shape, written as plain multiply-then-add so no lane pair
/// is folded into a single-rounding `fma`.
fn lane_sum(a: &str, b: &str) -> String {
    format!(
        "({a}).x * ({b}).x + ({a}).y * ({b}).y + ({a}).z * ({b}).z + ({a}).w * ({b}).w"
    )
}

/// `value` when it is finite, `fallback` otherwise — the shape every one of the
/// kernel's and the noise layer's `finite_or_*` constructors has.
fn finite_or(value: f32, fallback: f32) -> f32 {
    [fallback, value][usize::from(value.is_finite())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::FieldOp;
    use axiom_math::{Vec3, Vec4};
    use axiom_recipe::Scalar;

    fn operand(name: &str, ty: FieldType) -> EmitOperand {
        EmitOperand::new(String::from(name), ty)
    }

    /// Run the emitter for `op` over `inputs` and `words` at `width`.
    fn run(op: FieldOp, inputs: &[EmitOperand], words: &[u32], width: FieldType) -> String {
        EMIT[op.code() as usize](&EmitStep::new(inputs, words, width, 0, 8))
    }

    /// The common case: one or more `Vec3` operands at `Vec3` width.
    fn vec3s(count: usize) -> Vec<EmitOperand> {
        (0..count)
            .map(|index| operand(&format!("n{index}"), FieldType::Vec3))
            .collect()
    }

    #[test]
    fn the_table_has_one_row_per_operator_and_the_code_is_its_index() {
        assert_eq!(EMIT.len(), axiom_field::FIELD_OP_COUNT);
        // Every row is reachable through its own code and produces a `vec4`
        // expression — the register type every SSA value has.
        FieldOp::ALL.iter().for_each(|op| {
            let emitted = run(*op, &vec3s(4), &[2, 0, 0, 0, 0, 0], FieldType::Vec3);
            assert!(!emitted.is_empty(), "{op:?} emitted nothing");
        });
    }

    #[test]
    fn an_unknown_operator_code_emits_the_zero_default() {
        let step = EmitStep::new(&[], &[], FieldType::Scalar, 0, 0);
        assert_eq!(emit_node(23, &step), ZERO_VEC4, "code 23 names no operator");
        assert_eq!(emit_node(u16::MAX, &step), ZERO_VEC4);
        // Every real code dispatches to its own row.
        FieldOp::ALL.iter().for_each(|op| {
            assert_eq!(
                emit_node(op.code(), &step),
                EMIT[op.code() as usize](&step),
                "{op:?} must dispatch to its own row"
            );
        });
    }

    #[test]
    fn const_reads_its_type_code_and_lane_words_and_zeroes_the_rest() {
        let value = FieldValue::vec3(Vec3::new(1.5, -2.5, 3.5));
        let words = [
            u32::from(FieldType::Vec3.code()),
            1.5_f32.to_bits(),
            (-2.5_f32).to_bits(),
            3.5_f32.to_bits(),
            9.0_f32.to_bits(),
        ];
        assert_eq!(
            run(FieldOp::Const, &[], &words, FieldType::Scalar),
            "vec4<f32>(1.5, -2.5, 3.5, 0.0)",
            "the fourth word is past a Vec3's width and must be scrubbed"
        );
        // The same lanes the value itself reports, so the literal and the value
        // cannot disagree about which lanes a type carries.
        let lanes = value.as_vec4();
        assert_eq!(
            [lanes.x, lanes.y, lanes.z, lanes.w],
            [1.5, -2.5, 3.5, 0.0]
        );
    }

    #[test]
    fn const_with_a_type_code_naming_no_type_reads_as_one_lane_of_zero() {
        // The evaluator's `FieldValue::ZERO` fallback is a Scalar zero; the
        // emitter reaches the same lanes by falling back to a one-lane width.
        assert_eq!(
            run(FieldOp::Const, &[], &[9, 0, 0, 0, 0], FieldType::Scalar),
            "vec4<f32>(0.0, 0.0, 0.0, 0.0)"
        );
        assert_eq!(
            run(FieldOp::Const, &[], &[u32::MAX, 0, 0, 0, 0], FieldType::Scalar),
            "vec4<f32>(0.0, 0.0, 0.0, 0.0)"
        );
    }

    #[test]
    fn point_reads_the_object_space_position() {
        assert_eq!(
            run(FieldOp::Point, &[], &[], FieldType::Scalar),
            "vec4<f32>(in.object_pos, 0.0)"
        );
    }

    #[test]
    fn uv_reads_the_interpolated_parameterisation() {
        assert_eq!(
            run(FieldOp::Uv, &[], &[], FieldType::Scalar),
            "vec4<f32>(in.uv, 0.0, 0.0)"
        );
    }

    #[test]
    fn normal_reads_the_object_space_normal() {
        assert_eq!(
            run(FieldOp::Normal, &[], &[], FieldType::Scalar),
            "vec4<f32>(in.object_normal, 0.0)"
        );
    }

    #[test]
    fn time_reads_the_presentation_time() {
        assert_eq!(
            run(FieldOp::Time, &[], &[], FieldType::Scalar),
            "vec4<f32>(in.time, 0.0, 0.0, 0.0)"
        );
    }

    #[test]
    fn param_reads_its_slot_at_the_channels_offset_into_the_shared_region() {
        let step = EmitStep::new(&[], &[3], FieldType::Scalar, 5, 8);
        assert_eq!(parameter(&step), "params.slots[8u]");
        // A slot past the graph's own table, a word past `u16`, and a slot past
        // the shared region all read as the zero value.
        assert_eq!(
            parameter(&EmitStep::new(&[], &[9], FieldType::Scalar, 0, 8)),
            ZERO_VEC4
        );
        assert_eq!(
            parameter(&EmitStep::new(&[], &[u32::MAX], FieldType::Scalar, 0, u32::MAX)),
            ZERO_VEC4
        );
        assert_eq!(
            parameter(&EmitStep::new(&[], &[1], FieldType::Scalar, 31, 8)),
            ZERO_VEC4
        );
    }

    #[test]
    fn add_is_component_wise_over_the_broadcast_lanes() {
        assert_eq!(
            run(FieldOp::Add, &vec3s(2), &[], FieldType::Vec3),
            "(vec4<f32>(n0.x, n0.y, n0.z, 0.0)) + (vec4<f32>(n1.x, n1.y, n1.z, 0.0))"
        );
    }

    #[test]
    fn sub_keeps_its_operand_order() {
        assert_eq!(
            run(FieldOp::Sub, &vec3s(2), &[], FieldType::Vec3),
            "(vec4<f32>(n0.x, n0.y, n0.z, 0.0)) - (vec4<f32>(n1.x, n1.y, n1.z, 0.0))"
        );
    }

    #[test]
    fn mul_is_component_wise() {
        assert_eq!(
            run(FieldOp::Mul, &vec3s(2), &[], FieldType::Vec3),
            "(vec4<f32>(n0.x, n0.y, n0.z, 0.0)) * (vec4<f32>(n1.x, n1.y, n1.z, 0.0))"
        );
    }

    #[test]
    fn min_and_max_use_the_component_wise_builtins() {
        assert!(run(FieldOp::Min, &vec3s(2), &[], FieldType::Vec3).starts_with("min("));
        assert!(run(FieldOp::Max, &vec3s(2), &[], FieldType::Vec3).starts_with("max("));
    }

    #[test]
    fn abs_is_component_wise() {
        assert_eq!(
            run(FieldOp::Abs, &vec3s(1), &[], FieldType::Vec3),
            "abs(vec4<f32>(n0.x, n0.y, n0.z, 0.0))"
        );
    }

    #[test]
    fn clamp_is_max_of_min_so_a_low_above_its_high_yields_the_low() {
        let emitted = run(FieldOp::Clamp, &vec3s(3), &[], FieldType::Vec3);
        assert!(emitted.starts_with("max(min("), "the order is max(min(x, hi), lo)");
        // The high (slot 2) is the inner operand and the low (slot 1) the outer.
        let inner = emitted.find("n2").expect("the high edge is read");
        let outer = emitted.find("n1").expect("the low edge is read");
        assert!(inner < outer);
    }

    #[test]
    fn mix_is_a_plus_b_minus_a_times_t_not_the_builtin() {
        let emitted = run(FieldOp::Mix, &vec3s(3), &[], FieldType::Vec3);
        assert!(!emitted.contains("mix("), "the builtin's factoring is unspecified");
        assert!(emitted.contains(") - ("));
        assert!(emitted.contains(") + ("));
    }

    #[test]
    fn smoothstep_collapses_a_lane_whose_edges_are_equal() {
        let emitted = run(FieldOp::Smoothstep, &vec3s(3), &[], FieldType::Vec3);
        assert!(!emitted.contains("smoothstep("));
        assert!(emitted.starts_with("select("));
        assert!(emitted.contains("vec4<f32>(3.0) - 2.0"));
    }

    #[test]
    fn dot_sums_four_lane_products_rather_than_calling_the_builtin() {
        let emitted = run(FieldOp::Dot, &vec3s(2), &[], FieldType::Vec3);
        assert!(!emitted.contains("dot("), "the builtin's summation order is unspecified");
        assert!(emitted.starts_with("vec4<f32>("));
        assert!(emitted.ends_with(", 0.0, 0.0, 0.0)"));
    }

    #[test]
    fn length_is_the_square_root_of_the_self_dot() {
        let emitted = run(FieldOp::Length, &vec3s(1), &[], FieldType::Vec3);
        assert!(emitted.starts_with("vec4<f32>(sqrt("));
        assert!(emitted.ends_with(", 0.0, 0.0, 0.0)"));
    }

    #[test]
    fn normalize_scales_by_an_explicit_reciprocal_and_floors_at_the_epsilon() {
        let emitted = run(FieldOp::Normalize, &vec3s(1), &[], FieldType::Vec3);
        assert!(
            !emitted.contains("inverseSqrt"),
            "the GPU builtin is lower precision than 1.0 / sqrt(x)"
        );
        assert!(emitted.contains("1.0 / sqrt("));
        assert!(emitted.contains("vec4<f32>(0.0, 1.0, 0.0, 0.0)"));
        assert!(emitted.contains(&wgsl_float(Epsilon::DEFAULT.value())));
    }

    #[test]
    fn compose_takes_the_first_lane_of_each_input_up_to_its_width() {
        assert_eq!(
            run(FieldOp::Compose, &vec3s(4), &[2], FieldType::Vec3),
            "vec4<f32>(n0.x, n1.x, 0.0, 0.0)"
        );
        assert_eq!(
            run(FieldOp::Compose, &vec3s(4), &[4], FieldType::Vec3),
            "vec4<f32>(n0.x, n1.x, n2.x, n3.x)"
        );
        // A width past four cannot address a fifth lane.
        assert_eq!(
            run(FieldOp::Compose, &vec3s(4), &[9], FieldType::Vec3),
            "vec4<f32>(n0.x, n1.x, n2.x, n3.x)"
        );
    }

    #[test]
    fn component_extracts_the_lane_word_zero_names() {
        (0..4).for_each(|lane| {
            assert_eq!(
                run(FieldOp::Component, &vec3s(1), &[lane as u32], FieldType::Vec3),
                format!("vec4<f32>(n0.{}, 0.0, 0.0, 0.0)", LANE[lane])
            );
        });
        // A lane index past the fourth reads the fourth.
        assert_eq!(
            run(FieldOp::Component, &vec3s(1), &[99], FieldType::Vec3),
            "vec4<f32>(n0.w, 0.0, 0.0, 0.0)"
        );
    }

    #[test]
    fn noise_passes_its_seed_words_as_a_literal_high_half_first() {
        assert_eq!(
            run(FieldOp::Noise, &vec3s(1), &[7, 3], FieldType::Vec3),
            "vec4<f32>(axiom_noise(vec2<u32>(3u, 7u), n0.xyz), 0.0, 0.0, 0.0)"
        );
    }

    #[test]
    fn fbm_passes_its_decoded_knobs_as_literals() {
        let words = [
            7,
            3,
            5,
            1.5_f32.to_bits(),
            2.25_f32.to_bits(),
            0.375_f32.to_bits(),
        ];
        assert_eq!(
            run(FieldOp::Fbm, &vec3s(1), &words, FieldType::Vec3),
            "vec4<f32>(axiom_fbm(vec2<u32>(3u, 7u), 5u, 1.5, 2.25, 0.375, n0.xyz), 0.0, 0.0, 0.0)"
        );
    }

    #[test]
    fn a_non_finite_fbm_knob_decodes_to_its_documented_fallback() {
        let words = [
            0,
            0,
            1,
            f32::NAN.to_bits(),
            f32::INFINITY.to_bits(),
            f32::NEG_INFINITY.to_bits(),
        ];
        let emitted = run(FieldOp::Fbm, &vec3s(1), &words, FieldType::Vec3);
        assert!(
            emitted.contains("1u, 0.0, 2.0, 0.0,"),
            "a non-finite frequency and gain read as zero and a lacunarity as 2.0: {emitted}"
        );
        assert_eq!(finite_or(1.5, 9.0), 1.5);
    }

    #[test]
    fn transform_applies_the_column_sum_then_the_perspective_divide() {
        let emitted = run(FieldOp::Transform, &vec3s(1), &[0, 1, 2, 3], FieldType::Vec3);
        assert!(emitted.contains("params.slots[0u]"));
        assert!(emitted.contains("params.slots[3u]"));
        assert!(emitted.contains("select(1.0,"), "an affine w never divides");
        assert!(emitted.contains("!= 1.0)"));
    }

    #[test]
    fn a_scalar_operand_broadcasts_across_the_operating_width() {
        let inputs = [
            operand("n0", FieldType::Vec3),
            operand("n1", FieldType::Scalar),
        ];
        assert_eq!(
            run(FieldOp::Add, &inputs, &[], FieldType::Vec3),
            "(vec4<f32>(n0.x, n0.y, n0.z, 0.0)) + (vec4<f32>(n1.x, n1.x, n1.x, 0.0))"
        );
    }

    #[test]
    fn an_operand_slot_the_node_does_not_have_reads_as_the_zero_value() {
        let step = EmitStep::new(&[], &[], FieldType::Vec2, 0, 0);
        assert_eq!(step.lanes(0), "vec4<f32>(0.0, 0.0, 0.0, 0.0)");
        assert_eq!(step.input_lane(0, 2), "0.0");
        assert_eq!(step.input_vec3(0), ZERO_VEC3);
        assert_eq!(step.word(9), 0);
        assert!(format!("{step:?}").contains("EmitStep"));
    }

    #[test]
    fn an_operand_is_named_and_typed_and_compares_by_both() {
        let one = operand("n0", FieldType::Vec3);
        assert_eq!(one, operand("n0", FieldType::Vec3));
        assert_eq!(one.ty(), FieldType::Vec3);
        assert_ne!(one, operand("n0", FieldType::Vec4));
        assert!(format!("{one:?}").contains("EmitOperand"));
    }

    #[test]
    fn a_float_literal_round_trips_and_a_non_finite_one_goes_through_its_bits() {
        [0.0_f32, -0.0, 1.0, 0.1, -2.5, 1.0e-6, 3.4e38]
            .iter()
            .for_each(|value| {
                let text = wgsl_float(*value);
                assert!(
                    text.contains('.') | text.contains('e'),
                    "a WGSL float literal needs a point or an exponent: {text}"
                );
                assert_eq!(
                    text.parse::<f32>().expect("the literal must parse back"),
                    *value
                );
            });
        assert_eq!(
            wgsl_float(f32::INFINITY),
            format!("bitcast<f32>({}u)", f32::INFINITY.to_bits())
        );
        assert_eq!(
            wgsl_float(f32::NAN),
            format!("bitcast<f32>({}u)", f32::NAN.to_bits())
        );
        assert_eq!(wgsl_float(0.25), "0.25");
    }
}
