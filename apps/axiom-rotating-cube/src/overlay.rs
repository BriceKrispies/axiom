//! The backquote debug overlay, mounted from this app's own bundle (the
//! gallery's shared-shell overlay mount, now per-app).

use wasm_bindgen::prelude::*;

/// Mount the debug overlay (hidden until `` ` ``) and drive it with measured
/// diagnostics for the page's lifetime.
#[wasm_bindgen]
pub fn overlay_start() {
    console_error_panic_hook::set_once();
    axiom_debug_overlay::DebugOverlayApi::new()
        .mount_with_measured_diagnostics("axiom-rotating-cube");
}
