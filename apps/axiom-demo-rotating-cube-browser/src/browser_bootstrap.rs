//! Browser startup helpers.
//!
//! The deterministic, browser-free half builds a `HostPresentationRequest`
//! from plain viewport dimensions — no browser objects, fully testable on
//! native. The wasm32-only half locates the `<canvas>` element by id.

use axiom_host::{
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostError, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, KernelError, KernelErrorCode, KernelErrorScope, KernelResult};
use axiom_math::MathApi;

/// Map a host-boundary validation failure into the kernel error model so the
/// browser app reports a single `KernelResult` failure type at startup.
fn host_to_kernel(_: HostError) -> KernelError {
    KernelError::new(
        KernelErrorScope::Id,
        KernelErrorCode::InvalidId,
        "invalid host presentation data for the browser surface",
    )
}

/// The canvas element id the browser app looks for at startup.
pub const CANVAS_ELEMENT_ID: &str = "axiom-cube-canvas";

/// Deterministic kernel `HandleId` raw value for the presentation target.
pub(crate) const TARGET_HANDLE_RAW: u64 = 1;
/// Deterministic kernel `HandleId` raw value for the surface handle.
pub(crate) const SURFACE_HANDLE_RAW: u64 = 2;
/// Deterministic presentation-target label.
pub(crate) const TARGET_LABEL: &str = "axiom-rotating-cube-canvas";

/// Build the deterministic `HostPresentationRequest` for a `width` x `height`
/// canvas. **No browser objects are touched** — this is pure host-owned data,
/// so it runs and is tested on native exactly as it will on wasm32.
pub(crate) fn build_presentation_request(
    width: u32,
    height: u32,
) -> KernelResult<HostPresentationRequest> {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let math = MathApi::new();

    let viewport = host
        .viewport(&math, width, height, 1.0)
        .map_err(host_to_kernel)?;
    let target = host
        .presentation_target(&kernel, TARGET_HANDLE_RAW, TARGET_LABEL)
        .map_err(host_to_kernel)?;
    let surface = host
        .surface_handle(&kernel, SURFACE_HANDLE_RAW)
        .map_err(host_to_kernel)?;
    let descriptor = host.surface_descriptor(
        viewport,
        HostPresentMode::Fifo,
        HostAlphaMode::Opaque,
        HostColorFormat::Bgra8UnormSrgb,
    );
    let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
    let device = host.device_request(true, HostDeviceProfile::Baseline);

    host.presentation_request(target, surface, descriptor, adapter, device)
        .map_err(host_to_kernel)
}

// --- wasm32-only canvas lookup ---

#[cfg(target_arch = "wasm32")]
pub(crate) fn find_canvas(
    canvas_id: &str,
) -> Result<web_sys::HtmlCanvasElement, wasm_bindgen::JsValue> {
    use wasm_bindgen::{JsCast, JsValue};

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let element = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas element not found by id"))?;
    element
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("element is not an HtmlCanvasElement"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_request_without_browser_objects() {
        let request = build_presentation_request(800, 600).expect("valid request");
        assert_eq!(request.target().label(), TARGET_LABEL);
        assert_eq!(request.target().id().raw(), TARGET_HANDLE_RAW);
        assert_eq!(request.surface().id().raw(), SURFACE_HANDLE_RAW);
        assert!(request.surface().is_valid());
        assert_eq!(request.descriptor().viewport().physical_width(), 800);
        assert_eq!(request.descriptor().viewport().physical_height(), 600);
    }

    #[test]
    fn request_is_deterministic() {
        let a = build_presentation_request(1280, 720).unwrap();
        let b = build_presentation_request(1280, 720).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn zero_dimension_is_rejected_through_host() {
        assert!(build_presentation_request(0, 600).is_err());
    }
}
