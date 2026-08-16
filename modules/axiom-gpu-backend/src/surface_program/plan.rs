//! The backend-shaped program plan for one authored surface.
//!
//! `crates/axiom-surface` derives the backend-**neutral** half — which context
//! inputs a surface reads, which channels vary, how many nodes and parameters it
//! holds. This is the other half, and it is backend-shaped by construction: the
//! stage split, the interstage lanes, and the parameter layout are all decided by
//! this module's own ceilings (the 16-attribute WebGL2 downlevel guarantee, the
//! 40-float instance stride, the shared bind group layout), which is exactly why
//! there is no separate shader-IR stratum between the two.
//!
//! **Stage assignment is a two-valued fact, not an intermediate representation.**
//! [`axiom_surface::SurfaceChannel::Displacement`] is a vertex-stage channel; the
//! other six are fragment-stage. That is the entire scheduling problem.
//!
//! A plan is *derived*, never authored and never persisted. It is a pure function
//! of the surface, so two identical surfaces plan identically — which is what
//! makes [`SurfaceProgramPlan::program_id`] a usable program-cache key.

use axiom_surface::{Surface, SurfaceChannel, SurfaceInput, SurfaceRequirements};

use crate::surface_program::params::ParamLayout;

/// The channels a **vertex** stage evaluates: displacement, and nothing else.
const VERTEX_STAGE_CHANNELS: u16 = SurfaceChannel::Displacement.bit();

/// The channels a **fragment** stage evaluates: the other six.
const FRAGMENT_STAGE_CHANNELS: u16 = SurfaceChannel::BaseColor.bit()
    | SurfaceChannel::Roughness.bit()
    | SurfaceChannel::Metallic.bit()
    | SurfaceChannel::Normal.bit()
    | SurfaceChannel::Emission.bit()
    | SurfaceChannel::Opacity.bit();

/// The context inputs that must be *interpolated* to reach a fragment stage.
/// Time is a uniform, so it is never a varying however many channels read it.
const VARYING_INPUT_BITS: u16 =
    SurfaceInput::POINT.bits() | SurfaceInput::UV.bits() | SurfaceInput::NORMAL.bits();

/// What a surface's fragment stage needs the vertex stage to hand it.
///
/// This is a *budget* type, not a description: it says which lanes the main
/// pass's interstage struct carries **in the space a surface is evaluated in**,
/// and [`VaryingSet::AVAILABLE`] is that list.
///
/// All three are carried. The uv always was. The other two are the lanes WGSL
/// generation added: a surface's expressions are evaluated in *object* space
/// (see `crates/axiom-surface/ARCHITECTURE.md`) — a world-space pattern swims
/// when the object moves — and the pass's pre-existing `world_pos` and `normal`
/// lanes are both world-space, so neither could stand in. The vertex stage now
/// emits `object_pos` and `object_normal` alongside them, which is what makes
/// [`SurfaceInput::POINT`] and [`SurfaceInput::NORMAL`] honest here rather than
/// merely present.
///
/// [`SurfaceInput::TIME`] is deliberately absent from this set: it is a
/// *uniform*, not a varying: the frame writes it once, into the lighting
/// uniform's `camera.w` lane, and both stages read the same word. Whether a
/// surface needs it at all is [`SurfaceProgramPlan::reads_time`], not a lane
/// budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VaryingSet(u16);

impl VaryingSet {
    /// The lanes the main pass's vertex→fragment interface already carries in a
    /// form a surface can be evaluated against.
    pub(crate) const AVAILABLE: VaryingSet = VaryingSet(
        SurfaceInput::POINT.bits() | SurfaceInput::UV.bits() | SurfaceInput::NORMAL.bits(),
    );

    /// The lanes a surface reading `inputs` needs interpolated.
    pub(crate) const fn of(inputs: SurfaceInput) -> VaryingSet {
        VaryingSet(inputs.bits() & VARYING_INPUT_BITS)
    }

    /// Whether every lane this set needs is one the interface already carries.
    pub(crate) const fn is_available(self) -> bool {
        (self.0 & !VaryingSet::AVAILABLE.0) == 0
    }
}

/// Which of a surface's channels each shader stage evaluates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StageSplit {
    vertex: u16,
    fragment: u16,
}

impl StageSplit {
    /// The split a surface's requirements imply.
    ///
    /// The vertex half is keyed on
    /// [`SurfaceRequirements::has_displacement`] rather than on the varying-channel
    /// bitset, because a *constant* non-zero displacement still moves vertices and
    /// still needs a vertex stage. The fragment half is the varying channels that
    /// are not displacement: a constant colour needs no program.
    pub(crate) fn of(reqs: SurfaceRequirements) -> StageSplit {
        StageSplit {
            vertex: [0, VERTEX_STAGE_CHANNELS][usize::from(reqs.has_displacement())],
            fragment: reqs.varying_channels() & FRAGMENT_STAGE_CHANNELS,
        }
    }

    /// Whether the surface needs a vertex stage at all.
    pub(crate) const fn has_vertex_stage(self) -> bool {
        self.vertex != 0
    }

    /// The fragment-stage channels this surface actually programs — the ones a
    /// backend without a program has to fall back on. A channel *absent* from
    /// this set is a plain constant that the existing instance stream can still
    /// carry, which is what makes the fallback partial rather than total.
    pub(crate) const fn fragment_channels(self) -> u16 {
        self.fragment
    }

    /// The vertex-stage channels this surface programs — displacement, or
    /// nothing. Named separately from [`Self::has_vertex_stage`] because a
    /// report says *which* channels a failing program covered.
    pub(crate) const fn vertex_channels(self) -> u16 {
        self.vertex
    }
}

/// Everything this backend needs to decide about one authored surface before it
/// lowers anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceProgramPlan {
    program_id: u64,
    stage_split: StageSplit,
    param_layout: ParamLayout,
    inputs: SurfaceInput,
    requirements: SurfaceRequirements,
}

impl SurfaceProgramPlan {
    /// Derive the plan for `surface`.
    ///
    /// `program_id` is [`Surface::digest`], a *structural* content hash: a
    /// parameter retune does not move it, so a material tweak cannot invalidate a
    /// compiled program, and it is the same number the draw carries in
    /// [`axiom_host::FrameDrawItem::surface_program`].
    pub(crate) fn of(surface: &Surface) -> SurfaceProgramPlan {
        let requirements = surface.requirements();
        SurfaceProgramPlan {
            program_id: surface.digest().raw(),
            stage_split: StageSplit::of(requirements),
            param_layout: ParamLayout::of(requirements.param_count()),
            inputs: requirements.inputs(),
            requirements,
        }
    }

    /// The program-cache key — the draw's `surface_program`.
    pub(crate) const fn program_id(self) -> u64 {
        self.program_id
    }

    /// Which stage evaluates which channels.
    pub(crate) const fn stage_split(self) -> StageSplit {
        self.stage_split
    }

    /// The interstage lanes the fragment stage needs. Derived from the plan's
    /// inputs rather than stored beside them, so the two can never disagree.
    pub(crate) const fn varyings(self) -> VaryingSet {
        VaryingSet::of(self.inputs)
    }

    /// Where the program's parameters live inside its uniform region.
    pub(crate) const fn param_layout(self) -> ParamLayout {
        self.param_layout
    }

    /// The neutral requirements the plan was derived from.
    pub(crate) const fn requirements(self) -> SurfaceRequirements {
        self.requirements
    }

    /// Whether any of this surface's channels reads the clock.
    ///
    /// The one thing the frame has to *supply* rather than derive: every other
    /// input to a program is either a varying the vertex stage already writes or
    /// a parameter the surface itself declares. A surface that answers `false`
    /// here is a static surface, and the pass writes it no time at all — which
    /// is what keeps a static surface exactly as free as it was before there was
    /// a clock (see [`crate::surface_program::SurfaceProgramSet::surface_time`]).
    pub(crate) const fn reads_time(self) -> bool {
        self.inputs.contains(SurfaceInput::TIME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldValue};
    use axiom_math::Vec3;
    use axiom_recipe::Param;
    use axiom_surface::SurfaceBuilder;

    /// A vec4 base colour driven by `Uv.x` — the canonical field-authored surface.
    fn uv_color() -> axiom_field::FieldGraph {
        let (builder, uv) =
            FieldBuilder::new(FieldId::of_name("gpu/plan/uv"), 1).push(FieldOp::Uv, Vec::new(), Vec::new());
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let (builder, splat) = builder.push(
            FieldOp::Compose,
            vec![Param::int(4)],
            vec![lane, lane, lane, lane],
        );
        builder.build(splat)
    }

    /// A vec3 displacement driven by the object-space point.
    fn point_offset() -> axiom_field::FieldGraph {
        let (builder, point) = FieldBuilder::new(FieldId::of_name("gpu/plan/pt"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        builder.build(point)
    }

    #[test]
    fn a_constant_surface_plans_no_stage_no_varying_and_no_parameters() {
        let surface = SurfaceBuilder::new().build().expect("legal");
        let plan = SurfaceProgramPlan::of(&surface);
        assert_eq!(plan.program_id(), surface.digest().raw());
        assert!(!plan.stage_split().has_vertex_stage());
        assert_eq!(plan.stage_split().fragment_channels(), 0);
        assert_eq!(plan.param_layout(), ParamLayout::of(0));
        assert!(plan.varyings().is_available());
        assert_eq!(plan.requirements(), surface.requirements());
        assert_eq!(plan, SurfaceProgramPlan::of(&surface));
        assert!(format!("{plan:?}").contains("SurfaceProgramPlan"));
    }

    #[test]
    fn a_uv_driven_colour_is_a_fragment_stage_program_whose_lanes_already_exist() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("a vec4 uv field is a legal base colour");
        let plan = SurfaceProgramPlan::of(&surface);
        assert!(!plan.stage_split().has_vertex_stage());
        assert_eq!(
            plan.stage_split().fragment_channels(),
            SurfaceChannel::BaseColor.bit()
        );
        // Uv is interpolated by the existing interstage struct.
        assert!(plan.varyings().is_available());
        assert_eq!(plan.varyings(), VaryingSet::of(SurfaceInput::UV));
    }

    #[test]
    fn the_object_space_point_is_a_lane_the_interface_now_carries() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Displacement, point_offset())
            .build()
            .expect("a vec3 field is a legal displacement");
        let plan = SurfaceProgramPlan::of(&surface);
        // The vertex stage emits `object_pos`, so a surface reading the point is
        // no longer refused for want of a lane.
        assert!(plan.varyings().is_available());
        assert_eq!(VaryingSet::AVAILABLE.is_available(), true);
        assert_eq!(plan.varyings(), VaryingSet::of(SurfaceInput::POINT));
        assert_ne!(plan.varyings(), VaryingSet::AVAILABLE);
        // Displacement is the one vertex-stage channel.
        assert!(plan.stage_split().has_vertex_stage());
        assert_eq!(plan.stage_split().fragment_channels(), 0);
    }

    #[test]
    fn every_interpolatable_input_is_a_lane_the_interface_carries() {
        assert!(VaryingSet::of(SurfaceInput::POINT).is_available());
        assert!(VaryingSet::of(SurfaceInput::UV).is_available());
        assert!(VaryingSet::of(SurfaceInput::NORMAL).is_available());
        assert_eq!(
            VaryingSet::AVAILABLE,
            VaryingSet(VARYING_INPUT_BITS),
            "every input that can be interpolated at all now has a lane"
        );
    }

    #[test]
    fn a_constant_non_zero_displacement_still_needs_a_vertex_stage() {
        let surface = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(Vec3::new(0.0, 0.5, 0.0)),
            )
            .build()
            .expect("a vec3 constant is a legal displacement");
        let split = SurfaceProgramPlan::of(&surface).stage_split();
        assert!(split.has_vertex_stage());
        assert_ne!(split, StageSplit::of(SurfaceRequirements::EMPTY));
    }

    #[test]
    fn the_two_stage_channel_masks_partition_the_seven_channels() {
        assert_eq!(VERTEX_STAGE_CHANNELS & FRAGMENT_STAGE_CHANNELS, 0);
        let all = SurfaceChannel::ALL
            .iter()
            .fold(0_u16, |bits, channel| bits | channel.bit());
        assert_eq!(VERTEX_STAGE_CHANNELS | FRAGMENT_STAGE_CHANNELS, all);
        // Time is never a varying, however many channels read it.
        assert_eq!(VARYING_INPUT_BITS & SurfaceInput::TIME.bits(), 0);
        assert!(VaryingSet::of(SurfaceInput::TIME).is_available());
    }

    #[test]
    fn only_a_clock_reading_surface_plans_as_reading_the_clock() {
        let (builder, clock) = FieldBuilder::new(FieldId::of_name("gpu/plan/time"), 1).push(
            FieldOp::Time,
            Vec::new(),
            Vec::new(),
        );
        let (builder, splat) = builder.push(
            FieldOp::Compose,
            vec![Param::int(3)],
            vec![clock, clock, clock],
        );
        let timed = SurfaceBuilder::new()
            .field(SurfaceChannel::Displacement, builder.build(splat))
            .build()
            .expect("a vec3 field is a legal displacement");
        assert!(SurfaceProgramPlan::of(&timed).reads_time());
        let still = SurfaceBuilder::new()
            .field(SurfaceChannel::Displacement, point_offset())
            .build()
            .expect("a vec3 field is a legal displacement");
        assert!(!SurfaceProgramPlan::of(&still).reads_time());
    }
}
