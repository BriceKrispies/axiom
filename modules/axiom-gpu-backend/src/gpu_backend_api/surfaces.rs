//! The GPU backend's **surface** surface: the preparation barrier, and the
//! preparation-time queries a caller answers a frame's report from.
//!
//! A second `impl GpuBackendApi` block rather than a second type — the Module Law
//! gives this crate exactly one facade, and these are that facade's methods. What
//! separates them from [`super`] is *when* they run: every one of them is a
//! **startup** call, driven from the app's `axiom_runtime::PreparationTask`
//! before `RuntimeState::Prepared` — except the single lookup a frame makes
//! against what they produced, which is the last item below.
//!
//! [`GpuBackendApi::prepare_surfaces`] is the only place in this backend a shader
//! is ever compiled, and the only place a surface is ever **flattened**.
//! Everything else here is a pure question about what it produced — including
//! [`GpuBackendApi::frame_surface_set`], the one member of this file a frame does
//! call, which exists precisely to hand a present the answer the barrier already
//! computed instead of recomputing it.

use std::borrow::Cow;

use axiom_host::FramePacket;

use crate::gpu_backend_api::GpuBackendApi;
use crate::surface_program::SurfaceProgramSet;

impl GpuBackendApi {
    /// **Compile every authored surface's program, at the preparation barrier.**
    ///
    /// Call this from the app's `axiom_runtime::PreparationTask`, before
    /// `RuntimeState::Prepared` — the phase whose stated invariant is that the
    /// deterministic simulation cannot advance until preparation has completed.
    /// It is the **only** place in this backend a shader is compiled: the draw
    /// loop performs a lookup and nothing else, so no frame can stutter
    /// compiling a pipeline the driver sees for the first time. On the browser's
    /// WebGL2 fallback `wgpu` cross-compiles WGSL to GLSL at pipeline creation,
    /// which is what makes that failure mode concrete rather than theoretical.
    ///
    /// Preparation is deterministic: surfaces are deduplicated by digest and
    /// compiled in ascending digest order, so the same set produces the same
    /// programs in the same sequence however the app assembled the slice. Two
    /// surfaces authored independently that compute the same thing collapse to
    /// **one** program — the content-addressed key is the only structural defence
    /// against variant explosion.
    ///
    /// Returns how many programs were compiled — assert on it and a variant
    /// explosion is a failing test rather than a slow frame. Fails with an
    /// `axiom_kernel::KernelError` when the surface set needs more distinct
    /// programs than the bounded cache holds; there is no eviction, because a
    /// bound that fails at startup is a design signal an author can act on and an
    /// evicting cache is an unattributable mid-session stutter.
    ///
    /// An app that authors no surface never calls this and is entirely
    /// unaffected — that is the compatibility contract.
    ///
    /// **It also plans the per-frame view.** A present needs a second, much
    /// cheaper answer about the same surfaces — each one's constant channels, its
    /// packed parameter region, its pipeline marker, and whether anything in the
    /// set reads a clock — and producing that answer *also* flattens every
    /// surface, because flattening is what composes a layered surface's
    /// per-channel graphs into one. So it is produced here, once, next to the
    /// catalog, and kept: the two halves depend on exactly the same
    /// `(surfaces, capability profile)` pair, and both of those are fixed by the
    /// time this returns.
    pub fn prepare_surfaces(
        &mut self,
        surfaces: &[axiom_surface::Surface],
    ) -> Result<u32, axiom_kernel::KernelError> {
        let profile = self.capability;
        let set = SurfaceProgramSet::build(surfaces, profile);
        crate::surface_program::cache::SurfaceProgramCatalog::prepare(surfaces, profile).map(
            |catalog| {
                #[cfg(target_arch = "wasm32")]
                self.live
                    .iter_mut()
                    .for_each(|live| live.prepare_surfaces(&catalog));
                let count = catalog.program_count();
                self.catalog = catalog;
                self.surface_set = Some(set);
                count
            },
        )
    }

    /// **The surface view one frame presents with — never rebuilt when the
    /// barrier already built it.**
    ///
    /// Two paths, and which one a caller is on is decided entirely by whether
    /// [`Self::prepare_surfaces`] has run:
    ///
    /// * **The prepared path** — the one every app takes. The set planned at the
    ///   barrier is **authoritative**: it is borrowed as-is and the `surfaces`
    ///   slice the frame passed is not read at all. A frame that hands over a
    ///   surface the barrier never saw therefore renders the neutral
    ///   `(white, black)` constant fallback for it, exactly as a draw naming an
    ///   unprepared program already did, and
    ///   [`Self::frame_degradations`] reports it through
    ///   `axiom_host::FrameSubmissionReport::degraded_features`. That is the
    ///   existing rule, not a new one: *a cache miss is a hard error rather than
    ///   a lazy compile*, and it would be incoherent for the miss to be a hard
    ///   error for the program and a silent re-derivation for the view.
    /// * **The unprepared path** — a caller that presents surfaces without ever
    ///   running the barrier, which is what this backend's own tests and
    ///   [`Self::present_packet`]'s empty slice do. It builds the view from the
    ///   passed slice, which is what every present did before 2026-08-17. It is
    ///   correct and it is not what an app should be on: it flattens every
    ///   surface, per frame.
    ///
    /// The borrow is why this returns a [`Cow`] rather than a `SurfaceProgramSet`
    /// — the prepared path must cost no allocation and no copy, or it has not
    /// fixed anything.
    pub(crate) fn frame_surface_set(
        &self,
        surfaces: &[axiom_surface::Surface],
    ) -> Cow<'_, SurfaceProgramSet> {
        self.surface_set.as_ref().map_or_else(
            || Cow::Owned(SurfaceProgramSet::build(surfaces, self.capability)),
            Cow::Borrowed,
        )
    }

    /// How many surface programs the preparation barrier compiled.
    pub fn prepared_program_count(&self) -> u32 {
        self.catalog.program_count()
    }

    /// How many authored surfaces the preparation barrier saw — program or not.
    /// A surface whose every channel is a plain constant is prepared without
    /// costing a program, because the existing pipeline renders it exactly.
    pub fn prepared_surface_count(&self) -> u32 {
        self.catalog.prepared_count()
    }

    /// **What this frame could not honour**: the degraded features a caller puts
    /// in `axiom_host::FrameSubmissionReport::degraded_features`.
    ///
    /// A draw naming a surface program the preparation barrier did not prepare is
    /// a **cache miss, and a cache miss is a hard error rather than a lazy
    /// compile**. The draw renders the constant fallback — the neutral
    /// `(white, black)` `crate::frame_packet_adapter` folds for an unknown
    /// digest — and this reports it once, however many draws missed.
    ///
    /// The rule is what makes the renderer's twice-written anti-variant doctrine
    /// hold: `crate::post_chain`'s render-target comment states that a frame
    /// toggling a feature *"does not change which pipelines exist, so it cannot
    /// stutter on a pipeline the driver compiles the first time it is used"*, and
    /// that is only true while nothing compiles inside a frame.
    pub fn frame_degradations(&self, packet: &FramePacket) -> Vec<axiom_host::FrameFeature> {
        self.program_degradations(&crate::frame_packet_adapter::frame_packet_programs(packet))
    }

    /// [`Self::frame_degradations`] for a caller that holds the frame's **programs**
    /// rather than a packet — the batch path.
    ///
    /// [`Self::present_frame_result`] takes explicit `(mesh, material)` batches
    /// and a parallel program slice, never a packet, so it has no `FramePacket`
    /// to derive the programs from. That is the *transport* differing, not the
    /// rule: a program the barrier did not prepare renders the constant fallback
    /// and is reported, whichever entry drew it. Both entries therefore answer
    /// from this one query rather than each growing its own copy of the rule —
    /// `frame_degradations` is exactly this, preceded by reading the programs
    /// off the packet.
    pub fn program_degradations(&self, programs: &[u64]) -> Vec<axiom_host::FrameFeature> {
        self.catalog.degradations(programs)
    }

    /// Which features this backend cannot honour for `surfaces`, checked once
    /// against its capability profile at preparation time rather than per frame.
    ///
    /// The result is what a caller puts in the frame's
    /// [`axiom_host::FrameSubmissionReport`] degraded-features list, so a surface
    /// this backend drops is *reported*, never silently skipped. It is empty for
    /// a surface whose every channel is a constant: such a surface needs no
    /// program, and this backend renders it exactly.
    pub fn surface_degradations(
        &self,
        surfaces: &[axiom_surface::Surface],
    ) -> Vec<axiom_host::FrameFeature> {
        crate::surface_program::SurfaceProgramSet::build(surfaces, self.capability).degradations()
    }

    /// Which features this backend cannot honour for `surfaces` **when they are
    /// drawn on skinned geometry**.
    ///
    /// The skinned vertex stage binds all 16 vertex attributes a WebGL2
    /// downlevel target guarantees — the ceiling that already costs a skinned
    /// material its emissive and its specular — and the vertex it receives has
    /// already been deformed once, by the joint palette. So it runs **no**
    /// displacement program, and a displacing surface bound to a skinned draw is
    /// reported here rather than silently rendering the right colour on an
    /// undeformed shape. Everything else about a surface lowers identically on
    /// both paths, which is why this is a second query and not a second
    /// pipeline's worth of rules.
    pub fn skinned_surface_degradations(
        &self,
        surfaces: &[axiom_surface::Surface],
    ) -> Vec<axiom_host::FrameFeature> {
        crate::surface_program::SurfaceProgramSet::build_for(
            surfaces,
            self.capability,
            crate::surface_program::capability::GeometryPath::Skinned,
        )
        .degradations()
    }

    /// Which pipeline a draw naming `program_id` selects, given the frame's
    /// authored `surfaces`: `axiom_render::RenderPipelineKind::UNLIT` (`2`) for a
    /// surface whose [`axiom_surface::LightingModel`] is `Unlit`, and
    /// `BASIC_LIT` (`1`) for every other surface, for a program this backend was
    /// never handed, and for the `0` every draw that authored no surface carries.
    ///
    /// The render module has emitted that marker per object since it was
    /// written — and it has always died at the `axiom_host::FramePacket`
    /// boundary, which carries no pipeline lane. This answers it from the
    /// **surface** instead, which the packet already names by digest: the packet
    /// stays primitive-only, nothing is duplicated into a second lane that could
    /// disagree with the surface, and a caller holding both a packet and its
    /// surfaces can recover the selection. A preparation-time query, exactly like
    /// [`Self::surface_degradations`] and [`Self::surface_parameter_bytes`].
    ///
    /// This backend itself still runs **one** lit pipeline: the model is a value
    /// inside its single program, not a second module (see
    /// `crate::surface_program::emit_lighting`).
    pub fn surface_pipeline_kind(
        &self,
        surfaces: &[axiom_surface::Surface],
        program_id: u64,
    ) -> u32 {
        crate::surface_program::SurfaceProgramSet::build(surfaces, self.capability)
            .pipeline_kind(program_id)
    }

    /// The bytes this backend uploads into the shared surface-parameter buffer
    /// for `surfaces`: one fixed-size region per program, at
    /// `index * 512` — a 256-byte-aligned dynamic offset.
    ///
    /// Produced at the preparation barrier, not per frame, and produced as one
    /// buffer with per-program regions rather than one buffer rewritten between
    /// draws: a `queue.write_buffer` is ordered against submission, not against
    /// the passes inside an encoder, so N writes to one buffer would leave every
    /// draw in a pass reading the last of them (see `crate::post_chain`).
    pub fn surface_parameter_bytes(&self, surfaces: &[axiom_surface::Surface]) -> Vec<u8> {
        crate::surface_program::SurfaceProgramSet::build(surfaces, self.capability)
            .parameter_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu_backend_api::tests::request;
    /// A scalar `Uv.x` opacity — the canonical field-authored surface, which
    /// needs a program.
    fn uv_opacity(name: &str) -> axiom_surface::Surface {
        use axiom_field::{FieldBuilder, FieldId, FieldOp};
        use axiom_surface::{SurfaceBuilder, SurfaceChannel};
        let (builder, uv) =
            FieldBuilder::new(FieldId::of_name(name), 1).push(FieldOp::Uv, Vec::new(), Vec::new());
        let (builder, lane) = builder.push(
            FieldOp::Component,
            vec![axiom_recipe::Param::int(0)],
            vec![uv],
        );
        SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(lane))
            .build()
            .expect("a scalar uv field is a legal opacity")
    }

    /// A surface whose base colour is the plain constant `tint` — needs no
    /// program, and carries a *distinguishable* constant fallback, which is what
    /// makes "which set answered" observable.
    fn tinted(tint: [f32; 4]) -> axiom_surface::Surface {
        use axiom_surface::{SurfaceBuilder, SurfaceChannel};
        SurfaceBuilder::new()
            .constant(
                SurfaceChannel::BaseColor,
                axiom_field::FieldValue::vec4(axiom_math::Vec4::new(
                    tint[0], tint[1], tint[2], tint[3],
                )),
            )
            .build()
            .expect("a vec4 constant is a legal base colour")
    }

    /// A displacement driven by `Time` — the one thing that turns the frame's
    /// surface-time lane on.
    fn windy() -> axiom_surface::Surface {
        use axiom_field::{FieldBuilder, FieldId, FieldOp};
        use axiom_surface::{SurfaceBuilder, SurfaceChannel};
        let (builder, clock) = FieldBuilder::new(FieldId::of_name("gpu/api/wind"), 1).push(
            FieldOp::Time,
            Vec::new(),
            Vec::new(),
        );
        let (builder, node) = builder.push(
            FieldOp::Compose,
            vec![axiom_recipe::Param::int(3)],
            vec![clock, clock, clock],
        );
        SurfaceBuilder::new()
            .field(SurfaceChannel::Displacement, builder.build(node))
            .build()
            .expect("a vec3 field is a legal displacement")
    }

    /// Whether a frame's surface view was **borrowed** from the barrier (no
    /// per-frame construction) rather than built for this call. The whole claim
    /// of the fix, reduced to one bit a test can assert on without a timer.
    fn borrowed(set: &Cow<'_, SurfaceProgramSet>) -> bool {
        matches!(set, Cow::Borrowed(_))
    }

    /// A packet whose single draw names `program`.
    fn packet_naming(program: u64) -> FramePacket {
        use axiom_host::{FrameDrawItem, FrameFeatureSet, FrameViewport};
        FramePacket::new(
            1,
            60,
            FrameViewport::new(8, 8),
            [0.0; 4],
            None,
            vec![
                FrameDrawItem::new(0, 1, 1, [0.0; 16], [0.0; 16], [1.0; 4], false)
                    .with_surface_program(program),
            ],
            Vec::new(),
            [0.0; 16],
            FrameFeatureSet::new(false, false, 1, 0),
        )
    }

    /// **A field-authored surface is no longer a degradation — it is a compiled
    /// program**, and a constant-only one still costs none.
    #[test]
    fn a_field_authored_surface_now_lowers_and_a_constant_one_still_needs_no_program() {
        use axiom_field::FieldValue;
        use axiom_surface::{SurfaceBuilder, SurfaceChannel};

        let mut backend = GpuBackendApi::new(&request(320, 240));
        let field_authored = uv_opacity("gpu/api/uv");
        let constant_only = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::BaseColor,
                FieldValue::vec4(axiom_math::Vec4::new(0.25, 0.5, 0.75, 1.0)),
            )
            .build()
            .expect("a vec4 constant is a legal base colour");

        // The capability is on, so nothing about either surface is dropped.
        assert!(backend
            .surface_degradations(std::slice::from_ref(&field_authored))
            .is_empty());
        assert!(backend
            .surface_degradations(std::slice::from_ref(&constant_only))
            .is_empty());
        assert!(backend.surface_degradations(&[]).is_empty());

        // Preparation compiles ONE program for the two of them: the constant-only
        // surface needs none, because the existing pipeline renders it exactly.
        assert_eq!(
            backend
                .prepare_surfaces(&[field_authored.clone(), constant_only.clone()])
                .expect("two surfaces are well inside the cap"),
            1
        );
        assert_eq!(backend.prepared_program_count(), 1);
        assert_eq!(backend.prepared_surface_count(), 2);
        // Both are prepared, so a frame drawing either reports nothing.
        assert!(backend
            .frame_degradations(&packet_naming(field_authored.digest().raw()))
            .is_empty());
        assert!(backend
            .frame_degradations(&packet_naming(constant_only.digest().raw()))
            .is_empty());
        // One 512-byte region per surface, and nothing at all for no surfaces.
        assert_eq!(
            backend
                .surface_parameter_bytes(&[field_authored, constant_only])
                .len(),
            1024
        );
        assert!(backend.surface_parameter_bytes(&[]).is_empty());
    }

    /// **A frame naming a program the barrier never prepared is REPORTED, and
    /// nothing is compiled.** This is the rule that keeps the anti-variant
    /// doctrine true, so it is asserted at the facade a caller actually holds.
    #[test]
    fn a_frame_naming_an_unprepared_program_reports_a_degraded_feature() {
        let mut backend = GpuBackendApi::new(&request(320, 240));
        let prepared = uv_opacity("gpu/api/prepared");
        let never = uv_opacity("gpu/api/never");
        assert_ne!(prepared.digest().raw(), never.digest().raw());

        // Before any preparation at all, even a surface the app holds is a miss.
        assert_eq!(
            backend.frame_degradations(&packet_naming(prepared.digest().raw())),
            vec![axiom_host::FrameFeature::ProceduralSurface]
        );
        // A draw naming NO surface is never a miss: its program is compiled into
        // the pass itself.
        assert!(backend.frame_degradations(&packet_naming(0)).is_empty());

        assert_eq!(
            backend
                .prepare_surfaces(std::slice::from_ref(&prepared))
                .expect("one program fits"),
            1
        );
        assert!(backend
            .frame_degradations(&packet_naming(prepared.digest().raw()))
            .is_empty());
        assert_eq!(
            backend.frame_degradations(&packet_naming(never.digest().raw())),
            vec![axiom_host::FrameFeature::ProceduralSurface]
        );
        // The miss renders (the constant fallback) rather than panicking, and it
        // still compiles nothing: the program count did not move.
        assert!(!backend.present_packet(&packet_naming(never.digest().raw())));
        assert_eq!(backend.prepared_program_count(), 1);
    }

    /// **The batch path reports the same miss the packet path does.**
    ///
    /// `present_frame_result` carries explicit `(mesh, material)` batches and a
    /// parallel program slice, so it has no packet to read the programs off. The
    /// *rule* must not fork with the transport: the two entries answer from one
    /// query, and this pins that they agree — including that a batch naming no
    /// surface (`0`) is never a miss, and that one miss among many batches is
    /// reported once.
    #[test]
    fn the_batch_path_and_the_packet_path_report_the_same_miss() {
        let mut backend = GpuBackendApi::new(&request(320, 240));
        let prepared = uv_opacity("gpu/api/batch/prepared");
        let never = uv_opacity("gpu/api/batch/never");
        assert_eq!(
            backend
                .prepare_surfaces(std::slice::from_ref(&prepared))
                .expect("one program fits"),
            1
        );

        // A frame whose batches all name prepared programs (or none at all)
        // reports nothing.
        assert!(backend
            .program_degradations(&[0, prepared.digest().raw(), 0])
            .is_empty());
        assert!(backend.program_degradations(&[]).is_empty());

        // One unprepared program among several batches is one report, and it is
        // exactly what the packet path says about the same program.
        let missed = backend.program_degradations(&[
            prepared.digest().raw(),
            never.digest().raw(),
            never.digest().raw(),
        ]);
        assert_eq!(missed, vec![axiom_host::FrameFeature::ProceduralSurface]);
        assert_eq!(
            missed,
            backend.frame_degradations(&packet_naming(never.digest().raw())),
            "the two transports must not grow two rules"
        );
    }

    /// **The cache is bounded and preparation fails loudly past the bound.**
    /// No eviction: an evicting cache turns an authoring mistake into an
    /// unattributable mid-session stutter, while a bound that fails at the
    /// barrier is a design signal an author can act on.
    #[test]
    fn preparing_more_programs_than_the_cache_holds_fails_the_barrier() {
        let mut backend = GpuBackendApi::new(&request(320, 240));
        let many: Vec<axiom_surface::Surface> = (0..65)
            .map(|index| uv_opacity(&format!("gpu/api/many/{index}")))
            .collect();
        let error = backend
            .prepare_surfaces(&many)
            .expect_err("65 distinct programs must not fit a 64-program cache");
        assert_eq!(error.code(), axiom_kernel::KernelErrorCode::OutOfBounds);
        // A failed preparation leaves the previous catalog untouched — it does not
        // half-fill one.
        assert_eq!(backend.prepared_program_count(), 0);
        // Exactly the cap prepares.
        assert_eq!(
            backend
                .prepare_surfaces(&many[..64])
                .expect("exactly the cap fits"),
            64
        );
    }

    /// **Two equal surfaces are one program.** The content-addressed key at the
    /// facade: a scene that authors the same material twice compiles it once.
    #[test]
    fn two_equal_surfaces_prepare_one_program() {
        let mut backend = GpuBackendApi::new(&request(320, 240));
        let one = uv_opacity("gpu/api/twin");
        let twin = uv_opacity("gpu/api/twin");
        assert_eq!(
            backend
                .prepare_surfaces(&[one, twin])
                .expect("one program fits"),
            1
        );
        assert_eq!(backend.prepared_surface_count(), 1);
    }

    /// A displacing surface bound to a **skinned** draw is reported, because the
    /// skinned vertex stage runs no displacement program — the 16-attribute
    /// ceiling. Not a silent no-op: a character bound to a wind surface that did
    /// not move would be a wrong shape nobody was told about.
    #[test]
    fn a_displacing_surface_is_reported_dropped_for_the_skinned_vertex_path() {
        use axiom_field::FieldValue;
        use axiom_surface::{SurfaceBuilder, SurfaceChannel};

        let backend = GpuBackendApi::new(&request(320, 240));
        let displacing = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(axiom_math::Vec3::new(0.0, 0.5, 0.0)),
            )
            .build()
            .expect("a vec3 constant is a legal displacement");
        assert_eq!(
            backend.skinned_surface_degradations(std::slice::from_ref(&displacing)),
            vec![axiom_host::FrameFeature::ProceduralSurface]
        );
        // A surface that does not displace is fine on both paths — the ceiling
        // is about the vertex stage, not about skinning per se.
        let constant_only = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::BaseColor,
                FieldValue::vec4(axiom_math::Vec4::new(0.25, 0.5, 0.75, 1.0)),
            )
            .build()
            .expect("a vec4 constant is a legal base colour");
        assert!(backend
            .skinned_surface_degradations(std::slice::from_ref(&constant_only))
            .is_empty());
        assert!(backend.skinned_surface_degradations(&[]).is_empty());
    }

    /// **A prepared backend flattens nothing per frame.**
    ///
    /// The claim is checkable without a timer, which is the point: a present's
    /// surface view is a *borrow* of the one the barrier planned, so however many
    /// frames run and however many surfaces the app authored, the per-frame
    /// construction count is exactly zero. A timing assertion would prove the
    /// same thing flakily; this proves it structurally.
    ///
    /// Building that view is what calls `axiom_surface::Surface::flatten` — it
    /// linearises every layer tree and composes every channel graph — and a
    /// throttled browser profile of the shader crucible (11 surfaces) measured
    /// it at 5.4 ms of an 8.1 ms frame before this borrow existed.
    #[test]
    fn a_prepared_backend_borrows_its_surface_view_instead_of_rebuilding_it_each_frame() {
        let mut backend = GpuBackendApi::new(&request(320, 240));
        let station = uv_opacity("gpu/api/view");
        let surfaces = [station.clone(), windy(), tinted([0.2, 0.4, 0.6, 1.0])];

        // Before the barrier there is nothing to borrow, so a present must derive
        // the view itself — the unprepared path, per frame.
        assert!(!borrowed(&backend.frame_surface_set(&surfaces)));

        assert_eq!(
            backend
                .prepare_surfaces(&surfaces)
                .expect("three surfaces are well inside the cap"),
            2
        );

        // Sixty frames, sixty borrows, zero constructions — and the count does
        // not move with the number of surfaces, which is the scaling the defect
        // had.
        (0..60).for_each(|_| {
            assert!(borrowed(&backend.frame_surface_set(&surfaces)));
        });
        // The real present takes that path, not a private one.
        assert!(!backend
            .present_packet_with_surfaces(&packet_naming(station.digest().raw()), &surfaces));

        // The two answers a frame actually reads off the view are unchanged: the
        // surface-time lane is the packet's own supplied time because something
        // in the set reads a clock…
        let set = backend.frame_surface_set(&surfaces);
        assert_eq!(
            set.surface_time(axiom_kernel::Seconds::finite_or_zero(12.5)),
            12.5
        );
        // …and the constant fallback is still the authored constant.
        assert_eq!(
            set.constant_fallback(tinted([0.2, 0.4, 0.6, 1.0]).digest().raw()),
            ([0.2, 0.4, 0.6, 1.0], [0.0; 3])
        );
    }

    /// **The prepared set is authoritative, and a mismatch is a reported miss —
    /// never a silent re-derivation.**
    ///
    /// A frame that hands over a surface the barrier never saw gets the neutral
    /// `(white, black)` constant fallback for it, exactly as a draw naming an
    /// unprepared program already did, and the frame says so through
    /// `axiom_host::FrameSubmissionReport::degraded_features`. Re-deriving the
    /// view from the passed slice instead would make a cache miss a hard error
    /// for the *program* and a free silent recovery for the *view* — two rules
    /// for one fact.
    #[test]
    fn a_frame_passing_a_surface_the_barrier_never_saw_renders_the_fallback_and_reports_it() {
        let mut backend = GpuBackendApi::new(&request(320, 240));
        let prepared = tinted([0.9, 0.1, 0.1, 1.0]);
        let unseen = tinted([0.1, 0.9, 0.1, 1.0]);
        assert_ne!(prepared.digest().raw(), unseen.digest().raw());
        assert_eq!(
            backend
                .prepare_surfaces(std::slice::from_ref(&prepared))
                .expect("a constant-only surface needs no program"),
            0
        );

        // The frame passes only the surface that was NEVER prepared.
        let set = backend.frame_surface_set(std::slice::from_ref(&unseen));
        assert!(borrowed(&set), "the prepared view is what a frame reads");
        // The prepared surface still answers…
        assert_eq!(
            set.constant_fallback(prepared.digest().raw()),
            ([0.9, 0.1, 0.1, 1.0], [0.0; 3])
        );
        // …and the passed-but-unprepared one does not: it is the neutral
        // identity, not its own authored green.
        assert_eq!(
            set.constant_fallback(unseen.digest().raw()),
            ([1.0; 4], [0.0; 3])
        );
        // And the miss rides out on the existing channel.
        assert_eq!(
            backend.frame_degradations(&packet_naming(unseen.digest().raw())),
            vec![axiom_host::FrameFeature::ProceduralSurface]
        );
        assert!(backend
            .frame_degradations(&packet_naming(prepared.digest().raw()))
            .is_empty());
    }

    /// **The unprepared path still works, and it is the one that costs.** A
    /// caller that presents surfaces without ever running the barrier — this
    /// backend's own tests, and `GpuBackendApi::present_packet`'s empty slice —
    /// derives the view from the slice it passed, which is what every present did
    /// before the borrow existed. Correct, and not where an app belongs.
    #[test]
    fn a_backend_that_never_prepared_derives_its_view_from_the_slice_the_frame_passed() {
        let backend = GpuBackendApi::new(&request(320, 240));
        let passed = tinted([0.25, 0.5, 0.75, 1.0]);
        let set = backend.frame_surface_set(std::slice::from_ref(&passed));
        assert!(!borrowed(&set), "there is nothing to borrow");
        assert_eq!(
            set.constant_fallback(passed.digest().raw()),
            ([0.25, 0.5, 0.75, 1.0], [0.0; 3])
        );
        // Nothing in the passed set reads a clock, so the lane is an exact zero —
        // the byte-identity every surfaceless app depends on.
        assert_eq!(
            set.surface_time(axiom_kernel::Seconds::finite_or_zero(12.5)),
            0.0
        );
        // An empty slice is `present_packet`'s own case: an empty view whose every
        // lookup misses.
        assert_eq!(
            backend.frame_surface_set(&[]).constant_fallback(0),
            ([1.0; 4], [0.0; 3])
        );
    }
}
