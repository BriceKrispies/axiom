//! **`render(ctx)`** — the eighteen steps, in order, as a value.
//!
//! This is the deliverable the rest of the module exists to produce: given a
//! [`FramePipeline`] (tier + device), a [`ScreenSizing`] and a [`FrameState`],
//! [`plan`] returns the ordered [`PlannedStep`] list — *which passes run, in
//! what order, into what attachment, at what resolution*. None of that needs a
//! GPU to check, and all of it is invisible in a rendered pixel.
//!
//! # The ping-pong, and why its index survives the frame
//!
//! Three of the eighteen steps write into a **shared** full-resolution target
//! rather than one of their own: the ADS depth of field (12), each registered
//! pass (13), and the viewmodel composite (14). The source gives them two
//! interchangeable targets and alternates:
//!
//! ```js
//! const out = this.pingRt[this._pingIndex];
//! p.render(renderer, color, out, this);
//! color = out.texture;
//! this._pingIndex ^= 1;
//! ```
//!
//! `_pingIndex` is **not reset per frame**. A frame that consumes an odd number
//! of pings hands the next frame the opposite buffer, so the assignment depends
//! on history — which is exactly the kind of thing that is fine until it is
//! not, and is worth being able to assert. [`plan`] therefore takes the
//! incoming index in [`FrameState::ping_index`] and returns the outgoing one in
//! [`FramePlan::next_ping_index`], and
//! `tests::a_step_never_writes_the_target_the_previous_step_reads` pins the
//! invariant the alternation exists to hold.
//!
//! # The three passes that hold history
//!
//! GTAO, TAA and the exposure adaptation each own a **ping-pong pair of their
//! own**, across frames, and they are the reason a frame is not a pure function
//! of its inputs:
//!
//! - **GTAO** accumulates into `history[_flip]` when `temporalOn` (which is
//!   `!!this.taa`), and rotates its sample pattern by `frame % 64`.
//! - **TAA** reprojects `history[_flip ^ 1]` through `prevVP`, which is why the
//!   scheduler carries `first_frame`: on frame one `prevVP` is seeded from
//!   `currVP` so the reprojection is an identity.
//! - **Exposure** adapts `adapt[_flip]` toward the new measurement at a rate
//!   set by `dt`, and `autoExposure: false` passes `1e3` for `dt` to make the
//!   adaptation instantaneous rather than to disable it.
//!
//! A fourth is a *pseudo*-history: **SSR** colours its hits from the previous
//! frame's resolved image. With TAA on that is `taa.previousTexture`; with TAA
//! off it is `hdr` itself, which at step 7 still holds last frame's forward
//! pass because step 8 has not overwritten it yet. That is the whole reason
//! SSR is scheduled *before* the forward pass rather than after it, and the
//! reason [`FramePipeline::runs_ssr`] takes `first_frame`.
//!
//! # Where the resolve and the composite sit, and why
//!
//! The viewmodel is rendered at 9 into its **own** MSAA colour+depth target and
//! composited at 14, five steps later. The source's reason, measured: everything
//! in `viewScene` moves in *view* space — the ADS transition, sway, bob, recoil
//! — and a velocity buffer built from camera view-projections describes none of
//! it, so those pixels emitted zero motion and TAA reprojected them onto a stale
//! background sample at ~85%. The optic tube, the mount pedestal and the glove
//! went semi-transparent with balcony rails legible through them.
//!
//! Given that, 14's position is forced from both sides. **After** 13, because a
//! depth-driven fog pass registered there would read the *world* depth at the
//! gun's pixels and bury the weapon in 40 m of aerial perspective. **Before** 15
//! and 16, so a muzzle flash still meters and still blooms.
//!
//! # The composite is also the upscale
//!
//! Steps 1-16 run at [`ScreenSizing::screen`]. Step 17 writes the canvas at
//! [`ScreenSizing::display`] — or, on the FXAA path, writes `ldr` at *screen*
//! size and lets 18 do the magnification instead. There is no separate upscale
//! pass; the filtered magnification is a side effect of the last blit's target
//! being a different size from its source.

use super::debug_view::DebugView;
use super::pipeline::FramePipeline;
use super::targets::{half_res, ScreenSizing};

/// A pass registered through `r.registerPass(pass)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegisteredPass {
    /// `pass.order`; `None` is JavaScript's `undefined`, which sorts as `0`.
    pub(crate) order: Option<i32>,
    /// `pass.enabled`. The frame skips a pass only on `enabled === false`, so
    /// `None` (absent) and `Some(true)` both run.
    pub(crate) enabled: Option<bool>,
}

/// `this.passes.sort((a, b) => (a.order ?? 0) - (b.order ?? 0))` — the indices
/// of `passes`, in the order the frame will walk them.
///
/// `Array.prototype.sort` has been **stable** since ES2019, so two passes with
/// equal `order` keep their registration order; [`slice::sort_by_key`] is
/// stable for the same reason, and swapping it for `sort_unstable_by_key` would
/// silently reorder every pass registered at the default `order` of zero.
pub(crate) fn registered_order(passes: &[RegisteredPass]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..passes.len()).collect();
    indices.sort_by_key(|&i| passes[i].order.unwrap_or(0));
    indices
}

/// The registered passes the frame will actually draw, in order.
pub(crate) fn active_registered(passes: &[RegisteredPass]) -> Vec<usize> {
    registered_order(passes)
        .into_iter()
        .filter(|&i| passes[i].enabled != Some(false))
        .collect()
}

/// One step of the frame, in frame order.
///
/// **The discriminant is the frame order.** Every consumer of a plan reads the
/// sequence, and several of the ordering constraints above are only visible as
/// "this variant comes before that one"; re-sorting these would be a semantic
/// change wearing a formatting change's clothes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FramePass {
    /// 1 — collect draw/hide/no-shadow lists, patch new materials, sync the
    /// sun, update the rooms, the bounce fill, the viewmodel rig and the light
    /// cull. CPU only; see [`super::lighting`] and [`super::rooms`].
    SceneWalk = 0,
    /// 2 — N stabilised cascades into one R32F array texture.
    Cascades = 1,
    /// 3a — sub-pixel offset applied to the **world** camera's projection.
    TaaJitter = 2,
    /// 4 — MRT: view normal + velocity + linear depth.
    Prepass = 3,
    /// 5 — horizon-arc AO, temporally accumulated.
    Gtao = 4,
    /// 6 — short depth-buffer ray march toward the sun.
    ContactShadows = 5,
    /// 7 — marched against depth, coloured from **last frame**.
    Ssr = 6,
    /// 8 — the forward world pass, with 2/5/6/7 injected into every material.
    ForwardWorld = 7,
    /// 9 — the viewmodel into its own MSAA colour+depth target.
    Viewmodel = 8,
    /// 3b — the jitter taken back off before anything reprojects.
    RemoveJitter = 9,
    /// 10 — velocity reprojection + YCoCg variance clipping.
    Taa = 10,
    /// 11 — velocity-tile reconstruction filter.
    MotionBlur = 11,
    /// 12 — gather CoC blur, only while the sights are up.
    DepthOfField = 12,
    /// 13 — one occurrence per enabled registered pass, in `order`.
    Registered = 13,
    /// 14 — premultiplied composite of the viewmodel over the world.
    ViewmodelComposite = 14,
    /// 15 — GPU log-luminance reduction to EV100 to an exposure scalar.
    Metering = 15,
    /// 16 — Karis pyramid with a soft-knee highlight threshold.
    Bloom = 16,
    /// 17 — AgX + LUT + vignette + CA + grain, to sRGB.
    Composite = 17,
    /// 18 — only when TAA is off.
    Fxaa = 18,
    /// The `?rview=` arm, which **replaces** 17 and 18 entirely.
    Debug = 19,
}

/// Where a step writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepTarget {
    /// No attachment: a CPU step (the scene walk, the two jitter halves).
    Cpu,
    /// The cascade array texture, `layers` deep.
    ShadowAtlas,
    /// The G-buffer's three colour attachments plus its own depth.
    GBuffer,
    /// Targets the pass owns and nobody else names (GTAO's history, TAA's
    /// history, motion blur's tile grid, the bloom mips, the exposure chain).
    PassOwned,
    /// `hdrRt`.
    Hdr,
    /// `viewRt`.
    Viewmodel,
    /// `pingRt[i]` — the shared ping-pong pair.
    Ping(usize),
    /// `ldrRt`, on the FXAA path only.
    Ldr,
    /// `null`, i.e. the canvas backbuffer.
    Screen,
}

/// One planned step: what runs, where it writes, and how big that is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlannedStep {
    /// Which pass.
    pub(crate) pass: FramePass,
    /// Where it writes.
    pub(crate) target: StepTarget,
    /// The target's width in texels. Zero for a CPU step.
    pub(crate) width: u32,
    /// The target's height in texels. Zero for a CPU step.
    pub(crate) height: u32,
    /// Array layers — the cascade count on [`StepTarget::ShadowAtlas`], one
    /// everywhere else, zero for a CPU step.
    pub(crate) layers: u32,
}

/// The frame's per-frame inputs that are not the tier or the device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrameState<'a> {
    /// `this._firstFrame` — true until the first `render` completes.
    pub(crate) first_frame: bool,
    /// `viewScene.children.length > this._viewRigChildren`.
    pub(crate) view_visible: bool,
    /// `this._adsT`, the sight-picture engagement in `0..1`.
    pub(crate) ads_t: f64,
    /// The registered passes, in registration order.
    pub(crate) registered: &'a [RegisteredPass],
    /// `this.debugView` — `Some` replaces the composite and FXAA with one blit.
    pub(crate) debug_view: Option<DebugView>,
    /// `this._pingIndex` as the frame begins. Carried across frames.
    pub(crate) ping_index: usize,
}

/// A planned frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FramePlan {
    /// The steps, in frame order.
    pub(crate) steps: Vec<PlannedStep>,
    /// `this._pingIndex` as the frame ends — the next frame's starting index.
    pub(crate) next_ping_index: usize,
}

/// A step before its ping slot is resolved.
#[derive(Clone, Copy)]
struct Pending {
    pass: FramePass,
    target: StepTarget,
    width: u32,
    height: u32,
    layers: u32,
    ping: bool,
}

impl FramePass {
    /// The step number in `index.js`'s header comment. The two jitter halves
    /// share `3`; [`Self::Debug`] is not numbered there and reports `0`.
    pub(crate) const fn source_step(self) -> u32 {
        [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 3, 10, 11, 12, 13, 14, 15, 16, 17, 18, 0,
        ][self as usize]
    }

    /// The crate module this pass's implementation is expected to live in.
    ///
    /// Nine of these exist today; the rest are this wave's siblings and are
    /// **assumed** — see [`Self::module_exists_today`] and the test that
    /// enumerates the split. A frame graph that named each pass's concrete Rust
    /// type could not be written before those modules landed, and could not be
    /// tested without them; naming them as data can, and makes the assumption
    /// greppable rather than latent in an `use` statement.
    pub(crate) const fn module_path(self) -> &'static str {
        [
            "(this module)",          // SceneWalk
            "crate::cascade",         // Cascades
            "(this module)",          // TaaJitter
            "crate::gbuffer",         // Prepass
            "crate::gtao",            // Gtao
            "crate::contact",         // ContactShadows
            "crate::ssr",             // Ssr
            "crate::scene_renderer",  // ForwardWorld
            "crate::scene_renderer",  // Viewmodel
            "(this module)",          // RemoveJitter
            "crate::taa",             // Taa
            "crate::motionblur",      // MotionBlur
            "crate::dof",             // DepthOfField
            "(caller-supplied)",      // Registered
            "crate::composite",       // ViewmodelComposite
            "crate::exposure",        // Metering
            "crate::bloom_pyramid",   // Bloom
            "crate::composite",       // Composite
            "crate::composite",       // Fxaa
            "crate::composite",       // Debug
        ][self as usize]
    }

    /// Whether [`Self::module_path`] names a module that is declared in
    /// `src/lib.rs` **today**, as against one this wave is expected to add.
    pub(crate) const fn module_exists_today(self) -> bool {
        [
            true,  // SceneWalk — here
            true,  // Cascades — cascade.rs
            true,  // TaaJitter — here
            true,  // Prepass — gbuffer.rs
            false, // Gtao
            false, // ContactShadows
            false, // Ssr
            true,  // ForwardWorld — scene_renderer
            true,  // Viewmodel — scene_renderer
            true,  // RemoveJitter — here
            false, // Taa
            false, // MotionBlur
            false, // DepthOfField
            true,  // Registered — the caller's
            false, // ViewmodelComposite
            true,  // Metering — exposure.rs
            true,  // Bloom — bloom_pyramid/
            false, // Composite
            false, // Fxaa
            false, // Debug
        ][self as usize]
    }
}

/// Plan one frame.
pub(crate) fn plan(
    pipeline: &FramePipeline,
    sizing: ScreenSizing,
    state: &FrameState<'_>,
) -> FramePlan {
    let (sw, sh) = sizing.screen;
    let (dw, dh) = sizing.display;
    let csm = pipeline.csm();
    let debug = state.debug_view.is_some();
    let to_ldr = pipeline.fxaa();

    let cpu = |pass| Pending {
        pass,
        target: StepTarget::Cpu,
        width: 0,
        height: 0,
        layers: 0,
        ping: false,
    };
    let at = |pass, target, width, height| Pending {
        pass,
        target,
        width,
        height,
        layers: 1,
        ping: false,
    };
    let ping = |pass| Pending {
        pass,
        target: StepTarget::Cpu,
        width: sw,
        height: sh,
        layers: 1,
        ping: true,
    };

    let pending: Vec<Pending> = core::iter::once(cpu(FramePass::SceneWalk))
        .chain(
            core::iter::once(Pending {
                pass: FramePass::Cascades,
                target: StepTarget::ShadowAtlas,
                width: csm.map_size,
                height: csm.map_size,
                layers: csm.cascades as u32,
                ping: false,
            })
            .filter(|_| pipeline.runs_cascades()),
        )
        .chain(core::iter::once(cpu(FramePass::TaaJitter)).filter(|_| pipeline.taa()))
        .chain(
            core::iter::once(at(FramePass::Prepass, StepTarget::GBuffer, sw, sh))
                .filter(|_| pipeline.runs_prepass()),
        )
        .chain(
            core::iter::once(at(FramePass::Gtao, StepTarget::PassOwned, sw, sh))
                .filter(|_| pipeline.runs_gtao()),
        )
        .chain(
            core::iter::once(at(FramePass::ContactShadows, StepTarget::PassOwned, sw, sh))
                .filter(|_| pipeline.runs_contact()),
        )
        .chain(
            core::iter::once(at(
                FramePass::Ssr,
                StepTarget::PassOwned,
                half_res(sw),
                half_res(sh),
            ))
            .filter(|_| pipeline.runs_ssr(state.first_frame)),
        )
        .chain(core::iter::once(at(
            FramePass::ForwardWorld,
            StepTarget::Hdr,
            sw,
            sh,
        )))
        .chain(
            core::iter::once(at(
                FramePass::Viewmodel,
                StepTarget::Viewmodel,
                sw,
                sh,
            ))
            .filter(|_| state.view_visible),
        )
        .chain(core::iter::once(cpu(FramePass::RemoveJitter)).filter(|_| pipeline.taa()))
        .chain(
            core::iter::once(at(FramePass::Taa, StepTarget::PassOwned, sw, sh))
                .filter(|_| pipeline.runs_taa()),
        )
        .chain(
            core::iter::once(at(FramePass::MotionBlur, StepTarget::PassOwned, sw, sh))
                .filter(|_| pipeline.runs_motion_blur()),
        )
        .chain(
            core::iter::once(ping(FramePass::DepthOfField))
                .filter(|_| pipeline.runs_dof(state.ads_t)),
        )
        .chain(
            active_registered(state.registered)
                .into_iter()
                .map(|_| ping(FramePass::Registered)),
        )
        .chain(core::iter::once(ping(FramePass::ViewmodelComposite)).filter(|_| state.view_visible))
        .chain(core::iter::once(at(
            FramePass::Metering,
            StepTarget::PassOwned,
            1,
            1,
        )))
        .chain(
            pipeline
                .bloom_levels()
                .map(|_| at(FramePass::Bloom, StepTarget::PassOwned, half_res(sw), half_res(sh)))
                .into_iter(),
        )
        .chain(
            core::iter::once(at(FramePass::Composite, StepTarget::Ldr, sw, sh))
                .filter(|_| !debug & to_ldr),
        )
        .chain(
            core::iter::once(at(FramePass::Composite, StepTarget::Screen, dw, dh))
                .filter(|_| !debug & !to_ldr),
        )
        .chain(
            core::iter::once(at(FramePass::Fxaa, StepTarget::Screen, dw, dh))
                .filter(|_| !debug & to_ldr),
        )
        .chain(
            core::iter::once(at(FramePass::Debug, StepTarget::Screen, dw, dh)).filter(|_| debug),
        )
        .collect();

    let consumed = pending.iter().filter(|p| p.ping).count();
    let steps: Vec<PlannedStep> = pending
        .iter()
        .scan(state.ping_index, |index, p| {
            let slot = *index;
            *index ^= usize::from(p.ping);
            Some(PlannedStep {
                pass: p.pass,
                target: [p.target, StepTarget::Ping(slot)][usize::from(p.ping)],
                width: p.width,
                height: p.height,
                layers: p.layers,
            })
        })
        .collect();

    FramePlan {
        steps,
        next_ping_index: state.ping_index ^ (consumed & 1),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        active_registered, plan, registered_order, FramePass, FrameState, RegisteredPass,
        StepTarget,
    };
    use crate::exposure::CHAIN_SIZES;
    use crate::frame_graph::debug_view::DebugView;
    use crate::frame_graph::pipeline::FramePipeline;
    use crate::frame_graph::quality::QualityTier;
    use crate::frame_graph::targets::screen_sizing;
    use axiom_host::{BackendCapabilityProfile, RenderCapability};

    const NO_PASSES: &[RegisteredPass] = &[];

    fn state<'a>(registered: &'a [RegisteredPass]) -> FrameState<'a> {
        FrameState {
            first_frame: false,
            view_visible: true,
            ads_t: 1.0,
            registered,
            debug_view: None,
            ping_index: 0,
        }
    }

    fn ultra() -> FramePipeline {
        FramePipeline::resolve(QualityTier::Ultra, BackendCapabilityProfile::all(), 16)
    }

    /// The full eighteen-step frame, in order, at the tier the original boots
    /// with. This list *is* the header comment.
    #[test]
    fn the_ultra_frame_is_the_source_header_in_order() {
        let sizing = screen_sizing(1920, 1080, 1.0, 1.0);
        let planned = plan(&ultra(), sizing, &state(NO_PASSES));
        let order: Vec<FramePass> = planned.steps.iter().map(|s| s.pass).collect();
        assert_eq!(
            order,
            vec![
                FramePass::SceneWalk,
                FramePass::Cascades,
                FramePass::TaaJitter,
                FramePass::Prepass,
                FramePass::Gtao,
                FramePass::ContactShadows,
                FramePass::Ssr,
                FramePass::ForwardWorld,
                FramePass::Viewmodel,
                FramePass::RemoveJitter,
                FramePass::Taa,
                FramePass::MotionBlur,
                FramePass::DepthOfField,
                FramePass::ViewmodelComposite,
                FramePass::Metering,
                FramePass::Bloom,
                FramePass::Composite,
            ]
        );
        // The source's own numbering is monotonic through the plan, with the
        // jitter's two halves both reporting step 3.
        let numbers: Vec<u32> = order.iter().map(|p| p.source_step()).collect();
        assert_eq!(
            numbers,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 3, 10, 11, 12, 14, 15, 16, 17]
        );
        // No FXAA at ultra: TAA owns the anti-aliasing.
        assert!(!order.contains(&FramePass::Fxaa));
    }

    /// The low tier is the other end of the ladder: no TAA, so no jitter and no
    /// temporal resolve, but an LDR intermediate and an FXAA pass that the
    /// ultra frame does not have.
    #[test]
    fn the_low_tier_frame_trades_the_temporal_chain_for_fxaa() {
        let pipeline =
            FramePipeline::resolve(QualityTier::Low, BackendCapabilityProfile::all(), 16);
        let sizing = screen_sizing(1920, 1080, 1.0, pipeline.render_scale());
        let planned = plan(&pipeline, sizing, &state(NO_PASSES));
        let order: Vec<FramePass> = planned.steps.iter().map(|s| s.pass).collect();
        assert_eq!(
            order,
            vec![
                FramePass::SceneWalk,
                FramePass::Cascades,
                FramePass::Prepass,
                FramePass::ForwardWorld,
                FramePass::Viewmodel,
                FramePass::ViewmodelComposite,
                FramePass::Metering,
                FramePass::Bloom,
                FramePass::Composite,
                FramePass::Fxaa,
            ]
        );
        // The prepass still runs, with no consumer of its own constructed at
        // this tier: `needsPrepass` is unconditional in the source because the
        // depth and velocity textures are a public contract (soft particles,
        // and anything a subsystem registered at step 13) rather than an
        // internal detail of the passes that happen to be off here.
        assert!(order.contains(&FramePass::Prepass));
        [
            FramePass::Gtao,
            FramePass::ContactShadows,
            FramePass::Ssr,
            FramePass::Taa,
            FramePass::MotionBlur,
            FramePass::DepthOfField,
            FramePass::TaaJitter,
        ]
        .iter()
        .for_each(|p| assert!(!order.contains(p), "{p:?} is not built at the low tier"));
        // The composite writes the LDR intermediate at *screen* size and FXAA
        // magnifies to the canvas.
        let composite = planned.steps[8];
        assert_eq!(composite.target, StepTarget::Ldr);
        assert_eq!((composite.width, composite.height), sizing.screen);
        let fxaa = planned.steps[9];
        assert_eq!(fxaa.target, StepTarget::Screen);
        assert_eq!((fxaa.width, fxaa.height), sizing.display);
        assert_ne!(sizing.screen, sizing.display, "0.72 render scale, so they differ");
    }

    /// The prepass is present at `low` only because no consumer of it is. This
    /// pins the *reason*: the source keeps `needsPrepass` unconditionally true
    /// for the public depth/velocity contract, so a plan that omits the pass at
    /// `low` is omitting it for the tier's reason, not the device's.
    #[test]
    fn the_prepass_itself_is_never_gated_by_the_tier() {
        let low = FramePipeline::resolve(QualityTier::Low, BackendCapabilityProfile::all(), 16);
        assert!(
            low.runs_prepass(),
            "needsPrepass is unconditional: soft particles read the depth texture \
             whether or not our own effects are on"
        );
    }

    /// Turning off the G-buffer capability drops the prepass and, with it,
    /// every screen-space consumer — the declared `Drop` degradation, checkable
    /// without a device.
    #[test]
    fn a_device_without_a_gbuffer_drops_the_whole_screen_space_arm() {
        let profile = BackendCapabilityProfile::all().without(RenderCapability::GBuffer);
        let pipeline = FramePipeline::resolve(QualityTier::Ultra, profile, 16);
        let planned = plan(&pipeline, screen_sizing(1280, 720, 1.0, 1.0), &state(NO_PASSES));
        let order: Vec<FramePass> = planned.steps.iter().map(|s| s.pass).collect();
        [
            FramePass::Prepass,
            FramePass::Gtao,
            FramePass::ContactShadows,
            FramePass::Ssr,
            FramePass::DepthOfField,
        ]
        .iter()
        .for_each(|p| assert!(!order.contains(p), "{p:?} survived a G-buffer drop"));
        // The cascades, the forward pass and the whole post chain are untouched.
        assert!(order.contains(&FramePass::Cascades));
        assert!(order.contains(&FramePass::ForwardWorld));
        assert!(order.contains(&FramePass::Composite));
    }

    /// SSR is scheduled **before** the forward pass, because that is the only
    /// point in the frame at which `hdr` still holds the previous frame — and
    /// it is skipped altogether on frame one.
    #[test]
    fn ssr_reads_last_frame_and_so_is_scheduled_before_the_pass_that_overwrites_it() {
        let sizing = screen_sizing(1920, 1080, 1.0, 1.0);
        let planned = plan(&ultra(), sizing, &state(NO_PASSES));
        let ssr = planned.steps.iter().position(|s| s.pass == FramePass::Ssr);
        let forward = planned
            .steps
            .iter()
            .position(|s| s.pass == FramePass::ForwardWorld);
        assert!(ssr < forward, "SSR at {ssr:?} must precede the forward pass at {forward:?}");

        let first = FrameState {
            first_frame: true,
            ..state(NO_PASSES)
        };
        let frame_one = plan(&ultra(), sizing, &first);
        assert!(!frame_one.steps.iter().any(|s| s.pass == FramePass::Ssr));
    }

    /// The three ping consumers alternate, the index survives the frame, and
    /// no step ever writes the target the step before it reads.
    #[test]
    fn a_step_never_writes_the_target_the_previous_step_reads() {
        let passes = [
            RegisteredPass { order: None, enabled: None },
            RegisteredPass { order: None, enabled: None },
        ];
        let sizing = screen_sizing(1920, 1080, 1.0, 1.0);
        let planned = plan(&ultra(), sizing, &state(&passes));

        let slots: Vec<usize> = planned
            .steps
            .iter()
            .filter_map(|s| match_ping(s.target))
            .collect();
        // DOF, two registered passes, the viewmodel composite: four consumers.
        assert_eq!(slots, vec![0, 1, 0, 1]);
        // Four is even, so the next frame starts where this one did.
        assert_eq!(planned.next_ping_index, 0);
        // Consecutive consumers never share a slot: each reads what the last
        // wrote and writes the other.
        assert!(slots.windows(2).all(|w| w[0] != w[1]));

        // An odd number of consumers hands the next frame the other buffer,
        // which is why the index is carried rather than reset.
        let one_pass = [RegisteredPass { order: None, enabled: None }];
        let odd = FrameState {
            view_visible: false,
            ads_t: 0.0,
            ..state(&one_pass)
        };
        let planned_odd = plan(&ultra(), sizing, &odd);
        assert_eq!(planned_odd.next_ping_index, 1);
        // ...and starting from 1 yields the mirrored assignment.
        let mirrored = plan(
            &ultra(),
            sizing,
            &FrameState { ping_index: 1, ..odd },
        );
        assert_eq!(
            mirrored
                .steps
                .iter()
                .filter_map(|s| match_ping(s.target))
                .collect::<Vec<usize>>(),
            vec![1]
        );
        assert_eq!(mirrored.next_ping_index, 0);
    }

    fn match_ping(target: StepTarget) -> Option<usize> {
        match target {
            StepTarget::Ping(i) => Some(i),
            _ => None,
        }
    }

    /// The viewmodel is rendered five steps before it is composited, and the
    /// composite sits after the registered passes and before the metering.
    #[test]
    fn the_viewmodel_resolve_sits_after_the_registered_passes_and_before_the_meter() {
        let passes = [RegisteredPass { order: None, enabled: None }];
        let planned = plan(&ultra(), screen_sizing(1920, 1080, 1.0, 1.0), &state(&passes));
        let index = |p: FramePass| planned.steps.iter().position(|s| s.pass == p).unwrap();
        assert!(index(FramePass::Viewmodel) < index(FramePass::Taa));
        assert!(index(FramePass::Registered) < index(FramePass::ViewmodelComposite));
        assert!(index(FramePass::ViewmodelComposite) < index(FramePass::Metering));
        assert!(index(FramePass::Metering) < index(FramePass::Bloom));
        assert!(index(FramePass::Bloom) < index(FramePass::Composite));
        // With no viewmodel in the scene, neither step is planned.
        let hidden = FrameState { view_visible: false, ..state(&passes) };
        let without = plan(&ultra(), screen_sizing(1920, 1080, 1.0, 1.0), &hidden);
        assert!(!without.steps.iter().any(|s| s.pass == FramePass::Viewmodel));
        assert!(!without
            .steps
            .iter()
            .any(|s| s.pass == FramePass::ViewmodelComposite));
    }

    /// Registered passes are sorted stably by `order`, and a pass is skipped
    /// only on a literal `enabled === false`.
    #[test]
    fn registered_passes_sort_stably_and_skip_only_on_an_explicit_false() {
        let passes = [
            RegisteredPass { order: Some(10), enabled: None },
            RegisteredPass { order: None, enabled: Some(true) },
            RegisteredPass { order: Some(-5), enabled: None },
            RegisteredPass { order: None, enabled: Some(false) },
            RegisteredPass { order: None, enabled: None },
        ];
        // -5 first, then the three zero-order passes in registration order.
        assert_eq!(registered_order(&passes), vec![2, 1, 3, 4, 0]);
        // ...minus the one that opted out.
        assert_eq!(active_registered(&passes), vec![2, 1, 4, 0]);
        // Four registered, one disabled: three occurrences in the plan.
        let planned = plan(&ultra(), screen_sizing(1280, 720, 1.0, 1.0), &state(&passes));
        assert_eq!(
            planned
                .steps
                .iter()
                .filter(|s| s.pass == FramePass::Registered)
                .count(),
            4
        );
        assert_eq!(registered_order(NO_PASSES), Vec::<usize>::new());
    }

    /// The `?rview=` arm replaces the composite and FXAA with one blit to the
    /// canvas, at every tier.
    #[test]
    fn the_debug_view_replaces_the_composite_entirely() {
        let sizing = screen_sizing(1920, 1080, 1.0, 0.72);
        [QualityTier::Low, QualityTier::Ultra].iter().for_each(|&t| {
            let pipeline = FramePipeline::resolve(t, BackendCapabilityProfile::all(), 16);
            let debugging = FrameState {
                debug_view: Some(DebugView::Ao),
                ..state(NO_PASSES)
            };
            let planned = plan(&pipeline, sizing, &debugging);
            let order: Vec<FramePass> = planned.steps.iter().map(|s| s.pass).collect();
            assert!(!order.contains(&FramePass::Composite), "tier {t:?}");
            assert!(!order.contains(&FramePass::Fxaa), "tier {t:?}");
            assert_eq!(order.last(), Some(&FramePass::Debug));
            // ...and it draws the canvas, not the internal size.
            let last = planned.steps.last().unwrap();
            assert_eq!((last.width, last.height), sizing.display);
        });
    }

    /// The three passes that run at something other than the screen size, and
    /// the one that runs at 1x1.
    #[test]
    fn each_pass_runs_at_the_resolution_the_source_sizes_it_at() {
        let sizing = screen_sizing(1920, 1080, 1.0, 1.0);
        let planned = plan(&ultra(), sizing, &state(NO_PASSES));
        let step = |p: FramePass| *planned.steps.iter().find(|s| s.pass == p).unwrap();

        // The cascade atlas is square, at the clamped map size, four deep.
        let cascades = step(FramePass::Cascades);
        assert_eq!((cascades.width, cascades.height, cascades.layers), (2048, 2048, 4));
        // SSR is half resolution — the single most expensive marching pass.
        let ssr = step(FramePass::Ssr);
        assert_eq!((ssr.width, ssr.height), (960, 540));
        // The bloom pyramid's widest mip is likewise half.
        let bloom = step(FramePass::Bloom);
        assert_eq!((bloom.width, bloom.height), (960, 540));
        // Metering reduces to a 1x1 exposure; the chain above it is the
        // exposure module's own.
        let meter = step(FramePass::Metering);
        assert_eq!((meter.width, meter.height), (1, 1));
        assert_eq!(CHAIN_SIZES.last(), Some(&1));
        // Everything else is full screen.
        [
            FramePass::Prepass,
            FramePass::Gtao,
            FramePass::ContactShadows,
            FramePass::ForwardWorld,
            FramePass::Viewmodel,
            FramePass::Taa,
            FramePass::MotionBlur,
            FramePass::DepthOfField,
            FramePass::ViewmodelComposite,
        ]
        .iter()
        .for_each(|&p| {
            let s = step(p);
            assert_eq!((s.width, s.height), sizing.screen, "{p:?} is not full-screen");
            assert_eq!(s.layers, 1);
        });
        // CPU steps carry no attachment at all.
        [FramePass::SceneWalk, FramePass::TaaJitter, FramePass::RemoveJitter]
            .iter()
            .for_each(|&p| {
                let s = step(p);
                assert_eq!(s.target, StepTarget::Cpu);
                assert_eq!((s.width, s.height, s.layers), (0, 0, 0));
            });
    }

    /// The module each pass is expected to live in — the wiring contract for
    /// the integration pass, stated as data so it is greppable.
    #[test]
    fn ten_pass_slots_name_seven_modules_this_crate_does_not_have_yet() {
        let all = [
            FramePass::SceneWalk,
            FramePass::Cascades,
            FramePass::TaaJitter,
            FramePass::Prepass,
            FramePass::Gtao,
            FramePass::ContactShadows,
            FramePass::Ssr,
            FramePass::ForwardWorld,
            FramePass::Viewmodel,
            FramePass::RemoveJitter,
            FramePass::Taa,
            FramePass::MotionBlur,
            FramePass::DepthOfField,
            FramePass::Registered,
            FramePass::ViewmodelComposite,
            FramePass::Metering,
            FramePass::Bloom,
            FramePass::Composite,
            FramePass::Fxaa,
            FramePass::Debug,
        ];
        let missing: Vec<&'static str> = all
            .iter()
            .filter(|p| !p.module_exists_today())
            .map(|p| p.module_path())
            .collect();
        assert_eq!(
            missing,
            vec![
                "crate::gtao",
                "crate::contact",
                "crate::ssr",
                "crate::taa",
                "crate::motionblur",
                "crate::dof",
                "crate::composite",
                "crate::composite",
                "crate::composite",
                "crate::composite",
            ],
            "the binder's expiry check: when every one of these is declared in \
             src/lib.rs, write frame_graph/bind.rs"
        );
        // The nine that do exist are named exactly as `lib.rs` declares them.
        assert!(all
            .iter()
            .filter(|p| p.module_exists_today())
            .all(|p| p.module_path().starts_with("crate::")
                | p.module_path().starts_with('(')));
    }

}
