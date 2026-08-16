//! Reading a surface, and answering "will this render there?" without rendering.
//!
//! Two things live here, and they are the same audience's two questions.
//!
//! **What is this surface made of?** [`Surface::inspect`] answers per channel:
//! what it is bound to, the type it produces, how large the bound graph is, how
//! many knobs it carries, and that graph's own structural digest. From a channel
//! an agent drills into the graph itself, where `axiom_field`'s own inspection
//! takes over — this layer adds the channel vocabulary and nothing else.
//!
//! **Will a backend render it?** [`supported_by`] checks a surface's derived
//! [`SurfaceRequirements`] against a backend's declared capability profile. It
//! is a **pure query**: no device, no program, no render. That is the last item
//! on the agent checklist — an author can be told "the software rasterizer will
//! fall back to your constants" before a frame is ever drawn.
//!
//! ## What `supported_by` is, and precisely what it is not
//!
//! It answers the **capability** question and only that: does this surface need
//! a program at all, and does the profile attempt one? A backend's own gate
//! additionally checks ceilings that are properties of *that backend* — how many
//! parameters its shared uniform region holds, which interstage lanes its main
//! pass carries, how many nodes its shader budget allows, and which vertex stage
//! a particular draw uses. None of those are derivable from a backend-neutral
//! requirements summary, and inventing numbers for them here would be a second,
//! drifting definition of somebody else's limits.
//!
//! So: a `false` from this query is final — that backend will not run the
//! program. A `true` means the surface clears the capability gate, and the
//! backend still owns its own ceilings. That is the honest contract, and it is
//! the one an agent can act on.

use axiom_field::{FieldGraph, FieldType};
use axiom_host::{BackendCapabilityProfile, RenderCapability};
use axiom_kernel::StableHash;

use crate::binding::ChannelBinding;
use crate::channel::SurfaceChannel;
use crate::layer_tree;
use crate::lighting_model::LightingModel;
use crate::requirements::SurfaceRequirements;
use crate::surface::Surface;
use crate::surface_error::SurfaceResult;

/// Whether a backend with this capability profile will render a surface with
/// these requirements, or fall back to its constants.
///
/// A surface that needs **no program at all** — every channel a plain constant,
/// no displacement — is supported by every profile, including an empty one.
/// There is nothing for a capability to gate: such a surface is an ordinary
/// material, and reporting it as unsupported would be telling a backend it lost
/// something it never asked for.
pub fn supported_by(reqs: &SurfaceRequirements, profile: BackendCapabilityProfile) -> bool {
    !reqs.needs_program() | profile.contains(RenderCapability::ProceduralSurface)
}

/// One channel of a surface, as an agent reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelInspection {
    channel: SurfaceChannel,
    kind: u16,
    ty: FieldType,
    node_count: u16,
    param_count: u16,
    graph_digest: Option<StableHash>,
}

impl ChannelInspection {
    /// Which channel this row describes.
    pub const fn channel(self) -> SurfaceChannel {
        self.channel
    }

    /// The binding kind — [`ChannelBinding::KIND_CONSTANT`] or
    /// [`ChannelBinding::KIND_FIELD`].
    pub const fn kind(self) -> u16 {
        self.kind
    }

    /// Whether the channel is one constant value.
    pub const fn is_constant(self) -> bool {
        self.kind == ChannelBinding::KIND_CONSTANT
    }

    /// The type the channel's binding produces. Always the channel's declared
    /// type for a surface that validates.
    pub const fn ty(self) -> FieldType {
        self.ty
    }

    /// How many operator nodes the bound graph holds — zero for a constant.
    pub const fn node_count(self) -> u16 {
        self.node_count
    }

    /// How many parameter slots the bound graph holds — zero for a constant.
    pub const fn param_count(self) -> u16 {
        self.param_count
    }

    /// The bound graph's own structural digest, or `None` for a constant.
    pub const fn graph_digest(self) -> Option<StableHash> {
        self.graph_digest
    }
}

/// A whole surface, as an agent reads it: every channel, the lighting model, how
/// many layers the tree holds, the derived requirements, and the surface's own
/// structural digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceInspection {
    channels: Vec<ChannelInspection>,
    lighting: LightingModel,
    layer_count: u16,
    requirements: SurfaceRequirements,
    digest: StableHash,
}

impl SurfaceInspection {
    /// Every channel's row, in [`SurfaceChannel`] code order.
    pub fn channels(&self) -> &[ChannelInspection] {
        &self.channels
    }

    /// How this surface participates in lighting.
    pub const fn lighting(&self) -> LightingModel {
        self.lighting
    }

    /// How many layers the whole tree holds, not counting the root.
    pub const fn layer_count(&self) -> u16 {
        self.layer_count
    }

    /// What a backend must satisfy to render the surface.
    pub const fn requirements(&self) -> SurfaceRequirements {
        self.requirements
    }

    /// The surface's structural digest — the program-cache key, the label, never
    /// the proof.
    pub const fn digest(&self) -> StableHash {
        self.digest
    }
}

impl Surface {
    /// Read the surface: every channel's binding, its type and its size, plus
    /// the lighting model, the layer count, the derived requirements and the
    /// structural digest.
    ///
    /// Only the **root** surface's channels are reported; a layer's own channels
    /// are read by inspecting that layer's surface, and the resolved single
    /// binding per channel is [`Surface::flatten`] followed by `inspect`.
    ///
    /// Fails when a channel's bound graph does not type, and the failure names
    /// **both** the channel and the node of that channel's graph — which is the
    /// diagnostic an agent needs to act, rather than a line of generated shader.
    ///
    /// Preparation-time only: it type-checks every bound graph.
    pub fn inspect(&self) -> SurfaceResult<SurfaceInspection> {
        SurfaceChannel::ALL
            .iter()
            .try_fold(Vec::new(), |mut rows, channel| {
                inspect_channel(self.binding(*channel), *channel).map(|row| {
                    rows.push(row);
                    rows
                })
            })
            .map(|channels| SurfaceInspection {
                channels,
                lighting: self.lighting(),
                layer_count: (layer_tree::linearize(self).len().saturating_sub(1)) as u16,
                requirements: self.requirements(),
                digest: self.digest(),
            })
    }
}

/// One channel's row, or the located failure that reading it produced.
fn inspect_channel(
    binding: &ChannelBinding,
    channel: SurfaceChannel,
) -> SurfaceResult<ChannelInspection> {
    binding
        .ty()
        .map_err(|error| error.about_channel(channel))
        .map(|ty| ChannelInspection {
            channel,
            kind: binding.kind(),
            ty,
            node_count: binding
                .as_field()
                .map_or(0, |graph| graph.node_count() as u16),
            param_count: binding
                .as_field()
                .map_or(0, |graph| graph.params().len() as u16),
            graph_digest: binding.as_field().map(FieldGraph::digest),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldType, FieldValue, NodeId, Param};
    use axiom_math::Vec3;

    use crate::layer::{LayerBlend, SurfaceLayer};
    use crate::surface_builder::SurfaceBuilder;
    use crate::surface_error::SurfaceErrorCode;

    /// A profile that does attempt an authored surface's program.
    fn attempting() -> BackendCapabilityProfile {
        BackendCapabilityProfile::all().with(RenderCapability::ProceduralSurface)
    }

    /// A profile that renders, but never attempts an authored surface's program
    /// — the software rasterizer's relationship to one, in one value.
    fn declining() -> BackendCapabilityProfile {
        BackendCapabilityProfile::all().without(RenderCapability::ProceduralSurface)
    }

    /// `uv.x` — a scalar field over the surface parameterisation.
    fn uv_scalar() -> axiom_field::FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("surface/inspect/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        builder.build(lane)
    }

    fn plain() -> Surface {
        SurfaceBuilder::new()
            .build()
            .expect("a default surface is legal")
    }

    #[test]
    fn an_all_constant_surface_reports_every_channel_as_a_constant() {
        let read = plain().inspect().expect("a default surface types");
        assert_eq!(read.channels().len(), 7);
        assert_eq!(read.layer_count(), 0);
        assert_eq!(read.lighting(), LightingModel::LambertSpecular);
        assert_eq!(read.digest(), plain().digest());
        assert_eq!(read.requirements(), SurfaceRequirements::EMPTY);
        read.channels()
            .iter()
            .zip(SurfaceChannel::ALL.iter())
            .for_each(|(row, channel)| {
                assert_eq!(row.channel(), *channel);
                assert!(row.is_constant());
                assert_eq!(row.kind(), ChannelBinding::KIND_CONSTANT);
                assert_eq!(row.ty(), channel.ty());
                assert_eq!(row.node_count(), 0);
                assert_eq!(row.param_count(), 0);
                assert_eq!(row.graph_digest(), None);
            });
    }

    #[test]
    fn a_bound_channel_reports_its_graphs_size_and_digest() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Roughness, uv_scalar())
            .build()
            .expect("a scalar uv field is a legal roughness");
        let read = surface.inspect().expect("it types");
        let row = read.channels()[SurfaceChannel::Roughness.index()];
        assert!(!row.is_constant());
        assert_eq!(row.kind(), ChannelBinding::KIND_FIELD);
        assert_eq!(row.ty(), FieldType::Scalar);
        assert_eq!(row.node_count(), 2);
        assert_eq!(row.param_count(), 0);
        assert_eq!(row.graph_digest(), Some(uv_scalar().digest()));
        assert!(read.requirements().needs_program());
        // A row is a value: it compares and prints.
        assert_ne!(row, read.channels()[SurfaceChannel::Metallic.index()]);
        assert!(format!("{read:?}").contains("Roughness"));
    }

    #[test]
    fn the_layer_count_is_the_whole_tree_not_just_the_root() {
        let inner = SurfaceBuilder::new()
            .layer(SurfaceLayer::new(
                plain(),
                SurfaceLayer::opaque_mask(),
                LayerBlend::Over,
            ))
            .build()
            .expect("one layer is within budget");
        let outer = SurfaceBuilder::new()
            .layer(SurfaceLayer::new(
                inner,
                SurfaceLayer::opaque_mask(),
                LayerBlend::Add,
            ))
            .build()
            .expect("two layers are within budget");
        assert_eq!(outer.inspect().expect("it types").layer_count(), 2);
    }

    #[test]
    fn a_channel_whose_graph_does_not_type_names_the_channel_and_the_node() {
        let (builder, node) = FieldBuilder::new(FieldId::from_raw(9), 1).push(
            FieldOp::Dot,
            Vec::new(),
            Vec::new(),
        );
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(node))
            .build_unchecked();
        let error = surface
            .inspect()
            .expect_err("Dot with no inputs does not type");
        assert_eq!(error.kind(), SurfaceErrorCode::InvalidField);
        assert_eq!(error.channel(), Some(SurfaceChannel::Opacity));
        assert_eq!(error.node(), NodeId::from_raw(0));
    }

    #[test]
    fn a_surface_that_needs_no_program_is_supported_by_every_profile() {
        let reqs = plain().requirements();
        assert!(!reqs.needs_program());
        assert!(supported_by(&reqs, attempting()));
        assert!(supported_by(&reqs, declining()));
        assert!(supported_by(&reqs, BackendCapabilityProfile::none()));
    }

    #[test]
    fn a_surface_that_needs_a_program_is_supported_only_where_one_is_attempted() {
        let reqs = SurfaceBuilder::new()
            .field(SurfaceChannel::Roughness, uv_scalar())
            .build()
            .expect("a scalar uv field is a legal roughness")
            .requirements();
        assert!(reqs.needs_program());
        assert!(supported_by(&reqs, attempting()));
        assert!(!supported_by(&reqs, declining()));
    }

    #[test]
    fn a_displacement_needs_a_program_even_when_it_is_a_plain_constant() {
        // A constant non-zero displacement still moves vertices, so it still
        // needs a vertex stage — which is why the query keys on the whole
        // requirements summary and not on the varying-channel bitset alone.
        let reqs = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(Vec3::new(0.0, 1.0, 0.0)),
            )
            .build()
            .expect("a vec3 constant is a legal displacement")
            .requirements();
        assert!(!reqs.varies(SurfaceChannel::Displacement));
        assert!(reqs.has_displacement());
        assert!(reqs.needs_program());
        assert!(!supported_by(&reqs, declining()));
    }
}
