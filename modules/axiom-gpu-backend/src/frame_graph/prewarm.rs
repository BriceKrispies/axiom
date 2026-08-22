//! **`prewarmMaterials()`** and **`_ensureProbe()`** — the two things the frame
//! graph does that are not part of a frame.
//!
//! # Why a pre-warm exists at all, measured
//!
//! `renderer.compileAsync(scene, camera)` reaches only the *forward lit*
//! program of each material. It does not reach the CSM depth variant (skinned /
//! morphed / instanced / batched are four separate programs off one
//! `ShaderMaterial`), the MRT prepass variant (same four), or a single one of
//! the ~25 full-screen post programs, because those are in no scene graph.
//! Those are exactly the ones that used to land mid-play: up to **30 programs
//! compiling on one frame**, and that frame took 3.1-3.9 s on a cold shader
//! cache.
//!
//! # The patch-before-compile ordering, also measured
//!
//! 26 of 144 live programs were unpatched duplicates of a lit material —
//! compiled once by a caller pre-compiling the world scene, thrown away, and
//! compiled again the moment the first real frame walked the scene and injected
//! the shadow/AO/SSR chunk (because `patch()` sets `needsUpdate`). That is
//! **18% of the boot's compile budget** spent on programs that never draw
//! anything. So `prewarmMaterials` patches first, always, and `init` wraps
//! `renderer.compile` to patch the world and viewmodel scenes on the way
//! through.
//!
//! And the *set* it patches matters as much as the order.
//! `patchMaterials(root)` is the public, deliberately-broad version and reaches
//! every material in a subtree; using it here changed how materials the frame
//! loop never patches actually shade, measured at **0.04% of pixels, up to
//! 26/255**. `_patchLikeFrame` mirrors `_visit`/`_visitView` exactly — same
//! traversal, same object-type predicate — so pre-patching is a pure reordering
//! of *when* the identical set is patched. [`patchable`] is that predicate.
//!
//! # The `shadow` default is a deferral with an expiry, and the source states it
//!
//! ```js
//! async prewarmMaterials({ post = true, shadow = this.frame === 0 } = {})
//! ```
//!
//! Running the depth and prepass passes leaves a cascade fit and a G-buffer
//! behind, and the next frame's refit does not fully overwrite them: on the
//! `night` shot it moves 26 pixels by 2/255, and that survives snapshotting and
//! restoring the whole cascade fit and the sun takeover. So it is on by default
//! **only at `frame === 0`** — before a single frame has been drawn there is no
//! fit, no G-buffer and no shadow array to disturb — and has to be asked for
//! explicitly at any other time. [`prewarm_shadow_default`].

use super::pipeline::FramePipeline;

/// One full-screen program `prewarmMaterials` compiles, in
/// `_collectPassMaterials`'s order.
///
/// The order is the source's `add(...)` sequence and is preserved because it is
/// the order the scratch blits run in; nothing reads the discriminant as an
/// index, but a reader diffing the two files reads them in this order.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PassProgram {
    /// `this.composite`.
    Composite,
    /// `this.viewComposite`.
    ViewComposite,
    /// `this.fxaa` — only on the no-TAA path.
    Fxaa,
    /// `this.gtao.core`.
    GtaoCore,
    /// `this.gtao.temporal`.
    GtaoTemporal,
    /// `this.gtao.blur`.
    GtaoBlur,
    /// `this.contact.pass`.
    ContactPass,
    /// `this.contact.blur`.
    ContactBlur,
    /// `this.ssr.pass`.
    SsrPass,
    /// `this.ssr.blur`.
    SsrBlur,
    /// `this.taa.pass`.
    TaaPass,
    /// `this.motionBlur.tilePass`.
    MotionBlurTile,
    /// `this.motionBlur.blurPass`.
    MotionBlurBlur,
    /// `this.dof.pre`.
    DofPre,
    /// `this.dof.gather`.
    DofGather,
    /// `this.dof.combine`.
    DofCombine,
    /// `this.bloom.down`.
    BloomDown,
    /// `this.bloom.up`.
    BloomUp,
    /// `this.exposure.logPass`.
    ExposureLog,
    /// `this.exposure.reducePass`.
    ExposureReduce,
    /// `this.exposure.adaptPass`.
    ExposureAdapt,
}

/// `const scratch = hdrTarget(4, 4, { name: 'prewarm-scratch' })`.
///
/// A pass's program does not depend on the size of what it is drawn into, so a
/// 4x4 target compiles the whole chain for free.
pub(crate) const PREWARM_SCRATCH_SIZE: (u32, u32) = (4, 4);

/// `_collectPassMaterials(out)` — every full-screen program this pipeline owns,
/// in the source's order.
///
/// `add(p)` skips a null pass and a pass with no material, which is what makes
/// the list tier-dependent: `ultra` compiles twenty programs and `low`
/// thirteen, and the difference is not the same thirteen.
pub(crate) fn pass_programs(pipeline: &FramePipeline) -> Vec<PassProgram> {
    let gtao = pipeline.runs_gtao();
    // `_collectPassMaterials` tests the *object*, not whether the frame runs
    // it, so the contact and SSR arms follow their construction gate rather
    // than the prepass. On this crate's capability-degraded arm the two
    // coincide, because a dropped prepass drops the constructor too.
    let contact = pipeline.runs_contact();
    let ssr = pipeline.runs_ssr(false);
    let dof = pipeline.runs_dof(1.0);
    core::iter::once(PassProgram::Composite)
        .chain(core::iter::once(PassProgram::ViewComposite))
        .chain(core::iter::once(PassProgram::Fxaa).filter(|_| pipeline.fxaa()))
        .chain(
            [
                PassProgram::GtaoCore,
                PassProgram::GtaoTemporal,
                PassProgram::GtaoBlur,
            ]
            .into_iter()
            .filter(|_| gtao),
        )
        .chain(
            [PassProgram::ContactPass, PassProgram::ContactBlur]
                .into_iter()
                .filter(|_| contact),
        )
        .chain(
            [PassProgram::SsrPass, PassProgram::SsrBlur]
                .into_iter()
                .filter(|_| ssr),
        )
        .chain(core::iter::once(PassProgram::TaaPass).filter(|_| pipeline.runs_taa()))
        .chain(
            [PassProgram::MotionBlurTile, PassProgram::MotionBlurBlur]
                .into_iter()
                .filter(|_| pipeline.runs_motion_blur()),
        )
        .chain(
            [
                PassProgram::DofPre,
                PassProgram::DofGather,
                PassProgram::DofCombine,
            ]
            .into_iter()
            .filter(|_| dof),
        )
        .chain(
            [PassProgram::BloomDown, PassProgram::BloomUp]
                .into_iter()
                .filter(|_| pipeline.bloom_levels().is_some()),
        )
        .chain([
            PassProgram::ExposureLog,
            PassProgram::ExposureReduce,
            PassProgram::ExposureAdapt,
        ])
        .collect()
}

/// `shadow = this.frame === 0` — the depth/prepass compile arm's default.
///
/// See the module docs: after frame zero it perturbs the cascade fit and the
/// G-buffer by a measured 26 pixels at 2/255, so it has to be asked for.
pub(crate) fn prewarm_shadow_default(frame: u64) -> bool {
    frame == 0
}

/// What object types the frame loop's scene walk patches, and therefore what
/// `_patchLikeFrame` must patch and no more.
///
/// The world scene takes meshes, points, sprites and lines; the viewmodel scene
/// takes **meshes only**. Widening either set changes how the extra materials
/// shade — measured at 0.04% of pixels, up to 26/255 — so this is a behavioural
/// predicate rather than a traversal detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SceneObjectKind {
    /// `o.isMesh === true`.
    Mesh,
    /// `o.isPoints === true`.
    Points,
    /// `o.isSprite === true`.
    Sprite,
    /// `o.isLine === true`.
    Line,
    /// Anything else — a group, a camera, a light.
    Other,
}

/// `_patchLikeFrame(root, isViewScene)`'s predicate.
pub(crate) fn patchable(kind: SceneObjectKind, is_view_scene: bool) -> bool {
    // Written as four equalities rather than a `matches!`: that macro expands
    // to a `match`, which the Branchless Law forbids in spine code.
    let world = (kind == SceneObjectKind::Mesh)
        | (kind == SceneObjectKind::Points)
        | (kind == SceneObjectKind::Sprite)
        | (kind == SceneObjectKind::Line);
    [world, kind == SceneObjectKind::Mesh][usize::from(is_view_scene)]
}

/// `_visit`'s draw/hide split: a transparent material, a non-mesh, or an
/// explicit `owNoPrepass` keeps the object out of the depth, normal and
/// velocity buffers.
///
/// `if (o.isMesh !== true) transparent = true` is the part worth naming: points,
/// sprites and lines are treated as transparent whatever their material says,
/// because none of them writes a meaningful depth.
pub(crate) fn hidden_from_prepass(
    kind: SceneObjectKind,
    material_transparent: bool,
    no_prepass_flag: bool,
) -> bool {
    (material_transparent | (kind != SceneObjectKind::Mesh)) | no_prepass_flag
}

/// `_ensureProbe`'s `FOREIGN_LIMIT`. A couple of foreign meshes means another
/// subsystem is still showing its own placeholder; the probe scene only steps
/// aside for a real level.
pub(crate) const FOREIGN_MESH_LIMIT: usize = 6;

/// `_ensureProbe`'s decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeAction {
    /// Add the probe scene and mark every child `owProbe`.
    Build,
    /// Remove and dispose it, and reset the TAA history with it — the scene
    /// changed wholesale, so every history sample is stale.
    Retire,
    /// Leave it as it is.
    Leave,
}

/// `_ensureProbe(ctx)`.
///
/// Note the asymmetry, which is the source's: an *active* probe retires the
/// moment the level appears, at any frame; an *inactive* one can only be built
/// during the first five frames (`this.frame > 4` returns). After that the
/// renderer never draws a placeholder again, however empty the scene is.
pub(crate) fn probe_action(active: bool, frame: u64, foreign_meshes: usize) -> ProbeAction {
    let crowded = foreign_meshes >= FOREIGN_MESH_LIMIT;
    let retire = active & crowded;
    let build = !active & !crowded & (frame <= 4);
    [
        [ProbeAction::Leave, ProbeAction::Build][usize::from(build)],
        ProbeAction::Retire,
    ][usize::from(retire)]
}

#[cfg(test)]
mod tests {
    use super::{
        hidden_from_prepass, pass_programs, patchable, prewarm_shadow_default, probe_action,
        PassProgram, ProbeAction, SceneObjectKind, FOREIGN_MESH_LIMIT, PREWARM_SCRATCH_SIZE,
    };
    use crate::frame_graph::pipeline::FramePipeline;
    use crate::frame_graph::quality::QualityTier;
    use axiom_host::BackendCapabilityProfile;

    fn pipeline(tier: QualityTier) -> FramePipeline {
        FramePipeline::resolve(tier, BackendCapabilityProfile::all(), 16)
    }

    /// Twenty programs at ultra, thirteen at low, and the difference is not a
    /// subset in one direction: `low` compiles FXAA, which `ultra` does not.
    #[test]
    fn the_prewarm_program_list_is_a_tier_decision() {
        let ultra = pass_programs(&pipeline(QualityTier::Ultra));
        assert_eq!(ultra.len(), 20);
        assert_eq!(
            ultra,
            vec![
                PassProgram::Composite,
                PassProgram::ViewComposite,
                PassProgram::GtaoCore,
                PassProgram::GtaoTemporal,
                PassProgram::GtaoBlur,
                PassProgram::ContactPass,
                PassProgram::ContactBlur,
                PassProgram::SsrPass,
                PassProgram::SsrBlur,
                PassProgram::TaaPass,
                PassProgram::MotionBlurTile,
                PassProgram::MotionBlurBlur,
                PassProgram::DofPre,
                PassProgram::DofGather,
                PassProgram::DofCombine,
                PassProgram::BloomDown,
                PassProgram::BloomUp,
                PassProgram::ExposureLog,
                PassProgram::ExposureReduce,
                PassProgram::ExposureAdapt,
            ]
        );

        let low = pass_programs(&pipeline(QualityTier::Low));
        assert_eq!(low.len(), 13);
        assert!(low.contains(&PassProgram::Fxaa));
        assert!(!ultra.contains(&PassProgram::Fxaa));
        assert!(!low.contains(&PassProgram::TaaPass));
        assert!(!low.contains(&PassProgram::GtaoCore));
        assert!(!low.contains(&PassProgram::SsrPass));
        // The exposure chain and the composite are compiled at every tier.
        [
            PassProgram::Composite,
            PassProgram::ViewComposite,
            PassProgram::ExposureLog,
            PassProgram::ExposureReduce,
            PassProgram::ExposureAdapt,
        ]
        .iter()
        .for_each(|p| assert!(low.contains(p) & ultra.contains(p), "{p:?}"));

        // Medium sits between them: no SSR, but everything else.
        let medium = pass_programs(&pipeline(QualityTier::Medium));
        assert_eq!(medium.len(), 18);
        assert!(!medium.contains(&PassProgram::SsrPass));

        // A 4x4 scratch compiles all of them; a program does not depend on the
        // size of what it is drawn into.
        assert_eq!(PREWARM_SCRATCH_SIZE, (4, 4));
    }

    /// The shadow arm is only free of side effects before the first frame.
    #[test]
    fn the_shadow_prewarm_defaults_on_only_before_the_first_frame() {
        assert!(prewarm_shadow_default(0));
        assert!(!prewarm_shadow_default(1));
        assert!(!prewarm_shadow_default(9000));
    }

    /// The viewmodel scene patches meshes only; the world scene patches four
    /// object kinds. Widening either changes how the extra materials shade.
    #[test]
    fn the_viewmodel_scene_patches_meshes_and_the_world_scene_patches_four_kinds() {
        let kinds = [
            SceneObjectKind::Mesh,
            SceneObjectKind::Points,
            SceneObjectKind::Sprite,
            SceneObjectKind::Line,
            SceneObjectKind::Other,
        ];
        let world: Vec<bool> = kinds.iter().map(|&k| patchable(k, false)).collect();
        assert_eq!(world, vec![true, true, true, true, false]);
        let view: Vec<bool> = kinds.iter().map(|&k| patchable(k, true)).collect();
        assert_eq!(view, vec![true, false, false, false, false]);
    }

    /// Points, sprites and lines are excluded from the prepass whatever their
    /// material says, because none of them writes a meaningful depth.
    #[test]
    fn a_non_mesh_is_treated_as_transparent_however_its_material_is_authored() {
        assert!(!hidden_from_prepass(SceneObjectKind::Mesh, false, false));
        assert!(hidden_from_prepass(SceneObjectKind::Mesh, true, false));
        assert!(hidden_from_prepass(SceneObjectKind::Mesh, false, true));
        // Opaque, un-flagged, and still hidden — the `isMesh !== true` arm.
        assert!(hidden_from_prepass(SceneObjectKind::Points, false, false));
        assert!(hidden_from_prepass(SceneObjectKind::Sprite, false, false));
        assert!(hidden_from_prepass(SceneObjectKind::Line, false, false));
    }

    /// The probe is built only in the first five frames but retires at any
    /// frame — an asymmetry, and the source's.
    #[test]
    fn the_probe_builds_early_but_retires_whenever_the_level_arrives() {
        assert_eq!(probe_action(false, 0, 0), ProbeAction::Build);
        assert_eq!(probe_action(false, 4, 0), ProbeAction::Build);
        assert_eq!(probe_action(false, 5, 0), ProbeAction::Leave, "`frame > 4` returns");
        // A level already present at frame zero: never build one.
        assert_eq!(
            probe_action(false, 0, FOREIGN_MESH_LIMIT),
            ProbeAction::Leave
        );
        // Once active, the arrival of a real level retires it at any frame.
        assert_eq!(probe_action(true, 0, 0), ProbeAction::Leave);
        assert_eq!(probe_action(true, 900, FOREIGN_MESH_LIMIT), ProbeAction::Retire);
        assert_eq!(
            probe_action(true, 900, FOREIGN_MESH_LIMIT - 1),
            ProbeAction::Leave,
            "five foreign meshes is still somebody's placeholder"
        );
    }
}
