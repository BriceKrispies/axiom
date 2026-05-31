//! Manifest, isolation, and deterministic-bootstrap tests for the browser app.
//!
//! These are native, browser-free tests (no wasm runtime). The wasm/browser
//! runtime path is proven only at compile time (`cargo build --target
//! wasm32-unknown-unknown`); state-machine and orchestration determinism are
//! proven by the in-crate unit tests.

use std::fs;
use std::path::{Path, PathBuf};

use axiom_demo_rotating_cube_browser::BrowserRotatingCubeApi;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|_| panic!("missing file: {}", path.display()))
}

// 1. app.toml is valid and classifies this crate as an app.
#[test]
fn app_manifest_is_valid_and_classifies_as_an_app() {
    let app_toml = read(&manifest_dir().join("app.toml"));
    assert!(app_toml.contains("[app]"), "must declare [app]");
    assert!(app_toml.contains("crate_name = \"axiom-demo-rotating-cube-browser\""));
    assert!(!app_toml.contains("[layer]"), "must not be a layer");
    assert!(!app_toml.contains("[module]"), "must not be a module");
}

// 2. app.toml declares only the allowed layers and modules.
#[test]
fn app_declares_only_allowed_layers_and_modules() {
    let app_toml = read(&manifest_dir().join("app.toml"));
    for layer in ["kernel", "runtime", "math", "host", "frame"] {
        assert!(app_toml.contains(&format!("\"{layer}\"")), "missing layer {layer}");
    }
    for module in ["scene", "resources", "render", "webgpu"] {
        assert!(app_toml.contains(&format!("\"{module}\"")), "missing module {module}");
    }
    // No app-to-app dependency on the headless app.
    assert!(!app_toml.contains("rotating-cube-demo"));
}

// 4. lib.rs exposes exactly BrowserRotatingCubeApi.
#[test]
fn lib_rs_exposes_exactly_browser_rotating_cube_api() {
    let lib = read(&manifest_dir().join("src/lib.rs"));
    let public: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("pub ") && !l.starts_with("pub(crate)"))
        .collect();
    assert_eq!(public, vec!["pub use browser_api::BrowserRotatingCubeApi;"]);
}

// 5. Browser-only dependencies are confined to this app crate.
#[test]
fn browser_dependencies_are_confined_to_this_app() {
    let browser_needles = ["wasm-bindgen", "web-sys", "js-sys", "wgpu", "console_error_panic_hook"];
    let engine_manifests = [
        "../../crates/axiom-kernel/Cargo.toml",
        "../../crates/axiom-runtime/Cargo.toml",
        "../../crates/axiom-math/Cargo.toml",
        "../../crates/axiom-host/Cargo.toml",
        "../../crates/axiom-frame/Cargo.toml",
        "../../modules/axiom-scene/Cargo.toml",
        "../../modules/axiom-resources/Cargo.toml",
        "../../modules/axiom-render/Cargo.toml",
        "../../modules/axiom-webgpu/Cargo.toml",
        "../axiom-demo-rotating-cube/Cargo.toml",
    ];
    for rel in engine_manifests {
        let text = read(&manifest_dir().join(rel));
        for dep in browser_needles {
            // Match an actual dependency-key line (`dep = ...`), not a crate
            // name or prose mention (e.g. "axiom-webgpu" contains "wgpu").
            let has_dep = text.lines().map(str::trim).any(|line| {
                line.starts_with(&format!("{dep} =")) || line.starts_with(&format!("{dep}="))
            });
            assert!(
                !has_dep,
                "engine crate {rel} must not declare a dependency on browser crate `{dep}`"
            );
        }
    }
}

// 6. The headless app still uses the recording backend (unchanged in purpose).
#[test]
fn headless_app_still_uses_recording_backend() {
    let demo = read(&manifest_dir().join("../axiom-demo-rotating-cube/src/demo_api.rs"));
    assert!(
        demo.contains("WebGpuApi::new_recording()"),
        "headless app must construct the recording backend"
    );
    assert!(
        !demo.contains("new_live"),
        "headless app must not switch to a live backend"
    );
}

// 8. The app builds a HostPresentationRequest from deterministic
//    viewport/target/surface data with no browser objects (this test runs on
//    native — there is no browser).
#[test]
fn builds_host_presentation_request_without_browser_objects() {
    let app = BrowserRotatingCubeApi::new("axiom-cube-canvas", 1280, 720)
        .expect("deterministic request + live backend");
    assert_eq!(app.canvas_id(), "axiom-cube-canvas");
    assert!(app.is_live());
    assert!(app.has_presentation_request());
    assert_eq!(app.viewport_width(), 1280);
    assert_eq!(app.viewport_height(), 720);
    assert_eq!(app.presentation_target_label(), "axiom-rotating-cube-canvas");
    assert!(app.surface_handle_id() > 0);
    // The request is reachable as host-owned data (no browser objects).
    assert_eq!(app.presentation_request().descriptor().viewport().physical_width(), 1280);
}

// 8b. Construction is deterministic.
#[test]
fn construction_is_deterministic() {
    let a = BrowserRotatingCubeApi::new("axiom-cube-canvas", 800, 600).unwrap();
    let b = BrowserRotatingCubeApi::new("axiom-cube-canvas", 800, 600).unwrap();
    assert_eq!(a.presentation_request(), b.presentation_request());
}
