//! Browser startup helpers (wasm32 only).
//!
//! The deterministic, browser-free presentation-request assembly now lives in
//! `axiom_windowing::WindowingApi`. What remains here is the wasm32-only
//! `<canvas>` lookup by element id.

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
