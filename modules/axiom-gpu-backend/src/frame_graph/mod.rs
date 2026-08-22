//! **The frame graph** — `C:/dev/Claude-of-Duty/src/render/index.js` (1,696
//! lines) and `src/render/pass.js` (80), transcribed as a pure, CPU-testable
//! *sequencer*.
//!
//! Everything else in this crate that came out of `src/render/` is one pass:
//! [`crate::cascade`] is `csm.js`, [`crate::gbuffer`] is `prepass.js`,
//! [`crate::bloom_pyramid`] is `bloom.js`, [`crate::exposure`] is
//! `exposure.js`, [`crate::agx`] is the tone curve out of `glsl.js`. This
//! module is the thing that decides **which of them exist, at what size, and in
//! what order** — the file every other render pass hangs off.
//!
//! # Why this is a value, not a call chain
//!
//! The source's `render(ctx)` is 290 lines of straight-line GPU calls, and the
//! only way to ask it "does SSR run on medium?" is to run a browser. Here the
//! answer is a function of plain data: [`schedule::plan`] takes a
//! [`pipeline::FramePipeline`] (what the tier and the device allow), a
//! [`targets::ScreenSizing`] and a [`schedule::FrameState`], and returns the
//! ordered [`schedule::PlannedStep`] list — pass, attachment, resolution. The
//! sequencing is therefore testable without a GPU, which is the whole point:
//! *which passes run, in what order, at what size, for a given tier and
//! capability set* is exactly the part that a wrong pixel cannot tell you about.
//!
//! Binding a step to the wgpu work that executes it is deliberately **not**
//! here — see "What this module does not do" below.
//!
//! # The frame order, verbatim from the source header
//!
//! ```text
//!   1  scene walk        collect draw/hide lists, patch new materials
//!   2  CSM               N stabilised cascades into one R32F array texture
//!   3  jitter            sub-pixel offset on the WORLD camera for TAA
//!   4  prepass           MRT: view normal + velocity + linear depth
//!   5  GTAO              horizon-arc AO, temporally accumulated
//!   6  contact shadows   short depth-buffer ray march toward the sun
//!   7  SSR               marched against depth, coloured from last frame
//!   8  forward world     lit with 2/5/6/7 injected into every material
//!   9  viewmodel         same lighting, into its OWN MSAA colour+depth target
//!  10  TAA               velocity reprojection + YCoCg variance clipping
//!  11  motion blur       velocity-tile reconstruction filter
//!  12  ADS depth of field gather CoC blur, only while the sights are up
//!  13  custom passes     whatever fx/ui/sky registered
//!  14  viewmodel resolve premultiplied composite over the world, FXAA'd
//!  15  metering          GPU log-luminance reduction -> EV100 -> exposure
//!  16  bloom             Karis pyramid with a soft-knee highlight threshold
//!  17  composite         AgX + LUT + vignette + CA + grain -> sRGB
//!  18  FXAA              only when TAA is off
//! ```
//!
//! # The pass-ordering contract — what reads what
//!
//! | step | reads | writes |
//! |---|---|---|
//! | 2 CSM | the world scene, `sunDir` | the R32F cascade array (`cascade`) |
//! | 4 prepass | the world scene (transparents hidden) | normal / velocity / linear depth (`gbuffer`) |
//! | 5 GTAO | gbuffer depth + normal, **its own history** | `owAoTex`, injected into every lit material |
//! | 6 contact | gbuffer depth + normal, `sunDirView` | `owContactTex`, likewise |
//! | 7 SSR | gbuffer depth/normal/velocity **and last frame's resolved colour** | `owSsrTex`, likewise |
//! | 8 forward | 2/5/6/7 through the material patch | `hdr` |
//! | 9 viewmodel | its own 3-point rig | `viewmodel` (MSAA) |
//! | 10 TAA | `hdr`, gbuffer velocity, **its own history**, `prevVP` | its history ping-pong |
//! | 11 motion blur | previous colour, gbuffer velocity | its own full-res target |
//! | 12 DOF | previous colour, gbuffer depth | a **frame-graph ping** |
//! | 13 registered | previous colour | a **frame-graph ping** |
//! | 14 view composite | previous colour + `viewmodel` | a **frame-graph ping** |
//! | 15 metering | previous colour, gbuffer depth, **its own adapt history** | 1x1 exposure |
//! | 16 bloom | previous colour, the exposure scalar | its mip pyramid |
//! | 17 composite | previous colour, bloom, exposure | `ldr` (FXAA path) or the canvas |
//! | 18 FXAA | `ldr` | the canvas |
//!
//! Three kinds of ordering constraint fall out of that table, and
//! [`schedule::plan`] encodes all three:
//!
//! - **Producer/consumer.** 5/6/7 all read the G-buffer, so 4 precedes them;
//!   8 reads all three, so it follows them. 15 and 16 read the *composited*
//!   colour, so 14 precedes them — which is also why the muzzle flash meters
//!   and blooms (the source's own note).
//! - **History.** GTAO, TAA and the exposure adaptation each hold a
//!   **ping-pong pair** across frames, and SSR reads the *previous* frame's
//!   resolved colour. With TAA on that is `taa.previousTexture`; with TAA off
//!   it is `hdr` itself, which at step 7 still holds last frame — which is why
//!   7 must sit **before** 8, not after. SSR is therefore skipped on the very
//!   first frame ([`schedule::FrameState::first_frame`]).
//! - **Convention.** The viewmodel is resolved separately (9 + 14) because
//!   `viewScene` moves in *view* space and a camera-matrix velocity buffer
//!   describes none of it; and 14 sits after 13 because a depth-driven fog pass
//!   registered at 13 would otherwise bury the weapon in 40 m of aerial
//!   perspective.
//!
//! # Quality tiers are the file's main structure, not a detail
//!
//! The original boots logging
//! `[render] WebGL2 · ultra · 4x2048 CSM · taa:true gtao:true ssr:true mb:true`.
//! Everything in that line is a tier decision, and [`quality::boot_line`]
//! reproduces it exactly for all four tiers. The tier decides which passes are
//! *constructed at all* ([`pipeline::FramePipeline::resolve`]), the internal
//! resolution ([`quality::QualityPreset::render_scale`]), the cascade count and
//! map size, the viewmodel's MSAA sample count, and the bloom pyramid's depth.
//!
//! **`QualityTier` is an enum used as a table index** (`QUALITY_LEVEL[quality]`
//! in the source, and `qLevel >= 1` / `>= 2` comparisons everywhere after), so
//! its discriminant order is load-bearing and pinned by
//! `quality::tests::the_tier_order_is_the_source_table_order`.
//!
//! # What this module does not do
//!
//! It does not execute anything. There is no `wgpu` here, no pass binding, and
//! no `Instance`. The reason is structural rather than scheduling: a frame graph
//! whose plan type names each pass's concrete Rust type is not a graph, it is a
//! hard-wired chain, and it cannot be tested without every pass existing. So the
//! plan names passes by [`schedule::FramePass`] and records the module each one
//! is expected to live in ([`schedule::FramePass::module_path`]).
//!
//! **Deferral, and what makes it live.** The binder — `frame_graph/bind.rs`,
//! mapping each [`schedule::PlannedStep`] onto a real pass call — is not written
//! because seven of the modules it would call do not exist yet in this crate:
//! `gtao`, `contact`, `ssr`, `taa`, `motionblur`, `dof` and `composite` (that
//! last one being `render/composite.js`, 353 lines, which no slice of this wave
//! appears to own — see
//! `docs/work-manifests/shmup-port/notes/render-frame-graph.md` §7). The moment
//! all seven are declared in `src/lib.rs`, this deferral has expired:
//! add `frame_graph/bind.rs`, and change `src/lib.rs` to declare it. Nothing
//! else in this crate needs to move.
//!
//! # Storage widths
//!
//! `pass.js`'s `hdrTarget` is `HalfFloatType` + `RGBAFormat`, i.e. `Rgba16Float`
//! — every frame-graph colour target except the FXAA path's `ldr`
//! (`UnsignedByteType`, `Rgba8UnormSrgb`). The full-screen triangle's positions
//! and UVs are `Float32Array`. The exposure chain alone is `FloatType`
//! (`Rgba32Float`), because a half-float 1x1 would quantize an EV. All of it is
//! in [`targets::TargetDesc`] and [`fullscreen`], and none of it is incidental:
//! the source's own note is that the LDR intermediate was 13 MB allocated on
//! every resize and never sampled once until it was made conditional on FXAA.
//!
//! # Arithmetic width
//!
//! JavaScript has one number type. Every quantity `index.js` computes — the
//! render-scale floor, the bounce-fill hue push, the viewmodel key shaping, the
//! room transform — is `f64`, narrowed to `f32` only when it is written into a
//! uniform. This module does the same: `f64` throughout
//! ([`lighting`], [`rooms`], [`targets`]), narrowed once at the boundary. That
//! is the same decision [`crate::cascade`] made and for the same reason.

pub(crate) mod debug_view;
pub(crate) mod frame_inputs;
pub(crate) mod fullscreen;
pub(crate) mod lighting;
pub(crate) mod pipeline;
pub(crate) mod prewarm;
pub(crate) mod quality;
pub(crate) mod rooms;
pub(crate) mod schedule;
pub(crate) mod settings;
pub(crate) mod targets;

#[cfg(test)]
mod tests {
    use super::pipeline::FramePipeline;
    use super::quality::{QualityTier, QUALITY_TIERS};
    use super::schedule::{plan, FramePass, FrameState, RegisteredPass};
    use super::targets::{frame_targets, screen_sizing};
    use axiom_host::BackendCapabilityProfile;

    /// The whole point of the module, in one assertion: the original's boot
    /// banner, reproduced from the tier table alone.
    #[test]
    fn the_ultra_tier_reproduces_the_originals_boot_line() {
        assert_eq!(
            super::quality::boot_line(QualityTier::Ultra),
            "[render] WebGL2 · ultra · 4x2048 CSM · taa:true gtao:true ssr:true mb:true"
        );
    }

    /// An end-to-end walk: for every tier, on a fully capable device, the plan
    /// is non-empty, starts with the scene walk and ends on the screen.
    #[test]
    fn every_tier_plans_a_frame_that_starts_at_the_scene_and_ends_at_the_screen() {
        let profile = BackendCapabilityProfile::all();
        let registered = [
            RegisteredPass { order: Some(-1), enabled: None },
            RegisteredPass { order: None, enabled: Some(true) },
        ];
        let state = FrameState {
            first_frame: false,
            view_visible: true,
            ads_t: 1.0,
            registered: &registered,
            debug_view: None,
            ping_index: 0,
        };
        QUALITY_TIERS.iter().for_each(|&tier| {
            let pipeline = FramePipeline::resolve(tier, profile, 16);
            let sizing = screen_sizing(1920, 1080, 1.0, pipeline.render_scale());
            let planned = plan(&pipeline, sizing, &state);
            assert_eq!(
                planned.steps.first().map(|s| s.pass),
                Some(FramePass::SceneWalk),
                "tier {tier:?} did not start with the scene walk"
            );
            assert!(
                planned
                    .steps
                    .last()
                    .is_some_and(|s| s.target == super::schedule::StepTarget::Screen),
                "tier {tier:?} did not end on the screen"
            );
            // And every target it allocates is at a non-degenerate size.
            assert!(
                frame_targets(&pipeline, sizing, profile)
                    .iter()
                    .all(|t| (t.width >= 1) & (t.height >= 1)),
                "tier {tier:?} allocated a zero-sized target"
            );
        });
    }
}
