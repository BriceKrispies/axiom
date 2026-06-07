//! Frame-stable viewport snapshot derived from `HostViewport`.

use axiom_host::HostViewport;
use axiom_kernel::Ratio;

/// A frame-stable snapshot of the host viewport.
///
/// Built once per engine frame from a [`HostViewport`]. The four integer
/// dimensions are copied verbatim; the scale factor and aspect ratio are
/// copied as kernel [`Ratio`] quantities.
///
/// Construction is **infallible**: a [`HostViewport`] already guarantees
/// non-zero physical dimensions and a finite positive scale factor, and the
/// scale factor and aspect ratio are carried as kernel [`Ratio`] values, which
/// are finite by construction. There is no error path to validate here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameViewport {
    logical_width: u32,
    logical_height: u32,
    physical_width: u32,
    physical_height: u32,
    scale_factor: Ratio,
    aspect_ratio: Ratio,
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

    pub const fn scale_factor(&self) -> Ratio {
        self.scale_factor
    }

    pub const fn aspect_ratio(&self) -> Ratio {
        self.aspect_ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_vp() -> HostViewport {
        HostViewport::new(1600, 900, Ratio::new(1.0).unwrap()).unwrap()
    }

    #[test]
    fn values_are_copied_from_host_viewport() {
        let v = FrameViewport::from_host(&host_vp());
        assert_eq!(v.logical_width(), 1600);
        assert_eq!(v.logical_height(), 900);
        assert_eq!(v.physical_width(), 1600);
        assert_eq!(v.physical_height(), 900);
        assert_eq!(v.scale_factor().get(), 1.0);
    }

    #[test]
    fn aspect_ratio_matches_host_viewport() {
        let v = FrameViewport::from_host(&host_vp());
        assert!((v.aspect_ratio().get() - host_vp().aspect_ratio().get()).abs() < 1.0e-6);
        assert!((v.aspect_ratio().get() - 16.0 / 9.0).abs() < 1.0e-6);
    }

    #[test]
    fn scale_factor_is_copied_verbatim_not_unity() {
        // A scale distinct from 1.0 (and 0.0/-1.0) pins the accessor against
        // the `-> Ratio with 1.0` mutation.
        let host = HostViewport::new(800, 600, Ratio::new(2.0).unwrap()).unwrap();
        let v = FrameViewport::from_host(&host);
        assert_eq!(v.scale_factor().get(), 2.0);
        assert_ne!(v.scale_factor().get(), 1.0);
    }

    #[test]
    fn identical_input_produces_equal_frame_viewport() {
        let a = FrameViewport::from_host(&host_vp());
        let b = FrameViewport::from_host(&host_vp());
        assert_eq!(a, b);
    }

    #[test]
    fn different_host_viewport_produces_different_frame_viewport() {
        let other = HostViewport::new(800, 600, Ratio::new(2.0).unwrap()).unwrap();
        let a = FrameViewport::from_host(&host_vp());
        let b = FrameViewport::from_host(&other);
        assert_ne!(a, b);
    }
}
