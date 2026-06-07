//! The requested shape of a future live presentation surface.

use crate::host_alpha_mode::HostAlphaMode;
use crate::host_color_format::HostColorFormat;
use crate::host_present_mode::HostPresentMode;
use crate::host_viewport::HostViewport;

/// Describes the requested shape of a future live surface: its validated
/// viewport (logical/physical size + scale factor) plus the abstract present
/// mode, alpha mode, and colour format.
///
/// Dimension and scale-factor validity is **not duplicated** here — it is
/// carried by the embedded [`HostViewport`], which can only be constructed
/// through the math-validated [`HostViewport::new`] /
/// [`HostViewport::from_physical`]. A descriptor therefore cannot represent a
/// zero-extent surface or a non-finite scale factor; those failures occur
/// when the viewport is built.
///
/// The descriptor stores **no** WebGPU/OS/browser types — only abstract host
/// enums a future adapter maps onto the real backend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostSurfaceDescriptor {
    viewport: HostViewport,
    present_mode: HostPresentMode,
    alpha_mode: HostAlphaMode,
    color_format: HostColorFormat,
}

impl HostSurfaceDescriptor {
    /// Build a descriptor from an already-validated viewport and the abstract
    /// surface enums. Infallible: the viewport carries all dimension/scale
    /// validation.
    pub const fn new(
        viewport: HostViewport,
        present_mode: HostPresentMode,
        alpha_mode: HostAlphaMode,
        color_format: HostColorFormat,
    ) -> Self {
        HostSurfaceDescriptor {
            viewport,
            present_mode,
            alpha_mode,
            color_format,
        }
    }

    pub const fn viewport(&self) -> &HostViewport {
        &self.viewport
    }

    pub const fn present_mode(&self) -> HostPresentMode {
        self.present_mode
    }

    pub const fn alpha_mode(&self) -> HostAlphaMode {
        self.alpha_mode
    }

    pub const fn color_format(&self) -> HostColorFormat {
        self.color_format
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;
    use axiom_kernel::Ratio;

    fn viewport() -> HostViewport {
        HostViewport::new(800, 600, Ratio::new(1.0).unwrap()).unwrap()
    }

    #[test]
    fn descriptor_carries_its_fields() {
        let d = HostSurfaceDescriptor::new(
            viewport(),
            HostPresentMode::Fifo,
            HostAlphaMode::Opaque,
            HostColorFormat::Bgra8UnormSrgb,
        );
        assert_eq!(d.viewport().logical_width(), 800);
        assert_eq!(d.present_mode(), HostPresentMode::Fifo);
        assert_eq!(d.alpha_mode(), HostAlphaMode::Opaque);
        assert_eq!(d.color_format(), HostColorFormat::Bgra8UnormSrgb);
    }

    #[test]
    fn descriptor_dimension_validity_comes_from_the_viewport() {
        // A zero-width surface is unrepresentable: the viewport rejects it
        // before a descriptor can be built.
        let err = HostViewport::new(0, 600, Ratio::new(1.0).unwrap()).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidViewportDimensions);
    }

    #[test]
    fn same_inputs_produce_equal_descriptors() {
        let a = HostSurfaceDescriptor::new(
            viewport(),
            HostPresentMode::Mailbox,
            HostAlphaMode::PreMultiplied,
            HostColorFormat::Rgba8UnormSrgb,
        );
        let b = HostSurfaceDescriptor::new(
            viewport(),
            HostPresentMode::Mailbox,
            HostAlphaMode::PreMultiplied,
            HostColorFormat::Rgba8UnormSrgb,
        );
        assert_eq!(a, b);
    }
}
