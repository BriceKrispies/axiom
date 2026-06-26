//! The Canvas-only **depth-cue** presentation policy.
//!
//! [`CanvasDepthCueProfile`] configures the cheap, deterministic depth cues the
//! `LowPolyFramebuffer` profile layers on top of the flat-shaded z-buffer image
//! to make it read as 3D space: depth fog, fake per-triangle directional
//! lighting, height/elevation tint, distance colour falloff, contact-shadow
//! blobs and silhouette outlines for important objects, an optional far-horizon
//! silhouette, and a subtle camera-relative vertical colour grade.
//!
//! These are **presentation** knobs, owned entirely by the Canvas backend. They
//! never become game logic, never import scene/game/resource modules, and are
//! derived only from the neutral [`axiom_host::FramePacket`] plus this profile.
//! The default ([`CanvasDepthCueProfile::low_poly_framebuffer`]) is deliberately
//! **subtle** — depth readability, not a stylised filter.
//!
//! The two richest cues (fog, lighting) are grouped into [`FogCue`] /
//! [`LightingCue`] sub-structs; the rest are flat. Fields are `pub(crate)` — a
//! plain config DTO read across the backend (the cue math, the post-passes, the
//! facade), never widening the one public facade. Everything is `Copy`.

/// Depth fog / atmospheric perspective configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FogCue {
    pub(crate) enabled: bool,
    pub(crate) near: f32,
    pub(crate) far: f32,
    pub(crate) strength: f32,
    /// Fog target colour (the facade overrides this with the frame clear colour).
    pub(crate) color: [f32; 4],
}

/// Fake per-triangle directional lighting configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LightingCue {
    pub(crate) enabled: bool,
    pub(crate) direction: [f32; 3],
    pub(crate) ambient: f32,
    pub(crate) diffuse: f32,
    pub(crate) banded: bool,
    pub(crate) band_count: u32,
}

/// Configuration for every Canvas depth cue. `Copy` — pure scalars and small
/// arrays. Two profiles are equal iff every field is equal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CanvasDepthCueProfile {
    pub(crate) fog: FogCue,
    pub(crate) lighting: LightingCue,
    // Height / elevation tint.
    pub(crate) enable_height_tint: bool,
    pub(crate) height_tint_strength: f32,
    pub(crate) low_height_color: [f32; 4],
    pub(crate) high_height_color: [f32; 4],
    // Planar projected contact shadows (marked dynamic objects only): each
    // caster's geometry is projected along the directional light onto the ground
    // plane and rasterized depth-tested, so walls occlude the shadow and it never
    // paints onto a wall face. `depth_bias` (NDC) lets the floor-coplanar shadow
    // win the depth test against the floor it lands on.
    pub(crate) enable_contact_shadows: bool,
    pub(crate) contact_shadow_alpha: f32,
    pub(crate) contact_shadow_depth_bias: f32,
    // Depth-weighted silhouette outlines (important objects only).
    pub(crate) enable_depth_outlines: bool,
    pub(crate) near_outline_alpha: f32,
    pub(crate) far_outline_alpha: f32,
    // Distance detail / colour falloff.
    pub(crate) enable_distance_detail_falloff: bool,
    pub(crate) detail_falloff_near: f32,
    pub(crate) detail_falloff_far: f32,
    // Far horizon / far-terrain silhouette.
    pub(crate) enable_horizon_silhouette: bool,
    pub(crate) horizon_alpha: f32,
    // Camera-relative vertical colour grade.
    pub(crate) enable_vertical_grade: bool,
    pub(crate) vertical_grade_strength: f32,
}

impl CanvasDepthCueProfile {
    /// The shipping **subtle** depth-cue set for the `LowPolyFramebuffer`
    /// profile: modest fog, gentle lighting, light height tint, contact shadows
    /// + outlines for gameplay objects, distance falloff, a faint vertical
    ///   grade. The horizon silhouette ships **off** — see the renderer's
    ///   `ARCHITECTURE.md` for the neutral far-terrain band data it would need.
    pub(crate) fn low_poly_framebuffer() -> Self {
        CanvasDepthCueProfile {
            fog: FogCue {
                enabled: true,
                // NDC z is non-linear (most visible depth clusters high), so fog
                // starts late and stays gentle — only the far horizon recedes;
                // near/mid terrain keeps its colour.
                near: 0.85,
                far: 1.0,
                strength: 0.35,
                // Overridden per frame by the facade to the clear colour.
                color: [0.55, 0.65, 0.8, 1.0],
            },
            lighting: LightingCue {
                enabled: true,
                direction: [-0.4, 0.85, 0.35],
                ambient: 0.6,
                diffuse: 0.5,
                banded: false,
                band_count: 4,
            },

            enable_height_tint: true,
            height_tint_strength: 0.12,
            low_height_color: [0.24, 0.2, 0.16, 1.0],
            high_height_color: [0.86, 0.89, 0.96, 1.0],

            enable_contact_shadows: true,
            contact_shadow_alpha: 0.32,
            contact_shadow_depth_bias: 0.002,

            enable_depth_outlines: true,
            near_outline_alpha: 0.5,
            far_outline_alpha: 0.0,

            enable_distance_detail_falloff: true,
            detail_falloff_near: 0.7,
            detail_falloff_far: 1.0,

            enable_horizon_silhouette: false,
            horizon_alpha: 0.3,

            enable_vertical_grade: true,
            vertical_grade_strength: 0.12,
        }
    }

    /// A stable name for the cue profile, surfaced in telemetry
    /// (`depth_cue_profile_name`).
    pub(crate) fn name(&self) -> &'static str {
        "low-poly-framebuffer"
    }
}

impl Default for CanvasDepthCueProfile {
    fn default() -> Self {
        CanvasDepthCueProfile::low_poly_framebuffer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_subtle_and_enables_the_shipping_cues() {
        let p = CanvasDepthCueProfile::low_poly_framebuffer();
        assert!(p.fog.enabled);
        assert!(p.lighting.enabled);
        assert!(p.enable_height_tint);
        assert!(p.enable_contact_shadows);
        assert!(p.enable_depth_outlines);
        assert!(p.enable_distance_detail_falloff);
        assert!(p.enable_vertical_grade);
        // Horizon silhouette is off (clean far-terrain data is not available).
        assert!(!p.enable_horizon_silhouette);
        // Subtle: tint/grade strengths are small.
        assert!(p.height_tint_strength < 0.25);
        assert!(p.vertical_grade_strength < 0.25);
        assert_eq!(p.name(), "low-poly-framebuffer");
        assert_eq!(p, CanvasDepthCueProfile::default());
        assert!(format!("{p:?}").contains("CanvasDepthCueProfile"));
    }

    #[test]
    fn profiles_compare_by_value() {
        let a = CanvasDepthCueProfile::low_poly_framebuffer();
        let mut b = a;
        assert_eq!(a, b);
        b.fog.strength = 0.99;
        assert_ne!(a, b);
    }
}
