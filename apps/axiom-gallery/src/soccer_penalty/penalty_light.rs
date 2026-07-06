//! Pass 3 — the deterministic app-local light model.
//!
//! A tiny, fixed flat-shading model: one directional light + one ambient term,
//! with brightness quantized into a handful of fixed bands for the retro 32-bit-style
//! look. This is **not** a general lighting engine, not PBR, and not a shadow
//! renderer — it is a pure function from a face normal to a quantized
//! brightness, defined entirely by compile-time constants.
//!
//! ```text
//! face_brightness = ambient + max(dot(normal, -light_dir), 0) * directional
//! shade(base)     = base.rgb * quantize(face_brightness)   // alpha preserved
//! ```
//!
//! No wall-clock time, no randomness, no browser APIs, no light movement.

use axiom_math::Vec3;

use crate::soccer_penalty::low_poly_assets::Rgba;

/// Fixed ambient strength (flat fill light). Lifted again (0.72 -> 0.80) so the
/// shadowed/side faces sit even higher: the reference is a high-fill overcast-ish
/// sunlit stadium where the sky bounce nearly fills the shade, giving the figures
/// an almost flat, evenly-lit read with only a whisper of a terminator — not the
/// darker, contrastier facet-stepping a low fill produces.
pub const AMBIENT_STRENGTH: f32 = 0.80;
/// Fixed directional strength (the "sun" contribution). Softened (0.55 -> 0.45)
/// in step with the raised fill: a fully key-lit face (ambient + directional =
/// 1.25) still overshoots the top band and reads at full daylight, but the gap
/// between a key-lit face and a fill-only face narrows, flattening the terminator
/// to match the reference's soft, even key instead of a hard sunlit contrast.
pub const DIRECTIONAL_STRENGTH: f32 = 0.45;

/// The light direction, normalized. A low, raking stadium key from the
/// upper-behind-left: its shallower elevation lets the sun model the vertical
/// figure faces (jerseys, keeper, player fronts) instead of dumping straight down
/// onto the pitch. Already unit length (`|(-0.50,-0.66,-0.56)| = 0.99960`),
/// precomputed so no runtime normalization (and no fallible math) is needed.
pub const LIGHT_DIRECTION: Vec3 = Vec3::new(-0.50, -0.66, -0.56);

/// The fixed brightness bands, ascending. Quantization snaps a computed
/// brightness down to the largest band it meets or exceeds. The floor tracks the
/// ambient level (0.72 -> 0.80) and the intermediate bands are pulled up with it
/// (was `[0.72, 0.82, 0.91, 1.0]`), compressing the range into the bright upper
/// end so fill-only faces read as full daylight and the terminator steps stay
/// gentle — the reference's even, high-fill look — while the top band still
/// reaches full `1.0` for a key-lit face.
pub const BRIGHTNESS_BANDS: [f32; 4] = [0.80, 0.87, 0.94, 1.0];

/// The deterministic flat-shading light model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyLightModel {
    pub ambient_strength: f32,
    pub directional_strength: f32,
    /// Normalized direction the light travels.
    pub direction: Vec3,
    /// Ascending brightness bands used by [`Self::quantize`].
    pub bands: [f32; 4],
}

impl PenaltyLightModel {
    /// The fixed Stage 1 / Pass 3 light model.
    pub const fn stage1() -> Self {
        Self {
            ambient_strength: AMBIENT_STRENGTH,
            directional_strength: DIRECTIONAL_STRENGTH,
            direction: LIGHT_DIRECTION,
            bands: BRIGHTNESS_BANDS,
        }
    }

    /// Continuous flat-shaded brightness for a (unit) face normal:
    /// `ambient + max(dot(normal, -direction), 0) * directional`.
    pub fn face_brightness(&self, normal: Vec3) -> f32 {
        let toward_light = self.direction.mul_scalar(-1.0);
        let ndotl = normal.dot(toward_light).max(0.0);
        self.ambient_strength + ndotl * self.directional_strength
    }

    /// Snap a brightness to the largest band it meets or exceeds (floored at the
    /// first band). Deterministic and total.
    pub fn quantize(&self, brightness: f32) -> f32 {
        self.bands
            .iter()
            .rev()
            .copied()
            .find(|&band| brightness >= band)
            .unwrap_or(self.bands[0])
    }

    /// Flat-shade a base color for a face normal: multiply RGB by the quantized
    /// brightness, preserve alpha.
    pub fn shade(&self, base: Rgba, normal: Vec3) -> Rgba {
        let b = self.quantize(self.face_brightness(normal));
        Rgba::new(base.r * b, base.g * b, base.b * b, base.a)
    }
}
