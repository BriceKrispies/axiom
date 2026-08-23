//! The engine's first **content-addressed** program catalog: which surfaces were
//! prepared, in which order, and the WGSL each one compiles to.
//!
//! ## Why a catalog and a cache are two things
//!
//! This module holds the half of the cache that has no device in it: the key,
//! the bound, the deterministic order, the overflow error and the
//! *prepared-or-not* answer a frame needs. [`crate::surface_program::compile`]
//! holds the other half — the `wgpu::RenderPipeline`, the parameter buffer and
//! the bind group — because those need an adapter and therefore cannot be
//! compiled natively or measured by the coverage gate. Splitting them is what
//! puts the cache's *semantics* under test on every platform, and it is the same
//! split `mip_chain` and `texture_sampling` already make against
//! `scene_renderer`.
//!
//! ## The key is the surface's own digest, and that is the whole design
//!
//! Every other cache in this engine — `meshes: HashMap<u64, MeshBuffers>`,
//! `materials: HashMap<u64, BindGroup>`, the Canvas 2D `MeshCache` — is keyed on
//! a **caller-assigned** id, so two byte-identical resources upload twice. This
//! one is keyed on [`axiom_surface::Surface::digest`], a structural content
//! hash, so two surfaces authored independently that compute the same thing
//! **collapse to one program**. That collapse is the only structural defence
//! against variant explosion, and it is why the cap below can be a small number
//! rather than a guess.
//!
//! The property that makes the key work was designed into the field layer: **a
//! parameter value change does not move the digest.** Animating a material
//! parameter rewrites a uniform; it never compiles anything, and it never grows
//! this catalog. `parameter_animation_never_changes_the_catalog` is the test
//! that pins it.
//!
//! ## Compilation happens at the preparation barrier. Only there.
//!
//! `crates/axiom-runtime` already owns a startup phase whose stated invariant is
//! that the deterministic simulation cannot advance until preparation has
//! completed. Shader compilation is exactly the shape of work that phase exists
//! for, and the renderer's own comments state the doctrine twice — see
//! `crate::post_chain`'s note that *"a frame that toggles either one on and off
//! does not change which pipelines exist, so it cannot stutter on a pipeline the
//! driver compiles the first time it is used"*, and the same sentence again in
//! `crate::surface_encode`. On the browser's WebGL2 fallback path `wgpu`
//! cross-compiles WGSL to GLSL at pipeline creation, so a lazily compiled variant
//! is a guaranteed mid-session hitch.
//!
//! So there is **no lazy compilation anywhere in this module or the next**. A
//! frame naming a program the barrier never prepared is reported through
//! [`axiom_host::FrameFeature::ProceduralSurface`] and rendered with the constant
//! fallback. It is a miss, not a trigger.
//!
//! ## Nothing here persists
//!
//! No disk cache, no `.axpkg`, no shader binaries, no eviction. Programs are
//! regenerated each launch, and a bounded catalog that fails loudly at startup
//! beats an evicting one that stutters mid-session.

use axiom_host::{BackendCapabilityProfile, FrameFeature};
use axiom_kernel::{KernelError, KernelErrorCode, KernelErrorScope};
use axiom_surface::Surface;

use crate::surface_program::capability::{self, GeometryPath};
use crate::surface_program::emit_lighting::fragment_program;
use crate::surface_program::emit_vertex::displace_function;
use crate::surface_program::params::pack;
use crate::surface_program::plan::SurfaceProgramPlan;

/// How many distinct surface programs one preparation may compile.
///
/// A **bound**, not a budget to grow into. An unbounded pipeline cache is the
/// exact failure mode the anti-variant doctrine warns about, and a bound that
/// fails at the barrier is a design signal an author can act on — while an
/// eviction policy would turn the same authoring mistake into an unattributable
/// mid-session stutter. Sixty-four is generous *because* the digest collapses
/// duplicates: a scene needs sixty-four structurally distinct materials to reach
/// it, not sixty-four material instances.
pub(crate) const MAX_SURFACE_PROGRAMS: usize = 64;

/// Preparation asked for more programs than the catalog may hold.
///
/// The kernel's own error vocabulary rather than a bespoke type: the identity
/// that matters is `(scope, code)`, and a program count past a fixed capacity is
/// precisely an index outside the bounds of the storage that was reserved for
/// it.
pub(crate) const SURFACE_PROGRAM_OVERFLOW: KernelError = KernelError::new(
    KernelErrorScope::Memory,
    KernelErrorCode::OutOfBounds,
    "more distinct surface programs than the bounded program cache holds",
);

/// One surface's program, as text and bytes — everything a device needs and
/// nothing that needs a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceProgramSource {
    program_id: u64,
    /// Which **program** this region belongs to — the surface's digest.
    ///
    /// Separate from `program_id` (the region) so that N materials sharing a
    /// shape compile ONE pipeline and bind N parameter blocks. That is the
    /// property `Surface::digest`'s doc promises and this field is what keeps
    /// it once the region key stopped being the digest.
    pipeline_key: u64,
    vertex: String,
    fragment: String,
    params: Vec<u8>,
}

impl SurfaceProgramSource {
    /// The cache key: [`axiom_surface::Surface::digest`], which is the same
    /// number `axiom_host::FrameDrawItem::surface_program` carries.
    pub(crate) const fn program_id(&self) -> u64 {
        self.program_id
    }

    /// The generated `axiom_displace` — the vertex half.
    pub(crate) fn vertex(&self) -> &str {
        &self.vertex
    }

    /// The generated `axiom_lighting_model` + `axiom_surface` — the fragment
    /// half. Both halves compile into **one** module keyed by **one** digest: a
    /// displacing surface must never force a second pipeline for the same
    /// material.
    pub(crate) fn fragment(&self) -> &str {
        &self.fragment
    }

    /// The bytes of this program's own parameter region.
    ///
    /// Its **own** region, never a slice of one buffer rewritten between draws:
    /// `crate::post_chain` records that a `queue.write_buffer` is ordered against
    /// *submission*, not against the passes inside an encoder, so N writes to one
    /// buffer leave every draw in the pass reading the last of them. The engine
    /// paid for that bug once already.
    pub(crate) fn params(&self) -> &[u8] {
        &self.params
    }

    /// Which pipeline this region's draws are drawn with — the surface's digest.
    pub(crate) const fn pipeline_key(&self) -> u64 {
        self.pipeline_key
    }
}

/// Every surface one preparation saw, and the program each of them compiles to.
///
/// Two ordered vectors rather than a map, deliberately: preparation must be
/// deterministic, and the order a `HashMap` iterates in is not. Both are sorted
/// by digest, so the compile order is a function of the surface *set* and not of
/// the order an app happened to author them in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SurfaceProgramCatalog {
    /// The programs, ascending by digest. A surface whose every channel is a
    /// plain constant is **absent**: it needs no program, and the existing
    /// pipeline renders it exactly through the instance lanes.
    programs: Vec<SurfaceProgramSource>,
    /// Every digest the barrier saw, ascending — program or not. This is what
    /// separates "prepared, needed no program" from "never prepared", and only
    /// the second is a degraded frame.
    prepared: Vec<u64>,
}

impl SurfaceProgramCatalog {
    /// Prepare `surfaces` for the **rigid** vertex path — the one every
    /// `axiom_host::FramePacket` draw takes.
    pub(crate) fn prepare(
        surfaces: &[Surface],
        profile: BackendCapabilityProfile,
    ) -> Result<SurfaceProgramCatalog, KernelError> {
        SurfaceProgramCatalog::prepare_for(surfaces, profile, GeometryPath::Rigid)
    }

    /// Prepare `surfaces` for `geometry`, in **sorted digest order**.
    ///
    /// Sorted because preparation must be deterministic: the same surface set
    /// must produce the same catalog, the same program ids and the same compile
    /// order however the app assembled the slice. Deduplicated because the key is
    /// content-addressed — two equal surfaces are one program, which is the
    /// property the cap depends on.
    ///
    /// Fails with [`SURFACE_PROGRAM_OVERFLOW`] rather than truncating: a
    /// preparation that quietly dropped the sixty-fifth program would hand the
    /// frame a miss it could not explain.
    pub(crate) fn prepare_for(
        surfaces: &[Surface],
        profile: BackendCapabilityProfile,
        geometry: GeometryPath,
    ) -> Result<SurfaceProgramCatalog, KernelError> {
        let mut ordered: Vec<&Surface> = surfaces.iter().collect();
        // By PARAMETER REGION, not by digest. Two runtime materials with the
        // same shape and different numbers are one program and TWO regions;
        // deduplicating them by digest kept one and silently shaded both with
        // it.
        ordered.sort_by_key(|surface| surface.param_key().raw());
        ordered.dedup_by_key(|surface| surface.param_key().raw());
        let prepared: Vec<u64> = ordered
            .iter()
            .map(|surface| surface.digest().raw())
            .collect();
        let programs: Vec<SurfaceProgramSource> = ordered
            .iter()
            .filter_map(|surface| generate(surface, profile, geometry))
            .collect();
        (programs.len() <= MAX_SURFACE_PROGRAMS)
            .then_some(SurfaceProgramCatalog { programs, prepared })
            .ok_or(SURFACE_PROGRAM_OVERFLOW)
    }

    /// The programs, ascending by digest — the order a device compiles them in.
    pub(crate) fn sources(&self) -> &[SurfaceProgramSource] {
        &self.programs
    }

    /// How many programs this catalog holds. The number a scene asserts against
    /// so a variant explosion shows up as a failing test rather than a slow
    /// frame.
    pub(crate) fn program_count(&self) -> u32 {
        self.programs.len() as u32
    }

    /// How many surfaces the barrier saw, program or not.
    pub(crate) fn prepared_count(&self) -> u32 {
        self.prepared.len() as u32
    }

    /// The program `program_id` names, or `None` when this surface needed none.
    pub(crate) fn source(&self, program_id: u64) -> Option<&SurfaceProgramSource> {
        self.programs
            .binary_search_by_key(&program_id, SurfaceProgramSource::program_id)
            .ok()
            .and_then(|index| self.programs.get(index))
    }

    /// Whether the barrier saw this surface at all.
    ///
    /// `0` — the number every draw that authored no surface carries — is always
    /// prepared: it is the default program, which is compiled into the pass
    /// itself and cannot be missing.
    pub(crate) fn is_prepared(&self, program_id: u64) -> bool {
        (program_id == 0) | self.prepared.binary_search(&program_id).is_ok()
    }

    /// The features a frame drawing `program_ids` could not honour.
    ///
    /// **A miss is a report, never a compile.** A draw naming a program this
    /// catalog does not hold renders the constant fallback — the neutral
    /// `(white, black)` `crate::frame_packet_adapter` folds for an unknown
    /// digest — and the frame says so through
    /// `axiom_host::FrameSubmissionReport::degraded_features`. Deduplicated to a
    /// single entry however many draws missed, because the report enumerates
    /// *features*, not occurrences.
    pub(crate) fn degradations(&self, program_ids: &[u64]) -> Vec<FrameFeature> {
        program_ids
            .iter()
            .find(|program_id| !self.is_prepared(**program_id))
            .map(|_| FrameFeature::ProceduralSurface)
            .into_iter()
            .collect()
    }
}

/// The program `surface` compiles to, or `None` when it needs none.
///
/// Three ways to need none, and they are different facts:
///
/// * every channel is a plain constant, so the existing pipeline renders it
///   exactly through the instance lanes — the compatibility contract that keeps
///   an app which authors only constant materials paying nothing;
/// * the capability gate refuses it (too many nodes, a lane the interface does
///   not carry, a displacement on the skinned path), which
///   `crate::surface_program::SurfaceProgramSet::degradations` already reports;
/// * it will not flatten into one program, which the emitters report.
fn generate(
    surface: &Surface,
    profile: BackendCapabilityProfile,
    geometry: GeometryPath,
) -> Option<SurfaceProgramSource> {
    // A RUNTIME MATERIAL short-circuits the whole generator. Its WGSL is
    // hand-written (`crate::material_shader`) because the field algebra cannot
    // express a loop, a derivative or a texture fetch, and its parameters are
    // authored values rather than a flattened graph — so there is no plan to
    // make, no capability to validate against the algebra's vocabulary, and
    // nothing to flatten.
    //
    // Written as an `Option` chain rather than a branch: this is spine code and
    // the Branchless Law applies. `program_id` still comes from the surface's
    // own digest, which carries the KIND but not the parameter values — so every
    // runtime material in a scene is one program and one pipeline, differing
    // only in the bytes below.
    surface
        .kind()
        .material_params()
        .zip(displace_function(surface).ok())
        .map(|(params, vertex)| SurfaceProgramSource {
            program_id: SurfaceProgramPlan::of(surface).program_id(),
            pipeline_key: surface.digest().raw(),
            // The VERTEX half comes from the same emitter the field path uses,
            // not from a hard-coded zero: a runtime material binds Displacement
            // to its channel default, so this emits exactly the zero offset — and
            // it stays correct on its own if that default ever changes, rather
            // than agreeing with it by coincidence.
            vertex,
            // One call composes both halves: the de-tile gate decides which of the
            // two program shapes is emitted, and the same parameters pack the
            // block. Keeping them together is what stops a program being paired
            // with a block packed from different values.
            fragment: crate::material_shader::compose::material_program(&params).wgsl,
            params: crate::material_shader::params::param_bytes(&params),
        })
        .or_else(|| generate_field_program(surface, profile, geometry))
}

/// The original generator: WGSL emitted from a surface's channel bindings.
fn generate_field_program(
    surface: &Surface,
    profile: BackendCapabilityProfile,
    geometry: GeometryPath,
) -> Option<SurfaceProgramSource> {
    let plan = SurfaceProgramPlan::of(surface);
    let split = plan.stage_split();
    let wanted = (split.fragment_channels() != 0) | split.has_vertex_stage();
    let admitted = capability::validate(&plan, profile, geometry).is_ok();
    (wanted & admitted)
        .then_some(())
        .and_then(|_| fragment_program(surface).ok().zip(displace_function(surface).ok()))
        .zip(surface.flatten().ok())
        .map(|((fragment, vertex), flat)| SurfaceProgramSource {
            program_id: plan.program_id(),
            pipeline_key: surface.digest().raw(),
            vertex,
            fragment,
            params: pack(plan.param_layout(), &flat),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldGraph, FieldId, FieldOp, FieldType, FieldValue};
    use axiom_host::RenderCapability;
    use axiom_math::{Vec3, Vec4};
    use axiom_recipe::{Param, Scalar};
    use axiom_surface::{LayerBlend, SurfaceBuilder, SurfaceChannel, SurfaceLayer};

    /// The profile the GPU backend now ships: everything, procedural surfaces
    /// included.
    fn gpu_profile() -> BackendCapabilityProfile {
        BackendCapabilityProfile::all()
    }

    /// A vec4 base colour driven by `Uv.x` — the canonical field-authored
    /// surface, which needs a program.
    fn uv_color() -> FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("gpu/cache/uv"), 1).push(
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

    /// A vec4 base colour scaled by a declared parameter — the surface whose
    /// *value* an app animates.
    fn tinted(tint: f32) -> Surface {
        let (builder, slot) = FieldBuilder::new(FieldId::of_name("gpu/cache/tint"), 1)
            .declare("tint", FieldValue::scalar(Scalar::new(tint)));
        let (builder, param) = builder.push_param(slot, FieldType::Scalar);
        let (builder, uv) = builder.push(FieldOp::Uv, Vec::new(), Vec::new());
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let (builder, scaled) = builder.push(FieldOp::Mul, Vec::new(), vec![lane, param]);
        let (builder, splat) = builder.push(
            FieldOp::Compose,
            vec![Param::int(4)],
            vec![scaled, scaled, scaled, scaled],
        );
        SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, builder.build(splat))
            .build()
            .expect("a vec4 parameterised field is a legal base colour")
    }

    /// A scalar chain of `steps` `Add`s over fresh constants — a knob for making
    /// distinct surfaces cheaply.
    fn chain(name: &str, steps: u16) -> FieldGraph {
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

    /// `count` structurally distinct field-authored surfaces.
    fn distinct(count: u16) -> Vec<Surface> {
        (0..count)
            .map(|index| {
                SurfaceBuilder::new()
                    .field(SurfaceChannel::Opacity, chain("gpu/cache/many", index))
                    .build()
                    .expect("a scalar chain is a legal opacity")
            })
            .collect()
    }

    #[test]
    fn an_empty_preparation_holds_nothing_and_degrades_nothing() {
        let catalog = SurfaceProgramCatalog::prepare(&[], gpu_profile()).expect("empty fits");
        assert_eq!(catalog.program_count(), 0);
        assert_eq!(catalog.prepared_count(), 0);
        assert!(catalog.sources().is_empty());
        assert_eq!(catalog, SurfaceProgramCatalog::default());
        // The draw that authored no surface is always prepared: its program is
        // compiled into the pass itself.
        assert!(catalog.is_prepared(0));
        assert!(catalog.degradations(&[0, 0, 0]).is_empty());
        assert!(format!("{catalog:?}").contains("SurfaceProgramCatalog"));
    }

    /// **Two equal surfaces are one program; two different surfaces are two.**
    /// The content-addressed collapse, which is the whole reason the cap can be a
    /// small number.
    #[test]
    fn equal_surfaces_collapse_to_one_program_and_different_ones_do_not() {
        let one = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("legal");
        let twin = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("legal");
        assert_eq!(one.digest().raw(), twin.digest().raw());
        let collapsed = SurfaceProgramCatalog::prepare(&[one.clone(), twin], gpu_profile())
            .expect("two equal surfaces are one program");
        assert_eq!(collapsed.program_count(), 1);
        assert_eq!(collapsed.prepared_count(), 1);

        let other = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, chain("gpu/cache/other", 2))
            .build()
            .expect("legal");
        let both = SurfaceProgramCatalog::prepare(&[one, other], gpu_profile())
            .expect("two distinct surfaces fit");
        assert_eq!(both.program_count(), 2);
    }

    /// **The load-bearing test: animating a parameter never compiles anything.**
    ///
    /// A hundred frames of a moving tint value produce a hundred identical
    /// catalogs — same size, same program id, same generated text — and differ
    /// only in the parameter bytes, which is a uniform write. If this ever fails,
    /// every material tweak in the engine has become a pipeline compile.
    #[test]
    fn parameter_animation_never_changes_the_catalog() {
        let base = SurfaceProgramCatalog::prepare(
            std::slice::from_ref(&tinted(0.0)),
            gpu_profile(),
        )
        .expect("one program fits");
        let program_id = base.sources()[0].program_id();
        let text = String::from(base.sources()[0].fragment());
        let moved: Vec<Vec<u8>> = (0..100)
            .map(|frame| {
                let animated = tinted(frame as f32 / 100.0);
                let catalog =
                    SurfaceProgramCatalog::prepare(std::slice::from_ref(&animated), gpu_profile())
                        .expect("still one program");
                assert_eq!(catalog.program_count(), 1, "frame {frame} compiled a variant");
                assert_eq!(catalog.sources()[0].program_id(), program_id);
                assert_eq!(catalog.sources()[0].fragment(), text);
                Vec::from(catalog.sources()[0].params())
            })
            .collect();
        // The digest never moved, but the bytes the uniform carries did — which
        // is the only thing an animated parameter is allowed to change.
        assert_ne!(moved[0], moved[99]);
        assert_eq!(moved[0].len(), 512);
    }

    /// A constant-only surface is **prepared but programless**: it needs no
    /// shader, so it costs no cache slot, and a draw naming it is not degraded.
    #[test]
    fn a_constant_only_surface_is_prepared_without_costing_a_program() {
        let constant = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::BaseColor,
                FieldValue::vec4(Vec4::new(0.2, 0.4, 0.6, 1.0)),
            )
            .build()
            .expect("legal");
        let id = constant.digest().raw();
        let catalog = SurfaceProgramCatalog::prepare(std::slice::from_ref(&constant), gpu_profile())
            .expect("fits");
        assert_eq!(catalog.program_count(), 0);
        assert_eq!(catalog.prepared_count(), 1);
        assert!(catalog.is_prepared(id));
        assert_eq!(catalog.source(id), None);
        assert!(catalog.degradations(&[id]).is_empty());
    }

    /// A **constant** displacement still needs a vertex program, so it is one of
    /// the surfaces that does cost a slot.
    #[test]
    fn a_constant_displacement_still_costs_a_program_because_it_moves_vertices() {
        let pushed = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(Vec3::new(0.0, 0.5, 0.0)),
            )
            .build()
            .expect("legal");
        let catalog = SurfaceProgramCatalog::prepare(std::slice::from_ref(&pushed), gpu_profile())
            .expect("fits");
        assert_eq!(catalog.program_count(), 1);
        let source = catalog
            .source(pushed.digest().raw())
            .expect("a displacing surface has a program");
        assert!(source.vertex().contains("fn axiom_displace("));
        assert!(source.fragment().contains("fn axiom_surface("));
        assert!(source.fragment().contains("fn axiom_lighting_model()"));
        assert_eq!(source.params().len(), 512);
        // …and it is refused on the SKINNED path, where the vertex stage is at
        // the 16-attribute ceiling — so it costs no program there.
        let skinned = SurfaceProgramCatalog::prepare_for(
            std::slice::from_ref(&pushed),
            gpu_profile(),
            GeometryPath::Skinned,
        )
        .expect("fits");
        assert_eq!(skinned.program_count(), 0);
        assert_eq!(skinned.prepared_count(), 1);
    }

    /// A profile that does not attempt procedural surfaces compiles none — the
    /// state this backend shipped in before it could bind one.
    #[test]
    fn a_profile_without_the_capability_compiles_no_program() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("legal");
        let catalog = SurfaceProgramCatalog::prepare(
            std::slice::from_ref(&surface),
            BackendCapabilityProfile::all().without(RenderCapability::ProceduralSurface),
        )
        .expect("fits");
        assert_eq!(catalog.program_count(), 0);
        assert!(catalog.is_prepared(surface.digest().raw()));
    }

    /// A surface that will not flatten into one program is prepared without a
    /// program rather than compiled into a broken one.
    #[test]
    fn a_surface_that_will_not_flatten_compiles_no_program() {
        let over = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, chain("gpu/cache/under", 63))
            .layer(SurfaceLayer::new(
                SurfaceBuilder::new()
                    .field(SurfaceChannel::Opacity, chain("gpu/cache/over", 63))
                    .build()
                    .expect("legal"),
                SurfaceLayer::opaque_mask(),
                LayerBlend::Over,
            ))
            .build()
            .expect("one layer is within budget");
        let catalog =
            SurfaceProgramCatalog::prepare(std::slice::from_ref(&over), gpu_profile()).expect("fits");
        assert_eq!(catalog.program_count(), 0);
        assert!(catalog.is_prepared(over.digest().raw()));
    }

    /// **The cap fails preparation loudly.** Exactly sixty-four programs is the
    /// last legal catalog; the sixty-fifth is a structured error, never a
    /// truncation and never an eviction.
    #[test]
    fn the_cap_admits_sixty_four_programs_and_refuses_the_sixty_fifth() {
        assert_eq!(MAX_SURFACE_PROGRAMS, 64);
        let full = distinct(MAX_SURFACE_PROGRAMS as u16);
        let catalog = SurfaceProgramCatalog::prepare(&full, gpu_profile())
            .expect("exactly the cap must fit");
        assert_eq!(catalog.program_count(), MAX_SURFACE_PROGRAMS as u32);

        let over = distinct(MAX_SURFACE_PROGRAMS as u16 + 1);
        let error = SurfaceProgramCatalog::prepare(&over, gpu_profile())
            .expect_err("one past the cap must fail preparation");
        assert_eq!(error, SURFACE_PROGRAM_OVERFLOW);
        assert_eq!(error.code(), KernelErrorCode::OutOfBounds);
        assert_eq!(error.scope(), KernelErrorScope::Memory);
        assert!(!error.message().is_empty());
    }

    /// Preparation is **deterministic**: the same set in any order yields the
    /// same catalog, byte for byte, because the order is the digest order.
    #[test]
    fn preparation_is_sorted_by_digest_and_independent_of_authoring_order() {
        let surfaces = distinct(8);
        let mut reversed = surfaces.clone();
        reversed.reverse();
        let forward = SurfaceProgramCatalog::prepare(&surfaces, gpu_profile()).expect("fits");
        let backward = SurfaceProgramCatalog::prepare(&reversed, gpu_profile()).expect("fits");
        assert_eq!(forward, backward);
        let ids: Vec<u64> = forward
            .sources()
            .iter()
            .map(SurfaceProgramSource::program_id)
            .collect();
        let mut ascending = ids.clone();
        ascending.sort_unstable();
        assert_eq!(ids, ascending, "programs compile in sorted digest order");
        // And every id resolves back to its own source through the key.
        ids.iter().for_each(|id| {
            assert_eq!(
                forward.source(*id).map(SurfaceProgramSource::program_id),
                Some(*id)
            );
        });
    }

    /// **A frame naming an unprepared program is reported, once, and renders the
    /// fallback.** It does not compile, and it does not panic.
    #[test]
    fn an_unprepared_program_is_one_reported_degradation_however_many_draws_miss() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .build()
            .expect("legal");
        let prepared = surface.digest().raw();
        let catalog = SurfaceProgramCatalog::prepare(std::slice::from_ref(&surface), gpu_profile())
            .expect("fits");
        assert!(catalog.degradations(&[prepared, 0]).is_empty());
        assert_eq!(
            catalog.degradations(&[prepared, 0xDEAD, 0xBEEF, 0xDEAD]),
            vec![FrameFeature::ProceduralSurface]
        );
        assert!(!catalog.is_prepared(0xDEAD));
        assert_eq!(catalog.source(0xDEAD), None);
        // The source itself is comparable and printable — a cache entry a future
        // agent has to be able to look at.
        let source = catalog.source(prepared).expect("prepared");
        assert_eq!(source, source);
        assert!(format!("{source:?}").contains("SurfaceProgramSource"));
    }
}
