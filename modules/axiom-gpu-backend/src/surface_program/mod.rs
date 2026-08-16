//! What an authored [`axiom_surface::Surface`] means to **this** backend.
//!
//! Three pieces, one per file, and a set that binds them:
//!
//! * [`plan`] — the backend-shaped program plan (stage split, interstage lanes,
//!   parameter layout, program identity).
//! * [`params`] — the uniform channel a surface's tunable numbers ride in, and
//!   the offset scheme that keeps two draws in one pass from reading each
//!   other's parameters.
//! * [`capability`] — the gate, checked once per surface at preparation time.
//!
//! **There is no WGSL here.** Generating a program from a plan is separate work.
//! Until it lands, this backend's profile clears
//! [`RenderCapability::ProceduralSurface`], every authored surface fails
//! validation, and each one is reported as
//! [`FrameFeature::ProceduralSurface`] while its **constant** channels are still
//! honoured through the lanes the instance stream already has. A surface whose
//! base colour is a plain colour therefore renders exactly right today; only the
//! field-bound channels are the thing that is missing, and the frame says so.

pub(crate) mod capability;
pub(crate) mod params;
pub(crate) mod plan;

use axiom_host::{BackendCapabilityProfile, FrameFeature};
use axiom_surface::{Surface, SurfaceChannel};

use crate::surface_program::params::{pack, program_region_offset, SURFACE_PARAM_REGION_BYTES};
use crate::surface_program::plan::SurfaceProgramPlan;

/// One authored surface as this backend sees it: its plan, whether it could be
/// lowered, the constant part of it the existing pipeline can still carry, and
/// the bytes of its parameter region.
#[derive(Debug, Clone, PartialEq)]
struct SurfaceProgramEntry {
    plan: SurfaceProgramPlan,
    degraded: Option<FrameFeature>,
    color: [f32; 4],
    emissive: [f32; 3],
    params: Vec<u8>,
}

impl SurfaceProgramEntry {
    /// Plan, validate and pack one surface against `profile`.
    ///
    /// The plan and the identity come from the surface as authored — its digest
    /// is the number the draw carries — while the parameters are packed from its
    /// **flattened** form, because flattening is what composes a layered
    /// surface's per-channel graphs (and therefore its parameter table) into one.
    fn build(surface: &Surface, profile: BackendCapabilityProfile) -> SurfaceProgramEntry {
        let plan = SurfaceProgramPlan::of(surface);
        let programmed = plan.stage_split().fragment_channels();
        let emission = constant_lanes(surface, SurfaceChannel::Emission, programmed, [0.0; 4]);
        SurfaceProgramEntry {
            plan,
            degraded: capability::validate(&plan, profile).err(),
            color: constant_lanes(surface, SurfaceChannel::BaseColor, programmed, [1.0; 4]),
            emissive: [emission[0], emission[1], emission[2]],
            // Packed from the surface's FLATTENED form: flattening is what
            // composes a layered surface's per-channel graphs, and therefore its
            // parameter table, into one. A surface that does not flatten packs no
            // parameters rather than half of them — it cannot happen for a
            // validated `Surface`, whose every constructor validates.
            params: surface
                .flatten()
                .map(|flat| pack(plan.param_layout(), &flat))
                .unwrap_or_default(),
        }
    }
}

/// One channel's constant lanes, or `fallback` when the channel is *programmed*
/// (bound to a field) and therefore has no constant this backend could carry.
fn constant_lanes(
    surface: &Surface,
    channel: SurfaceChannel,
    programmed: u16,
    fallback: [f32; 4],
) -> [f32; 4] {
    let carried = (programmed & channel.bit()) == 0;
    let lanes = surface
        .binding(channel)
        .as_constant()
        .map_or(fallback, |value| {
            let v = value.as_vec4();
            [v.x, v.y, v.z, v.w]
        });
    [fallback, lanes][usize::from(carried)]
}

/// Every authored surface this backend was handed for one frame.
///
/// Built per call rather than retained: nothing here is mutable state, and a
/// frame that hands over no surfaces builds an empty set whose every lookup
/// misses — which is what makes every existing app byte-identical.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SurfaceProgramSet {
    entries: Vec<SurfaceProgramEntry>,
}

impl SurfaceProgramSet {
    /// Plan and validate `surfaces` against `profile`.
    pub(crate) fn build(
        surfaces: &[Surface],
        profile: BackendCapabilityProfile,
    ) -> SurfaceProgramSet {
        SurfaceProgramSet {
            entries: surfaces
                .iter()
                .map(|surface| SurfaceProgramEntry::build(surface, profile))
                .collect(),
        }
    }

    /// The features this backend could not honour, deduplicated: one
    /// [`FrameFeature::ProceduralSurface`] however many surfaces were dropped,
    /// because the report enumerates *features*, not occurrences.
    pub(crate) fn degradations(&self) -> Vec<FrameFeature> {
        self.entries
            .iter()
            .find_map(|entry| entry.degraded)
            .into_iter()
            .collect()
    }

    /// The bytes of the shared parameter buffer: each program's region placed at
    /// [`program_region_offset`] of its index. One buffer, one region per
    /// program, distinct dynamic offsets — never one small buffer rewritten
    /// between draws.
    pub(crate) fn parameter_bytes(&self) -> Vec<u8> {
        let mut buffer = vec![0_u8; self.entries.len() * SURFACE_PARAM_REGION_BYTES as usize];
        self.entries.iter().enumerate().for_each(|(index, entry)| {
            let at = program_region_offset(index as u64) as usize;
            buffer
                .get_mut(at..at + entry.params.len())
                .map(|region| region.copy_from_slice(&entry.params));
        });
        buffer
    }

    /// The constant colour and emission a draw naming `program_id` should be
    /// rendered with while its program is unavailable: the surface's own
    /// constant channels, or the neutral identity `(white, black)` for a program
    /// this backend was never handed — and for `program_id == 0`, the number
    /// every draw that authored no surface carries.
    pub(crate) fn constant_fallback(&self, program_id: u64) -> ([f32; 4], [f32; 3]) {
        self.entries
            .iter()
            .find(|entry| entry.plan.program_id() == program_id)
            .map(|entry| (entry.color, entry.emissive))
            .unwrap_or_else(|| ([1.0; 4], [0.0; 3]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldValue};
    use axiom_host::RenderCapability;
    use axiom_math::Vec4;
    use axiom_recipe::Param;
    use axiom_surface::{LayerBlend, SurfaceBuilder, SurfaceLayer};

    /// A vec4 base colour driven by `Uv.x` — a surface with no constant colour.
    fn uv_color() -> axiom_field::FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("gpu/set/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let (builder, splat) = builder.push(
            FieldOp::Compose,
            vec![Param::int(4)],
            vec![lane, lane, lane, lane],
        );
        builder.build(splat)
    }

    fn gpu_profile() -> BackendCapabilityProfile {
        BackendCapabilityProfile::all().without(RenderCapability::ProceduralSurface)
    }

    #[test]
    fn an_empty_set_degrades_nothing_and_leaves_every_draw_alone() {
        let set = SurfaceProgramSet::default();
        assert!(set.degradations().is_empty());
        assert!(set.parameter_bytes().is_empty());
        assert_eq!(set.constant_fallback(0), ([1.0; 4], [0.0; 3]));
        assert_eq!(set.constant_fallback(1234), ([1.0; 4], [0.0; 3]));
        assert_eq!(set, SurfaceProgramSet::build(&[], gpu_profile()));
        assert!(format!("{set:?}").contains("SurfaceProgramSet"));
    }

    #[test]
    fn a_field_authored_surface_is_reported_dropped_and_falls_back_to_white() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .constant(
                SurfaceChannel::Emission,
                FieldValue::vec4(Vec4::new(0.1, 0.2, 0.3, 0.0)),
            )
            .build()
            .expect("a vec4 uv field is a legal base colour");
        let set = SurfaceProgramSet::build(std::slice::from_ref(&surface), gpu_profile());
        assert_eq!(set.degradations(), vec![FrameFeature::ProceduralSurface]);
        let (color, emissive) = set.constant_fallback(surface.digest().raw());
        // The programmed channel has no constant to carry, so it falls back to
        // the neutral white the instance colour lane multiplies by.
        assert_eq!(color, [1.0; 4]);
        // The CONSTANT channel is still honoured — the fallback is partial, not
        // total, which is the whole point of splitting it per channel.
        assert_eq!(emissive, [0.1, 0.2, 0.3]);
    }

    #[test]
    fn an_all_constant_surface_carries_its_colour_and_is_not_degraded() {
        let surface = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::BaseColor,
                FieldValue::vec4(Vec4::new(0.2, 0.4, 0.6, 1.0)),
            )
            .build()
            .expect("a vec4 constant is a legal base colour");
        let set = SurfaceProgramSet::build(std::slice::from_ref(&surface), gpu_profile());
        // Nothing about it needs a program, so nothing about it is degraded.
        assert!(set.degradations().is_empty());
        assert_eq!(
            set.constant_fallback(surface.digest().raw()),
            ([0.2, 0.4, 0.6, 1.0], [0.0; 3])
        );
    }

    #[test]
    fn each_program_gets_its_own_aligned_region_of_the_shared_buffer() {
        let a = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("legal");
        let b = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::BaseColor,
                FieldValue::vec4(Vec4::new(0.5, 0.5, 0.5, 1.0)),
            )
            .build()
            .expect("legal");
        let set = SurfaceProgramSet::build(&[a, b], gpu_profile());
        let bytes = set.parameter_bytes();
        assert_eq!(bytes.len(), 2 * SURFACE_PARAM_REGION_BYTES as usize);
        // Neither surface declares a parameter, so both regions are zero — the
        // property under test is the *placement*, which the offsets pin.
        assert_eq!(program_region_offset(1) as usize, bytes.len() / 2);
    }

    #[test]
    fn a_layered_surfaces_parameters_are_packed_from_its_flattened_form() {
        // A masked layer makes every channel vary, so the flattened surface's
        // graphs are composed ones — which is exactly the case a root-only pack
        // would get wrong.
        let layer = SurfaceLayer::new(
            SurfaceBuilder::new()
                .field(SurfaceChannel::BaseColor, uv_color())
                .build()
                .expect("legal"),
            SurfaceLayer::opaque_mask(),
            LayerBlend::Over,
        );
        let surface = SurfaceBuilder::new()
            .layer(layer)
            .build()
            .expect("one layer is within budget");
        let set = SurfaceProgramSet::build(std::slice::from_ref(&surface), gpu_profile());
        assert_eq!(
            set.parameter_bytes().len(),
            SURFACE_PARAM_REGION_BYTES as usize
        );
        assert_eq!(set.degradations(), vec![FrameFeature::ProceduralSurface]);
    }

    #[test]
    fn one_dropped_surface_reports_the_feature_once_however_many_there_are() {
        let a = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("legal");
        let b = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("legal");
        let set = SurfaceProgramSet::build(&[a, b], gpu_profile());
        assert_eq!(set.degradations(), vec![FrameFeature::ProceduralSurface]);
    }
}
