//! The canonical byte form of a surface, and the structural digest folded from
//! it.
//!
//! A surface is a **recursive value type** and its bytes are deliberately not.
//! The tree is linearised breadth-first into one flat record per surface, each
//! naming its parent by index, so writing is one pass and reading is one reverse
//! pass — no recursive `write_to`/`read_from` pair, which
//! `engine_no_recursion` would reject and which a hostile depth could blow the
//! stack with anyway.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::binding::ChannelBinding;
use crate::channel::SURFACE_CHANNEL_COUNT;
use crate::layer::{LayerBlend, SurfaceLayer, MAX_LAYERS};
use crate::layer_tree::{self, SurfaceNode, ROOT_PARENT};
use crate::lighting_model::LightingModel;
use crate::surface::{Surface, SURFACE_SCHEMA_VERSION};
use crate::surface_error::{SurfaceError, SurfaceErrorCode, SurfaceResult};

/// The parent index a record uses for the root — the wire form of
/// [`ROOT_PARENT`], stated as one derivation so the two can never drift.
const NO_PARENT: u32 = ROOT_PARENT as u32;

/// Undecodable surface bytes.
const MALFORMED_SURFACE: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::MalformedData,
    "the serialized surface could not be decoded from its bytes",
);

/// The records decode, but their parent links are not a tree rooted at record
/// zero.
const NOT_A_TREE: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::MalformedData,
    "the serialized surface's parent links do not form a tree rooted at record zero",
);

/// More records than one root plus the layer budget.
const TOO_MANY_RECORDS: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::LayerBudgetExceeded,
    "the serialized surface holds more layers than the layer budget allows",
);

/// A blend code that names no blend.
const UNKNOWN_BLEND: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::UnknownBlend,
    "the serialized surface declares a blend code that names no layer blend",
);

/// A lighting-model code that names no model.
const UNKNOWN_LIGHTING: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::UnknownLightingModel,
    "the serialized surface declares a code that names no lighting model",
);

/// How one binding is appended — the full bytes, or the structure only.
type BindingWriter = fn(&ChannelBinding, &mut BinaryWriter);

/// One decoded surface record, before the tree is rebuilt from the parent links.
#[derive(Debug)]
struct Record {
    parent: u32,
    blend: LayerBlend,
    lighting: LightingModel,
    mask: ChannelBinding,
    bindings: [ChannelBinding; SURFACE_CHANNEL_COUNT],
}

/// The surface's canonical bytes.
pub(crate) fn serialize(root: &Surface) -> Vec<u8> {
    write_all(root, ChannelBinding::write_to)
}

/// The surface's structural digest — the same walk, with each binding's
/// structure in place of its full bytes.
pub(crate) fn digest(root: &Surface) -> axiom_kernel::StableHash {
    axiom_kernel::StableHash::of_bytes(&write_all(root, ChannelBinding::write_structure_to))
}

/// One pass over the linearised tree, writing each binding through `write`.
fn write_all(root: &Surface, write: BindingWriter) -> Vec<u8> {
    let nodes = layer_tree::linearize(root);
    let opaque = SurfaceLayer::opaque_mask();
    let mut writer = BinaryWriter::new();
    SURFACE_SCHEMA_VERSION.write_to(&mut writer);
    // The surface's KIND, in the header rather than per record: it is a property
    // of the whole surface, not of a layer.
    //
    // Only the code is written, never the `MaterialParams` behind it. That is
    // deliberate and it is what makes every runtime material **one program**:
    // `digest` is structural and excludes parameter values so that retuning one
    // cannot force a recompile, and a runtime material obeys the same rule. Its
    // parameters travel in the parameter buffer, exactly as a field surface's
    // constants do.
    writer.write_u16(root.kind().code());
    writer.write_u32(nodes.len() as u32);
    nodes
        .iter()
        .for_each(|node| write_record(&mut writer, node, &opaque, write));
    writer.into_bytes()
}

/// One record: the parent index, the blend, the lighting model, the mask, then
/// the seven bindings in channel order.
///
/// The root owns no mask and no blend, so the record synthesizes the fixed
/// [`SurfaceLayer::opaque_mask`] and [`LayerBlend::Over`] for it — a uniform
/// record shape rather than a conditional write, and a canonical one because
/// the synthesized values are fixed.
fn write_record(
    writer: &mut BinaryWriter,
    node: &SurfaceNode<'_>,
    opaque: &ChannelBinding,
    write: BindingWriter,
) {
    writer.write_u32(node.parent as u32);
    writer.write_u16(
        node.layer
            .map_or(LayerBlend::Over, SurfaceLayer::blend)
            .code(),
    );
    writer.write_u16(node.surface.lighting().code());
    write(node.layer.map_or(opaque, SurfaceLayer::mask), writer);
    node.surface
        .bindings()
        .iter()
        .for_each(|binding| write(binding, writer));
}

/// Decode and validate a surface written by [`serialize`].
pub(crate) fn deserialize(bytes: &[u8]) -> SurfaceResult<Surface> {
    let mut reader = BinaryReader::new(bytes);
    read_count(&mut reader)
        .and_then(|count| {
            (0..count).try_fold(Vec::with_capacity(count), |mut records, _| {
                read_record(&mut reader).map(|record| {
                    records.push(record);
                    records
                })
            })
        })
        .and_then(check_tree)
        .map(rebuild)
        .and_then(|surface| surface.validate().map(|()| surface))
}

/// Read the schema stamp and the record count, which must name at least the
/// root and at most the root plus the layer budget.
fn read_count(reader: &mut BinaryReader<'_>) -> SurfaceResult<usize> {
    axiom_kernel::SchemaVersion::read_from(reader)
        .and_then(|_stamp| reader.read_u16())
        .and_then(|_kind| reader.read_u32())
        .map_err(|_| MALFORMED_SURFACE)
        .and_then(|count| (count > 0).then_some(count as usize).ok_or(MALFORMED_SURFACE))
        .and_then(|count| {
            (count <= MAX_LAYERS + 1)
                .then_some(count)
                .ok_or_else(|| TOO_MANY_RECORDS.about_layer((MAX_LAYERS + 1) as u16))
        })
}

/// Read one record.
fn read_record(reader: &mut BinaryReader<'_>) -> SurfaceResult<Record> {
    read_head(reader).and_then(|(parent, blend, lighting)| {
        ChannelBinding::read_from(reader).and_then(|mask| {
            read_bindings(reader).map(|bindings| Record {
                parent,
                blend,
                lighting,
                mask,
                bindings,
            })
        })
    })
}

/// Read a record's parent index, blend and lighting model.
fn read_head(reader: &mut BinaryReader<'_>) -> SurfaceResult<(u32, LayerBlend, LightingModel)> {
    reader
        .read_u32()
        .and_then(|parent| reader.read_u16().map(|blend| (parent, blend)))
        .and_then(|(parent, blend)| {
            reader
                .read_u16()
                .map(|lighting| (parent, blend, lighting))
        })
        .map_err(|_| MALFORMED_SURFACE)
        .and_then(|(parent, blend, lighting)| {
            LayerBlend::from_code(blend)
                .ok_or(UNKNOWN_BLEND)
                .and_then(|blend| {
                    LightingModel::from_code(lighting)
                        .ok_or(UNKNOWN_LIGHTING)
                        .map(|lighting| (parent, blend, lighting))
                })
        })
}

/// Read a record's seven channel bindings, in channel order.
fn read_bindings(
    reader: &mut BinaryReader<'_>,
) -> SurfaceResult<[ChannelBinding; SURFACE_CHANNEL_COUNT]> {
    (0..SURFACE_CHANNEL_COUNT)
        .try_fold(
            Vec::with_capacity(SURFACE_CHANNEL_COUNT),
            |mut bindings, _| {
                ChannelBinding::read_from(reader).map(|binding| {
                    bindings.push(binding);
                    bindings
                })
            },
        )
        .map(|bindings| {
            bindings
                .try_into()
                .expect("seven bindings were read, so seven bindings come back")
        })
}

/// The parent links must name a tree rooted at record zero: record zero has no
/// parent, and every later record's parent is a strictly earlier record. That is
/// the same strictly-earlier rule the field container proves for node inputs,
/// and it is what makes the rebuild one reverse pass.
fn check_tree(records: Vec<Record>) -> SurfaceResult<Vec<Record>> {
    records
        .iter()
        .enumerate()
        .all(|(index, record)| {
            let root = (index == 0) & (record.parent == NO_PARENT);
            let child = (index > 0) & ((record.parent as usize) < index);
            root | child
        })
        .then_some(records)
        .ok_or(NOT_A_TREE)
}

/// Rebuild the tree from the flat records.
///
/// One descending pass: a record's children are always later in the list, so by
/// the time the pass reaches a record every child of it is already assembled and
/// waiting to be taken. Children are gathered in ascending index order, which is
/// the order they were written in, which is the layer order.
fn rebuild(records: Vec<Record>) -> Surface {
    let count = records.len();
    let mut assembled: Vec<Option<SurfaceLayer>> = (0..count).map(|_| None).collect();
    (0..count).rev().for_each(|index| {
        let layers: Vec<SurfaceLayer> = ((index + 1)..count)
            .filter(|child| records[*child].parent as usize == index)
            .filter_map(|child| assembled[child].take())
            .collect();
        let record = &records[index];
        assembled[index] = Some(SurfaceLayer::new(
            Surface::new(record.bindings.clone(), record.lighting, layers),
            record.mask.clone(),
            record.blend,
        ));
    });
    assembled[0]
        .take()
        .expect("record zero is assembled last, so it is always present")
        .into_surface()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface_kind::SurfaceKind;
    use crate::channel::SurfaceChannel;
    use crate::surface_builder::SurfaceBuilder;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldType, FieldValue, Param, Scalar};

    fn uv_scalar() -> axiom_field::FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("surface/bytes/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        builder.build(lane)
    }

    /// A layered, multi-channel surface: a field roughness, a tuned parameter
    /// opacity, two layers with different blends, and a non-default lighting
    /// model.
    fn sample() -> Surface {
        let tuned = |value: f32| {
            let (builder, slot) = FieldBuilder::new(FieldId::of_name("surface/bytes/tuned"), 1)
                .declare("fade", FieldValue::scalar(Scalar::new(value)));
            let (builder, node) = builder.push_param(slot, FieldType::Scalar);
            builder.build(node)
        };
        let inner = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::BaseColor,
                FieldValue::vec4(axiom_math::Vec4::new(1.0, 0.0, 0.0, 1.0)),
            )
            .lighting(LightingModel::Unlit)
            .build()
            .expect("legal");
        SurfaceBuilder::new()
            .field(SurfaceChannel::Roughness, uv_scalar())
            .field(SurfaceChannel::Opacity, tuned(0.5))
            .lighting(LightingModel::Lambert)
            .layer(SurfaceLayer::new(
                inner.clone(),
                ChannelBinding::field(uv_scalar()),
                LayerBlend::Multiply,
            ))
            .layer(SurfaceLayer::new(
                inner,
                SurfaceLayer::opaque_mask(),
                LayerBlend::Add,
            ))
            .build()
            .expect("two layers are within budget")
    }

    #[test]
    fn a_layered_surface_round_trips_through_its_canonical_bytes() {
        let surface = sample();
        assert_eq!(Surface::deserialize(&surface.serialize()), Ok(surface));
    }

    #[test]
    fn the_same_surface_always_produces_the_same_bytes_and_digest() {
        assert_eq!(sample().serialize(), sample().serialize());
        assert_eq!(sample().digest(), sample().digest());
    }

    #[test]
    fn every_truncation_fails_cleanly() {
        let bytes = sample().serialize();
        (0..bytes.len()).for_each(|n| {
            assert!(
                Surface::deserialize(&bytes[..n]).is_err(),
                "prefix of length {n} must not decode"
            );
        });
    }

    #[test]
    fn a_record_count_of_zero_is_malformed() {
        let mut writer = BinaryWriter::new();
        SURFACE_SCHEMA_VERSION.write_to(&mut writer);
        writer.write_u16(SurfaceKind::Field.code());
        writer.write_u32(0);
        let error = Surface::deserialize(&writer.into_bytes())
            .expect_err("a surface with no records names no root");
        assert_eq!(error.kind(), SurfaceErrorCode::MalformedData);
    }

    #[test]
    fn more_records_than_the_budget_are_rejected() {
        let mut writer = BinaryWriter::new();
        SURFACE_SCHEMA_VERSION.write_to(&mut writer);
        writer.write_u16(SurfaceKind::Field.code());
        writer.write_u32((MAX_LAYERS + 2) as u32);
        let error = Surface::deserialize(&writer.into_bytes())
            .expect_err("six records exceed one root plus four layers");
        assert_eq!(error.kind(), SurfaceErrorCode::LayerBudgetExceeded);
        assert_eq!(error.layer(), Some((MAX_LAYERS + 1) as u16));
    }

    #[test]
    fn an_unknown_blend_code_is_rejected() {
        let mut bytes = sample().serialize();
        // record 0 begins after the 4-byte stamp, the 2-byte surface-kind code
        // and the 4-byte count; its parent is 4 bytes, then the blend code.
        bytes[14] = 9;
        assert_eq!(
            Surface::deserialize(&bytes)
                .expect_err("blend code 9 names no blend")
                .kind(),
            SurfaceErrorCode::UnknownBlend
        );
    }

    #[test]
    fn an_unknown_lighting_code_is_rejected() {
        let mut bytes = sample().serialize();
        // ...and the lighting code follows the 2-byte blend.
        bytes[16] = 9;
        assert_eq!(
            Surface::deserialize(&bytes)
                .expect_err("lighting code 9 names no model")
                .kind(),
            SurfaceErrorCode::UnknownLightingModel
        );
    }

    #[test]
    fn parent_links_that_are_not_a_tree_are_rejected() {
        let mut bytes = sample().serialize();
        // Record zero must have no parent.
        bytes[8] = 0;
        bytes[9] = 0;
        bytes[10] = 0;
        bytes[11] = 0;
        assert_eq!(
            Surface::deserialize(&bytes)
                .expect_err("record zero cannot have a parent")
                .kind(),
            SurfaceErrorCode::MalformedData
        );
    }

    #[test]
    fn a_child_pointing_forward_is_rejected() {
        // Two records of identical shape, so the second one's parent field is
        // exactly one record length past the header.
        let uniform = SurfaceBuilder::new()
            .layer(SurfaceLayer::new(
                SurfaceBuilder::new().build().expect("legal"),
                SurfaceLayer::opaque_mask(),
                LayerBlend::Over,
            ))
            .build()
            .expect("one layer is within budget");
        let mut broken = uniform.serialize();
        let record_len = (broken.len() - 8) / 2;
        broken[8 + record_len] = 1;
        assert_eq!(
            Surface::deserialize(&broken)
                .expect_err("record one cannot be parented to record two")
                .kind(),
            SurfaceErrorCode::MalformedData
        );
    }

    #[test]
    fn the_digest_ignores_a_bound_parameter_value_but_not_structure() {
        let tuned = |value: f32| {
            let (builder, slot) = FieldBuilder::new(FieldId::of_name("surface/bytes/knob"), 1)
                .declare("fade", FieldValue::scalar(Scalar::new(value)));
            let (builder, node) = builder.push_param(slot, FieldType::Scalar);
            SurfaceBuilder::new()
                .field(SurfaceChannel::Opacity, builder.build(node))
                .build()
                .expect("a scalar param field is a legal opacity")
        };
        let low = tuned(0.1);
        let high = tuned(0.9);
        assert_ne!(low.serialize(), high.serialize());
        assert_eq!(low.digest(), high.digest());

        let restructured = SurfaceBuilder::new()
            .field(SurfaceChannel::Roughness, uv_scalar())
            .build()
            .expect("legal");
        assert_ne!(low.digest(), restructured.digest());
    }

    #[test]
    fn the_digest_moves_when_a_constant_a_blend_or_a_lighting_model_changes() {
        let base = SurfaceBuilder::new().build().expect("legal");
        let recoloured = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::BaseColor,
                FieldValue::vec4(axiom_math::Vec4::new(0.0, 1.0, 0.0, 1.0)),
            )
            .build()
            .expect("legal");
        let unlit = SurfaceBuilder::new()
            .lighting(LightingModel::Unlit)
            .build()
            .expect("legal");
        let layered = |blend: LayerBlend| {
            SurfaceBuilder::new()
                .layer(SurfaceLayer::new(
                    base.clone(),
                    SurfaceLayer::opaque_mask(),
                    blend,
                ))
                .build()
                .expect("legal")
        };
        assert_ne!(base.digest(), recoloured.digest());
        assert_ne!(base.digest(), unlit.digest());
        assert_ne!(base.digest(), layered(LayerBlend::Over).digest());
        assert_ne!(
            layered(LayerBlend::Over).digest(),
            layered(LayerBlend::Add).digest()
        );
    }
}
