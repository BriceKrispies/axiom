//! The authoring surface for a [`Surface`].

use crate::material_params::MaterialParams;
use crate::surface_kind::SurfaceKind;
use axiom_field::{FieldGraph, FieldId, FieldOp, FieldType, FieldValue, NodeId, Param, Scalar};
use axiom_kernel::{Meters, Ratio};
use axiom_math::Vec3;

use crate::binding::ChannelBinding;
use crate::channel::{SurfaceChannel, SURFACE_CHANNEL_COUNT};
use crate::compose::Composer;
use crate::layer::SurfaceLayer;
use crate::lighting_model::LightingModel;
use crate::surface::Surface;
use crate::surface_error::{SurfaceError, SurfaceErrorCode, SurfaceResult};

/// A height field is a scalar. Anything else has no gradient to difference.
const HEIGHT_MUST_BE_SCALAR: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::ChannelTypeMismatch,
    "a height field must be a scalar for a normal to be derived from it",
);

/// Builds a [`Surface`] by binding channels and stacking layers.
///
/// Every step takes the builder **by value** and returns it, because the Axiom
/// State Law forbids `&mut self` on a public boundary — the same shape as
/// `axiom_field::FieldBuilder`, for the same reason.
///
/// An unbound channel keeps [`SurfaceChannel::default_value`], so
/// `SurfaceBuilder::new().build()` is the engine's existing default material and
/// binding one channel changes exactly that one channel.
#[derive(Debug, Clone)]
pub struct SurfaceBuilder {
    bindings: [ChannelBinding; SURFACE_CHANNEL_COUNT],
    lighting: LightingModel,
    layers: Vec<SurfaceLayer>,
}

impl SurfaceBuilder {
    /// A builder with every channel at its default, the default lighting model,
    /// and no layers.
    pub fn new() -> Self {
        SurfaceBuilder {
            bindings: SurfaceChannel::ALL
                .map(|channel| ChannelBinding::constant(channel.default_value())),
            lighting: LightingModel::default(),
            layers: Vec::new(),
        }
    }

    /// Bind `channel`. The general form; [`Self::constant`] and [`Self::field`]
    /// are the two ways to make the binding.
    pub fn bind(self, channel: SurfaceChannel, binding: ChannelBinding) -> Self {
        let SurfaceBuilder {
            mut bindings,
            lighting,
            layers,
        } = self;
        bindings[channel.index()] = binding;
        SurfaceBuilder {
            bindings,
            lighting,
            layers,
        }
    }

    /// Bind `channel` to a constant value.
    pub fn constant(self, channel: SurfaceChannel, value: FieldValue) -> Self {
        self.bind(channel, ChannelBinding::constant(value))
    }

    /// Bind `channel` to a field expression, evaluated in object space.
    pub fn field(self, channel: SurfaceChannel, graph: FieldGraph) -> Self {
        self.bind(channel, ChannelBinding::field(graph))
    }

    /// Choose how the surface participates in lighting.
    pub fn lighting(self, model: LightingModel) -> Self {
        SurfaceBuilder {
            lighting: model,
            ..self
        }
    }

    /// Stack one more layer onto the surface. The budget is checked by
    /// [`Self::build`], so exceeding it is a reported failure and never a
    /// silent truncation.
    pub fn layer(self, layer: SurfaceLayer) -> Self {
        let SurfaceBuilder {
            bindings,
            lighting,
            mut layers,
        } = self;
        layers.push(layer);
        SurfaceBuilder {
            bindings,
            lighting,
            layers,
        }
    }

    /// Bind [`SurfaceChannel::Normal`] to a normal **derived** from a scalar
    /// `height` field by central differences.
    ///
    /// The height graph is inlined four times, each read against a sample point
    /// displaced by `offset` along `+x`, `-x`, `+y` and `-y`, and the four
    /// samples are composed into
    /// `normalize(vec3(-dx * strength, -dy * strength, 2 * offset))`. Scaling
    /// the `z` lane by `2 * offset` is what divides the differences by their own
    /// step *without* a division — the field algebra deliberately has none.
    ///
    /// **The offset is the caller's, and that is the point.** There is no
    /// screen-space derivative operator in the algebra: `dpdx`/`dpdy` are
    /// backend-specific, absent on the CPU and on a software rasterizer, and
    /// already the cause of a real mobile-GPU NaN defect in this engine. A
    /// finite difference at an offset the author chose is expressible
    /// everywhere and reproducible bit-for-bit.
    ///
    /// A zero `offset` leaves a degenerate vector, which the field layer's
    /// `Normalize` resolves to its documented `+Y` fallback rather than to a
    /// NaN.
    pub fn normal_from_height(
        self,
        height: FieldGraph,
        offset: Meters,
        strength: Ratio,
    ) -> SurfaceResult<Self> {
        height
            .type_at(height.output())
            .map_err(SurfaceError::from_field)
            .and_then(|ty| {
                (ty == FieldType::Scalar)
                    .then_some(())
                    .ok_or(HEIGHT_MUST_BE_SCALAR)
            })
            .and_then(|()| height_to_normal(&height, offset, strength))
            .map_err(|error| error.about_channel(SurfaceChannel::Normal))
            .map(|graph| self.field(SurfaceChannel::Normal, graph))
    }

    /// Finish, proving the surface is a legal appearance description.
    pub fn build(self) -> SurfaceResult<Surface> {
        let surface = self.build_unchecked();
        surface.validate().map(|()| surface)
    }

    /// Finish **without** validating — the shared half of [`Self::build`], and
    /// the only way this crate's own tests can mint a surface that breaks a rule
    /// in order to prove the rule catches it.
    pub(crate) fn build_unchecked(self) -> Surface {
        Surface::new(self.bindings, self.lighting, self.layers)
    }
}

/// The hand-written runtime material shader, as a surface an app can author.
///
/// Not a `SurfaceBuilder` method, because there is nothing to build: a runtime
/// material binds no channels and stacks no layers. Its appearance comes
/// entirely from [`MaterialParams`] and the textures the material already
/// carries, so the only thing to say is *which* program and *with what
/// parameters*.
///
/// Every runtime material shares one digest regardless of its parameters — see
/// [`crate::SurfaceKind`] — so authoring a hundred of them costs one pipeline.
pub fn runtime_material(params: MaterialParams) -> Surface {
    // Each channel gets its own default constant, exactly as `SurfaceBuilder::new`
    // does. The runtime material overwrites every one of them in its own WGSL, so
    // these values are never read — but a surface with well-formed bindings
    // validates, inspects and serialises like any other, which is the whole point
    // of making this a kind of surface rather than a parallel path.
    Surface::of_kind(
        SurfaceChannel::ALL.map(|channel| ChannelBinding::constant(channel.default_value())),
        LightingModel::LambertSpecular,
        Vec::new(),
        SurfaceKind::RuntimeMaterial(params),
    )
}

impl Default for SurfaceBuilder {
    /// [`SurfaceBuilder::new`].
    fn default() -> Self {
        SurfaceBuilder::new()
    }
}

/// Build the tangent-space normal a scalar height field implies.
fn height_to_normal(
    height: &FieldGraph,
    offset: Meters,
    strength: Ratio,
) -> SurfaceResult<FieldGraph> {
    let step = offset.get();
    let deltas = [
        Vec3::new(step, 0.0, 0.0),
        Vec3::new(-step, 0.0, 0.0),
        Vec3::new(0.0, step, 0.0),
        Vec3::new(0.0, -step, 0.0),
    ];
    let (composer, point) = Composer::new(FieldId::of_name("surface/normal-from-height")).push(
        FieldOp::Point,
        Vec::new(),
        Vec::new(),
    );
    deltas
        .iter()
        .try_fold(
            (composer, Vec::<NodeId>::new()),
            |(composer, mut samples), delta| {
                let (composer, shift) = composer.push_const(FieldValue::vec3(*delta));
                let (composer, moved) = composer.push(FieldOp::Add, Vec::new(), vec![point, shift]);
                composer
                    .inline(height, Some(moved))
                    .map(|(composer, sample)| {
                        samples.push(sample);
                        (composer, samples)
                    })
            },
        )
        .map(|(composer, samples)| {
            let (composer, dx) =
                composer.push(FieldOp::Sub, Vec::new(), vec![samples[0], samples[1]]);
            let (composer, dy) =
                composer.push(FieldOp::Sub, Vec::new(), vec![samples[2], samples[3]]);
            let (composer, gain) =
                composer.push_const(FieldValue::scalar(Scalar::new(-strength.get())));
            let (composer, nx) = composer.push(FieldOp::Mul, Vec::new(), vec![dx, gain]);
            let (composer, ny) = composer.push(FieldOp::Mul, Vec::new(), vec![dy, gain]);
            let (composer, nz) = composer.push_const(FieldValue::scalar(Scalar::new(step + step)));
            let (composer, vector) =
                composer.push(FieldOp::Compose, vec![Param::int(3)], vec![nx, ny, nz]);
            let (composer, normal) = composer.push(FieldOp::Normalize, Vec::new(), vec![vector]);
            composer.build(normal)
        })
        .and_then(|graph| graph.canonicalize().map_err(SurfaceError::from_field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::LayerBlend;
    use axiom_field::{EvalContext, FieldBuilder};
    use axiom_math::{Vec2, Vec4};

    /// `h(p) = p.x` — a unit ramp along object-space `x`.
    fn ramp() -> FieldGraph {
        let (builder, point) = FieldBuilder::new(FieldId::of_name("surface/ramp"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![point]);
        builder.build(lane)
    }

    #[test]
    fn a_new_builder_binds_every_channel_to_its_default() {
        let surface = SurfaceBuilder::default()
            .build()
            .expect("a default surface is legal");
        SurfaceChannel::ALL.iter().for_each(|channel| {
            assert_eq!(
                surface.binding(*channel).as_constant(),
                Some(channel.default_value())
            );
        });
        assert_eq!(surface.lighting(), LightingModel::LambertSpecular);
    }

    #[test]
    fn every_channel_can_be_bound_as_a_constant_and_as_a_field() {
        let field_for = |channel: SurfaceChannel| {
            let (builder, node) = FieldBuilder::new(FieldId::from_raw(11), 1)
                .push_const(channel.default_value());
            builder.build(node)
        };
        SurfaceChannel::ALL.iter().for_each(|channel| {
            let constant = SurfaceBuilder::new()
                .constant(*channel, channel.default_value())
                .build()
                .expect("a channel's own default is a legal constant");
            assert!(constant.binding(*channel).is_constant());

            let bound = SurfaceBuilder::new()
                .field(*channel, field_for(*channel))
                .build()
                .expect("a literal graph of the channel's type is a legal field");
            assert_eq!(bound.binding(*channel).ty(), Ok(channel.ty()));
            assert!(bound.binding(*channel).as_field().is_some());
        });
    }

    #[test]
    fn every_lighting_model_can_be_chosen() {
        LightingModel::ALL.iter().for_each(|model| {
            let surface = SurfaceBuilder::new()
                .lighting(*model)
                .build()
                .expect("every lighting model is legal");
            assert_eq!(surface.lighting(), *model);
        });
    }

    #[test]
    fn every_blend_can_be_layered() {
        LayerBlend::ALL.iter().for_each(|blend| {
            let surface = SurfaceBuilder::new()
                .layer(SurfaceLayer::new(
                    SurfaceBuilder::new().build().expect("legal"),
                    SurfaceLayer::opaque_mask(),
                    *blend,
                ))
                .build()
                .expect("one layer is within budget");
            assert_eq!(surface.layers()[0].blend(), *blend);
        });
    }

    #[test]
    fn a_binding_can_be_carried_across_with_bind() {
        let source = SurfaceBuilder::new()
            .constant(SurfaceChannel::Opacity, FieldValue::scalar(Scalar::new(0.5)))
            .build()
            .expect("legal");
        let carried = SurfaceBuilder::new()
            .bind(
                SurfaceChannel::Opacity,
                source.binding(SurfaceChannel::Opacity).clone(),
            )
            .build()
            .expect("legal");
        assert_eq!(
            carried.binding(SurfaceChannel::Opacity),
            source.binding(SurfaceChannel::Opacity)
        );
    }

    #[test]
    fn a_unit_ramp_yields_the_hand_computed_normal() {
        let surface = SurfaceBuilder::new()
            .normal_from_height(ramp(), Meters::finite_or_zero(0.5), Ratio::finite_or_zero(1.0))
            .expect("a scalar height derives a normal")
            .build()
            .expect("the derived normal is a legal vec3 channel");
        let normal = surface
            .binding(SurfaceChannel::Normal)
            .as_field()
            .expect("the derived normal is a field")
            .evaluate(&EvalContext::at(
                Vec3::new(3.0, 0.0, -2.0),
                Vec2::ZERO,
                Vec3::UNIT_Y,
            ))
            .expect("the derived normal evaluates");
        // dx = (x + 0.5) - (x - 0.5) = 1, dy = 0, so the vector is
        // (-1, 0, 2 * 0.5) and its normalization is (-1, 0, 1) / sqrt(2).
        let expected = 1.0_f32 / 2.0_f32.sqrt();
        assert!((expected - 0.707_106_78).abs() < 1e-7);
        assert_eq!(normal.ty(), FieldType::Vec3);
        assert_eq!(normal.as_vec3(), Vec3::new(-expected, 0.0, expected));
    }

    #[test]
    fn strength_scales_the_derived_slope() {
        let derived = |strength: f32| {
            SurfaceBuilder::new()
                .normal_from_height(
                    ramp(),
                    Meters::finite_or_zero(0.5),
                    Ratio::finite_or_zero(strength),
                )
                .expect("a scalar height derives a normal")
                .build()
                .expect("legal")
                .binding(SurfaceChannel::Normal)
                .as_field()
                .expect("a field")
                .evaluate(&EvalContext::ORIGIN)
                .expect("evaluates")
                .as_vec3()
        };
        // Strength 0 leaves the vector (0, 0, 1) — a flat tangent-space normal.
        assert_eq!(derived(0.0), Vec3::new(0.0, 0.0, 1.0));
        // A steeper strength tips the normal further from +Z.
        assert!(derived(4.0).x < derived(1.0).x);
        assert!(derived(4.0).z < derived(1.0).z);
    }

    #[test]
    fn a_zero_offset_falls_back_rather_than_producing_a_nan() {
        let normal = SurfaceBuilder::new()
            .normal_from_height(ramp(), Meters::finite_or_zero(0.0), Ratio::finite_or_zero(1.0))
            .expect("a scalar height derives a normal")
            .build()
            .expect("legal")
            .binding(SurfaceChannel::Normal)
            .as_field()
            .expect("a field")
            .evaluate(&EvalContext::ORIGIN)
            .expect("evaluates")
            .as_vec3();
        assert_eq!(normal, Vec3::UNIT_Y);
    }

    #[test]
    fn a_height_with_no_object_space_gradient_yields_a_flat_normal() {
        // The differences are taken in object space, so a height authored over
        // `Uv` alone has nothing to difference and the derivation says so
        // plainly rather than inventing a slope.
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("surface/uv-height"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let normal = SurfaceBuilder::new()
            .normal_from_height(
                builder.build(lane),
                Meters::finite_or_zero(0.5),
                Ratio::finite_or_zero(1.0),
            )
            .expect("a scalar height derives a normal")
            .build()
            .expect("legal")
            .binding(SurfaceChannel::Normal)
            .as_field()
            .expect("a field")
            .evaluate(&EvalContext::at(
                Vec3::new(2.0, 3.0, 4.0),
                Vec2::new(0.25, 0.75),
                Vec3::UNIT_Y,
            ))
            .expect("evaluates")
            .as_vec3();
        assert_eq!(normal, Vec3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn a_height_that_is_not_a_scalar_is_rejected() {
        let (builder, node) = FieldBuilder::new(FieldId::from_raw(12), 1)
            .push_const(FieldValue::vec4(Vec4::ONE));
        let error = SurfaceBuilder::new()
            .normal_from_height(
                builder.build(node),
                Meters::finite_or_zero(0.5),
                Ratio::finite_or_zero(1.0),
            )
            .expect_err("a vec4 has no scalar gradient");
        assert_eq!(error.kind(), SurfaceErrorCode::ChannelTypeMismatch);
        assert_eq!(error.channel(), Some(SurfaceChannel::Normal));
    }

    #[test]
    fn a_height_that_does_not_type_is_rejected_with_the_field_diagnostic() {
        let (builder, node) =
            FieldBuilder::new(FieldId::from_raw(13), 1).push(FieldOp::Dot, Vec::new(), Vec::new());
        let error = SurfaceBuilder::new()
            .normal_from_height(
                builder.build(node),
                Meters::finite_or_zero(0.5),
                Ratio::finite_or_zero(1.0),
            )
            .expect_err("Dot with no inputs does not type");
        assert_eq!(error.kind(), SurfaceErrorCode::InvalidField);
        assert_eq!(error.channel(), Some(SurfaceChannel::Normal));
    }

    #[test]
    fn a_height_field_too_wide_to_inline_four_times_is_rejected() {
        let (builder, last) = (0..80).fold(
            (FieldBuilder::new(FieldId::from_raw(14), 1), NodeId::NULL),
            |(builder, _last), _| builder.push(FieldOp::Time, Vec::new(), Vec::new()),
        );
        let error = SurfaceBuilder::new()
            .normal_from_height(
                builder.build(last),
                Meters::finite_or_zero(0.5),
                Ratio::finite_or_zero(1.0),
            )
            .expect_err("four copies of eighty nodes do not fit the field node budget");
        assert_eq!(error.kind(), SurfaceErrorCode::InvalidField);
        assert_eq!(error.field_code(), 1);
    }
}
