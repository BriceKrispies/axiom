//! The deterministic result artifact of evaluating a presentation request.

use crate::host_presentation_request::HostPresentationRequest;
use crate::host_presentation_status::HostPresentationStatus;
use crate::host_presentation_target::HostPresentationTarget;
use crate::host_surface_handle::HostSurfaceHandle;
use crate::host_viewport::HostViewport;

/// The deterministic outcome of evaluating a [`HostPresentationRequest`] at
/// the host boundary.
///
/// In this pass the host has no live backend, so evaluating any valid request
/// yields [`HostPresentationStatus::PendingBackend`]: the report records that
/// presentation was *structurally requested and validated*, echoes the target
/// / surface identity and the requested viewport for inspection, but makes no
/// claim that a real GPU, adapter, device, or surface exists.
///
/// The report is plain, equality-comparable data; identical requests produce
/// equal reports. It is built only through [`crate::HostApi`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostPresentationReport {
    status: HostPresentationStatus,
    target: HostPresentationTarget,
    surface: HostSurfaceHandle,
    viewport: HostViewport,
}

impl HostPresentationReport {
    /// Evaluate a validated request. Crate-private: callers go through
    /// [`crate::HostApi::evaluate_presentation`].
    ///
    /// Because no live backend exists this pass, the status is always
    /// [`HostPresentationStatus::PendingBackend`].
    pub(crate) fn from_request(request: &HostPresentationRequest) -> Self {
        HostPresentationReport {
            status: HostPresentationStatus::PendingBackend,
            target: request.target(),
            surface: request.surface(),
            viewport: *request.descriptor().viewport(),
        }
    }

    pub const fn status(&self) -> HostPresentationStatus {
        self.status
    }

    pub const fn target(&self) -> HostPresentationTarget {
        self.target
    }

    pub const fn surface(&self) -> HostSurfaceHandle {
        self.surface
    }

    pub const fn viewport(&self) -> &HostViewport {
        &self.viewport
    }

    /// Whether a live backend is bound and ready. Always `false` this pass.
    pub const fn is_ready(&self) -> bool {
        self.status.is_ready()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_adapter_request::HostAdapterRequest;
    use crate::host_alpha_mode::HostAlphaMode;
    use crate::host_color_format::HostColorFormat;
    use crate::host_device_profile::HostDeviceProfile;
    use crate::host_device_request::HostDeviceRequest;
    use crate::host_power_preference::HostPowerPreference;
    use crate::host_present_mode::HostPresentMode;
    use crate::host_presentation_target::HostPresentationTarget;
    use crate::host_surface_descriptor::HostSurfaceDescriptor;
    use crate::host_surface_handle::HostSurfaceHandle;
    use crate::host_viewport::HostViewport;
    use axiom_kernel::HandleId;
    use axiom_math::MathApi;

    fn request() -> HostPresentationRequest {
        let viewport = HostViewport::new(&MathApi::new(), 800, 600, 1.0).unwrap();
        let descriptor = HostSurfaceDescriptor::new(
            viewport,
            HostPresentMode::Fifo,
            HostAlphaMode::Opaque,
            HostColorFormat::Bgra8UnormSrgb,
        );
        HostPresentationRequest::new(
            HostPresentationTarget::new(HandleId::from_raw(1), "primary").unwrap(),
            HostSurfaceHandle::new(HandleId::from_raw(2)).unwrap(),
            descriptor,
            HostAdapterRequest::new(HostPowerPreference::HighPerformance, true),
            HostDeviceRequest::new(true, HostDeviceProfile::Baseline),
        )
        .unwrap()
    }

    #[test]
    fn report_is_pending_backend_and_echoes_request() {
        let report = HostPresentationReport::from_request(&request());
        assert_eq!(report.status(), HostPresentationStatus::PendingBackend);
        assert!(!report.is_ready());
        assert_eq!(report.target().id().raw(), 1);
        assert_eq!(report.surface().id().raw(), 2);
        assert_eq!(report.viewport().logical_width(), 800);
    }

    #[test]
    fn identical_requests_produce_equal_reports() {
        let a = HostPresentationReport::from_request(&request());
        let b = HostPresentationReport::from_request(&request());
        assert_eq!(a, b);
    }
}
