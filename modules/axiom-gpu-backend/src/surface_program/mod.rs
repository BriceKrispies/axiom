//! What an authored [`axiom_surface::Surface`] means to **this** backend.
//!
//! Seven pieces, one per file, and a set that binds them:
//!
//! * [`plan`] — the backend-shaped program plan (stage split, interstage lanes,
//!   parameter layout, program identity).
//! * [`params`] — the uniform channel a surface's tunable numbers ride in, and
//!   the offset scheme that keeps two draws in one pass from reading each
//!   other's parameters.
//! * [`capability`] — the gate, checked once per surface at preparation time.
//! * [`emit_ops`] — one WGSL emitter per field operator, in a `const` table
//!   indexed by the operator code.
//! * [`emit`] — the flat forward pass that turns a surface's channel graphs into
//!   one `axiom_surface` function.
//! * [`emit_vertex`] — the same fold over the one **vertex-stage** channel,
//!   `Displacement`, into one `axiom_displace` function. Both stages compile into
//!   ONE module keyed by ONE digest: a displacing surface must never force a
//!   second pipeline for the same material.
//! * [`wgsl_template`] — the fixed WGSL that function is written against, the
//!   default program, and the concatenation that splices one into the main pass.
//! * [`program_error`] — why a surface produced no runnable program.
//!
//! * [`cache`] — the content-addressed program catalog: the key, the bound, the
//!   deterministic compile order, and the *prepared-or-not* answer a frame needs.
//! * [`compile`] — the same catalog bound to real pipelines, parameter buffers
//!   and bind groups. Device-only, which is why it is a separate file.
//!
//! **Generating a program is not the same as binding one, and both happen at the
//! preparation barrier.** [`cache::SurfaceProgramCatalog::prepare`] runs the
//! emitters once per distinct surface and [`compile::SurfaceProgramCache`] turns
//! each into a pipeline — before `axiom_runtime::RuntimeState::Prepared`, never
//! inside a frame. A frame naming a program the barrier did not prepare renders
//! the constant fallback and reports [`FrameFeature::ProceduralSurface`]; it does
//! **not** compile one, because a lazily compiled variant is exactly the
//! mid-session stutter `crate::post_chain` and `crate::surface_encode` both warn
//! about in writing.
//!
//! A [`SurfaceProgramSet`] is the other half of that story: the **per-frame
//! view**. It generates no text and compiles nothing. It answers what a frame
//! needs to pack its instance stream — a surface's constant channels, its
//! surface-time lane, its pipeline marker — and it is deliberately cheap enough
//! to build every frame. A surface whose channels are all constants needs no
//! program at all: the existing pipeline renders it exactly through the lanes the
//! instance stream already has, and [`RenderCapability::ProceduralSurface`] is
//! now ON in this backend's profile because a surface that does need one can
//! finally be bound.

pub(crate) mod cache;
pub(crate) mod capability;
// Binding a compiled program to a real pipeline, a real parameter buffer and a
// real bind group. Compiled only where a device exists — the same gate
// `mip_chain` and `texture_sampling` carry — which is exactly why the cache's
// SEMANTICS live in `cache` instead, where every platform can test them.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) mod compile;
pub(crate) mod emit;
pub(crate) mod emit_lighting;
pub(crate) mod emit_ops;
pub(crate) mod emit_vertex;
// The CPU/GPU parity proof: every operator driven through both the reference
// evaluator and the emitted shader on a real device. Compiled only with the
// `offscreen` feature, which is what makes a real adapter available — and it
// asserts one was acquired rather than skipping, because a parity test that
// passes when nothing ran proves nothing.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity;
// The same proof for the VERTEX stage — a displacement graph sampled at vertex
// positions — plus the wind/ripple/bend/squash library graphs, which exist to
// show that deformation needed no new Rust operator. Its own file because
// `parity` is already near the engine file-size budget and because the two prove
// different stages.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity_vertex;
// The same proof for the three LIGHTING MODELS, driven through the main pass's
// real `fs` — the whole shader, not a restatement of it — on a real device. Its
// own file for the same reason `parity_vertex` is: `parity` is already near the
// engine file-size budget, and this proves a third thing (how much of the
// lighting maths a model takes) rather than a third operator.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity_lighting;
// **The picture.** A prepared program, bound to the real `SceneRenderer`, and the
// pixels it produced: a displacing surface deforms a captured silhouette, a
// field-authored colour renders the field's ramp rather than the constant
// fallback, and a replayed tick is byte-identical. The debt manifests 10 and 11
// left, which only a bound program could pay.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod bound_image;

pub(crate) mod params;
pub(crate) mod plan;
pub(crate) mod program_error;

// The fixed WGSL a generated program is spliced into. Needed only where a real
// shader is compiled — the live wasm arm, the off-screen arm, and tests — which
// is the same gate `mip_chain` and `texture_sampling` carry.
#[cfg(any(test, target_arch = "wasm32", feature = "offscreen"))]
pub(crate) mod wgsl_template;

use axiom_host::{BackendCapabilityProfile, FrameFeature};
use axiom_kernel::Seconds;
use axiom_surface::{Surface, SurfaceChannel};

use crate::surface_program::capability::GeometryPath;
use crate::surface_program::emit_lighting::{pipeline_kind, PIPELINE_BASIC_LIT};
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
    fn build(
        surface: &Surface,
        profile: BackendCapabilityProfile,
        geometry: GeometryPath,
    ) -> SurfaceProgramEntry {
        let plan = SurfaceProgramPlan::of(surface);
        let programmed = plan.stage_split().fragment_channels();
        let emission = constant_lanes(surface, SurfaceChannel::Emission, programmed, [0.0; 4]);
        // **A set is a per-frame VIEW; it generates no shader text.** Flattening
        // is the one thing both emitters can fail at — each of them starts by
        // flattening and maps that error — so asking the surface directly is the
        // same fact for a fraction of the work, and it is asked ONCE here for both
        // the degradation and the parameter pack.
        //
        // The emitters themselves run in `crate::surface_program::cache`, at the
        // preparation barrier, exactly once per distinct surface. They used to run
        // here, which meant a frame carrying N surfaces generated 2N WGSL
        // functions and threw them away — per frame.
        let flat = surface.flatten();
        SurfaceProgramEntry {
            plan,
            degraded: capability::validate(&plan, profile, geometry)
                .err()
                .map(|_| FrameFeature::ProceduralSurface)
                .or_else(|| flat.as_ref().err().map(|_| FrameFeature::ProceduralSurface)),
            color: constant_lanes(surface, SurfaceChannel::BaseColor, programmed, [1.0; 4]),
            emissive: [emission[0], emission[1], emission[2]],
            // Packed from the surface's FLATTENED form: flattening is what
            // composes a layered surface's per-channel graphs, and therefore its
            // parameter table, into one. A surface that does not flatten packs no
            // parameters rather than half of them — it cannot happen for a
            // validated `Surface`, whose every constructor validates.
            params: flat
                .as_ref()
                .map(|flat| pack(plan.param_layout(), flat))
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
    /// Plan and validate `surfaces` against `profile` for the **rigid** vertex
    /// path — the one every `axiom_host::FramePacket` draw takes.
    pub(crate) fn build(
        surfaces: &[Surface],
        profile: BackendCapabilityProfile,
    ) -> SurfaceProgramSet {
        SurfaceProgramSet::build_for(surfaces, profile, GeometryPath::Rigid)
    }

    /// Plan and validate `surfaces` against `profile` for `geometry`.
    ///
    /// The path is a parameter because exactly one ceiling depends on it: the
    /// skinned vertex stage is at the 16-attribute limit and cannot run a
    /// displacement program (see [`GeometryPath`]). Everything else about a
    /// surface is the same on both.
    pub(crate) fn build_for(
        surfaces: &[Surface],
        profile: BackendCapabilityProfile,
        geometry: GeometryPath,
    ) -> SurfaceProgramSet {
        SurfaceProgramSet {
            entries: surfaces
                .iter()
                .map(|surface| SurfaceProgramEntry::build(surface, profile, geometry))
                .collect(),
        }
    }

    /// The seconds the pass writes into its surface-time lane for this set,
    /// given the time the frame supplied.
    ///
    /// **A set whose surfaces read no clock is written an exact zero**, whatever
    /// the frame supplied — so a static surface's frame is byte-identical to the
    /// frame it produced before there was a clock at all, and the packed
    /// lighting uniform it rides in is unchanged to the bit. A set holding one
    /// clock-reading surface is written the frame's own time; the lane sits in a
    /// uniform that is already written once per frame, so that costs no extra
    /// write.
    ///
    /// Time is the one input the frame has to *supply* — every other input to a
    /// program is a varying the vertex stage already writes or a parameter the
    /// surface itself declares — which is why the decision lives here, with the
    /// set that knows whether anything asked.
    pub(crate) fn surface_time(&self, supplied: Seconds) -> f32 {
        let reads = self.entries.iter().any(|entry| entry.plan.reads_time());
        [0.0, supplied.get()][usize::from(reads)]
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

    /// The pipeline marker a draw naming `program_id` selects: the mirror of
    /// `axiom_render::RenderPipelineKind`, **derived** from the surface rather
    /// than carried on the draw.
    ///
    /// The render module has emitted `BASIC_LIT`/`UNLIT` per object since it was
    /// written, run-length-encoded into its `SetPipeline` stream — and the value
    /// died at the `axiom_host::FramePacket` boundary, which carries no pipeline
    /// lane. This is the other end of that seam, and deriving it is the right
    /// end to fix: the packet exists to stay primitive-only, the surface digest
    /// it *already* carries names the surface, and a surface knows how it is lit.
    /// A program this backend was never handed, and the `program_id == 0` every
    /// draw that authored no surface carries, are lit — which is what the engine
    /// has always done.
    ///
    /// It selects no second pipeline **here**: this backend runs one lit program
    /// and the model is a value inside it. The marker is what a composing tier
    /// asks for when it is translating a packet into a submission for a backend
    /// that does keep two.
    pub(crate) fn pipeline_kind(&self, program_id: u64) -> u32 {
        self.entries
            .iter()
            .find(|entry| entry.plan.program_id() == program_id)
            .map_or(PIPELINE_BASIC_LIT, |entry| {
                pipeline_kind(entry.plan.lighting())
            })
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
    fn a_surface_whose_program_will_not_emit_is_reported_dropped_even_by_a_full_profile() {
        // Two 127-node chains total 254 nodes, which is INSIDE the 256-node
        // shader budget the capability gate checks — but composing them through a
        // masked layer does not fit the field node budget, so the surface will
        // not flatten into one program. Nothing
        // about its CAPABILITIES is wrong — a profile that attempts procedural
        // surfaces still admits it — so the emission attempt is the only thing
        // that can catch it, and it must.
        let over = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, chain("gpu/set/under", 63))
            .layer(SurfaceLayer::new(
                SurfaceBuilder::new()
                    .field(SurfaceChannel::Opacity, chain("gpu/set/over", 63))
                    .build()
                    .expect("legal"),
                SurfaceLayer::opaque_mask(),
                LayerBlend::Over,
            ))
            .build()
            .expect("one layer is within budget");
        let attempting =
            BackendCapabilityProfile::all().with(RenderCapability::ProceduralSurface);
        assert_eq!(
            capability::validate(
                &SurfaceProgramPlan::of(&over),
                attempting,
                GeometryPath::Rigid
            ),
            Ok(())
        );
        let set = SurfaceProgramSet::build(std::slice::from_ref(&over), attempting);
        assert_eq!(set.degradations(), vec![FrameFeature::ProceduralSurface]);
    }

    /// A scalar chain of `steps` `Add`s over fresh constants: `2 * steps + 1`
    /// nodes.
    fn chain(name: &str, steps: u16) -> axiom_field::FieldGraph {
        let (builder, node) = (0..steps).fold(
            FieldBuilder::new(FieldId::of_name(name), 1)
                .push_const(FieldValue::scalar(axiom_recipe::Scalar::new(1.0))),
            |(builder, acc), _| {
                let (builder, one) =
                    builder.push_const(FieldValue::scalar(axiom_recipe::Scalar::new(1.0)));
                builder.push(FieldOp::Add, Vec::new(), vec![acc, one])
            },
        );
        builder.build(node)
    }

    /// A time-varying displacement — wind, ripple — and a static one, so the
    /// difference between "reads the clock" and "does not" is a real pair.
    fn clock_displacement() -> axiom_field::FieldGraph {
        let (builder, clock) = FieldBuilder::new(FieldId::of_name("gpu/set/wind"), 1).push(
            FieldOp::Time,
            Vec::new(),
            Vec::new(),
        );
        let (builder, node) = builder.push(
            FieldOp::Compose,
            vec![Param::int(3)],
            vec![clock, clock, clock],
        );
        builder.build(node)
    }

    /// **A surface that reads no clock is written no time.** The lane it would
    /// ride in holds an exact zero whatever the frame supplied, which is what
    /// makes a static surface cost precisely what it did before there was a
    /// clock — the packed lighting uniform is unchanged to the bit.
    #[test]
    fn a_set_with_no_clock_reading_surface_is_written_an_exact_zero() {
        let supplied = Seconds::finite_or_zero(12.5);
        // The empty set: every app that authors no surface at all.
        assert_eq!(SurfaceProgramSet::default().surface_time(supplied), 0.0);
        // A field-authored but static surface.
        let still = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("legal");
        let set = SurfaceProgramSet::build(std::slice::from_ref(&still), gpu_profile());
        assert_eq!(set.surface_time(supplied), 0.0);
        assert_eq!(set.surface_time(Seconds::finite_or_zero(0.0)), 0.0);
    }

    /// One clock-reading surface in the set is what turns the lane on, and it is
    /// written the frame's own supplied time — never a wall clock, so the same
    /// tick replays to the same displacement.
    #[test]
    fn a_set_holding_one_clock_reading_surface_is_written_the_frames_own_time() {
        let windy = SurfaceBuilder::new()
            .field(SurfaceChannel::Displacement, clock_displacement())
            .build()
            .expect("a vec3 field is a legal displacement");
        let still = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("legal");
        let set = SurfaceProgramSet::build(&[still, windy], gpu_profile());
        assert_eq!(set.surface_time(Seconds::finite_or_zero(12.5)), 12.5);
        // Replay: the same supplied time yields the same lane, exactly.
        assert_eq!(set.surface_time(Seconds::finite_or_zero(12.5)), 12.5);
        assert_ne!(
            set.surface_time(Seconds::finite_or_zero(13.5)),
            set.surface_time(Seconds::finite_or_zero(12.5))
        );
    }

    /// A displacing surface lowers on the rigid path and is a **reported** drop
    /// on the skinned one — the attribute ceiling, stated, never a silent no-op.
    #[test]
    fn a_displacing_surface_is_dropped_on_the_skinned_path_and_kept_on_the_rigid_one() {
        let attempting =
            BackendCapabilityProfile::all().with(RenderCapability::ProceduralSurface);
        let windy = SurfaceBuilder::new()
            .field(SurfaceChannel::Displacement, clock_displacement())
            .build()
            .expect("a vec3 field is a legal displacement");
        let rigid = SurfaceProgramSet::build_for(
            std::slice::from_ref(&windy),
            attempting,
            GeometryPath::Rigid,
        );
        assert!(rigid.degradations().is_empty());
        let skinned = SurfaceProgramSet::build_for(
            std::slice::from_ref(&windy),
            attempting,
            GeometryPath::Skinned,
        );
        assert_eq!(skinned.degradations(), vec![FrameFeature::ProceduralSurface]);
    }

    /// **The no-new-variants proof, at the set.** A scene's program count is the
    /// number of distinct surfaces in it, whichever lighting model they use — N,
    /// never 3N — because the model is a value inside the one program a surface
    /// already has, not an axis a program is specialised along.
    ///
    /// The parameter buffer is the observable size of the set (one fixed region
    /// per program), so it is what the assertion reads: it is the number a cache
    /// or a bind-group allocator would be sized from.
    #[test]
    fn the_program_count_of_a_scene_is_independent_of_its_lighting_models() {
        let scene = |model: axiom_surface::LightingModel| {
            [0_u16, 1, 2]
                .iter()
                .map(|index| {
                    SurfaceBuilder::new()
                        .lighting(model)
                        .field(SurfaceChannel::Opacity, chain("gpu/set/count", *index))
                        .build()
                        .expect("legal")
                })
                .collect::<Vec<_>>()
        };
        let counts: Vec<usize> = axiom_surface::LightingModel::ALL
            .iter()
            .map(|model| {
                let surfaces = scene(*model);
                let set = SurfaceProgramSet::build(&surfaces, gpu_profile());
                assert_eq!(set.entries.len(), surfaces.len());
                set.parameter_bytes().len() / SURFACE_PARAM_REGION_BYTES as usize
            })
            .collect();
        assert_eq!(counts, vec![3, 3, 3], "a lighting model must cost 0 programs");
        // And mixing all three models in ONE scene is still one program each —
        // the case a variant-keyed design would have paid three times over.
        let mixed: Vec<axiom_surface::Surface> = axiom_surface::LightingModel::ALL
            .iter()
            .map(|model| {
                SurfaceBuilder::new()
                    .lighting(*model)
                    .field(SurfaceChannel::BaseColor, uv_color())
                    .build()
                    .expect("legal")
            })
            .collect();
        let set = SurfaceProgramSet::build(&mixed, gpu_profile());
        assert_eq!(set.entries.len(), 3);
        assert_eq!(
            set.parameter_bytes().len(),
            3 * SURFACE_PARAM_REGION_BYTES as usize
        );
    }

    /// **`RenderPipelineKind::UNLIT` reaches a backend.** The marker is derived
    /// from the surface a draw's `surface_program` names — the first time in the
    /// repository's history that the render module's second pipeline marker is
    /// answered by a backend rather than dropped at the packet boundary.
    #[test]
    fn an_unlit_surface_selects_the_unlit_pipeline_marker_and_everything_else_is_lit() {
        let surfaces: Vec<axiom_surface::Surface> = axiom_surface::LightingModel::ALL
            .iter()
            .map(|model| {
                SurfaceBuilder::new()
                    .lighting(*model)
                    .field(SurfaceChannel::BaseColor, uv_color())
                    .build()
                    .expect("legal")
            })
            .collect();
        let set = SurfaceProgramSet::build(&surfaces, gpu_profile());
        assert_eq!(
            set.pipeline_kind(surfaces[0].digest().raw()),
            emit_lighting::PIPELINE_UNLIT
        );
        assert_eq!(
            set.pipeline_kind(surfaces[1].digest().raw()),
            emit_lighting::PIPELINE_BASIC_LIT
        );
        assert_eq!(
            set.pipeline_kind(surfaces[2].digest().raw()),
            emit_lighting::PIPELINE_BASIC_LIT
        );
        // The draw that authored no surface, and a digest this backend was never
        // handed, are both lit — the engine's behaviour before models existed.
        assert_eq!(set.pipeline_kind(0), emit_lighting::PIPELINE_BASIC_LIT);
        assert_eq!(set.pipeline_kind(0xDEAD), emit_lighting::PIPELINE_BASIC_LIT);
        assert_eq!(
            SurfaceProgramSet::default().pipeline_kind(0),
            emit_lighting::PIPELINE_BASIC_LIT
        );
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
