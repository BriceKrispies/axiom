//! **`resize(w, h, ctx)`** — the render-scale arithmetic, and the set of render
//! targets the frame graph itself owns.
//!
//! # Two sizes, and they are not the same size
//!
//! ```js
//! const pr = Math.min(globalThis.devicePixelRatio || 1, 1.5);
//! const dw = Math.max(1, Math.floor(w * pr));           // displaySize
//! const rw = Math.max(1, Math.floor(dw * this.q.renderScale)); // screenSize
//! ```
//!
//! `displaySize` is the canvas backbuffer; `screenSize` is what everything
//! before the composite runs at. Note the second floor takes the **device**
//! width, not the CSS width — `renderScale` scales the already-ratio'd size, so
//! a 0.72 tier on a 1.5x display renders at `floor(floor(w * 1.5) * 0.72)` and
//! not at `floor(w * 1.08)`. Those disagree: at `w = 1000` the first is 1080
//! and the second is 1080, but at `w = 1001` they are 1081 and 1081, and at
//! `w = 999` they are 1078 and 1078 — they agree far more often than they
//! differ, which is exactly why folding the two floors into one is the kind of
//! "tidying" that survives a smoke test and shows up as an off-by-one row of
//! pixels on somebody else's monitor.
//!
//! **There is no upscale pass.** The internal chain runs at `screenSize` and
//! the *composite* (or, on the FXAA path, FXAA) writes the canvas at
//! `displaySize`; the filtered magnification happens in that one blit. This
//! module records it because [`crate::upscale`] exists in this crate and does
//! something else — it is the live binding's own reduced-resolution present.
//!
//! # Arithmetic width
//!
//! Every one of those is a JS number, so `f64`. The two floors are the only
//! places the width could matter and they are both integer-valued, but the
//! *products* feeding them are not: `1920 * 0.72` is `1382.3999999999999` at
//! double width, and a single-precision evaluation of the same product is
//! `1382.4000244140625`. Both floor to 1382 here, and there is no reason to
//! rely on that.
//!
//! # Storage widths
//!
//! Every frame-graph colour target is `hdrTarget(...)`, i.e. `HalfFloatType` +
//! `RGBAFormat` = [`HostAttachmentFormat::Rgba16Float`]. The single exception is
//! `ldrRt`, `UnsignedByteType` + `RGBAFormat`, and it is allocated **only** on
//! the FXAA path — the source's own note records that when TAA was on this was
//! "a full-resolution RGBA8 target (13 MB at 3.34 MP) allocated on every resize
//! and never sampled once".
//!
//! On an arm without [`axiom_host::RenderCapability::HdrTargets`] every one of
//! them degrades through [`HostAttachmentFormat::ldr_substitute`] — the
//! identical passes into a coarser target, which is that capability's declared
//! [`axiom_host::CapabilityDegradation::Substitute`]. Nothing is skipped.

use axiom_host::{BackendCapabilityProfile, HostAttachmentFormat};

use super::pipeline::FramePipeline;

/// `Math.min(globalThis.devicePixelRatio || 1, 1.5)` — the ratio ceiling.
///
/// Not a quality setting: it is a hard cap on how many pixels a 3x phone
/// display is allowed to ask the whole chain for.
pub(crate) const MAX_PIXEL_RATIO: f64 = 1.5;

/// The two sizes `resize` computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScreenSizing {
    /// `displaySize` — the canvas backbuffer, `floor(css * pixelRatio)`.
    pub(crate) display: (u32, u32),
    /// `screenSize` — the internal HDR chain, `floor(display * renderScale)`.
    pub(crate) screen: (u32, u32),
}

/// `globalThis.devicePixelRatio || 1`, then `Math.min(_, 1.5)`.
///
/// `|| 1` is JavaScript falsiness, which catches `0` and `NaN` (and `-0`)
/// before the `min` ever sees them. That ordering matters in Rust for a reason
/// it does not in JS: `Math.min(NaN, 1.5)` is `NaN`, while `f64::min(NaN, 1.5)`
/// is `1.5`. The falsiness test runs first here, so the two implementations
/// cannot be told apart — but the divergence is real and is why the test is
/// written before the clamp rather than folded into it.
pub(crate) fn pixel_ratio(device_pixel_ratio: f64) -> f64 {
    let falsy = (device_pixel_ratio == 0.0) | device_pixel_ratio.is_nan();
    [device_pixel_ratio, 1.0][usize::from(falsy)].min(MAX_PIXEL_RATIO)
}

/// `Math.max(1, Math.floor(v))`, narrowed to the integer a texture dimension is.
///
/// A Rust `as u32` **saturates** where JS would carry a huge float onward; a
/// canvas dimension is bounded by the browser at four orders of magnitude below
/// that, so the two cannot be told apart from any reachable input.
fn floor_at_least_one(v: f64) -> u32 {
    v.floor().max(1.0) as u32
}

/// `resize(w, h, ctx)`'s sizing half, as a value.
pub(crate) fn screen_sizing(
    css_width: u32,
    css_height: u32,
    device_pixel_ratio: f64,
    render_scale: f64,
) -> ScreenSizing {
    let ratio = pixel_ratio(device_pixel_ratio);
    let display = (
        floor_at_least_one(f64::from(css_width) * ratio),
        floor_at_least_one(f64::from(css_height) * ratio),
    );
    ScreenSizing {
        display,
        screen: (
            floor_at_least_one(f64::from(display.0) * render_scale),
            floor_at_least_one(f64::from(display.1) * render_scale),
        ),
    }
}

/// `Math.max(1, w >> 1)` — the half-resolution rule SSR and the ADS depth of
/// field both size their internal targets by. A shift, so it floors.
pub(crate) const fn half_res(v: u32) -> u32 {
    let halved = v >> 1;
    [halved, 1][((halved < 1) as usize)]
}

/// `Math.max(1, Math.ceil(w / 16))` — motion blur's velocity-tile grid.
pub(crate) const fn tile_res(v: u32) -> u32 {
    let tiles = v.div_ceil(16);
    [tiles, 1][((tiles < 1) as usize)]
}

/// A render target the **frame graph itself** owns.
///
/// The discriminant is the allocation order in `resize`, which is also the
/// order a reader of `dispose()` will find them in. Targets belonging to a
/// *pass* (the cascade atlas, the G-buffer, GTAO's history, TAA's history, the
/// bloom mips, the exposure chain) are that pass's, not this one's.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameTarget {
    /// `hdrRt` — the forward world pass's colour + depth. `r.hdrTexture`.
    Hdr = 0,
    /// `viewRt` — the viewmodel's own MSAA colour + depth, cleared to
    /// **transparent** black so the composite has real coverage to resolve.
    Viewmodel = 1,
    /// `pingRt[0]`.
    Ping0 = 2,
    /// `pingRt[1]`.
    Ping1 = 3,
    /// `ldrRt` — allocated **only** when FXAA is on (i.e. when TAA is off).
    Ldr = 4,
}

/// One target's full allocation description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetDesc {
    /// Which target this is.
    pub(crate) target: FrameTarget,
    /// `rt.texture.name` in the source — kept so a captured frame names itself.
    pub(crate) label: &'static str,
    /// Width in texels.
    pub(crate) width: u32,
    /// Height in texels.
    pub(crate) height: u32,
    /// The colour attachment format, already degraded for this profile.
    pub(crate) format: HostAttachmentFormat,
    /// `depthBuffer: true` → a `Depth32Float` companion; `false` → `None`.
    pub(crate) depth: Option<HostAttachmentFormat>,
    /// `samples` — MSAA, non-zero on the viewmodel target alone.
    pub(crate) samples: u32,
}

impl TargetDesc {
    /// What one texel of the colour attachment costs, in bytes.
    const fn colour_bytes_per_texel(self) -> u64 {
        // Only the two formats the frame graph's own targets can hold.
        [4, 8][((self.format as u32 == HostAttachmentFormat::Rgba16Float as u32) as usize)]
    }

    /// Total bytes, colour + depth, across every MSAA sample.
    ///
    /// `samples` is `0` in the source when MSAA is off, and a zero-sample
    /// target still stores one sample — hence the `max(1)`.
    pub(crate) fn bytes(self) -> u64 {
        let depth_bytes = u64::from(self.depth.is_some()) * 4;
        let per_texel = self.colour_bytes_per_texel() + depth_bytes;
        u64::from(self.width)
            * u64::from(self.height)
            * per_texel
            * u64::from(self.samples.max(1))
    }
}

/// `hdrTarget(w, h, opts)`'s format, degraded for this device.
fn colour_format(profile: BackendCapabilityProfile, hdr: bool) -> HostAttachmentFormat {
    let wanted = [
        HostAttachmentFormat::Rgba8UnormSrgb,
        HostAttachmentFormat::Rgba16Float,
    ][usize::from(hdr)];
    [wanted.ldr_substitute(), wanted][usize::from(profile.supports_attachment(wanted))]
}

/// Every target `resize` allocates, in allocation order.
///
/// `ldr` is present only when the pipeline runs FXAA — the source's
/// `this.ldrRt = null; if (this.fxaa) this.ldrRt = new WebGLRenderTarget(...)`.
pub(crate) fn frame_targets(
    pipeline: &FramePipeline,
    sizing: ScreenSizing,
    profile: BackendCapabilityProfile,
) -> Vec<TargetDesc> {
    let (w, h) = sizing.screen;
    let hdr = colour_format(profile, true);
    let depth = Some(HostAttachmentFormat::Depth32Float);
    core::iter::empty()
        .chain(core::iter::once(TargetDesc {
            target: FrameTarget::Hdr,
            label: "hdr",
            width: w,
            height: h,
            format: hdr,
            depth,
            samples: 0,
        }))
        .chain(core::iter::once(TargetDesc {
            target: FrameTarget::Viewmodel,
            label: "viewmodel",
            width: w,
            height: h,
            format: hdr,
            depth,
            samples: pipeline.view_samples(),
        }))
        .chain(core::iter::once(TargetDesc {
            target: FrameTarget::Ping0,
            label: "ping0",
            width: w,
            height: h,
            format: hdr,
            depth: None,
            samples: 0,
        }))
        .chain(core::iter::once(TargetDesc {
            target: FrameTarget::Ping1,
            label: "ping1",
            width: w,
            height: h,
            format: hdr,
            depth: None,
            samples: 0,
        }))
        .chain(
            core::iter::once(TargetDesc {
                target: FrameTarget::Ldr,
                label: "ldr",
                width: w,
                height: h,
                format: colour_format(profile, false),
                depth: None,
                samples: 0,
            })
            .filter(|_| pipeline.fxaa()),
        )
        .collect()
}

/// What the frame graph's own targets cost this device, in bytes.
///
/// Not a curiosity: the LDR target the source removed from the TAA path was 13
/// MB per resize, and the viewmodel's 4x MSAA is the single largest line in
/// this table at the top two tiers.
pub(crate) fn total_bytes(targets: &[TargetDesc]) -> u64 {
    targets.iter().map(|t| t.bytes()).sum()
}

#[cfg(test)]
mod tests {
    use super::{
        frame_targets, half_res, pixel_ratio, screen_sizing, tile_res, total_bytes, FrameTarget,
        MAX_PIXEL_RATIO,
    };
    use crate::frame_graph::pipeline::FramePipeline;
    use crate::frame_graph::quality::QualityTier;
    use axiom_host::{BackendCapabilityProfile, HostAttachmentFormat, RenderCapability};

    /// The ratio is capped at 1.5 and floored at 1 through JS falsiness.
    #[test]
    fn the_device_pixel_ratio_is_clamped_at_both_ends() {
        assert_eq!(pixel_ratio(1.0), 1.0);
        assert_eq!(pixel_ratio(1.25), 1.25);
        assert_eq!(pixel_ratio(3.0), MAX_PIXEL_RATIO);
        // `|| 1` catches every falsy number before the clamp sees it.
        assert_eq!(pixel_ratio(0.0), 1.0);
        assert_eq!(pixel_ratio(-0.0), 1.0);
        assert_eq!(pixel_ratio(f64::NAN), 1.0);
    }

    /// The render scale multiplies the **device** size, not the CSS size, and
    /// each step floors independently.
    #[test]
    fn the_render_scale_multiplies_the_device_size_not_the_css_size() {
        // 1920x1080 at 1x, low tier: 0.72 of the device size.
        let low = screen_sizing(1920, 1080, 1.0, QualityTier::Low.preset().render_scale);
        assert_eq!(low.display, (1920, 1080));
        assert_eq!(low.screen, (1382, 777));
        // 1382 and not 1383: `1920 * 0.72` is 1382.3999999999999 at f64.
        assert_eq!((1920.0_f64 * 0.72).floor() as u32, 1382);

        // The same canvas at 2x is clamped to 1.5x first, and the two floors
        // then compose: floor(floor(1920*1.5) * 0.72).
        let retina = screen_sizing(1920, 1080, 2.0, 0.72);
        assert_eq!(retina.display, (2880, 1620));
        assert_eq!(retina.screen, (2073, 1166));
        assert_eq!((2880.0_f64 * 0.72).floor() as u32, 2073);

        // Full scale is an identity on both axes.
        let ultra = screen_sizing(1600, 900, 1.0, QualityTier::Ultra.preset().render_scale);
        assert_eq!(ultra.display, ultra.screen);

        // A degenerate canvas still yields a legal texture size.
        assert_eq!(screen_sizing(0, 0, 1.0, 1.0).screen, (1, 1));
        assert_eq!(screen_sizing(1, 1, 1.0, 0.001).screen, (1, 1));
    }

    /// SSR's and the ADS depth of field's half-res rule, and motion blur's tile
    /// grid, both floored at one.
    #[test]
    fn the_internal_pass_resolutions_never_reach_zero() {
        assert_eq!(half_res(1920), 960);
        assert_eq!(half_res(1081), 540, "a shift floors an odd width");
        assert_eq!(half_res(1), 1);
        assert_eq!(half_res(0), 1);
        assert_eq!(tile_res(1920), 120);
        assert_eq!(tile_res(1081), 68, "ceil, not floor: 1081/16 is 67.6");
        assert_eq!(tile_res(1), 1);
        assert_eq!(tile_res(0), 1);
    }

    /// The FXAA path allocates a fifth target and the TAA path does not — the
    /// 13 MB the source's own comment records.
    #[test]
    fn the_ldr_target_exists_only_on_the_fxaa_path() {
        let profile = BackendCapabilityProfile::all();
        let sizing = screen_sizing(1920, 1080, 1.0, 1.0);

        let low = FramePipeline::resolve(QualityTier::Low, profile, 16);
        let low_targets = frame_targets(&low, sizing, profile);
        assert_eq!(low_targets.len(), 5);
        assert_eq!(low_targets[4].target, FrameTarget::Ldr);
        assert_eq!(low_targets[4].format, HostAttachmentFormat::Rgba8UnormSrgb);
        // The 13 MB: 1920 * 1080 * 4 bytes.
        assert_eq!(low_targets[4].bytes(), 1920 * 1080 * 4);

        let ultra = FramePipeline::resolve(QualityTier::Ultra, profile, 16);
        let ultra_targets = frame_targets(&ultra, sizing, profile);
        assert_eq!(ultra_targets.len(), 4);
        assert!(ultra_targets.iter().all(|t| t.target != FrameTarget::Ldr));
    }

    /// The viewmodel is the only multisampled target, and its sample count is a
    /// tier decision that dominates the frame graph's own memory.
    #[test]
    fn the_viewmodel_is_the_only_multisampled_target() {
        let profile = BackendCapabilityProfile::all();
        let sizing = screen_sizing(1920, 1080, 1.0, 1.0);
        let ultra = FramePipeline::resolve(QualityTier::Ultra, profile, 16);
        let targets = frame_targets(&ultra, sizing, profile);
        let multisampled: Vec<FrameTarget> = targets
            .iter()
            .filter(|t| t.samples > 1)
            .map(|t| t.target)
            .collect();
        assert_eq!(multisampled, vec![FrameTarget::Viewmodel]);
        assert_eq!(targets[1].samples, 4);

        // Colour (8) + depth (4) per texel, times four samples.
        assert_eq!(targets[1].bytes(), 1920 * 1080 * 12 * 4);
        // ...against the un-multisampled hdr target of the same shape.
        assert_eq!(targets[0].bytes(), 1920 * 1080 * 12);
        // And the pings carry no depth at all.
        assert_eq!(targets[2].depth, None);
        assert_eq!(targets[2].bytes(), 1920 * 1080 * 8);

        // The low tier's viewmodel target is single-sampled (`_viewSamples 0`).
        let low = FramePipeline::resolve(QualityTier::Low, profile, 16);
        let low_targets = frame_targets(&low, screen_sizing(1920, 1080, 1.0, 1.0), profile);
        assert_eq!(low_targets[1].samples, 0);
        assert_eq!(low_targets[1].bytes(), low_targets[0].bytes());
    }

    /// On an arm without HDR targets every colour attachment degrades to the
    /// declared substitute; nothing is dropped and the target count is
    /// unchanged.
    #[test]
    fn a_device_without_hdr_targets_renders_the_same_passes_into_a_coarser_target() {
        let sizing = screen_sizing(1280, 720, 1.0, 1.0);
        let full = BackendCapabilityProfile::all();
        let ldr_only = full.without(RenderCapability::HdrTargets);
        let pipeline = FramePipeline::resolve(QualityTier::Ultra, ldr_only, 16);

        let degraded = frame_targets(&pipeline, sizing, ldr_only);
        assert!(degraded
            .iter()
            .all(|t| t.format == HostAttachmentFormat::Rgba8UnormSrgb));

        let hdr = frame_targets(&FramePipeline::resolve(QualityTier::Ultra, full, 16), sizing, full);
        assert_eq!(degraded.len(), hdr.len(), "a degradation drops no target");
        assert!(hdr
            .iter()
            .all(|t| t.format == HostAttachmentFormat::Rgba16Float));
        // ...and it is cheaper, which is the only observable difference.
        assert!(total_bytes(&degraded) < total_bytes(&hdr));
    }
}
