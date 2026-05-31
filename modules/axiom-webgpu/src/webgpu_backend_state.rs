//! The tiny, deterministic backend state machine behind `WebGpuApi`.

use axiom_host::HostPresentationRequest;

use crate::backend_kind::BackendKind;
use crate::gpu_submission_status::GpuSubmissionStatus;

/// The module-internal state a [`crate::WebGpuApi`] carries.
///
/// Intentionally tiny — three states, no adapter/device/swapchain
/// management. It models the *seam* between recording and live presentation,
/// not a renderer backend. A future live pass adds a `LiveReady` state only
/// once a real surface/device binding actually exists.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WebGpuBackendState {
    /// Deterministic recorder. The default.
    #[default]
    Recording,
    /// Live backend with no presentation target/surface bound yet.
    LiveUnbound,
    /// Live backend bound to a validated host presentation request. Carries
    /// the request so a future live pass can build a real surface/device
    /// from host-owned data without re-plumbing it.
    LivePresentationRequested(HostPresentationRequest),
}

impl WebGpuBackendState {
    /// The coarse [`BackendKind`] this state belongs to.
    pub const fn kind(&self) -> BackendKind {
        match self {
            WebGpuBackendState::Recording => BackendKind::Recording,
            WebGpuBackendState::LiveUnbound
            | WebGpuBackendState::LivePresentationRequested(_) => BackendKind::Live,
        }
    }

    /// The deterministic status a submission gets in this state.
    pub const fn submission_status(&self) -> GpuSubmissionStatus {
        match self {
            WebGpuBackendState::Recording => GpuSubmissionStatus::Recorded,
            WebGpuBackendState::LiveUnbound => GpuSubmissionStatus::LiveNotBound,
            WebGpuBackendState::LivePresentationRequested(_) => {
                GpuSubmissionStatus::LiveNotInitialized
            }
        }
    }

    /// The bound presentation request, if any.
    pub const fn presentation_request(&self) -> Option<&HostPresentationRequest> {
        match self {
            WebGpuBackendState::LivePresentationRequested(request) => Some(request),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{
        HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
        HostPresentMode,
    };
    use axiom_kernel::KernelApi;
    use axiom_math::MathApi;

    fn request() -> HostPresentationRequest {
        let host = HostApi::new();
        let kernel = KernelApi::new();
        let viewport = host.viewport(&MathApi::new(), 800, 600, 1.0).unwrap();
        let descriptor = host.surface_descriptor(
            viewport,
            HostPresentMode::Fifo,
            HostAlphaMode::Opaque,
            HostColorFormat::Bgra8UnormSrgb,
        );
        host.presentation_request(
            host.presentation_target(&kernel, 1, "primary").unwrap(),
            host.surface_handle(&kernel, 2).unwrap(),
            descriptor,
            host.adapter_request(HostPowerPreference::HighPerformance, true),
            host.device_request(true, HostDeviceProfile::Baseline),
        )
        .unwrap()
    }

    #[test]
    fn recording_state_maps_to_recording_kind_and_status() {
        let s = WebGpuBackendState::Recording;
        assert_eq!(s.kind(), BackendKind::Recording);
        assert_eq!(s.submission_status(), GpuSubmissionStatus::Recorded);
        assert!(s.presentation_request().is_none());
    }

    #[test]
    fn live_unbound_maps_to_live_kind_and_not_bound_status() {
        let s = WebGpuBackendState::LiveUnbound;
        assert_eq!(s.kind(), BackendKind::Live);
        assert_eq!(s.submission_status(), GpuSubmissionStatus::LiveNotBound);
        assert!(s.presentation_request().is_none());
    }

    #[test]
    fn live_requested_maps_to_live_kind_and_not_initialized_status() {
        let s = WebGpuBackendState::LivePresentationRequested(request());
        assert_eq!(s.kind(), BackendKind::Live);
        assert_eq!(s.submission_status(), GpuSubmissionStatus::LiveNotInitialized);
        assert!(s.presentation_request().is_some());
    }
}
