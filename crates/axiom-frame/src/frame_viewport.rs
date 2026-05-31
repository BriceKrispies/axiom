//! Frame-stable viewport snapshot derived from `HostViewport`.

use axiom_host::HostViewport;

/// A frame-stable snapshot of the host viewport.
///
/// Built once per engine frame from a [`HostViewport`]. The four integer
/// dimensions and the scale factor are copied verbatim; the aspect ratio is
/// computed from the physical size.
///
/// Construction is **infallible**: a [`HostViewport`] already guarantees
/// non-zero physical dimensions and a finite positive scale factor, so the
/// derived aspect ratio is always a finite positive `f32`. There is no error
/// path to validate here — a guard against a non-finite aspect would be
/// unreachable dead code. (Frame still adapts Layer-02 math elsewhere, e.g.
/// [`crate::FrameContext::viewport_aspect_is_finite`].)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameViewport {
    logical_width: u32,
    logical_height: u32,
    physical_width: u32,
    physical_height: u32,
    scale_factor: f32,
    aspect_ratio: f32,
}

impl FrameViewport {
    /// Project a validated [`HostViewport`] into a frame viewport.
    pub fn from_host(viewport: &HostViewport) -> Self {
        FrameViewport {
            logical_width: viewport.logical_width(),
            logical_height: viewport.logical_height(),
            physical_width: viewport.physical_width(),
            physical_height: viewport.physical_height(),
            scale_factor: viewport.scale_factor(),
            aspect_ratio: viewport.aspect_ratio(),
        }
    }

    pub const fn logical_width(&self) -> u32 {
        self.logical_width
    }

    pub const fn logical_height(&self) -> u32 {
        self.logical_height
    }

    pub const fn physical_width(&self) -> u32 {
        self.physical_width
    }

    pub const fn physical_height(&self) -> u32 {
        self.physical_height
    }

    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    pub const fn aspect_ratio(&self) -> f32 {
        self.aspect_ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::MathApi;

    fn math() -> MathApi {
        MathApi::new()
    }

    fn host_vp() -> HostViewport {
        HostViewport::new(&math(), 1600, 900, 1.0).unwrap()
    }

    #[test]
    fn values_are_copied_from_host_viewport() {
        let v = FrameViewport::from_host(&host_vp());
        assert_eq!(v.logical_width(), 1600);
        assert_eq!(v.logical_height(), 900);
        assert_eq!(v.physical_width(), 1600);
        assert_eq!(v.physical_height(), 900);
        assert_eq!(v.scale_factor(), 1.0);
    }

    #[test]
    fn aspect_ratio_matches_host_viewport() {
        let v = FrameViewport::from_host(&host_vp());
        assert!((v.aspect_ratio() - host_vp().aspect_ratio()).abs() < 1.0e-6);
        assert!((v.aspect_ratio() - 16.0 / 9.0).abs() < 1.0e-6);
    }

    #[test]
    fn derived_aspect_is_finite() {
        // The host viewport guarantees a finite aspect; confirm the projected
        // value is what math considers finite (frame's math adapter lives in
        // FrameContext, but this pins the invariant the projection relies on).
        let v = FrameViewport::from_host(&host_vp());
        assert!(math().is_finite_value(v.aspect_ratio()));
    }

    #[test]
    fn scale_factor_is_copied_verbatim_not_unity() {
        // A scale distinct from 1.0 (and 0.0/-1.0) pins the accessor against
        // the `-> f32 with 1.0` mutation.
        let host = HostViewport::new(&math(), 800, 600, 2.0).unwrap();
        let v = FrameViewport::from_host(&host);
        assert_eq!(v.scale_factor(), 2.0);
        assert_ne!(v.scale_factor(), 1.0);
    }

    #[test]
    fn identical_input_produces_equal_frame_viewport() {
        let a = FrameViewport::from_host(&host_vp());
        let b = FrameViewport::from_host(&host_vp());
        assert_eq!(a, b);
    }

    #[test]
    fn different_host_viewport_produces_different_frame_viewport() {
        let other = HostViewport::new(&math(), 800, 600, 2.0).unwrap();
        let a = FrameViewport::from_host(&host_vp());
        let b = FrameViewport::from_host(&other);
        assert_ne!(a, b);
    }
}
