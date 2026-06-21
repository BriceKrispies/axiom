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
///
/// This is a **tagged struct**, not a data-carrying enum: a `kind` code names
/// the state and an `Option<HostPresentationRequest>` carries the only
/// per-state payload. The coarse kind and submission status are then derived
/// from `kind` by indexing a `const` lookup table — never by `match` — and the
/// bound request is `self.request.as_ref()`, a field access. Const
/// constructors stand in for the former enum variants, keeping their
/// invariant that only the live-requested state carries a request.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WebGpuBackendState {
    kind: u8,
    request: Option<HostPresentationRequest>,
}

impl WebGpuBackendState {
    const KIND_RECORDING: u8 = 0;
    const KIND_LIVE_UNBOUND: u8 = 1;
    const KIND_LIVE_PRESENTATION_REQUESTED: u8 = 2;

    /// `BackendKind` for each `kind`, indexed by `self.kind`. Replaces the
    /// per-state `match` with a single table read.
    const BACKEND_KINDS: [BackendKind; 3] = [
        BackendKind::Recording, // KIND_RECORDING
        BackendKind::Live,      // KIND_LIVE_UNBOUND
        BackendKind::Live,      // KIND_LIVE_PRESENTATION_REQUESTED
    ];

    /// `GpuSubmissionStatus` for each `kind`, indexed by `self.kind`.
    const SUBMISSION_STATUSES: [GpuSubmissionStatus; 3] = [
        GpuSubmissionStatus::Recorded,           // KIND_RECORDING
        GpuSubmissionStatus::LiveNotBound,       // KIND_LIVE_UNBOUND
        GpuSubmissionStatus::LiveNotInitialized, // KIND_LIVE_PRESENTATION_REQUESTED
    ];

    /// Deterministic recorder. The default.
    pub const fn recording() -> Self {
        WebGpuBackendState {
            kind: Self::KIND_RECORDING,
            request: None,
        }
    }

    /// Live backend with no presentation target/surface bound yet.
    pub const fn live_unbound() -> Self {
        WebGpuBackendState {
            kind: Self::KIND_LIVE_UNBOUND,
            request: None,
        }
    }

    /// Live backend bound to a validated host presentation request. Carries
    /// the request so a future live pass can build a real surface/device
    /// from host-owned data without re-plumbing it.
    pub const fn live_presentation_requested(request: HostPresentationRequest) -> Self {
        WebGpuBackendState {
            kind: Self::KIND_LIVE_PRESENTATION_REQUESTED,
            request: Some(request),
        }
    }

    /// The coarse [`BackendKind`] this state belongs to.
    pub const fn kind(&self) -> BackendKind {
        Self::BACKEND_KINDS[self.kind as usize]
    }

    /// The deterministic status a submission gets in this state.
    pub const fn submission_status(&self) -> GpuSubmissionStatus {
        Self::SUBMISSION_STATUSES[self.kind as usize]
    }

    /// The bound presentation request, if any.
    pub const fn presentation_request(&self) -> Option<&HostPresentationRequest> {
        self.request.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{
        HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
        HostPresentMode,
    };
    use axiom_kernel::{KernelApi, Ratio};

    fn request() -> HostPresentationRequest {
        let host = HostApi::new();
        let kernel = KernelApi::new();
        let viewport = host.viewport(800, 600, Ratio::new(1.0).unwrap()).unwrap();
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
        let s = WebGpuBackendState::recording();
        assert_eq!(s.kind(), BackendKind::Recording);
        assert_eq!(s.submission_status(), GpuSubmissionStatus::Recorded);
        assert!(s.presentation_request().is_none());
    }

    #[test]
    fn default_state_is_recording() {
        let s = WebGpuBackendState::default();
        assert_eq!(s.kind(), BackendKind::Recording);
        assert_eq!(s.submission_status(), GpuSubmissionStatus::Recorded);
        assert!(s.presentation_request().is_none());
    }

    #[test]
    fn live_unbound_maps_to_live_kind_and_not_bound_status() {
        let s = WebGpuBackendState::live_unbound();
        assert_eq!(s.kind(), BackendKind::Live);
        assert_eq!(s.submission_status(), GpuSubmissionStatus::LiveNotBound);
        assert!(s.presentation_request().is_none());
    }

    #[test]
    fn live_requested_maps_to_live_kind_and_not_initialized_status() {
        let s = WebGpuBackendState::live_presentation_requested(request());
        assert_eq!(s.kind(), BackendKind::Live);
        assert_eq!(
            s.submission_status(),
            GpuSubmissionStatus::LiveNotInitialized
        );
        assert!(s.presentation_request().is_some());
    }
}
