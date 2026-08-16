//! Resolving a layer tree into one surface with no layers.
//!
//! Flattening is a **reverse fold over the breadth-first linearisation**. A
//! parent's index is always strictly smaller than its children's, so walking the
//! list backwards reaches every surface with all of its layers already resolved
//! — one pass, no recursion, no worklist that can grow, and no order a future
//! change can accidentally drift.

use crate::binding::ChannelBinding;
use crate::channel::{SurfaceChannel, SURFACE_CHANNEL_COUNT};
use crate::compose;
use crate::layer::LayerBlend;
use crate::layer_tree;
use crate::surface::Surface;
use crate::surface_error::SurfaceResult;

/// One surface's seven resolved channels.
type Bindings = [ChannelBinding; SURFACE_CHANNEL_COUNT];

/// Resolve `root` and every surface layered onto it into a single surface whose
/// channels are one binding each and whose layer list is empty.
///
/// The result keeps the **root's** lighting model: a layer contributes channel
/// values, not a way of participating in lighting, and one draw has one lighting
/// model.
pub(crate) fn flatten(root: &Surface) -> SurfaceResult<Surface> {
    let nodes = layer_tree::linearize(root);
    let count = nodes.len();
    let unresolved: Vec<Option<Bindings>> = (0..count).map(|_| None).collect();
    (0..count)
        .rev()
        .try_fold(unresolved, |mut resolved, index| {
            let contributions: Vec<(Bindings, LayerBlend, &ChannelBinding)> = ((index + 1)..count)
                .filter(|child| nodes[*child].parent == index)
                .filter_map(|child| {
                    resolved[child].take().and_then(|bindings| {
                        nodes[child]
                            .layer
                            .map(|layer| (bindings, layer.blend(), layer.mask()))
                    })
                })
                .collect();
            contributions
                .into_iter()
                .try_fold(
                    own_bindings(nodes[index].surface),
                    |under, (over, blend, mask)| blend_all(&under, &over, mask, blend),
                )
                .map(|merged| {
                    resolved[index] = Some(merged);
                    resolved
                })
        })
        .map(|mut resolved| {
            let bindings = resolved[0]
                .take()
                .expect("the root is resolved last, so it is always present");
            Surface::new(bindings, root.lighting(), Vec::new())
        })
}

/// A surface's own seven bindings, before anything is layered onto them.
fn own_bindings(surface: &Surface) -> Bindings {
    surface
        .bindings()
        .to_vec()
        .try_into()
        .expect("a surface always holds exactly seven bindings")
}

/// Compose every channel of one layer into the channels under it.
fn blend_all(
    under: &Bindings,
    over: &Bindings,
    mask: &ChannelBinding,
    blend: LayerBlend,
) -> SurfaceResult<Bindings> {
    SurfaceChannel::ALL
        .iter()
        .try_fold(
            Vec::with_capacity(SURFACE_CHANNEL_COUNT),
            |mut merged, channel| {
                compose::blend(
                    &under[channel.index()],
                    &over[channel.index()],
                    mask,
                    blend,
                    *channel,
                )
                .map(|binding| {
                    merged.push(binding);
                    merged
                })
            },
        )
        .map(|merged| {
            merged
                .try_into()
                .expect("seven channels in, seven channels out")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::SurfaceLayer;
    use crate::lighting_model::LightingModel;
    use crate::surface_builder::SurfaceBuilder;
    use axiom_field::{EvalContext, FieldBuilder, FieldGraph, FieldId, FieldOp, FieldValue, Param, Scalar};
    use axiom_math::{Vec2, Vec3};

    fn scalar(value: f32) -> FieldValue {
        FieldValue::scalar(Scalar::new(value))
    }

    fn roughness_layer(value: f32, mask: f32, blend: LayerBlend) -> SurfaceLayer {
        SurfaceLayer::new(
            SurfaceBuilder::new()
                .constant(SurfaceChannel::Roughness, scalar(value))
                .build()
                .expect("a scalar constant is a legal roughness"),
            ChannelBinding::constant(scalar(mask)),
            blend,
        )
    }

    /// `uv.x` — the varying the flattening tests ride on.
    fn uv_x() -> FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("surface/flatten/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        builder.build(lane)
    }

    fn at(u: f32) -> EvalContext {
        EvalContext::at(Vec3::ZERO, Vec2::new(u, 0.0), Vec3::UNIT_Y)
    }

    #[test]
    fn a_surface_with_no_layers_flattens_to_itself() {
        let surface = SurfaceBuilder::new()
            .constant(SurfaceChannel::Roughness, scalar(0.25))
            .lighting(LightingModel::Unlit)
            .build()
            .expect("legal");
        let flat = surface.flatten().expect("nothing to compose");
        assert_eq!(flat, surface);
        assert_eq!(flat.lighting(), LightingModel::Unlit);
        assert!(flat.layers().is_empty());
    }

    #[test]
    fn three_constant_layers_compose_in_order() {
        let flat = SurfaceBuilder::new()
            .constant(SurfaceChannel::Roughness, scalar(0.25))
            .layer(roughness_layer(0.5, 0.5, LayerBlend::Over))
            .layer(roughness_layer(0.75, 0.25, LayerBlend::Over))
            .layer(roughness_layer(1.0, 0.5, LayerBlend::Over))
            .build()
            .expect("three layers are within budget")
            .flatten()
            .expect("three constant layers compose");
        // 0.25 -> 0.375 -> 0.46875 -> 0.734375, every step exact in binary.
        assert_eq!(
            flat.binding(SurfaceChannel::Roughness).as_constant(),
            Some(scalar(0.734375))
        );
        assert!(flat.layers().is_empty());
    }

    #[test]
    fn a_flattened_channel_equals_the_hand_composed_mix_chain() {
        let flat = SurfaceBuilder::new()
            .field(SurfaceChannel::Roughness, uv_x())
            .layer(roughness_layer(0.5, 0.5, LayerBlend::Over))
            .layer(roughness_layer(0.75, 0.25, LayerBlend::Over))
            .layer(roughness_layer(1.0, 0.5, LayerBlend::Over))
            .build()
            .expect("three layers are within budget")
            .flatten()
            .expect("a field base composes with three constant layers");

        let (builder, uv) = FieldBuilder::new(FieldId::of_name("surface/flatten/hand"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, base) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let hand = [(0.5_f32, 0.5_f32), (0.75, 0.25), (1.0, 0.5)].iter().fold(
            (builder, base),
            |(builder, under), (value, mask)| {
                let (builder, over) = builder.push_const(scalar(*value));
                let (builder, selector) = builder.push_const(scalar(*mask));
                builder.push(FieldOp::Mix, Vec::new(), vec![under, over, selector])
            },
        );
        let expected = hand.0.build(hand.1);

        let composed = flat.binding(SurfaceChannel::Roughness).as_graph();
        [0.0_f32, 0.25, 0.5, 1.0].iter().for_each(|u| {
            assert_eq!(
                composed.evaluate(&at(*u)),
                expected.evaluate(&at(*u)),
                "the flattened graph must agree with the hand-composed chain at u = {u}"
            );
        });
    }

    #[test]
    fn every_blend_rule_survives_flattening() {
        let flattened = |blend: LayerBlend| {
            SurfaceBuilder::new()
                .constant(SurfaceChannel::Roughness, scalar(0.25))
                .layer(roughness_layer(0.75, 0.5, blend))
                .build()
                .expect("one layer is within budget")
                .flatten()
                .expect("constants compose")
                .binding(SurfaceChannel::Roughness)
                .as_constant()
                .expect("a constant composition stays constant")
                .as_scalar()
                .get()
        };
        assert_eq!(flattened(LayerBlend::Over), 0.5);
        assert_eq!(flattened(LayerBlend::Add), 0.625);
        assert_eq!(flattened(LayerBlend::Multiply), 0.21875);
    }

    #[test]
    fn a_nested_layer_is_resolved_before_it_is_blended_into_its_parent() {
        let inner = SurfaceBuilder::new()
            .constant(SurfaceChannel::Roughness, scalar(0.25))
            .layer(roughness_layer(0.75, 0.5, LayerBlend::Over))
            .build()
            .expect("one nested layer");
        let flat = SurfaceBuilder::new()
            .constant(SurfaceChannel::Roughness, scalar(0.0))
            .layer(SurfaceLayer::new(
                inner,
                ChannelBinding::constant(scalar(0.5)),
                LayerBlend::Over,
            ))
            .build()
            .expect("two layers in the tree")
            .flatten()
            .expect("a nested tree composes");
        // inner resolves to 0.5, then mix(0.0, 0.5, 0.5) = 0.25.
        assert_eq!(
            flat.binding(SurfaceChannel::Roughness).as_constant(),
            Some(scalar(0.25))
        );
    }

    #[test]
    fn flattening_is_idempotent() {
        let once = SurfaceBuilder::new()
            .field(SurfaceChannel::Roughness, uv_x())
            .layer(roughness_layer(0.5, 0.5, LayerBlend::Over))
            .build()
            .expect("one layer")
            .flatten()
            .expect("composes");
        let twice = once.flatten().expect("a flat surface flattens to itself");
        assert_eq!(twice, once);
        assert_eq!(twice.serialize(), once.serialize());
    }

    #[test]
    fn a_composition_that_cannot_be_built_is_reported_not_swallowed() {
        let (builder, _node) = FieldBuilder::new(FieldId::from_raw(4), 1).push(
            FieldOp::Abs,
            Vec::new(),
            vec![axiom_field::NodeId::from_raw(9)],
        );
        let broken = ChannelBinding::field(builder.build(axiom_field::NodeId::from_raw(0)));
        let surface = Surface::new(
            own_bindings(&SurfaceBuilder::new().build().expect("legal")),
            LightingModel::Lambert,
            vec![SurfaceLayer::new(
                Surface::new(
                    {
                        let mut bindings =
                            own_bindings(&SurfaceBuilder::new().build().expect("legal"));
                        bindings[SurfaceChannel::Roughness.index()] = broken;
                        bindings
                    },
                    LightingModel::Lambert,
                    Vec::new(),
                ),
                ChannelBinding::constant(scalar(0.5)),
                LayerBlend::Over,
            )],
        );
        assert!(surface.flatten().is_err());
    }
}
