//! Backend-neutral **hemisphere ambient light** for a frame: a sky colour overhead
//! and a warm-dark ground colour below, blended by a surface normal's up-component,
//! lighting the faces that no directional light reaches. Carried as neutral frame
//! data — like [`crate::FrameVolumetrics`] — so every backend (Canvas 2D software
//! raster, WebGPU/WebGL via wgpu) lights unlit faces identically instead of each
//! hardcoding its own hemisphere. The colours are **strength-folded**: a backend
//! blends them directly (`mix(ground, sky, up)`), with no separate scale.

/// Hemisphere ambient: the linear-RGB sky (overhead) and ground (below) tints an
/// unlit face receives, blended by its normal's up-component. Strength is folded into
/// the colours, so a backend applies a plain `mix` with no extra scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameAmbient {
    sky: [f32; 3],
    ground: [f32; 3],
}

impl FrameAmbient {
    /// A hemisphere ambient from its strength-folded sky + ground linear-RGB tints.
    pub const fn new(sky: [f32; 3], ground: [f32; 3]) -> Self {
        FrameAmbient { sky, ground }
    }

    /// The engine's default hemisphere — a cool sky over a warm-dark ground, the exact
    /// values the backends historically hardcoded (`[0.55,0.65,0.85]` / `[0.30,0.26,0.22]`
    /// at `0.6` strength, folded in). A frame that carries no ambient renders identically.
    pub const fn default_hemisphere() -> Self {
        FrameAmbient::new([0.33, 0.39, 0.51], [0.18, 0.156, 0.132])
    }

    /// The overhead sky tint (strength-folded linear RGB).
    pub const fn sky(&self) -> [f32; 3] {
        self.sky
    }

    /// The below / ground tint (strength-folded linear RGB).
    pub const fn ground(&self) -> [f32; 3] {
        self.ground
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_default_and_equality() {
        let a = FrameAmbient::new([0.1, 0.2, 0.3], [0.4, 0.5, 0.6]);
        assert_eq!(a.sky(), [0.1, 0.2, 0.3]);
        assert_eq!(a.ground(), [0.4, 0.5, 0.6]);
        let d = FrameAmbient::default_hemisphere();
        assert_eq!(d.sky(), [0.33, 0.39, 0.51]);
        assert_eq!(d.ground(), [0.18, 0.156, 0.132]);
        assert_eq!(d, FrameAmbient::default_hemisphere());
        assert_ne!(d, a);
        assert!(format!("{a:?}").contains("FrameAmbient"));
    }
}
