//! Pass 3 — the deterministic retro 32-bit-style visual descriptor.
//!
//! A small, fixed bag of style *intentions* the app declares for a
//! low-poly/retro 32-bit look. It is **descriptor-only**: it turns nothing on by itself
//! and implements no postprocessing pipeline. A renderer (or a future backend
//! adapter) reads it to decide internal resolution, pixel snapping, flat
//! shading, brightness quantization, and texture filtering. All constants are
//! fixed and documented — no PBR, no dynamic shadows, no browser APIs.

/// Texture filtering intent. `Nearest` is the retro 32-bit-style choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilter {
    Nearest,
    Linear,
}

/// The fixed low internal render target (16:9-ish, retro 32-bit-era scale).
pub const INTERNAL_WIDTH: u32 = 426;
pub const INTERNAL_HEIGHT: u32 = 240;

/// The deterministic retro 32-bit-style descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyVisualStyle {
    /// Internal (pre-upscale) render width in pixels.
    pub internal_width: u32,
    /// Internal (pre-upscale) render height in pixels.
    pub internal_height: u32,
    /// Snap vertices/pixels to the low-res grid (the retro 32-bit "wobble").
    pub pixel_snapping: bool,
    /// Flat (per-face) shading rather than smooth.
    pub flat_shading: bool,
    /// Quantize brightness into the light model's fixed bands.
    pub brightness_quantization: bool,
    /// Texture filtering intent.
    pub texture_filter: TextureFilter,
    /// Physically-based shading — deliberately off.
    pub physically_based: bool,
    /// Dynamic/real-time shadows — deliberately off (blob shadows are faked).
    pub dynamic_shadows: bool,
}

impl PenaltyVisualStyle {
    /// The fixed Stage 1 / Pass 3 style.
    pub const fn stage1() -> Self {
        Self {
            internal_width: INTERNAL_WIDTH,
            internal_height: INTERNAL_HEIGHT,
            pixel_snapping: true,
            flat_shading: true,
            brightness_quantization: true,
            texture_filter: TextureFilter::Nearest,
            physically_based: false,
            dynamic_shadows: false,
        }
    }
}
