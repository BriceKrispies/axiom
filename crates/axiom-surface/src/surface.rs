//! The appearance artifact itself.

use crate::surface_kind::SurfaceKind;
use axiom_field::FieldType;
use axiom_kernel::{SchemaVersion, StableHash};

use crate::binding::ChannelBinding;
use crate::channel::{SurfaceChannel, SURFACE_CHANNEL_COUNT};
use crate::flatten;
use crate::layer::{SurfaceLayer, MAX_LAYERS};
use crate::layer_tree;
use crate::lighting_model::LightingModel;
use crate::requirements::{self, SurfaceRequirements};
use crate::surface_bytes;
use crate::surface_error::{SurfaceError, SurfaceErrorCode, SurfaceResult};

/// The wire-format version stamped into every serialized surface. Bumping it
/// deliberately changes the bytes (and therefore every digest and golden), so a
/// format change can never be silent.
/// Bumped to 2.0 when the canonical bytes gained a surface-kind code in their
/// header (see [`crate::SurfaceKind`]). The layout changed, so a 1.0 reader
/// would misparse a 2.0 buffer from the very next field — a major bump, not a
/// minor one.
pub const SURFACE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(2, 0);

/// The surface tree holds more layers than the budget allows.
const LAYER_BUDGET_EXCEEDED: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::LayerBudgetExceeded,
    "the surface tree holds more layers than the layer budget allows",
);

/// A channel binding produces a different type than the channel declares.
const CHANNEL_TYPE_MISMATCH: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::ChannelTypeMismatch,
    "the channel binding produces a different type than the channel declares",
);

/// A layer mask is not a scalar.
const MASK_TYPE_MISMATCH: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::MaskTypeMismatch,
    "a layer mask must be a scalar field or constant",
);

/// The engine's neutral appearance artifact.
///
/// Seven named channels, each bound to a constant or to a field expression; one
/// [`LightingModel`] discriminant; and up to [`MAX_LAYERS`] mask-driven layers,
/// each of which is itself a `Surface`. It names no shader, stage, binding,
/// varying, pipeline, backend, mesh, entity or asset — those are all somebody
/// else's vocabulary.
///
/// **Channel graphs are evaluated in object space.** A surface's field
/// expressions read `EvalContext::point` as a position in the *object's own*
/// frame, which is what makes a noise pattern ride with the object instead of
/// swimming as it moves. See this crate's `ARCHITECTURE.md`.
///
/// **A `Surface` is preparation-time data.** It holds graphs and a `Vec`; it is
/// addressed by identity after preparation and must never be cloned per frame.
///
/// **Every `Surface` value is legal.** The two constructors —
/// [`crate::SurfaceBuilder::build`] and [`Surface::deserialize`] — both validate,
/// so a `Surface` in hand always types, always fits the layer budget, and always
/// has scalar masks.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    bindings: [ChannelBinding; SURFACE_CHANNEL_COUNT],
    lighting: LightingModel,
    layers: Vec<SurfaceLayer>,
    /// Which program this surface names — see [`crate::SurfaceKind`]. `Field`
    /// for every surface authored from channel bindings, which is the default
    /// and almost all of them.
    kind: SurfaceKind,
}

impl Surface {
    /// Assemble a surface from its parts. Authoring goes through
    /// [`crate::SurfaceBuilder`], which is what validates it.
    pub(crate) fn new(
        bindings: [ChannelBinding; SURFACE_CHANNEL_COUNT],
        lighting: LightingModel,
        layers: Vec<SurfaceLayer>,
    ) -> Self {
        Surface::of_kind(bindings, lighting, layers, SurfaceKind::Field)
    }

    /// [`Self::new`], naming the kind explicitly.
    pub(crate) fn of_kind(
        bindings: [ChannelBinding; SURFACE_CHANNEL_COUNT],
        lighting: LightingModel,
        layers: Vec<SurfaceLayer>,
        kind: SurfaceKind,
    ) -> Self {
        Surface {
            bindings,
            lighting,
            layers,
            kind,
        }
    }

    /// Which program this surface names.
    ///
    /// A backend reads this to decide whether to *generate* WGSL from the
    /// channel bindings or to splice its hand-written runtime material shader.
    pub fn kind(&self) -> SurfaceKind {
        self.kind
    }

    /// What one channel is bound to.
    pub fn binding(&self, channel: SurfaceChannel) -> &ChannelBinding {
        &self.bindings[channel.index()]
    }

    /// Every channel's binding, in [`SurfaceChannel`] code order.
    pub fn bindings(&self) -> &[ChannelBinding] {
        &self.bindings
    }

    /// How this surface participates in lighting.
    pub const fn lighting(&self) -> LightingModel {
        self.lighting
    }

    /// The layers composed onto this surface, in application order.
    pub fn layers(&self) -> &[SurfaceLayer] {
        &self.layers
    }

    /// Prove the surface is a legal appearance description: the layer tree fits
    /// the budget, every binding produces the type its channel declares, every
    /// bound graph is a well-formed well-typed field, and every mask is a
    /// scalar. Each rejection names the channel and the layer index it concerns.
    pub fn validate(&self) -> SurfaceResult<()> {
        let nodes = layer_tree::linearize(self);
        (nodes.len() <= MAX_LAYERS + 1)
            .then_some(())
            .ok_or_else(|| LAYER_BUDGET_EXCEEDED.about_layer((MAX_LAYERS + 1) as u16))
            .and_then(|()| {
                nodes.iter().enumerate().try_fold((), |(), (index, node)| {
                    let at = index as u16;
                    validate_bindings(node.surface, at)
                        .and_then(|()| validate_mask(node.layer.map(SurfaceLayer::mask), at))
                })
            })
    }

    /// What a backend must satisfy to render this surface. Derived from the
    /// whole layer tree, never authored — see [`SurfaceRequirements`].
    pub fn requirements(&self) -> SurfaceRequirements {
        requirements::requirements(self)
    }

    /// Resolve the layer tree into one surface with no layers, whose every
    /// channel is a single binding — the form a backend lowers.
    ///
    /// A pure function of the surface, order-stable and idempotent: flattening a
    /// flat surface returns it unchanged.
    pub fn flatten(&self) -> SurfaceResult<Surface> {
        flatten::flatten(self)
    }

    /// The surface's canonical bytes: the schema stamp, then the layer tree
    /// linearised into one record per surface, each carrying its parent index,
    /// its blend, its lighting model, its mask and its seven bindings.
    pub fn serialize(&self) -> Vec<u8> {
        surface_bytes::serialize(self)
    }

    /// The surface's **structural** digest.
    ///
    /// It folds the schema stamp, the tree shape, every blend and lighting
    /// model, every binding's kind and constant, and every bound graph's own
    /// structural digest — which deliberately excludes that graph's parameter
    /// *values* and includes each slot's declared *type*. So **retuning a
    /// parameter does not move a surface's digest**, and that is the property
    /// that makes the digest a safe program-cache key: a material tweak cannot
    /// force a recompile, and parameter animation cannot explode into variants.
    ///
    /// A channel bound to a *constant* is structure, exactly as a field `Const`
    /// node is — changing it does move the digest. To retune a channel without
    /// moving the digest, bind it to a one-node parameter field.
    ///
    /// **The bytes are the determinism proof; the digest is the label.** Compare
    /// [`Surface::serialize`] output to prove two surfaces identical, and use
    /// the digest to index and locate them.
    pub fn digest(&self) -> StableHash {
        surface_bytes::digest(self)
    }

    /// Decode and validate a surface produced by [`Self::serialize`].
    ///
    /// Bounds-checked throughout and never panics: a buffer truncated at *any*
    /// prefix length fails cleanly, an unknown blend, lighting model, binding
    /// kind or type code fails, a parent link that is not a tree fails, and a
    /// tree over the layer budget fails.
    pub fn deserialize(bytes: &[u8]) -> SurfaceResult<Surface> {
        surface_bytes::deserialize(bytes)
    }
}

/// Every channel of one surface produces the type its channel declares, and
/// every bound graph is a legal field.
fn validate_bindings(surface: &Surface, at: u16) -> SurfaceResult<()> {
    SurfaceChannel::ALL.iter().try_fold((), |(), channel| {
        let binding = surface.binding(*channel);
        validate_graph(binding)
            .and_then(|()| binding.ty())
            .and_then(|ty| {
                (ty == channel.ty())
                    .then_some(())
                    .ok_or(CHANNEL_TYPE_MISMATCH)
            })
            .map_err(|error| error.about_channel(*channel).about_layer(at))
    })
}

/// A layer's mask is a legal scalar field or constant. The root has no mask.
fn validate_mask(mask: Option<&ChannelBinding>, at: u16) -> SurfaceResult<()> {
    mask.map_or(Ok(()), |mask| {
        validate_graph(mask)
            .and_then(|()| mask.ty())
            .and_then(|ty| {
                (ty == FieldType::Scalar)
                    .then_some(())
                    .ok_or(MASK_TYPE_MISMATCH)
            })
            .map_err(|error| error.about_layer(at))
    })
}

/// A bound graph, if there is one, is a well-formed well-typed field.
fn validate_graph(binding: &ChannelBinding) -> SurfaceResult<()> {
    binding.as_field().map_or(Ok(()), |graph| {
        graph.validate().map_err(SurfaceError::from_field)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::LayerBlend;
    use crate::surface_builder::SurfaceBuilder;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldValue, NodeId, Scalar};
    use axiom_math::{Vec3, Vec4};

    fn plain() -> Surface {
        SurfaceBuilder::new().build().expect("a default surface is legal")
    }

    fn opaque_layer(surface: Surface) -> SurfaceLayer {
        SurfaceLayer::new(surface, SurfaceLayer::opaque_mask(), LayerBlend::Over)
    }

    #[test]
    fn a_default_surface_reports_every_channel_default_and_the_default_lighting() {
        let surface = plain();
        SurfaceChannel::ALL.iter().for_each(|channel| {
            assert_eq!(
                surface.binding(*channel).as_constant(),
                Some(channel.default_value())
            );
        });
        assert_eq!(surface.bindings().len(), SURFACE_CHANNEL_COUNT);
        assert_eq!(surface.lighting(), LightingModel::LambertSpecular);
        assert!(surface.layers().is_empty());
        assert_eq!(surface.validate(), Ok(()));
        assert_eq!(SURFACE_SCHEMA_VERSION, SchemaVersion::new(2, 0));
    }

    #[test]
    fn a_channel_bound_to_the_wrong_type_names_the_channel_and_the_layer() {
        let error = SurfaceBuilder::new()
            .constant(SurfaceChannel::Roughness, FieldValue::vec3(Vec3::ZERO))
            .build()
            .expect_err("roughness is a scalar");
        assert_eq!(error.kind(), SurfaceErrorCode::ChannelTypeMismatch);
        assert_eq!(error.channel(), Some(SurfaceChannel::Roughness));
        assert_eq!(error.layer(), Some(0));
    }

    #[test]
    fn a_wrong_type_inside_a_layer_names_that_layer() {
        let bad = SurfaceBuilder::new()
            .constant(SurfaceChannel::BaseColor, FieldValue::scalar(Scalar::new(1.0)))
            .build_unchecked();
        let error = SurfaceBuilder::new()
            .layer(opaque_layer(plain()))
            .layer(opaque_layer(bad))
            .build()
            .expect_err("the second layer's base colour is not a vec4");
        assert_eq!(error.kind(), SurfaceErrorCode::ChannelTypeMismatch);
        assert_eq!(error.channel(), Some(SurfaceChannel::BaseColor));
        assert_eq!(error.layer(), Some(2));
    }

    #[test]
    fn a_non_scalar_mask_is_rejected_at_its_layer() {
        let error = SurfaceBuilder::new()
            .layer(SurfaceLayer::new(
                plain(),
                ChannelBinding::constant(FieldValue::vec4(Vec4::ONE)),
                LayerBlend::Add,
            ))
            .build()
            .expect_err("a mask must be a scalar");
        assert_eq!(error.kind(), SurfaceErrorCode::MaskTypeMismatch);
        assert_eq!(error.layer(), Some(1));
    }

    #[test]
    fn a_bound_graph_that_does_not_type_is_rejected_with_the_field_diagnostic() {
        let (builder, node) =
            FieldBuilder::new(FieldId::from_raw(5), 1).push(FieldOp::Dot, Vec::new(), Vec::new());
        let error = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(node))
            .build()
            .expect_err("Dot with no inputs does not type");
        assert_eq!(error.kind(), SurfaceErrorCode::InvalidField);
        assert_eq!(error.channel(), Some(SurfaceChannel::Opacity));
        assert_eq!(error.node(), NodeId::from_raw(0));
    }

    #[test]
    fn a_mask_graph_that_does_not_type_is_rejected_at_its_layer() {
        let (builder, node) = FieldBuilder::new(FieldId::from_raw(6), 1).push(
            FieldOp::Normalize,
            Vec::new(),
            Vec::new(),
        );
        let error = SurfaceBuilder::new()
            .layer(SurfaceLayer::new(
                plain(),
                ChannelBinding::field(builder.build(node)),
                LayerBlend::Over,
            ))
            .build()
            .expect_err("Normalize with no input does not type");
        assert_eq!(error.kind(), SurfaceErrorCode::InvalidField);
        assert_eq!(error.layer(), Some(1));
    }

    #[test]
    fn a_tree_over_the_layer_budget_is_rejected_not_truncated() {
        let error = (0..=MAX_LAYERS)
            .fold(SurfaceBuilder::new(), |builder, _| {
                builder.layer(opaque_layer(plain()))
            })
            .build()
            .expect_err("five layers exceed the budget of four");
        assert_eq!(error.kind(), SurfaceErrorCode::LayerBudgetExceeded);
        assert_eq!(error.layer(), Some((MAX_LAYERS + 1) as u16));
    }

    #[test]
    fn exactly_the_budget_is_allowed() {
        let surface = (0..MAX_LAYERS)
            .fold(SurfaceBuilder::new(), |builder, _| {
                builder.layer(opaque_layer(plain()))
            })
            .build()
            .expect("four layers are exactly the budget");
        assert_eq!(surface.layers().len(), MAX_LAYERS);
    }
}
