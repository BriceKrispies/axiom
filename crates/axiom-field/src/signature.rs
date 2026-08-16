//! The per-operator signature table — the whole implementation of the algebra's
//! shape.
//!
//! One `const` row per [`FieldOp`], in discriminant order. The row says how many
//! inputs and how many raw parameter words the operator carries, and by which
//! rule its output type is derived. This file lands the table and the accessor;
//! [`crate::type_check`] is the single forward fold that reads it.

use core::mem::size_of;

use axiom_math::{Mat4, Vec4};

use crate::field_op::{FieldOp, FIELD_OP_COUNT};
use crate::field_type::FieldType;
use crate::noise_words::{FBM_KNOB_WORDS, SEED_WORDS};

/// The two words of the `u64` seed a spatial operator carries.
const SEED_PARAMS: u8 = SEED_WORDS as u8;

/// The knob words an `Fbm` node carries: `octaves`, `frequency`, `lacunarity`,
/// `gain`, one 32-bit word each.
///
/// The count comes from [`crate::noise_words`], where it is pinned by an
/// encoder that destructures an [`axiom_noise::FbmConfig`] **exhaustively** — so
/// adding a knob to the config fails to compile there. It deliberately no longer
/// comes from `size_of::<FbmConfig>() / size_of::<u32>()`: a memory-layout
/// quotient is not a parameter count, and it would keep happening to equal
/// *something* after a knob was added, removed or repadded, silently changing
/// this operator's arity and every graph's bytes.
const FBM_KNOB_PARAMS: u8 = FBM_KNOB_WORDS as u8;

/// The parameter slots a `Transform` node carries: one [`axiom_math::Vec4`]
/// column per column of the [`axiom_math::Mat4`] it applies, derived from the
/// matrix itself.
const TRANSFORM_PARAMS: u8 = (size_of::<Mat4>() / size_of::<Vec4>()) as u8;

/// How an operator's output type is derived from its inputs and parameters.
///
/// A **fieldless** enum: the concrete type a `Fixed`/`ScalarOut`/`Vec3Out` row
/// yields rides in [`FieldSignature::fixed_type`], never as an enum payload — a
/// data-carrying variant would force a `match` on read and violate the
/// Branchless Law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum SignatureKind {
    /// A context source with no inputs: the output type is intrinsic to the
    /// operator and is reported by [`FieldSignature::fixed_type`].
    Fixed = 0,
    /// The output type is declared in a parameter word (`Const`, `Param`).
    FromParams = 1,
    /// The output type is the widest input; all non-scalar inputs must agree.
    /// A scalar broadcasting to a vector is the language's only implicit
    /// conversion.
    WidthGeneric = 2,
    /// The operator collapses its inputs to a scalar.
    ScalarOut = 3,
    /// The operator yields a three-component vector whatever it consumed.
    Vec3Out = 4,
    /// The output width is decided by the operator's own rule — `Compose` reads
    /// it from a parameter, `Component` always yields a scalar.
    Explicit = 5,
}

/// One operator's shape: its arity, its parameter-word count, and the rule that
/// derives its output type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldSignature {
    inputs: u8,
    params: u8,
    kind: SignatureKind,
    fixed_type: FieldType,
}

impl FieldSignature {
    /// The input count of an operator whose arity is not fixed by the table but
    /// decided by a parameter. Today only [`FieldOp::Compose`], whose input
    /// count equals the output width it declares.
    pub const PARAM_DECIDED_INPUTS: u8 = u8::MAX;

    /// How many inputs the operator consumes, or [`Self::PARAM_DECIDED_INPUTS`].
    pub const fn inputs(self) -> u8 {
        self.inputs
    }

    /// How many raw parameter words the operator's node carries.
    pub const fn params(self) -> u8 {
        self.params
    }

    /// The rule that derives the operator's output type.
    pub const fn kind(self) -> SignatureKind {
        self.kind
    }

    /// The output type, meaningful only when [`Self::kind`] is
    /// [`SignatureKind::Fixed`], [`SignatureKind::ScalarOut`] or
    /// [`SignatureKind::Vec3Out`]. For every other rule it holds the documented
    /// default [`FieldType::Scalar`] and is never read.
    pub const fn fixed_type(self) -> FieldType {
        self.fixed_type
    }

    /// Whether the operator's input count is decided by a parameter.
    pub const fn has_param_decided_inputs(self) -> bool {
        self.inputs == FieldSignature::PARAM_DECIDED_INPUTS
    }
}

/// One row per [`FieldOp`], in discriminant order. The operator code indexes this
/// table directly, which is what makes the `const` table safe.
///
/// The rows are struct literals rather than calls to a constructor on purpose: a
/// `const fn` used only by a `const` initializer is evaluated at compile time and
/// so can never be reached by a test, and the Coverage Law's answer to
/// unreachable code is to remove the shape, not to excuse it.
#[rustfmt::skip]
const SIGNATURES: [FieldSignature; FIELD_OP_COUNT] = {
    use FieldSignature as S;
    use FieldType::{Scalar, Vec2, Vec3};
    use SignatureKind::{Explicit, Fixed, FromParams, ScalarOut, Vec3Out, WidthGeneric};
    const VARIADIC: u8 = FieldSignature::PARAM_DECIDED_INPUTS;
    [
        // Const: [type, x, y, z, w].
        S { inputs: 0, params: 5, kind: FromParams, fixed_type: Scalar },
        // Point / Uv / Normal / Time — the context sources.
        S { inputs: 0, params: 0, kind: Fixed, fixed_type: Vec3 },
        S { inputs: 0, params: 0, kind: Fixed, fixed_type: Vec2 },
        S { inputs: 0, params: 0, kind: Fixed, fixed_type: Vec3 },
        S { inputs: 0, params: 0, kind: Fixed, fixed_type: Scalar },
        // Param: [slot, type].
        S { inputs: 0, params: 2, kind: FromParams, fixed_type: Scalar },
        // Add / Sub / Mul / Min / Max — binary, width-generic.
        S { inputs: 2, params: 0, kind: WidthGeneric, fixed_type: Scalar },
        S { inputs: 2, params: 0, kind: WidthGeneric, fixed_type: Scalar },
        S { inputs: 2, params: 0, kind: WidthGeneric, fixed_type: Scalar },
        S { inputs: 2, params: 0, kind: WidthGeneric, fixed_type: Scalar },
        S { inputs: 2, params: 0, kind: WidthGeneric, fixed_type: Scalar },
        // Abs — unary, width-generic.
        S { inputs: 1, params: 0, kind: WidthGeneric, fixed_type: Scalar },
        // Clamp / Mix / Smoothstep — ternary, width-generic.
        S { inputs: 3, params: 0, kind: WidthGeneric, fixed_type: Scalar },
        S { inputs: 3, params: 0, kind: WidthGeneric, fixed_type: Scalar },
        S { inputs: 3, params: 0, kind: WidthGeneric, fixed_type: Scalar },
        // Dot / Length — collapse to a scalar.
        S { inputs: 2, params: 0, kind: ScalarOut, fixed_type: Scalar },
        S { inputs: 1, params: 0, kind: ScalarOut, fixed_type: Scalar },
        // Normalize — a unit Vec3.
        S { inputs: 1, params: 0, kind: Vec3Out, fixed_type: Vec3 },
        // Compose: [width]; the input count is that declared width.
        S { inputs: VARIADIC, params: 1, kind: Explicit, fixed_type: Scalar },
        // Component: [lane].
        S { inputs: 1, params: 1, kind: Explicit, fixed_type: Scalar },
        // Noise / Fbm — one sample point in, a scalar out.
        S { inputs: 1, params: SEED_PARAMS, kind: ScalarOut, fixed_type: Scalar },
        S { inputs: 1, params: SEED_PARAMS + FBM_KNOB_PARAMS, kind: ScalarOut, fixed_type: Scalar },
        // Transform — one point through a matrix held in the parameter table.
        S { inputs: 1, params: TRANSFORM_PARAMS, kind: Vec3Out, fixed_type: Vec3 },
    ]
};

impl FieldOp {
    /// This operator's row of the signature table. Indexed by the discriminant,
    /// so there is no lookup and no branch.
    pub const fn signature(self) -> FieldSignature {
        SIGNATURES[self as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_exactly_one_row_per_operator() {
        assert_eq!(SIGNATURES.len(), FIELD_OP_COUNT);
        FieldOp::ALL.iter().enumerate().for_each(|(index, op)| {
            assert_eq!(op.signature(), SIGNATURES[index]);
        });
    }

    #[test]
    fn the_context_sources_have_fixed_types_and_no_inputs() {
        let sources = [
            (FieldOp::Point, FieldType::Vec3),
            (FieldOp::Uv, FieldType::Vec2),
            (FieldOp::Normal, FieldType::Vec3),
            (FieldOp::Time, FieldType::Scalar),
        ];
        sources.iter().for_each(|(op, ty)| {
            let sig = op.signature();
            assert_eq!(sig.kind(), SignatureKind::Fixed);
            assert_eq!(sig.fixed_type(), *ty);
            assert_eq!(sig.inputs(), 0);
            assert_eq!(sig.params(), 0);
        });
    }

    #[test]
    fn const_and_param_declare_their_type_in_a_parameter() {
        assert_eq!(FieldOp::Const.signature().kind(), SignatureKind::FromParams);
        assert_eq!(FieldOp::Const.signature().params(), 5);
        assert_eq!(FieldOp::Const.signature().inputs(), 0);
        assert_eq!(FieldOp::Param.signature().kind(), SignatureKind::FromParams);
        assert_eq!(FieldOp::Param.signature().params(), 2);
        assert_eq!(FieldOp::Param.signature().inputs(), 0);
    }

    #[test]
    fn the_width_generic_operators_are_the_nine_documented_ones() {
        let generic: Vec<FieldOp> = FieldOp::ALL
            .iter()
            .copied()
            .filter(|op| op.signature().kind() == SignatureKind::WidthGeneric)
            .collect();
        assert_eq!(
            generic,
            vec![
                FieldOp::Add,
                FieldOp::Sub,
                FieldOp::Mul,
                FieldOp::Min,
                FieldOp::Max,
                FieldOp::Abs,
                FieldOp::Clamp,
                FieldOp::Mix,
                FieldOp::Smoothstep,
            ]
        );
        assert_eq!(FieldOp::Abs.signature().inputs(), 1);
        assert_eq!(FieldOp::Add.signature().inputs(), 2);
        assert_eq!(FieldOp::Mix.signature().inputs(), 3);
    }

    #[test]
    fn the_scalar_out_operators_are_dot_length_noise_and_fbm() {
        let scalar_out: Vec<FieldOp> = FieldOp::ALL
            .iter()
            .copied()
            .filter(|op| op.signature().kind() == SignatureKind::ScalarOut)
            .collect();
        assert_eq!(
            scalar_out,
            vec![FieldOp::Dot, FieldOp::Length, FieldOp::Noise, FieldOp::Fbm]
        );
        scalar_out
            .iter()
            .for_each(|op| assert_eq!(op.signature().fixed_type(), FieldType::Scalar));
    }

    #[test]
    fn the_vec3_out_operators_are_normalize_and_transform() {
        let vec3_out: Vec<FieldOp> = FieldOp::ALL
            .iter()
            .copied()
            .filter(|op| op.signature().kind() == SignatureKind::Vec3Out)
            .collect();
        assert_eq!(vec3_out, vec![FieldOp::Normalize, FieldOp::Transform]);
        vec3_out
            .iter()
            .for_each(|op| assert_eq!(op.signature().fixed_type(), FieldType::Vec3));
    }

    #[test]
    fn compose_is_the_only_operator_whose_arity_a_parameter_decides() {
        let variadic: Vec<FieldOp> = FieldOp::ALL
            .iter()
            .copied()
            .filter(|op| op.signature().has_param_decided_inputs())
            .collect();
        assert_eq!(variadic, vec![FieldOp::Compose]);
        assert_eq!(FieldOp::Compose.signature().kind(), SignatureKind::Explicit);
        assert_eq!(FieldOp::Compose.signature().params(), 1);
        assert_eq!(
            FieldOp::Compose.signature().inputs(),
            FieldSignature::PARAM_DECIDED_INPUTS
        );
    }

    #[test]
    fn component_takes_one_input_and_one_lane_parameter() {
        let sig = FieldOp::Component.signature();
        assert_eq!(sig.kind(), SignatureKind::Explicit);
        assert_eq!(sig.inputs(), 1);
        assert_eq!(sig.params(), 1);
        assert!(!sig.has_param_decided_inputs());
    }

    #[test]
    fn the_spatial_arities_are_derived_from_the_layers_they_lower_to() {
        assert_eq!(SEED_PARAMS, 2);
        // Pinned by the exhaustive `FbmConfig` destructuring in `noise_words`,
        // not by the config's memory layout.
        assert_eq!(FBM_KNOB_PARAMS, 4);
        assert_eq!(TRANSFORM_PARAMS, 4);
        assert_eq!(FieldOp::Noise.signature().params(), 2);
        assert_eq!(FieldOp::Fbm.signature().params(), 6);
        assert_eq!(FieldOp::Transform.signature().params(), 4);
        assert_eq!(FieldOp::Noise.signature().inputs(), 1);
        assert_eq!(FieldOp::Fbm.signature().inputs(), 1);
        assert_eq!(FieldOp::Transform.signature().inputs(), 1);
    }

    #[test]
    fn every_signature_kind_is_used_by_at_least_one_row() {
        let kinds = [
            SignatureKind::Fixed,
            SignatureKind::FromParams,
            SignatureKind::WidthGeneric,
            SignatureKind::ScalarOut,
            SignatureKind::Vec3Out,
            SignatureKind::Explicit,
        ];
        kinds.iter().enumerate().for_each(|(index, kind)| {
            assert_eq!(*kind as u16, index as u16);
            assert!(FieldOp::ALL.iter().any(|op| op.signature().kind() == *kind));
        });
    }
}
