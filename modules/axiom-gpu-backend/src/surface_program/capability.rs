//! The capability gate: what this backend will and will not lower, decided once
//! per surface **before** anything is lowered.
//!
//! Validation is a pure function of `(plan, profile)`, and a plan is a pure
//! function of the surface — so this is a pure function of
//! `(requirements, profile)`, exactly as the design requires, and it is checked
//! at bind/preparation time rather than per frame. A surface this backend cannot
//! support is reported through the existing
//! [`axiom_host::FrameSubmissionReport`] degraded-features channel, never
//! silently skipped.
//!
//! It takes the *plan* rather than the bare requirements because every ceiling it
//! checks against is one the plan already resolved: the parameter layout's fit in
//! the shared uniform region, the interstage lanes the main pass carries, and the
//! stage split. Re-deriving those here would be a second definition of them.

use axiom_host::{BackendCapabilityProfile, FrameFeature, RenderCapability};
use axiom_surface::SurfaceInput;

use crate::surface_program::plan::SurfaceProgramPlan;

/// How many operator nodes one surface program may hold, across every channel
/// and every layer. A budget, not a limit of the language: a lowered program is
/// straight-line code with one statement per node, and this is what keeps a
/// pathological graph from producing a shader the browser refuses to compile.
pub(crate) const MAX_SURFACE_NODES: u16 = 256;

/// Whether this backend can lower `plan` under `profile`, or the feature it must
/// report as degraded instead.
///
/// The four rejections, each with the reason it is a rejection and not a silent
/// approximation:
///
/// * **The profile does not attempt procedural surfaces.** Until WGSL generation
///   lands there is no program to bind, so this backend's default profile clears
///   the bit and every authored surface takes the constant fallback.
/// * **The surface displaces geometry.** Displacement is the one vertex-stage
///   channel and vertex deformation is a separate piece of work; a fragment-only
///   lowering of a displacing surface would render the right colour on the wrong
///   shape.
/// * **The surface holds more parameters than the shared region.** The region is
///   fixed-size precisely so every program can share one bind group layout, so an
///   over-cap surface is rejected rather than truncated.
/// * **The surface needs an interstage lane the main pass does not carry**, or
///   more nodes than the shader budget allows.
/// * **The surface reads the clock.** `SurfaceInput::TIME` is a uniform, not a
///   varying, and this pass has no frame-time uniform to bind one to — the
///   `SurfaceIn::time` lane the emitter writes against is filled with zero. A
///   time-reading surface is therefore refused rather than lowered against a
///   frozen clock, which would be a silently wrong answer instead of an absent
///   one. Wiring a frame time through `SceneRenderer::record` is a change to the
///   frame contract, not to the emitter.
///
/// Every rejection reports the same [`FrameFeature::ProceduralSurface`], because
/// that is what the frame did not get. The *reason* is a property of the plan,
/// which the caller still holds.
///
/// A surface that **needs no program at all** — every channel a plain constant,
/// no displacement — is always admitted, whatever the profile says. There is
/// nothing for the capability to gate: such a surface is a material, and the
/// existing pipeline renders it exactly. Reporting it as degraded would be
/// telling the frame it lost something it never asked for.
pub(crate) fn validate(
    plan: &SurfaceProgramPlan,
    profile: BackendCapabilityProfile,
) -> Result<(), FrameFeature> {
    let split = plan.stage_split();
    let needs_program = split.has_vertex_stage() | (split.fragment_channels() != 0);
    let lowerable = profile.contains(RenderCapability::ProceduralSurface)
        & !split.has_vertex_stage()
        & plan.param_layout().fits()
        & plan.varyings().is_available()
        & !plan.requirements().inputs().contains(SurfaceInput::TIME)
        & (plan.requirements().node_count() <= MAX_SURFACE_NODES);
    (!needs_program | lowerable)
        .then_some(())
        .ok_or(FrameFeature::ProceduralSurface)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldType, FieldValue};
    use axiom_math::Vec3;
    use axiom_recipe::{Param, Scalar};
    use axiom_surface::{Surface, SurfaceBuilder, SurfaceChannel};

    /// A profile that does attempt procedural surfaces — what this backend's
    /// profile becomes once WGSL generation lands.
    fn attempting() -> BackendCapabilityProfile {
        BackendCapabilityProfile::all().with(RenderCapability::ProceduralSurface)
    }

    /// A scalar opacity driven by `Uv.x`: a lowerable, fragment-only surface.
    fn uv_opacity() -> Surface {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("gpu/cap/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(lane))
            .build()
            .expect("a scalar uv field is a legal opacity")
    }

    /// A surface holding `count` declared parameter slots on its opacity channel.
    fn parameterised(count: u16) -> Surface {
        let (builder, node) = (0..count).fold(
            {
                let (builder, first) = FieldBuilder::new(FieldId::of_name("gpu/cap/params"), 1)
                    .push_const(FieldValue::scalar(Scalar::new(0.0)));
                (builder, first)
            },
            |(builder, acc), index| {
                let (builder, slot) =
                    builder.declare(&format!("p{index}"), FieldValue::scalar(Scalar::new(0.5)));
                let (builder, param) = builder.push_param(slot, FieldType::Scalar);
                builder.push(FieldOp::Add, Vec::new(), vec![acc, param])
            },
        );
        SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(node))
            .build()
            .expect("a scalar sum is a legal opacity")
    }

    #[test]
    fn a_lowerable_surface_is_admitted_by_an_attempting_profile() {
        let plan = SurfaceProgramPlan::of(&uv_opacity());
        assert_eq!(validate(&plan, attempting()), Ok(()));
    }

    #[test]
    fn a_profile_that_does_not_attempt_procedural_surfaces_reports_the_feature() {
        let plan = SurfaceProgramPlan::of(&uv_opacity());
        assert_eq!(
            validate(
                &plan,
                BackendCapabilityProfile::all().without(RenderCapability::ProceduralSurface)
            ),
            Err(FrameFeature::ProceduralSurface)
        );
        assert_eq!(
            validate(&plan, BackendCapabilityProfile::none()),
            Err(FrameFeature::ProceduralSurface)
        );
    }

    #[test]
    fn a_displacing_surface_is_rejected_because_the_vertex_stage_is_later_work() {
        let displacing = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(Vec3::new(0.0, 1.0, 0.0)),
            )
            .build()
            .expect("a vec3 constant is a legal displacement");
        let plan = SurfaceProgramPlan::of(&displacing);
        assert!(plan.stage_split().has_vertex_stage());
        assert_eq!(
            validate(&plan, attempting()),
            Err(FrameFeature::ProceduralSurface)
        );
    }

    /// A scalar opacity driven by lane 0 of the context source `op`.
    fn source_opacity(name: &str, op: FieldOp) -> Surface {
        let (builder, source) =
            FieldBuilder::new(FieldId::of_name(name), 1).push(op, Vec::new(), Vec::new());
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![source]);
        SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(lane))
            .build()
            .expect("a scalar lane is a legal opacity")
    }

    #[test]
    fn a_surface_reading_the_object_space_position_is_admitted() {
        // The vertex stage emits `object_pos`, so the lane a point-reading
        // surface needs is one the interface carries.
        let plan = SurfaceProgramPlan::of(&source_opacity("gpu/cap/pt", FieldOp::Point));
        assert!(plan.varyings().is_available());
        assert_eq!(validate(&plan, attempting()), Ok(()));
    }

    #[test]
    fn a_surface_reading_the_clock_is_rejected_because_no_frame_time_reaches_the_pass() {
        let surface = source_opacity("gpu/cap/time", FieldOp::Time);
        let plan = SurfaceProgramPlan::of(&surface);
        // Time is a uniform, never a varying, so the lane budget says nothing
        // about it — the gate is what refuses it.
        assert!(plan.varyings().is_available());
        assert!(plan.requirements().inputs().contains(SurfaceInput::TIME));
        assert_eq!(
            validate(&plan, attempting()),
            Err(FrameFeature::ProceduralSurface)
        );
    }

    #[test]
    fn exactly_the_parameter_cap_is_admitted_and_one_over_is_rejected() {
        let at_cap = parameterised(
            crate::surface_program::params::MAX_SURFACE_PARAMS,
        );
        let plan = SurfaceProgramPlan::of(&at_cap);
        assert!(plan.param_layout().fits());
        assert_eq!(validate(&plan, attempting()), Ok(()));

        let over = parameterised(crate::surface_program::params::MAX_SURFACE_PARAMS + 1);
        let over_plan = SurfaceProgramPlan::of(&over);
        assert!(!over_plan.param_layout().fits());
        assert_eq!(
            validate(&over_plan, attempting()),
            Err(FrameFeature::ProceduralSurface)
        );
    }

    /// A scalar chain of `steps` `Add`s over fresh constants: `2 * steps + 1`
    /// nodes, which is how a *surface* exceeds the budget even though one graph
    /// cannot — the budget is the whole surface's node total, summed over every
    /// channel and every layer.
    fn chain(name: &str, steps: u16) -> axiom_field::FieldGraph {
        let (builder, node) = (0..steps).fold(
            FieldBuilder::new(FieldId::of_name(name), 1)
                .push_const(FieldValue::scalar(Scalar::new(1.0))),
            |(builder, acc), _| {
                let (builder, one) = builder.push_const(FieldValue::scalar(Scalar::new(1.0)));
                builder.push(FieldOp::Add, Vec::new(), vec![acc, one])
            },
        );
        builder.build(node)
    }

    #[test]
    fn a_surface_over_the_node_budget_is_rejected() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, chain("gpu/cap/big/a", 100))
            .field(SurfaceChannel::Roughness, chain("gpu/cap/big/b", 100))
            .build()
            .expect("two scalar chains are legal opacity and roughness");
        let plan = SurfaceProgramPlan::of(&surface);
        assert!(plan.requirements().node_count() > MAX_SURFACE_NODES);
        assert_eq!(
            validate(&plan, attempting()),
            Err(FrameFeature::ProceduralSurface)
        );
        // One chain alone is inside the budget and is admitted.
        let small = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, chain("gpu/cap/small", 100))
            .build()
            .expect("one scalar chain is a legal opacity");
        assert_eq!(validate(&SurfaceProgramPlan::of(&small), attempting()), Ok(()));
    }

    #[test]
    fn an_all_constant_surface_is_admitted_by_every_profile_because_it_needs_no_program() {
        let plan = SurfaceProgramPlan::of(&SurfaceBuilder::new().build().expect("legal"));
        assert_eq!(validate(&plan, attempting()), Ok(()));
        assert_eq!(validate(&plan, BackendCapabilityProfile::none()), Ok(()));
        assert_eq!(MAX_SURFACE_NODES, 256);
    }
}
