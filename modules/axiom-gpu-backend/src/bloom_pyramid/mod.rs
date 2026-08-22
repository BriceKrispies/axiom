//! **The bloom pyramid**: `render/bloom.js`, as WGSL plus its CPU reference.
//!
//! Ported from Claude-of-Duty `src/render/bloom.js` (215 lines) — the
//! progressive dual-filter pyramid from *"Next Generation Post Processing in
//! Call of Duty: Advanced Warfare"* (Jimenez 2014). It is **not** an
//! UnrealBloomPass, and it is not what [`crate::post_chain`] does.
//!
//! # What [`crate::post_chain`] had, and how it differed
//!
//! `post_chain` already carried a bloom: one bright pass at half resolution, one
//! separable nine-tap Gaussian (horizontal then vertical), one composite. Every
//! line of it was reasonable and almost none of it was this algorithm. The
//! differences, in the order they change the picture:
//!
//! | | `post_chain` (before) | `bloom.js` (the source) |
//! |---|---|---|
//! | structure | 1 level, half res | **pyramid**, 6 levels (5 on the low tier) |
//! | downsample | none — the bright pass is a 1-tap copy | **13-tap**, `±2` and `±1` texel |
//! | firefly guard | none | **Karis luminance average** on level 0 only |
//! | prefilter driver | Rec.709 **luma** | **max channel** |
//! | prefilter denominator | `4·knee` | `4·knee + 1e-5` |
//! | prefilter clamp | ratio clamped to `0..=1` | unclamped |
//! | exposure | none | taps scaled by metered exposure **before** the threshold |
//! | clamp | none | `min(24)` after the karis combine |
//! | blur | separable 9-tap Gaussian | **9-tap tent upsample** back up the chain |
//! | accumulation | n/a | **50/50 alpha blend**, not a sum |
//! | wide levels | n/a | radius `0.62`, weight `0.34` on the top two |
//! | combine | `scene + glow·intensity`, then a per-channel rolloff | `hdr += max(bloom,0)·max(strength,0)`, **pre-tonemap** |
//! | storage | 8-bit sRGB | **`Rgba16Float`** at every level |
//!
//! The prefilter driver is the one a reader is most likely to wave through.
//! Luma-driven, a saturated red tracer at `(1.6, 0, 0)` measures `0.34` and does
//! not bloom at all under a threshold of `1.6`; max-channel-driven it measures
//! `1.6` and blooms, which is the entire reason the source's comment says *"a red
//! tracer, an orange muzzle flash — blooms as readily as a white one instead of
//! being judged on its luminance alone."* [`prefilter::tests`] pins that case.
//!
//! # The 8-bit ceiling is still there, and this module cannot lift it
//!
//! `01-engine-gaps.md` gap **G1** — "the scene target is 8-bit sRGB on every arm,
//! so nothing above 1.0 survives to the post chain" — is **not** fixed by
//! [`axiom_host::RenderCapability::HdrTargets`] landing.
//! [`crate::hdr_target`] resolves and grants a *capability bit*;
//! [`crate::surface_encode::scene_target_format`] still returns
//! `surface.add_srgb_suffix()` (8-bit) and `crate::offscreen`'s `COLOR_FORMAT` is
//! still `Rgba8UnormSrgb`, so the value the bright pass samples is still clamped
//! at 1.0. Nothing in this module can change that: the clamp happens in the pass
//! *upstream*, and rewiring the scene target is
//! [`crate::live_gpu_binding`]/`crate::offscreen`'s line to write, not this one.
//!
//! What this module does do is stop being part of the problem. Every target in
//! [`chain`] is `Rgba16Float`, the prefilter runs in exposure-scaled linear light
//! with no clamp of its own, and the combine is a plain add into HDR ahead of a
//! tone map. Feed it a clamped scene and it will faithfully bloom a clamped
//! scene; feed it an HDR one and it ranks two highlights, which is the point.
//!
//! # The shape
//!
//! One concern per file, and each carries three things that land together —
//! exactly the shape [`crate::material_shader`] established:
//!
//! 1. the **WGSL**, as a `&str` (in [`wgsl`], one definition shared by the real
//!    pass and the parity harness, so neither can drift from the other);
//! 2. a **CPU reference** in Rust, which is the semantic definition;
//! 3. a **parity test** proving the two agree on a real adapter, at a tolerance
//!    derived from a measurement (in [`parity`]).
//!
//! - [`prefilter`] — the soft-knee highlight prefilter and the Karis weight.
//! - [`filters`] — the 13-tap downsample (both arms), the 9-tap tent upsample,
//!   the alpha blend, and the final combine.
//! - [`schedule`] — the mip sizing (`setSize`) and the per-level radius/weight
//!   schedule (`render`).
//! - [`half_storage`] — `Rgba16Float` round-to-nearest-even, because the mips are
//!   half-float in the source and storage width is part of the algorithm.
//! - [`reference`] — the whole pyramid over a CPU image: the semantic definition
//!   of the chain, not merely of its arithmetic.
//! - [`chain`] — the real wgpu passes.
//!
//! # What is deliberately NOT here
//!
//! **Tone mapping and exposure metering.** `composite.js` adds this pyramid's
//! output into HDR and *then* runs AgX; `exposure.js` meters EV100 into a 1x1
//! `FloatType` target that the level-0 downsample samples. Both are a sibling
//! slice's work. The boundary taken here is the honest one: [`BloomTuning`]
//! carries `exposure` as a plain `f32`, which is *exactly* what
//! `texture2D(tExposure, vec2(0.5)).r` yields from a full-float 1x1 target — no
//! precision is lost by not owning the texture — and [`filters::combine`] stops
//! at the add, handing HDR to whatever tone maps it.

pub(crate) mod filters;
pub(crate) mod half_storage;
pub(crate) mod prefilter;
pub(crate) mod reference;
pub(crate) mod schedule;

#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) mod chain;
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) mod wgsl;

#[cfg(all(test, feature = "offscreen"))]
mod parity;

/// The pyramid's four authored numbers.
///
/// `threshold`/`knee`/`strength` are `index.js`'s live settings, which *override*
/// the `Bloom` constructor's own `1.0`/`0.6` — see [`SOURCE_SETTINGS`] and
/// [`CONSTRUCTOR_DEFAULTS`]. `exposure` is the metered scalar the level-0
/// downsample scales every tap by before it thresholds, so that "brighter than
/// display white" means the same thing at every time of day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BloomTuning {
    /// `texture2D( tExposure, vec2( 0.5 ) ).r` — the metered exposure scalar.
    pub(crate) exposure: f32,
    /// `uParams.y`, in exposure-scaled linear light.
    pub(crate) threshold: f32,
    /// `uParams.z`. Floored at `1e-4` by [`prefilter::knee_floor`] before use.
    pub(crate) knee: f32,
    /// `uGrade.x` in `composite.js` — the gain on the finished pyramid.
    pub(crate) strength: f32,
}

/// `index.js`'s live settings: `bloomThreshold 1.6`, `bloomKnee 0.9`,
/// `bloomStrength 0.14`, with a unit exposure (the metered value at EV0).
///
/// These are the numbers the frame actually runs with.
pub(crate) const SOURCE_SETTINGS: BloomTuning = BloomTuning {
    exposure: 1.0,
    threshold: 1.6,
    knee: 0.9,
    strength: 0.14,
};

/// The `Bloom` constructor's own `this.threshold = 1.0; this.knee = 0.6;`.
///
/// Dead in the source — `index.js` overwrites both on the first settings sync,
/// every frame — but ported anyway, because dead computation in the source is
/// still part of the source and a reader comparing the two files will look for
/// it. Nothing in this crate reads it except the test that pins the override.
pub(crate) const CONSTRUCTOR_DEFAULTS: BloomTuning = BloomTuning {
    exposure: 1.0,
    threshold: 1.0,
    knee: 0.6,
    strength: 0.14,
};

#[cfg(test)]
mod tests {
    use super::{BloomTuning, CONSTRUCTOR_DEFAULTS, SOURCE_SETTINGS};

    /// The constructor's defaults are not the numbers the frame runs with, and
    /// the difference is not cosmetic: a threshold of `1.0` blooms every
    /// display-white surface in the scene, `1.6` blooms only what is genuinely
    /// over it. Pinning both is what stops a future reader "simplifying" the
    /// override away.
    #[test]
    fn the_live_settings_override_the_constructor_defaults() {
        assert_eq!(SOURCE_SETTINGS.threshold, 1.6);
        assert_eq!(SOURCE_SETTINGS.knee, 0.9);
        assert_eq!(SOURCE_SETTINGS.strength, 0.14);
        assert_eq!(CONSTRUCTOR_DEFAULTS.threshold, 1.0);
        assert_eq!(CONSTRUCTOR_DEFAULTS.knee, 0.6);
        assert_ne!(SOURCE_SETTINGS, CONSTRUCTOR_DEFAULTS);
        // Both meter at unit exposure until `exposure.js` says otherwise.
        assert_eq!(SOURCE_SETTINGS.exposure, CONSTRUCTOR_DEFAULTS.exposure);
    }

    /// The struct is a value: copying it cannot alias, so a caller tweaking one
    /// level's tuning cannot reach back into another's.
    #[test]
    fn tuning_is_a_value_not_a_handle() {
        let mut copy: BloomTuning = SOURCE_SETTINGS;
        copy.threshold = 0.0;
        assert_eq!(SOURCE_SETTINGS.threshold, 1.6);
        assert_ne!(copy, SOURCE_SETTINGS);
    }
}
