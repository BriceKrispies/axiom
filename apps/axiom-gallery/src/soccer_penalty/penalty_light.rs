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

/// Fixed ambient strength (flat fill light).
pub const AMBIENT_STRENGTH: f32 = 0.35;
/// Fixed directional strength (the "sun" contribution).
pub const DIRECTIONAL_STRENGTH: f32 = 0.65;

/// The light direction, normalized. This is the unit form of the documented
/// raw direction `(-0.45, -1.0, -0.35)` (roughly from the upper-front-left),
/// precomputed as a constant so no runtime normalization (and no fallible math)
/// is needed. `|(-0.45,-1.0,-0.35)| = 1.151086`.
pub const LIGHT_DIRECTION: Vec3 = Vec3::new(-0.390932, -0.868799, -0.304059);

/// The fixed brightness bands, ascending. Quantization snaps a computed
/// brightness down to the largest band it meets or exceeds.
pub const BRIGHTNESS_BANDS: [f32; 4] = [0.35, 0.50, 0.70, 0.90];

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
