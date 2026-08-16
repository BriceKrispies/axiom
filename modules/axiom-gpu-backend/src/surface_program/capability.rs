//! The capability gate: what this backend will and will not lower, decided once
//! per surface **before** anything is lowered.
//!
//! Validation is a pure function of `(plan, profile, geometry)`, and a plan is a
//! pure function of the surface — so this is a pure function of
//! `(requirements, profile, geometry)`, exactly as the design requires, and it is
//! checked at bind/preparation time rather than per frame. A surface this backend
//! cannot support is reported through the existing
//! [`axiom_host::FrameSubmissionReport`] degraded-features channel, never
//! silently skipped.
//!
//! `geometry` is the third input because exactly one ceiling is a property of the
//! *draw* rather than of the surface: the skinned vertex stage is at the
//! 16-attribute limit and runs no displacement program (see [`GeometryPath`]).
//! Folding it into the plan would be a lie — one surface can be drawn on both
//! kinds of geometry in the same frame.
//!
//! It takes the *plan* rather than the bare requirements because every ceiling it
//! checks against is one the plan already resolved: the parameter layout's fit in
//! the shared uniform region, the interstage lanes the main pass carries, and the
//! stage split. Re-deriving those here would be a second definition of them.

use axiom_host::{BackendCapabilityProfile, RenderCapability};

use crate::surface_program::plan::SurfaceProgramPlan;
use crate::surface_program::program_error::{SurfaceProgramError, SurfaceProgramFault};

/// How many operator nodes one surface program may hold, across every channel
/// and every layer. A budget, not a limit of the language: a lowered program is
/// straight-line code with one statement per node, and this is what keeps a
/// pathological graph from producing a shader the browser refuses to compile.
pub(crate) const MAX_SURFACE_NODES: u16 = 256;

/// Which vertex stage a surface will be drawn through.
///
/// Not a preference — a hard fact about the two pipelines this backend builds,
/// and the only axis on which they differ for a surface program. The rigid
/// pipeline binds 14 of the 16 vertex attributes a WebGL2 downlevel target
/// guarantees; the skinned one binds **all sixteen** (6 per-vertex + 10
/// per-instance), which is why it already drops a skinned material's emissive
/// and specular. A displacement program needs no new attribute — it reads
/// position, normal and uv, which both pipelines have — but the skinned path
/// deforms the vertex *itself* through the joint palette first, and stacking a
/// second deformation on a stage already at its ceiling is a change to the
/// skinned draw contract, not to the emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeometryPath {
    /// `vs` — the rigid vertex stage, which runs `axiom_displace`.
    Rigid = 0,
    /// `vs_skinned` — the linear-blend-skinning vertex stage, which does not.
    Skinned = 1,
}

impl GeometryPath {
    /// Whether this path's vertex stage runs a displacement program.
    pub(crate) fn displaces(self) -> bool {
        (self as u8) == (GeometryPath::Rigid as u8)
    }
}

/// One ceiling, and the sentence a report gives when a surface hits it. Ordered
/// so [`validate`] can name the *first* thing that was wrong rather than a bag
/// of flags.
const REJECTIONS: [&str; 5] = [
    "the frame's capability profile does not attempt procedural surfaces",
    "displacement needs the rigid vertex stage: the skinned pipeline binds all 16 \
     vertex attributes the WebGL2 downlevel target guarantees and already drops \
     emissive and specular for that reason, so it cannot also deform against a \
     surface program",
    "the surface declares more parameters than the shared uniform region holds",
    "the surface reads an interstage lane the main pass does not carry",
    "the surface holds more operator nodes than the shader budget allows",
];

/// Whether this backend can lower `plan` under `profile` for `geometry`, or the
/// explained failure it must report as a degraded feature instead.
///
/// The five rejections, each with the reason it is a rejection and not a silent
/// approximation:
///
/// * **The profile does not attempt procedural surfaces.** Until a generated
///   program is bound to a pipeline there is nothing to run, so this backend's
///   default profile clears the bit and every authored surface takes the
///   constant fallback.
/// * **The surface displaces geometry and the draw is skinned.** See
///   [`GeometryPath`]. This is the one rejection that is a property of the
///   *draw* rather than of the surface alone, and it is reported rather than
///   silently no-oped: a skinned character bound to a wind surface that simply
///   did not move would be a wrong shape nobody was told about.
/// * **The surface holds more parameters than the shared region.** The region is
///   fixed-size precisely so every program can share one bind group layout, so an
///   over-cap surface is rejected rather than truncated.
/// * **The surface needs an interstage lane the main pass does not carry**, or
///   more nodes than the shader budget allows.
///
/// `SurfaceInput::TIME` is **no longer** a rejection. It was one for exactly as
/// long as no frame time reached the pass: lowering a time-reading surface
/// against a frozen clock is a silently wrong answer, which is worse than an
/// absent one. The frame now supplies a deterministic
/// [`axiom_kernel::Seconds`] through `axiom_host::FramePacket::time`, so a
/// clock-reading surface has a real clock to read.
///
/// Every rejection is reported to the frame as the same
/// [`axiom_host::FrameFeature::ProceduralSurface`], because that is what the
/// frame did not get; the returned error is what says *which* ceiling, in a
/// sentence an author can act on.
///
/// A surface that **needs no program at all** — every channel a plain constant,
/// no displacement — is always admitted, whatever the profile says. There is
/// nothing for the capability to gate: such a surface is a material, and the
/// existing pipeline renders it exactly. Reporting it as degraded would be
/// telling the frame it lost something it never asked for.
pub(crate) fn validate(
    plan: &SurfaceProgramPlan,
    profile: BackendCapabilityProfile,
    geometry: GeometryPath,
) -> Result<(), SurfaceProgramError> {
    let split = plan.stage_split();
    let needs_program = split.has_vertex_stage() | (split.fragment_channels() != 0);
    let admitted = [
        profile.contains(RenderCapability::ProceduralSurface),
        !split.has_vertex_stage() | geometry.displaces(),
        plan.param_layout().fits(),
        plan.varyings().is_available(),
        plan.requirements().node_count() <= MAX_SURFACE_NODES,
    ];
    let covered = split.fragment_channels() | split.vertex_channels();
    needs_program
        .then(|| admitted.iter().position(|ok| !ok))
        .flatten()
        .map_or(Ok(()), |reason| {
            Err(SurfaceProgramError::new(
                plan.program_id(),
                covered,
                SurfaceProgramFault::Capability,
                String::from(REJECTIONS[reason]),
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldType, FieldValue};
    use axiom_math::Vec3;
    use axiom_recipe::{Param, Scalar};
    use axiom_surface::{Surface, SurfaceBuilder, SurfaceChannel, SurfaceInput};

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

    /// The reason string a rejection carried, or `None` when it was admitted.
    fn reason(
        plan: &SurfaceProgramPlan,
        profile: BackendCapabilityProfile,
        geometry: GeometryPath,
    ) -> Option<String> {
        validate(plan, profile, geometry)
            .err()
            .map(|error| String::from(error.detail()))
    }

    #[test]
    fn a_lowerable_surface_is_admitted_by_an_attempting_profile() {
        let plan = SurfaceProgramPlan::of(&uv_opacity());
        assert_eq!(validate(&plan, attempting(), GeometryPath::Rigid), Ok(()));
    }

    #[test]
    fn a_profile_that_does_not_attempt_procedural_surfaces_reports_the_feature() {
        let plan = SurfaceProgramPlan::of(&uv_opacity());
        let refused = validate(
            &plan,
            BackendCapabilityProfile::all().without(RenderCapability::ProceduralSurface),
            GeometryPath::Rigid,
        )
        .expect_err("a profile that clears the bit has no program to run");
        assert_eq!(refused.fault(), SurfaceProgramFault::Capability);
        assert_eq!(refused.program_id(), uv_opacity().digest().raw());
        assert_eq!(refused.channel_names(), vec!["opacity"]);
        assert!(refused.detail().contains("does not attempt procedural surfaces"));
        assert!(
            reason(&plan, BackendCapabilityProfile::none(), GeometryPath::Rigid).is_some()
        );
    }

    /// A vec3 displacement bound as a plain constant — the smallest thing that
    /// still needs a vertex stage.
    fn displacing() -> Surface {
        SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(Vec3::new(0.0, 1.0, 0.0)),
            )
            .build()
            .expect("a vec3 constant is a legal displacement")
    }

    #[test]
    fn a_displacing_surface_is_admitted_on_the_rigid_vertex_stage() {
        let plan = SurfaceProgramPlan::of(&displacing());
        assert!(plan.stage_split().has_vertex_stage());
        assert_eq!(validate(&plan, attempting(), GeometryPath::Rigid), Ok(()));
    }

    /// The skinned pipeline is at the 16-attribute ceiling. A displacing surface
    /// drawn through it is a **reported** failure, never a silent no-op — a
    /// character bound to a wind surface that simply did not move is a wrong
    /// shape nobody was told about — and the error says which ceiling.
    #[test]
    fn a_displacing_surface_on_the_skinned_path_is_a_reported_degradation_not_a_no_op() {
        let surface = displacing();
        let plan = SurfaceProgramPlan::of(&surface);
        let refused = validate(&plan, attempting(), GeometryPath::Skinned)
            .expect_err("the skinned stage cannot run a displacement program");
        assert_eq!(refused.fault(), SurfaceProgramFault::Capability);
        assert_eq!(refused.program_id(), surface.digest().raw());
        assert_eq!(refused.channel_names(), vec!["displacement"]);
        assert!(refused.detail().contains("16"));
        assert!(refused.detail().contains("skinned pipeline"));
        assert!(refused.detail().contains("emissive and specular"));
        // A surface that does NOT displace is fine on either path: the ceiling
        // is about the vertex stage, not about skinning per se.
        let flat = SurfaceProgramPlan::of(&uv_opacity());
        assert_eq!(validate(&flat, attempting(), GeometryPath::Skinned), Ok(()));
        assert!(GeometryPath::Rigid.displaces());
        assert!(!GeometryPath::Skinned.displaces());
        assert_ne!(GeometryPath::Rigid, GeometryPath::Skinned);
        assert!(format!("{:?}", GeometryPath::Skinned).contains("Skinned"));
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
        assert_eq!(validate(&plan, attempting(), GeometryPath::Rigid), Ok(()));
    }

    /// The clock was a rejection only for as long as no frame time reached the
    /// pass. It does now, so a time-reading surface lowers.
    #[test]
    fn a_surface_reading_the_clock_is_admitted_because_the_frame_now_supplies_one() {
        let surface = source_opacity("gpu/cap/time", FieldOp::Time);
        let plan = SurfaceProgramPlan::of(&surface);
        // Time is a uniform, never a varying, so the lane budget says nothing
        // about it either way.
        assert!(plan.varyings().is_available());
        assert!(plan.requirements().inputs().contains(SurfaceInput::TIME));
        assert!(plan.reads_time());
        assert_eq!(validate(&plan, attempting(), GeometryPath::Rigid), Ok(()));
    }

    #[test]
    fn exactly_the_parameter_cap_is_admitted_and_one_over_is_rejected() {
        let at_cap = parameterised(
            crate::surface_program::params::MAX_SURFACE_PARAMS,
        );
        let plan = SurfaceProgramPlan::of(&at_cap);
        assert!(plan.param_layout().fits());
        assert_eq!(validate(&plan, attempting(), GeometryPath::Rigid), Ok(()));

        let over = parameterised(crate::surface_program::params::MAX_SURFACE_PARAMS + 1);
        let over_plan = SurfaceProgramPlan::of(&over);
        assert!(!over_plan.param_layout().fits());
        assert!(reason(&over_plan, attempting(), GeometryPath::Rigid)
            .is_some_and(|why| why.contains("more parameters than the shared uniform region")));
    }

    /// Every ceiling has its own sentence, and the gate names the first one hit
    /// rather than a bag of flags — so a report is actionable.
    #[test]
    fn each_rejection_names_exactly_one_ceiling_and_they_are_all_distinct() {
        let mut sorted = REJECTIONS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), REJECTIONS.len());
        // The interstage-lane row is unreachable through a validated surface
        // today — every interpolatable input has a lane — so it is pinned by its
        // text rather than by a surface that cannot be built.
        assert!(REJECTIONS[3].contains("interstage lane"));
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
        assert!(reason(&plan, attempting(), GeometryPath::Rigid)
            .is_some_and(|why| why.contains("more operator nodes than the shader budget")));
        // One chain alone is inside the budget and is admitted.
        let small = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, chain("gpu/cap/small", 100))
            .build()
            .expect("one scalar chain is a legal opacity");
        assert_eq!(
            validate(&SurfaceProgramPlan::of(&small), attempting(), GeometryPath::Rigid),
            Ok(())
        );
    }

    #[test]
    fn an_all_constant_surface_is_admitted_by_every_profile_because_it_needs_no_program() {
        let plan = SurfaceProgramPlan::of(&SurfaceBuilder::new().build().expect("legal"));
        assert_eq!(validate(&plan, attempting(), GeometryPath::Rigid), Ok(()));
        assert_eq!(
            validate(&plan, BackendCapabilityProfile::none(), GeometryPath::Skinned),
            Ok(())
        );
        assert_eq!(MAX_SURFACE_NODES, 256);
    }
}
