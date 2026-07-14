//! The `wasm32` browser entry: mount the module's overlay and hand it to the
//! module's own measured-diagnostics driver.
//!
//! This is the harness's whole job and its nondeterministic edge. It owns no
//! overlay logic and no measurement code — both live behind the module facade.
//! `harness_start()` takes no arguments, so any page that loads this wasm
//! drives it the same way.

use axiom_debug_overlay::DebugOverlayApi;
use wasm_bindgen::prelude::*;

/// Browser entry: mount the overlay (hidden until `` ` `` is pressed) and let
/// the module's measured-diagnostics loop own it for the page's lifetime.
#[wasm_bindgen]
pub fn harness_start() {
    console_error_panic_hook::set_once();
    DebugOverlayApi::new().mount_with_measured_diagnostics("dev-harness");
}
