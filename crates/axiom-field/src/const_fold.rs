//! **Exact** constant folding of the arithmetic and shaping operators.
//!
//! One `const [fn; 23]` table, indexed by the operator code, that answers a
//! single question: *given the values of this node's inputs, what is its value?*
//! A node whose answer is `Some` is replaced by a `Const` during
//! canonicalisation.
//!
//! ## What is deliberately not folded
//!
//! * **`Point` / `Uv` / `Normal` / `Time`** — the context is supplied per
//!   evaluation, so they have no value at canonicalisation time.
//! * **`Param`** — its value lives in the parameter table. Folding it would move
//!   a *value* into *structure*, which is precisely what the split between
//!   [`crate::FieldGraph::serialize`] and [`crate::FieldGraph::digest`] exists to
//!   prevent: retuning a parameter would start changing the digest.
//! * **`Transform`** — its matrix lives in the parameter table too, for the same
//!   reason.
//! * **`Noise` / `Fbm`** — folding them requires the CPU evaluator to already be
//!   the semantic reference for what the GPU will compute. Until that exists,
//!   folding one here would silently fix a value the backend may disagree with.
//!
//! ## And no algebra
//!
//! There is **no** rewriting beyond exact folding: no `x*1 -> x`, no `x+0 -> x`,
//! no reassociation, no strength reduction. Every one of those can change the
//! last bit of an `f32`, and the CPU/GPU parity budget cannot absorb that. A fold
//! that would produce a non-finite lane is refused outright, so a degenerate
//! `Smoothstep` or a zero-length `Normalize` simply stays a node.

use axiom_recipe::Param;

use crate::field_op::{FieldOp, FIELD_OP_COUNT};
use crate::field_type::FieldType;
use crate::field_value::FieldValue;

/// The four lanes a fold works on. Lanes past the operating width are always
/// zero, which is what lets every folder run all four lanes unconditionally.
type Lanes = [f32; 4];

/// The value of one node's inputs, already broadened to a common width.
#[derive(Clone, Copy)]
struct FoldInput<'a> {
    /// One entry per input, in slot order, each masked to `widest`.
    lanes: &'a [Lanes],
    /// The node's raw parameter words.
    params: &'a [Param],
    /// The node's derived output type, from the type fold.
    out: FieldType,
}

impl FoldInput<'_> {
    /// Input slot `slot`'s lanes, or four zeroes when the node has no such slot.
    fn lane(self, slot: usize) -> Lanes {
        self.lanes.get(slot).copied().unwrap_or_default()
    }

    /// Parameter word `slot`, or `0`.
    fn word(self, slot: usize) -> u32 {
        self.params.get(slot).map_or(0, |param| param.bits())
    }
}

/// The value of the node `op` computes from `inputs`, or `None` when it cannot
/// be known at canonicalisation time.
///
/// `inputs` carries `None` for an input whose value is not constant, so an
/// operator folds only when **every** input folded. `out` is the type the type
/// checker derived for the node, which is what the result is stamped with.
pub(crate) fn fold_value(
    op: FieldOp,
    inputs: &[Option<FieldValue>],
    params: &[Param],
    out: FieldType,
) -> Option<FieldValue> {
    inputs
        .iter()
        .copied()
        .collect::<Option<Vec<FieldValue>>>()
        .and_then(|values| {
            let widest = values
                .iter()
                .fold(FieldType::Scalar, |widest, value| widest.max(value.ty()));
            let lanes: Vec<Lanes> = values
                .iter()
                .map(|value| broadcast(*value, widest))
                .collect();
            FOLDERS[op as usize](FoldInput {
                lanes: &lanes,
                params,
                out,
            })
        })
        .filter(|value| value.is_finite())
}

/// A value's lanes at `width`. A scalar replicates across every lane of the
/// width — the language's one implicit conversion — and every lane past the
/// width is zeroed so an operator that sums lanes (`Dot`, `Length`) can run all
/// four without knowing the width.
fn broadcast(value: FieldValue, width: FieldType) -> Lanes {
    let scalar = usize::from(value.ty() == FieldType::Scalar);
    let single = value.as_scalar().get();
    let vector = value.as_vec4();
    let source: Lanes = [
        [vector.x, vector.y, vector.z, vector.w],
        [single, single, single, single],
    ][scalar];
    let lanes = width.lanes();
    [0_u8, 1, 2, 3].map(|index| [0.0, source[index as usize]][usize::from(index < lanes)])
}

/// A folded value: the lanes stamped with the derived output type, which scrubs
/// every lane past that type's width back to the documented default.
fn value(out: FieldType, lanes: Lanes) -> FieldValue {
    FieldValue::from_words(
        out,
        [
            lanes[0].to_bits(),
            lanes[1].to_bits(),
            lanes[2].to_bits(),
            lanes[3].to_bits(),
        ],
    )
}

/// Apply a two-operand lane function across all four lanes. A bare `fn` pointer,
/// not a generic `F: Fn` bound — the Axiom State Law bans the latter, and a
/// function pointer captures no environment.
fn zip2(a: Lanes, b: Lanes, op: fn(f32, f32) -> f32) -> Lanes {
    [
        op(a[0], b[0]),
        op(a[1], b[1]),
        op(a[2], b[2]),
        op(a[3], b[3]),
    ]
}

/// The sum of the four lane products — the dot product, given that lanes past
/// the operating width are zero.
///
/// Written as plain multiply-then-add, never `mul_add`: a fused multiply-add
/// rounds once where the shader rounds twice, and `engine_no_unportable_float`
/// bans it for exactly that reason.
fn dot(a: Lanes, b: Lanes) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// One folder per [`FieldOp`], in discriminant order — the same `const` dispatch
/// table shape the signature table uses, so the operator code indexes it
/// directly and no `match` is involved.
type Folder = fn(FoldInput<'_>) -> Option<FieldValue>;

#[rustfmt::skip]
const FOLDERS: [Folder; FIELD_OP_COUNT] = [
    fold_const,                                     // Const
    never, never, never, never,                     // Point / Uv / Normal / Time
    never,                                          // Param
    fold_add, fold_sub, fold_mul, fold_min, fold_max, fold_abs,
    fold_clamp, fold_mix, fold_smoothstep,
    fold_dot, fold_length, fold_normalize,
    fold_compose, fold_component,
    never, never,                                   // Noise / Fbm — see the module docs
    never,                                          // Transform
];

/// An operator whose value canonicalisation cannot know.
fn never(_: FoldInput<'_>) -> Option<FieldValue> {
    None
}

/// A `Const` already *is* its value: word 0 is the declared type, words 1..5 the
/// lanes.
fn fold_const(cx: FoldInput<'_>) -> Option<FieldValue> {
    u16::try_from(cx.word(0))
        .ok()
        .and_then(FieldType::from_code)
        .map(|ty| FieldValue::from_words(ty, [cx.word(1), cx.word(2), cx.word(3), cx.word(4)]))
}

fn fold_add(cx: FoldInput<'_>) -> Option<FieldValue> {
    Some(value(cx.out, zip2(cx.lane(0), cx.lane(1), sum)))
}

fn fold_sub(cx: FoldInput<'_>) -> Option<FieldValue> {
    Some(value(cx.out, zip2(cx.lane(0), cx.lane(1), difference)))
}

fn fold_mul(cx: FoldInput<'_>) -> Option<FieldValue> {
    Some(value(cx.out, zip2(cx.lane(0), cx.lane(1), product)))
}

fn fold_min(cx: FoldInput<'_>) -> Option<FieldValue> {
    Some(value(cx.out, zip2(cx.lane(0), cx.lane(1), f32::min)))
}

fn fold_max(cx: FoldInput<'_>) -> Option<FieldValue> {
    Some(value(cx.out, zip2(cx.lane(0), cx.lane(1), f32::max)))
}

fn fold_abs(cx: FoldInput<'_>) -> Option<FieldValue> {
    let a = cx.lane(0);
    Some(value(
        cx.out,
        [a[0].abs(), a[1].abs(), a[2].abs(), a[3].abs()],
    ))
}

/// `clamp(value, lo, hi)`, spelled as `min(max(v, lo), hi)` so a `lo > hi` node
/// is total rather than a panic.
fn fold_clamp(cx: FoldInput<'_>) -> Option<FieldValue> {
    let lifted = zip2(cx.lane(0), cx.lane(1), f32::max);
    Some(value(cx.out, zip2(lifted, cx.lane(2), f32::min)))
}

/// `mix(a, b, t) = a + (b - a) * t` — the WGSL spelling, kept literally so the
/// folded value and the shader's value agree bit for bit.
fn fold_mix(cx: FoldInput<'_>) -> Option<FieldValue> {
    let (a, b, t) = (cx.lane(0), cx.lane(1), cx.lane(2));
    Some(value(
        cx.out,
        [0_usize, 1, 2, 3].map(|lane| a[lane] + (b[lane] - a[lane]) * t[lane]),
    ))
}

/// `smoothstep(edge0, edge1, x)`. Equal edges divide by zero; rather than let
/// `f32::max` quietly swallow the resulting NaN, the non-finite intermediate is
/// propagated so [`fold_value`] refuses the fold and the node survives.
fn fold_smoothstep(cx: FoldInput<'_>) -> Option<FieldValue> {
    let (edge0, edge1, x) = (cx.lane(0), cx.lane(1), cx.lane(2));
    Some(value(
        cx.out,
        [0_usize, 1, 2, 3].map(|lane| {
            let raw = (x[lane] - edge0[lane]) / (edge1[lane] - edge0[lane]);
            let t = raw.clamp(0.0, 1.0);
            [f32::NAN, t * t * (3.0 - 2.0 * t)][usize::from(raw.is_finite())]
        }),
    ))
}

fn fold_dot(cx: FoldInput<'_>) -> Option<FieldValue> {
    Some(value(
        cx.out,
        [dot(cx.lane(0), cx.lane(1)), 0.0, 0.0, 0.0],
    ))
}

fn fold_length(cx: FoldInput<'_>) -> Option<FieldValue> {
    let a = cx.lane(0);
    Some(value(cx.out, [dot(a, a).sqrt(), 0.0, 0.0, 0.0]))
}

/// A zero-length input divides by zero, so the fold is refused and the node
/// stays — exactly the behaviour a runtime evaluator has to have anyway.
fn fold_normalize(cx: FoldInput<'_>) -> Option<FieldValue> {
    let a = cx.lane(0);
    let length = dot(a, a).sqrt();
    Some(value(
        cx.out,
        [a[0] / length, a[1] / length, a[2] / length, a[3] / length],
    ))
}

/// `Compose` takes the first lane of each input, in slot order.
fn fold_compose(cx: FoldInput<'_>) -> Option<FieldValue> {
    Some(value(
        cx.out,
        [0_usize, 1, 2, 3].map(|slot| cx.lane(slot)[0]),
    ))
}

/// `Component` selects one lane, named by parameter word 0 and already proved to
/// be in range by the type checker.
fn fold_component(cx: FoldInput<'_>) -> Option<FieldValue> {
    let lane = (cx.word(0) as usize).min(3);
    Some(value(cx.out, [cx.lane(0)[lane], 0.0, 0.0, 0.0]))
}

fn sum(a: f32, b: f32) -> f32 {
    a + b
}

fn difference(a: f32, b: f32) -> f32 {
    a - b
}

fn product(a: f32, b: f32) -> f32 {
    a * b
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3, Vec4};
    use axiom_recipe::Scalar;

    fn scalar(value: f32) -> Option<FieldValue> {
        Some(FieldValue::scalar(Scalar::new(value)))
    }

    fn vec3(x: f32, y: f32, z: f32) -> Option<FieldValue> {
        Some(FieldValue::vec3(Vec3::new(x, y, z)))
    }

    fn fold(op: FieldOp, inputs: &[Option<FieldValue>], out: FieldType) -> Option<FieldValue> {
        fold_value(op, inputs, &[], out)
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
                fold(*op, &[scalar(4.0), scalar(3.0)], FieldType::Scalar),
                scalar(*expected),
                "{op:?} must fold exactly"
            );
        });
    }

    #[test]
    fn a_scalar_broadcasts_across_every_lane_of_the_vector_it_meets() {
        assert_eq!(
            fold(
                FieldOp::Mul,
                &[vec3(1.0, 2.0, 3.0), scalar(2.0)],
                FieldType::Vec3
            ),
            vec3(2.0, 4.0, 6.0)
        );
    }

    #[test]
    fn the_unary_and_ternary_shaping_operators_fold() {
        assert_eq!(
            fold(FieldOp::Abs, &[vec3(-1.0, 2.0, -3.0)], FieldType::Vec3),
            vec3(1.0, 2.0, 3.0)
        );
        assert_eq!(
            fold(
                FieldOp::Clamp,
                &[scalar(5.0), scalar(0.0), scalar(1.0)],
                FieldType::Scalar
            ),
            scalar(1.0)
        );
        assert_eq!(
            fold(
                FieldOp::Mix,
                &[scalar(0.0), scalar(10.0), scalar(0.25)],
                FieldType::Scalar
            ),
            scalar(2.5)
        );
        assert_eq!(
            fold(
                FieldOp::Smoothstep,
                &[scalar(0.0), scalar(2.0), scalar(1.0)],
                FieldType::Scalar
            ),
            scalar(0.5)
        );
    }

    #[test]
    fn smoothstep_with_equal_edges_is_left_as_a_node() {
        assert_eq!(
            fold(
                FieldOp::Smoothstep,
                &[scalar(1.0), scalar(1.0), scalar(1.0)],
                FieldType::Scalar
            ),
            None
        );
    }

    #[test]
    fn the_collapsing_operators_fold_to_a_scalar() {
        assert_eq!(
            fold(
                FieldOp::Dot,
                &[vec3(1.0, 2.0, 3.0), vec3(4.0, 5.0, 6.0)],
                FieldType::Scalar
            ),
            scalar(32.0)
        );
        assert_eq!(
            fold(FieldOp::Length, &[vec3(3.0, 4.0, 0.0)], FieldType::Scalar),
            scalar(5.0)
        );
        assert_eq!(
            fold(
                FieldOp::Length,
                &[Some(FieldValue::vec2(Vec2::new(3.0, 4.0)))],
                FieldType::Scalar
            ),
            scalar(5.0)
        );
    }

    #[test]
    fn normalize_folds_but_never_divides_by_zero() {
        assert_eq!(
            fold(FieldOp::Normalize, &[vec3(0.0, 5.0, 0.0)], FieldType::Vec3),
            vec3(0.0, 1.0, 0.0)
        );
        assert_eq!(
            fold(FieldOp::Normalize, &[vec3(0.0, 0.0, 0.0)], FieldType::Vec3),
            None
        );
    }

    #[test]
    fn compose_takes_the_first_lane_of_each_input_and_component_takes_one_lane() {
        assert_eq!(
            fold(
                FieldOp::Compose,
                &[scalar(1.0), scalar(2.0), scalar(3.0)],
                FieldType::Vec3
            ),
            vec3(1.0, 2.0, 3.0)
        );
        assert_eq!(
            fold_value(
                FieldOp::Compose,
                &[scalar(1.0), scalar(2.0), scalar(3.0), scalar(4.0)],
                &[Param::int(4)],
                FieldType::Vec4
            ),
            Some(FieldValue::vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)))
        );
        assert_eq!(
            fold_value(
                FieldOp::Component,
                &[vec3(1.0, 2.0, 3.0)],
                &[Param::int(2)],
                FieldType::Scalar
            ),
            scalar(3.0)
        );
    }

    #[test]
    fn a_const_is_its_own_value_and_a_bad_type_code_folds_to_nothing() {
        let value = FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(
            fold_value(FieldOp::Const, &[], &value.const_params(), FieldType::Vec3),
            Some(value)
        );
        assert_eq!(
            fold_value(
                FieldOp::Const,
                &[],
                &[Param::int(9), Param::int(0), Param::int(0), Param::int(0), Param::int(0)],
                FieldType::Scalar
            ),
            None
        );
    }

    #[test]
    fn the_operators_canonicalisation_cannot_know_never_fold() {
        let opaque = [
            FieldOp::Point,
            FieldOp::Uv,
            FieldOp::Normal,
            FieldOp::Time,
            FieldOp::Param,
            FieldOp::Noise,
            FieldOp::Fbm,
            FieldOp::Transform,
        ];
        opaque.iter().for_each(|op| {
            assert_eq!(
                fold(*op, &[vec3(1.0, 2.0, 3.0)], FieldType::Vec3),
                None,
                "{op:?} must not fold"
            );
        });
    }

    #[test]
    fn one_unknown_input_stops_the_whole_fold() {
        assert_eq!(
            fold(FieldOp::Add, &[scalar(1.0), None], FieldType::Scalar),
            None
        );
    }

    #[test]
    fn a_fold_that_would_produce_a_non_finite_lane_is_refused() {
        let huge = scalar(f32::MAX);
        assert_eq!(fold(FieldOp::Mul, &[huge, huge], FieldType::Scalar), None);
    }
}
