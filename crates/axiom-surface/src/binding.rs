//! What one channel is bound to: a constant value, or a field expression.

use axiom_field::{FieldBuilder, FieldGraph, FieldId, FieldType, FieldValue};
use axiom_kernel::{BinaryReader, BinaryWriter};
use axiom_math::{Vec2, Vec3, Vec4};
use axiom_recipe::Scalar;

use crate::surface_error::{SurfaceError, SurfaceErrorCode, SurfaceResult};

/// Undecodable binding bytes name no channel and no node.
const MALFORMED_BINDING: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::MalformedData,
    "the channel binding could not be decoded from its bytes",
);

/// A binding kind code that names neither a constant nor a field.
const UNKNOWN_BINDING_KIND: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::UnknownBindingKind,
    "the channel binding declares a kind code that names no binding kind",
);

/// A type code in a binding's constant value that names no field type.
const UNKNOWN_VALUE_TYPE: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::UnknownType,
    "the channel binding's constant declares a type code that names no field type",
);

/// Rebuild a value of each [`FieldType`] from four lanes, indexed by the type
/// code. A table, so decoding is a lookup rather than a `match`.
const REBUILD: [fn(Vec4) -> FieldValue; 4] = [
    |lanes| FieldValue::scalar(Scalar::new(lanes.x)),
    |lanes| FieldValue::vec2(Vec2::new(lanes.x, lanes.y)),
    |lanes| FieldValue::vec3(Vec3::new(lanes.x, lanes.y, lanes.z)),
    FieldValue::vec4,
];

/// Rebuild a binding from its decoded constant and its graph payload, indexed
/// by `kind - 1`. The kind alone decides which half is authoritative — a
/// constant binding ignores the payload entirely, which is also why a constant
/// writes an **empty** payload and costs no graph bytes at all.
const REBUILD_BINDING: [fn(FieldValue, &[u8]) -> SurfaceResult<ChannelBinding>; 2] = [
    |value, _payload| Ok(ChannelBinding::constant(value)),
    |_value, payload| {
        FieldGraph::deserialize(payload)
            .map(ChannelBinding::field)
            .map_err(SurfaceError::from_field)
    },
];

/// What one [`crate::SurfaceChannel`] is bound to.
///
/// A **tagged struct**, not a data-carrying enum (the Branchless Law; the
/// `RenderCommand` precedent in `modules/axiom-render`): [`ChannelBinding::kind`]
/// selects which half is meaningful and the other half holds a fixed default
/// that is never read for the wrong kind. Construction goes through
/// [`ChannelBinding::constant`] / [`ChannelBinding::field`], inspection through
/// the branchless `as_*` accessors, and there is no `match` over the binding's
/// shape anywhere in this layer.
///
/// **Every binding has a graph form.** [`ChannelBinding::as_graph`] answers with
/// the bound graph, or with the one-node `Const` graph a constant *is*. That is
/// what lets layer flattening treat constants and fields uniformly and compose
/// them without a single branch.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelBinding {
    kind: u16,
    value: FieldValue,
    graph: FieldGraph,
}

impl ChannelBinding {
    /// The kind code of a binding whose value is a constant.
    pub const KIND_CONSTANT: u16 = 1;

    /// The kind code of a binding whose value is a field expression.
    pub const KIND_FIELD: u16 = 2;

    /// Bind the channel to a constant `value`.
    pub fn constant(value: FieldValue) -> Self {
        ChannelBinding {
            kind: ChannelBinding::KIND_CONSTANT,
            value,
            graph: placeholder_graph(),
        }
    }

    /// Bind the channel to a field expression, evaluated in **object space**
    /// (see this crate's `ARCHITECTURE.md`).
    pub fn field(graph: FieldGraph) -> Self {
        ChannelBinding {
            kind: ChannelBinding::KIND_FIELD,
            value: FieldValue::ZERO,
            graph,
        }
    }

    /// Which kind of binding this is: [`Self::KIND_CONSTANT`] or
    /// [`Self::KIND_FIELD`].
    pub const fn kind(&self) -> u16 {
        self.kind
    }

    /// Whether the binding is a constant.
    pub const fn is_constant(&self) -> bool {
        self.kind == ChannelBinding::KIND_CONSTANT
    }

    /// The constant value, or `None` when the binding is a field.
    pub fn as_constant(&self) -> Option<FieldValue> {
        self.is_constant().then_some(self.value)
    }

    /// The bound field graph, or `None` when the binding is a constant.
    pub fn as_field(&self) -> Option<&FieldGraph> {
        (self.kind == ChannelBinding::KIND_FIELD).then_some(&self.graph)
    }

    /// The binding as a field graph, whichever kind it is: the bound graph, or
    /// the single-node `Const` graph a constant is.
    pub fn as_graph(&self) -> FieldGraph {
        self.as_field()
            .cloned()
            .unwrap_or_else(|| constant_graph(self.value))
    }

    /// The type the binding produces.
    ///
    /// A constant reports its own type; a field reports the derived type of its
    /// declared output, which type-checks the whole graph and therefore fails
    /// exactly when the graph does not type.
    pub fn ty(&self) -> SurfaceResult<FieldType> {
        self.as_field().map_or(Ok(self.value.ty()), |graph| {
            graph
                .type_of(graph.output())
                .map_err(SurfaceError::from_field)
        })
    }

    /// Append the binding's canonical bytes: the `u16` kind, the constant's
    /// `u16` type code and four `f32` lanes, then the bound graph's own bytes as
    /// a length-prefixed slice — **empty** for a constant, which carries no
    /// graph.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        self.write_head_to(writer);
        writer.write_byte_slice(&self.as_field().map_or_else(Vec::new, FieldGraph::serialize));
    }

    /// Append only the binding's **structure**: the head, then the bound graph's
    /// structural digest in place of its bytes. A field's parameter *values*
    /// live inside that digest's excluded half, which is exactly why retuning a
    /// parameter cannot move [`crate::Surface::digest`].
    pub(crate) fn write_structure_to(&self, writer: &mut BinaryWriter) {
        self.write_head_to(writer);
        writer.write_u64(self.as_field().map_or(0, |graph| graph.digest().raw()));
    }

    /// The part both forms share: the kind and the constant's typed lanes.
    fn write_head_to(&self, writer: &mut BinaryWriter) {
        writer.write_u16(self.kind);
        writer.write_u16(self.value.ty().code());
        let lanes = self.value.as_vec4();
        [lanes.x, lanes.y, lanes.z, lanes.w]
            .iter()
            .for_each(|lane| writer.write_f32(*lane));
    }

    /// Read a binding written by [`Self::write_to`]. Bounds-checked throughout:
    /// a truncated buffer, an unknown kind, an unknown type code and an
    /// undecodable graph all fail cleanly rather than producing a binding
    /// nobody can name.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> SurfaceResult<ChannelBinding> {
        read_head(reader).and_then(|(kind, value)| {
            reader
                .read_byte_slice()
                .map_err(|_| MALFORMED_BINDING)
                .and_then(|payload| {
                    REBUILD_BINDING
                        .get(kind.wrapping_sub(1) as usize)
                        .ok_or(UNKNOWN_BINDING_KIND)
                        .and_then(|build| build(value, payload))
                })
        })
    }
}

/// Read the kind and the typed constant lanes.
fn read_head(reader: &mut BinaryReader<'_>) -> SurfaceResult<(u16, FieldValue)> {
    reader
        .read_u16()
        .and_then(|kind| reader.read_u16().map(|code| (kind, code)))
        .map_err(|_| MALFORMED_BINDING)
        .and_then(|(kind, code)| {
            REBUILD
                .get(code as usize)
                .ok_or(UNKNOWN_VALUE_TYPE)
                .map(|rebuild| (kind, rebuild))
        })
        .and_then(|(kind, rebuild)| {
            read_lanes(reader).map(|lanes| (kind, rebuild(lanes)))
        })
}

/// Read the four `f32` lanes of a binding's constant.
fn read_lanes(reader: &mut BinaryReader<'_>) -> SurfaceResult<Vec4> {
    (0..4)
        .try_fold([0.0_f32; 4], |mut lanes, index| {
            reader.read_f32().map(|lane| {
                lanes[index] = lane;
                lanes
            })
        })
        .map_err(|_| MALFORMED_BINDING)
        .map(|lanes| Vec4::new(lanes[0], lanes[1], lanes[2], lanes[3]))
}

/// The single-node `Const` graph a constant binding *is*.
fn constant_graph(value: FieldValue) -> FieldGraph {
    let (builder, node) = FieldBuilder::new(FieldId::from_raw(0), 0).push_const(value);
    builder.build(node)
}

/// The fixed graph a **constant** binding parks in its unused graph lane.
///
/// It is deliberately *not* the constant's own graph form: the kind alone
/// decides what a binding means, so the unused lane holds one documented value
/// — the zero `Const` — and can never be mistaken for the binding's meaning.
fn placeholder_graph() -> FieldGraph {
    constant_graph(FieldValue::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{EvalContext, FieldOp};
    use axiom_recipe::{NodeId, Param};

    fn uv_field() -> FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("surface/test/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        builder.build(lane)
    }

    fn bytes_of(binding: &ChannelBinding) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        binding.write_to(&mut writer);
        writer.into_bytes()
    }

    #[test]
    fn a_constant_binding_reports_its_value_and_never_a_field() {
        let binding = ChannelBinding::constant(FieldValue::scalar(Scalar::new(0.25)));
        assert_eq!(binding.kind(), ChannelBinding::KIND_CONSTANT);
        assert!(binding.is_constant());
        assert_eq!(
            binding.as_constant(),
            Some(FieldValue::scalar(Scalar::new(0.25)))
        );
        assert_eq!(binding.as_field(), None);
        assert_eq!(binding.ty(), Ok(FieldType::Scalar));
    }

    #[test]
    fn a_field_binding_reports_its_graph_and_never_a_constant() {
        let binding = ChannelBinding::field(uv_field());
        assert_eq!(binding.kind(), ChannelBinding::KIND_FIELD);
        assert!(!binding.is_constant());
        assert_eq!(binding.as_constant(), None);
        assert_eq!(binding.as_field(), Some(&uv_field()));
        assert_eq!(binding.ty(), Ok(FieldType::Scalar));
    }

    #[test]
    fn every_binding_has_a_graph_form_that_evaluates_to_its_value() {
        let constant = ChannelBinding::constant(FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(
            constant.as_graph().evaluate(&EvalContext::ORIGIN),
            Ok(FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0)))
        );
        assert_eq!(constant.as_graph().node_count(), 1);
        assert_eq!(ChannelBinding::field(uv_field()).as_graph(), uv_field());
    }

    #[test]
    fn a_graph_that_does_not_type_is_reported_as_an_invalid_field() {
        let (builder, node) = FieldBuilder::new(FieldId::from_raw(3), 1).push(
            FieldOp::Length,
            Vec::new(),
            Vec::new(),
        );
        let error = ChannelBinding::field(builder.build(node))
            .ty()
            .expect_err("Length with no input does not type");
        assert_eq!(error.kind(), SurfaceErrorCode::InvalidField);
        assert_eq!(error.node(), NodeId::from_raw(0));
    }

    #[test]
    fn a_binding_of_each_kind_round_trips_through_its_bytes() {
        [
            ChannelBinding::constant(FieldValue::vec4(Vec4::new(0.1, 0.2, 0.3, 0.4))),
            ChannelBinding::constant(FieldValue::vec2(Vec2::new(1.5, 2.5))),
            ChannelBinding::constant(FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0))),
            ChannelBinding::constant(FieldValue::scalar(Scalar::new(-4.0))),
            ChannelBinding::field(uv_field()),
        ]
        .iter()
        .for_each(|binding| {
            let bytes = bytes_of(binding);
            let decoded = ChannelBinding::read_from(&mut BinaryReader::new(&bytes))
                .expect("a binding writes bytes it can read back");
            assert_eq!(&decoded, binding);
        });
    }

    #[test]
    fn every_truncation_of_a_binding_fails_cleanly() {
        let bytes = bytes_of(&ChannelBinding::field(uv_field()));
        (0..bytes.len()).for_each(|n| {
            assert!(
                ChannelBinding::read_from(&mut BinaryReader::new(&bytes[..n])).is_err(),
                "prefix of length {n} must not decode"
            );
        });
    }

    #[test]
    fn an_unknown_kind_code_is_rejected() {
        let mut bytes = bytes_of(&ChannelBinding::constant(FieldValue::ZERO));
        bytes[0] = 0;
        assert_eq!(
            ChannelBinding::read_from(&mut BinaryReader::new(&bytes))
                .expect_err("kind 0 names no binding kind")
                .kind(),
            SurfaceErrorCode::UnknownBindingKind
        );
        bytes[0] = 9;
        assert_eq!(
            ChannelBinding::read_from(&mut BinaryReader::new(&bytes))
                .expect_err("kind 9 names no binding kind")
                .kind(),
            SurfaceErrorCode::UnknownBindingKind
        );
    }

    #[test]
    fn an_unknown_constant_type_code_is_rejected() {
        let mut bytes = bytes_of(&ChannelBinding::constant(FieldValue::ZERO));
        bytes[2] = 7;
        let error = ChannelBinding::read_from(&mut BinaryReader::new(&bytes))
            .expect_err("type code 7 names no field type");
        assert_eq!(error.kind(), SurfaceErrorCode::UnknownType);
        assert_eq!(error.code(), 5);
    }

    #[test]
    fn undecodable_graph_bytes_are_reported_as_an_invalid_field() {
        let mut bytes = bytes_of(&ChannelBinding::field(uv_field()));
        // The head is the kind, the type code and four lanes; the graph then
        // arrives as a length-prefixed slice. Shrink that length to one byte, so
        // the slice reads cleanly and the graph inside it cannot decode.
        let length_at = 2 + 2 + 16;
        bytes[length_at] = 1;
        [1, 2, 3]
            .iter()
            .for_each(|offset| bytes[length_at + offset] = 0);
        assert_eq!(
            ChannelBinding::read_from(&mut BinaryReader::new(&bytes))
                .expect_err("the embedded graph no longer decodes")
                .kind(),
            SurfaceErrorCode::InvalidField
        );
    }

    #[test]
    fn the_structural_form_hides_a_bound_parameter_value_but_not_the_constant() {
        let structure_of = |binding: &ChannelBinding| {
            let mut writer = BinaryWriter::new();
            binding.write_structure_to(&mut writer);
            writer.into_bytes()
        };
        let tuned = |value: f32| {
            let (builder, slot) = FieldBuilder::new(FieldId::of_name("surface/test/tuned"), 1)
                .declare("tint", FieldValue::scalar(Scalar::new(value)));
            let (builder, node) = builder.push_param(slot, FieldType::Scalar);
            ChannelBinding::field(builder.build(node))
        };
        assert_eq!(structure_of(&tuned(0.1)), structure_of(&tuned(0.9)));
        assert_ne!(bytes_of(&tuned(0.1)), bytes_of(&tuned(0.9)));

        let low = ChannelBinding::constant(FieldValue::scalar(Scalar::new(0.1)));
        let high = ChannelBinding::constant(FieldValue::scalar(Scalar::new(0.9)));
        assert_ne!(structure_of(&low), structure_of(&high));
    }

    #[test]
    fn the_unused_graph_lane_of_a_constant_is_one_fixed_value() {
        let one = ChannelBinding::constant(FieldValue::scalar(Scalar::new(3.0)));
        let other = ChannelBinding::constant(FieldValue::scalar(Scalar::new(4.0)));
        assert_eq!(placeholder_graph(), constant_graph(FieldValue::ZERO));
        assert_ne!(one, other);
        assert_eq!(
            one.as_graph().evaluate(&EvalContext::ORIGIN),
            Ok(FieldValue::scalar(Scalar::new(3.0)))
        );
    }
}
