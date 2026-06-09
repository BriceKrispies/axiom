//! Manifest + isolation tests for the browser app.
//!
//! These are native, browser-free tests. The wasm/browser run path is proven at
//! compile time (`cargo build --target wasm32-unknown-unknown`) and by a real
//! browser screenshot; the deterministic scene is proven by the in-crate unit
//! tests (which drive `App::build` + `tick`).

use std::fs;
use std::path::{Path, PathBuf};

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

// 2. The app composes only the `axiom` umbrella (`engine`): no slice modules, no
//    layers, no app-to-app dependency.
#[test]
fn app_composes_only_the_engine_umbrella() {
    let app_toml = read(&manifest_dir().join("app.toml"));
    assert!(app_toml.contains("\"engine\""), "must allow the engine umbrella");
    for not_allowed in ["scene", "resources", "render-pipeline", "webgpu", "windowing"] {
        assert!(
            !app_toml.contains(&format!("\"{not_allowed}\"")),
            "the app composes the umbrella, not the slice module `{not_allowed}` directly"
        );
    }
    assert!(!app_toml.contains("rotating-cube-demo"), "no app-to-app dependency");
}

// 3. The Cargo manifest depends only on the `axiom` umbrella (plus the wasm32
//    entry crates). No direct engine-module or layer dependency.
#[test]
fn cargo_depends_only_on_the_umbrella() {
    let cargo = read(&manifest_dir().join("Cargo.toml"));
    assert!(cargo.contains("axiom = {"), "must depend on the axiom umbrella");
    for direct in ["axiom-scene", "axiom-render", "axiom-webgpu", "axiom-windowing", "axiom-host"] {
        let declares = cargo
            .lines()
            .map(str::trim)
            .any(|line| line.starts_with(&format!("{direct} =")));
        assert!(!declares, "must reach `{direct}` through the umbrella, not directly");
    }
}

// 4. Browser-only dependencies are confined to apps + the windowing module: no
//    other engine crate declares them.
#[test]
fn browser_dependencies_are_confined() {
    let browser_needles = ["wasm-bindgen", "web-sys", "js-sys", "wgpu", "console_error_panic_hook"];
    let engine_manifests = [
        "../../crates/axiom-kernel/Cargo.toml",
        "../../crates/axiom-host/Cargo.toml",
        "../../crates/axiom-frame/Cargo.toml",
        "../../modules/axiom-scene/Cargo.toml",
        "../../modules/axiom-resources/Cargo.toml",
        "../../modules/axiom-render/Cargo.toml",
        "../../modules/axiom-webgpu/Cargo.toml",
        "../../modules/axiom/Cargo.toml",
        "../axiom-demo-rotating-cube/Cargo.toml",
    ];
    for rel in engine_manifests {
        let text = read(&manifest_dir().join(rel));
        for dep in browser_needles {
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

// 5. The headless app still uses the recording backend (unchanged in purpose).
#[test]
fn headless_app_still_uses_recording_backend() {
    let demo = read(&manifest_dir().join("../axiom-demo-rotating-cube/src/demo_api.rs"));
    assert!(
        demo.contains("WebGpuApi::new_recording()"),
        "headless app must construct the recording backend"
    );
    assert!(!demo.contains("new_live"), "headless app must not switch to a live backend");
}
