//! External integration tests for the recording-vs-live backend seam.
//!
//! These run from *outside* the module, so they may name only the single
//! facade (`WebGpuApi`) plus the host/kernel boundary types — exactly the
//! surface a future browser/native adapter app will have. The submission and
//! report types are intentionally not nameable here (the module exposes one
//! facade), so submissions are built inline and reports are inspected through
//! report accessor methods, just like the headless app does.

use axiom_host::{
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, Ratio};
use axiom_math::Mat4;
use axiom_webgpu::WebGpuApi;

/// Build a host presentation request. `presentable` controls whether the
/// adapter requires a presentation surface (and the device requires
/// presentation) — `false` yields a request unsuitable for a live presenting
/// backend, which the host itself still accepts.
fn host_request(presentable: bool) -> HostPresentationRequest {
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
        host.adapter_request(HostPowerPreference::HighPerformance, presentable),
        host.device_request(presentable, HostDeviceProfile::Baseline),
    )
    .unwrap()
}

/// Build the demo cube submission through `api`. The submission type is not
/// nameable outside the module, so it is built and returned by inference and
/// only ever fed straight back into `api.submit(...)`.
macro_rules! cube_submission {
    ($api:expr) => {{
        let mut sub = $api.new_submission(800, 600);
        $api.submission_clear_frame(&mut sub, [0.05, 0.06, 0.08, 1.0]);
        $api.submission_set_pipeline(&mut sub, 1);
        $api.submission_draw_indexed(&mut sub, 36, Mat4::IDENTITY);
        $api.submission_present(&mut sub);
        sub
    }};
}

#[test]
fn default_and_recording_backends_are_recording() {
    assert!(WebGpuApi::new().is_recording());
    assert!(WebGpuApi::new_recording().is_recording());
    assert!(!WebGpuApi::new_recording().is_live());
}

#[test]
fn recording_backend_is_deterministic() {
    let api = WebGpuApi::new_recording();
    let a = api.submit(cube_submission!(api));
    let b = api.submit(cube_submission!(api));
    assert_eq!(a, b);
    assert!(a.is_recorded());
    assert!(!a.presented());
}

#[test]
fn live_backend_constructed_from_host_request_reports_live() {
    let api = WebGpuApi::new_live(&host_request(true)).unwrap();
    assert!(api.is_live());
    assert!(api.has_presentation_request());
}

#[test]
fn live_backend_accepts_same_submission_shape_without_presenting() {
    let live = WebGpuApi::new_live(&host_request(true)).unwrap();
    let report = live.submit(cube_submission!(live));
    // The same submission contract is accepted and recorded ...
    assert_eq!(report.clear_count(), 1);
    assert_eq!(report.draw_count(), 1);
    assert_eq!(report.present_count(), 1);
    // ... but the live backend presents nothing this pass.
    assert!(!report.presented());
    assert!(report.is_live_not_initialized());
}

#[test]
fn recording_and_live_record_the_same_submission_shape() {
    let recording = WebGpuApi::new_recording();
    let live = WebGpuApi::new_live(&host_request(true)).unwrap();
    let r = recording.submit(cube_submission!(recording));
    let l = live.submit(cube_submission!(live));
    assert_eq!(r.clear_count(), l.clear_count());
    assert_eq!(r.draw_count(), l.draw_count());
    assert_eq!(r.present_count(), l.present_count());
    assert!(r.is_recorded());
    assert!(!l.presented());
}

#[test]
fn live_unbound_backend_reports_not_bound() {
    let api = WebGpuApi::new_live_unbound();
    assert!(api.is_live());
    assert!(!api.has_presentation_request());
    let report = api.submit(cube_submission!(api));
    assert!(report.is_live_not_bound());
    assert!(!report.presented());
}

#[test]
fn invalid_live_setup_fails_through_kernel_result() {
    let err = WebGpuApi::new_live(&host_request(false)).unwrap_err();
    assert_eq!(err.code(), axiom_kernel::KernelErrorCode::InvalidId);
}
