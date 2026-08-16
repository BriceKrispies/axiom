//! What a backend must be able to do before it may render a surface.
//!
//! This is the backend-**neutral** half of the shader IR this design
//! deliberately does not have. It is *derived* from the bound graphs, never
//! authored, and it is what a backend checks against its own capability profile
//! **before attempting to lower anything**.

use axiom_field::{FieldGraph, FieldOp};

use crate::channel::SurfaceChannel;
use crate::layer_tree;
use crate::surface::Surface;

/// Which evaluation-context inputs a surface's field graphs read, as a bitset.
///
/// A surface that reads only `Uv` must not claim `Point`: the set is derived by
/// scanning the bound graphs for the four context-source operators, so it is
/// exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceInput(u16);

impl SurfaceInput {
    /// Reads nothing from the evaluation context.
    pub const NONE: SurfaceInput = SurfaceInput(0);
    /// Reads the object-space sample position.
    pub const POINT: SurfaceInput = SurfaceInput(1);
    /// Reads the surface parameterisation.
    pub const UV: SurfaceInput = SurfaceInput(2);
    /// Reads the surface normal.
    pub const NORMAL: SurfaceInput = SurfaceInput(4);
    /// Reads presentation time.
    pub const TIME: SurfaceInput = SurfaceInput(8);

    /// The raw bitset.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Both sets together.
    pub const fn union(self, other: SurfaceInput) -> SurfaceInput {
        SurfaceInput(self.0 | other.0)
    }

    /// Whether every input of `other` is in this set.
    pub const fn contains(self, other: SurfaceInput) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// The operator code of each context source and the input bit it implies.
const INPUT_OPS: [(u16, u16); 4] = [
    (FieldOp::Point.code(), SurfaceInput::POINT.bits()),
    (FieldOp::Uv.code(), SurfaceInput::UV.bits()),
    (FieldOp::Normal.code(), SurfaceInput::NORMAL.bits()),
    (FieldOp::Time.code(), SurfaceInput::TIME.bits()),
];

/// What a backend must satisfy to render a surface.
///
/// Derived by [`Surface::requirements`] from the whole layer tree — never
/// authored, and never stale, because there is nowhere to store a stale copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceRequirements {
    inputs: SurfaceInput,
    varying_channels: u16,
    has_displacement: bool,
    param_count: u16,
    node_count: u16,
}

impl SurfaceRequirements {
    /// A surface that needs nothing: every channel constant, no graph, no
    /// displacement.
    pub const EMPTY: SurfaceRequirements = SurfaceRequirements {
        inputs: SurfaceInput::NONE,
        varying_channels: 0,
        has_displacement: false,
        param_count: 0,
        node_count: 0,
    };

    /// Which evaluation-context inputs the surface's graphs read.
    pub const fn inputs(self) -> SurfaceInput {
        self.inputs
    }

    /// The bitset of channels that are not a single constant. Bit *n* is
    /// [`SurfaceChannel::bit`] of the channel with code *n*.
    pub const fn varying_channels(self) -> u16 {
        self.varying_channels
    }

    /// Whether `channel` is anything other than one constant value.
    pub const fn varies(self, channel: SurfaceChannel) -> bool {
        (self.varying_channels & channel.bit()) != 0
    }

    /// Whether any surface in the tree displaces its geometry — the one
    /// requirement that concerns a **vertex** stage rather than a fragment one.
    pub const fn has_displacement(self) -> bool {
        self.has_displacement
    }

    /// How many parameter slots the bound graphs hold in total. Retuning any of
    /// them cannot move [`Surface::digest`], which is what this count exists to
    /// make budgetable.
    pub const fn param_count(self) -> u16 {
        self.param_count
    }

    /// How many operator nodes the bound graphs hold in total.
    pub const fn node_count(self) -> u16 {
        self.node_count
    }

    /// Absorb a graph's inputs and its node and parameter counts. A constant
    /// binding contributes nothing: it is not a graph a backend has to lower.
    fn absorb_graph(self, graph: Option<&FieldGraph>) -> SurfaceRequirements {
        graph.map_or(self, |graph| SurfaceRequirements {
            inputs: self.inputs.union(inputs_of(graph)),
            param_count: self.param_count.saturating_add(graph.params().len() as u16),
            node_count: self.node_count.saturating_add(graph.node_count() as u16),
            ..self
        })
    }

    /// Mark `channel` varying, and record a displacement.
    fn mark(self, channel: SurfaceChannel, varies: bool, displaces: bool) -> SurfaceRequirements {
        SurfaceRequirements {
            varying_channels: self.varying_channels | [0, channel.bit()][usize::from(varies)],
            has_displacement: self.has_displacement | displaces,
            ..self
        }
    }
}

/// Which context sources a graph reads.
fn inputs_of(graph: &FieldGraph) -> SurfaceInput {
    SurfaceInput(graph.recipe().nodes().iter().fold(0_u16, |bits, node| {
        INPUT_OPS.iter().fold(bits, |bits, (code, bit)| {
            bits | [0, *bit][usize::from(node.op() == *code)]
        })
    }))
}

/// Derive the requirements of a whole layer tree.
///
/// **The varying-channel rule, stated once.** A channel varies when some
/// surface in the tree binds it to a field, **or** when some layer's mask is a
/// field — because every blend rule makes each channel a function of the mask.
/// That second clause is exact for [`crate::LayerBlend::Add`] and conservative
/// for the other two in the one case where the composed constants happen to
/// agree; a backend is never told a channel is constant when it is not.
pub(crate) fn requirements(root: &Surface) -> SurfaceRequirements {
    let nodes = layer_tree::linearize(root);
    let masked = nodes.iter().any(|node| {
        node.layer
            .is_some_and(|layer| layer.mask().as_field().is_some())
    });
    nodes.iter().fold(SurfaceRequirements::EMPTY, |summary, node| {
        let with_mask = node
            .layer
            .map_or(summary, |layer| summary.absorb_graph(layer.mask().as_field()));
        SurfaceChannel::ALL.iter().fold(with_mask, |summary, channel| {
            let binding = node.surface.binding(*channel);
            let constant = binding.as_constant();
            summary.absorb_graph(binding.as_field()).mark(
                *channel,
                constant.is_none() | masked,
                (*channel == SurfaceChannel::Displacement)
                    & (constant != Some(channel.default_value())),
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::ChannelBinding;
    use crate::layer::{LayerBlend, SurfaceLayer};
    use crate::surface_builder::SurfaceBuilder;
    use axiom_field::{FieldBuilder, FieldId, FieldType, FieldValue};
    use axiom_math::Vec3;
    use axiom_recipe::{Param, Scalar};

    fn uv_scalar() -> FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("surface/req/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        builder.build(lane)
    }

    fn time_and_point_vec3() -> FieldGraph {
        let (builder, point) = FieldBuilder::new(FieldId::of_name("surface/req/pt"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (builder, time) = builder.push(FieldOp::Time, Vec::new(), Vec::new());
        let (builder, sum) = builder.push(FieldOp::Add, Vec::new(), vec![point, time]);
        builder.build(sum)
    }

    #[test]
    fn an_input_set_is_a_bitset() {
        assert_eq!(SurfaceInput::NONE.bits(), 0);
        assert_eq!(SurfaceInput::POINT.bits(), 1);
        assert_eq!(SurfaceInput::UV.bits(), 2);
        assert_eq!(SurfaceInput::NORMAL.bits(), 4);
        assert_eq!(SurfaceInput::TIME.bits(), 8);
        let both = SurfaceInput::POINT.union(SurfaceInput::TIME);
        assert_eq!(both.bits(), 9);
        assert!(both.contains(SurfaceInput::POINT));
        assert!(both.contains(SurfaceInput::TIME));
        assert!(!both.contains(SurfaceInput::UV));
        assert!(both.contains(SurfaceInput::NONE));
        assert_ne!(both, SurfaceInput::POINT);
    }

    #[test]
    fn an_all_constant_surface_requires_nothing() {
        let summary = SurfaceBuilder::new()
            .build()
            .expect("a default surface is legal")
            .requirements();
        assert_eq!(summary, SurfaceRequirements::EMPTY);
        assert_eq!(summary.inputs(), SurfaceInput::NONE);
        assert_eq!(summary.varying_channels(), 0);
        assert!(!summary.has_displacement());
        assert_eq!(summary.param_count(), 0);
        assert_eq!(summary.node_count(), 0);
        SurfaceChannel::ALL
            .iter()
            .for_each(|channel| assert!(!summary.varies(*channel)));
    }

    #[test]
    fn a_surface_reading_only_uv_does_not_claim_point() {
        let summary = SurfaceBuilder::new()
            .field(SurfaceChannel::Roughness, uv_scalar())
            .build()
            .expect("a scalar uv field is a legal roughness")
            .requirements();
        assert_eq!(summary.inputs(), SurfaceInput::UV);
        assert!(!summary.inputs().contains(SurfaceInput::POINT));
        assert!(summary.varies(SurfaceChannel::Roughness));
        assert!(!summary.varies(SurfaceChannel::BaseColor));
        assert_eq!(summary.varying_channels(), SurfaceChannel::Roughness.bit());
        assert_eq!(summary.node_count(), 2);
        assert_eq!(summary.param_count(), 0);
    }

    #[test]
    fn every_context_source_a_graph_reads_is_reported() {
        let summary = SurfaceBuilder::new()
            .field(SurfaceChannel::Displacement, time_and_point_vec3())
            .build()
            .expect("a vec3 field is a legal displacement")
            .requirements();
        assert_eq!(
            summary.inputs(),
            SurfaceInput::POINT.union(SurfaceInput::TIME)
        );
        assert!(summary.has_displacement());
        assert_eq!(summary.node_count(), 3);
    }

    #[test]
    fn a_non_zero_constant_displacement_still_counts_as_displacement() {
        let summary = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(Vec3::new(0.0, 1.0, 0.0)),
            )
            .build()
            .expect("a vec3 constant is a legal displacement")
            .requirements();
        assert!(summary.has_displacement());
        assert!(!summary.varies(SurfaceChannel::Displacement));
        assert_eq!(summary.inputs(), SurfaceInput::NONE);
    }

    #[test]
    fn parameters_of_a_bound_graph_are_counted() {
        let (builder, slot) = FieldBuilder::new(FieldId::of_name("surface/req/tuned"), 1)
            .declare("tint", FieldValue::scalar(Scalar::new(0.25)));
        let (builder, node) = builder.push_param(slot, FieldType::Scalar);
        let summary = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(node))
            .build()
            .expect("a scalar param field is a legal opacity")
            .requirements();
        assert_eq!(summary.param_count(), 1);
        assert_eq!(summary.node_count(), 1);
        assert_eq!(summary.inputs(), SurfaceInput::NONE);
    }

    #[test]
    fn a_field_mask_makes_every_channel_vary_and_is_itself_counted() {
        let layer = SurfaceLayer::new(
            SurfaceBuilder::new().build().expect("legal"),
            ChannelBinding::field(uv_scalar()),
            LayerBlend::Over,
        );
        let summary = SurfaceBuilder::new()
            .layer(layer)
            .build()
            .expect("one masked layer is within budget")
            .requirements();
        assert_eq!(summary.inputs(), SurfaceInput::UV);
        assert_eq!(summary.node_count(), 2);
        SurfaceChannel::ALL
            .iter()
            .for_each(|channel| assert!(summary.varies(*channel)));
    }

    #[test]
    fn a_constant_mask_leaves_constant_channels_constant() {
        let layer = SurfaceLayer::new(
            SurfaceBuilder::new().build().expect("legal"),
            SurfaceLayer::opaque_mask(),
            LayerBlend::Multiply,
        );
        let summary = SurfaceBuilder::new()
            .layer(layer)
            .build()
            .expect("one masked layer is within budget")
            .requirements();
        assert_eq!(summary, SurfaceRequirements::EMPTY);
    }
}
