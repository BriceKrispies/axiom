//! Mechanical architecture enforcement for Axiom Layer 03 (axiom-host).

use std::fs;
use std::path::{Path, PathBuf};

fn host_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn sibling_src_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(name)
        .join("src")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("src directory must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn host_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&host_src_dir(), &mut files);
    assert!(!files.is_empty(), "expected host source files");
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("source must be valid UTF-8")
}

/// Strip `//` line comments and string-literal contents so a forbidden token
/// appearing only inside documentation or a string literal cannot trip the
/// scan.
fn strip_comments_and_strings(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut in_char = false;
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                chars.next();
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        if in_char {
            if c == '\\' {
                chars.next();
                continue;
            }
            if c == '\'' {
                in_char = false;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            continue;
        }
        if c == '\'' {
            in_char = true;
            continue;
        }
        out.push(c);
    }
    out
}

fn assert_absent_in(dir: &Path, label: &str, forbidden: &[&str], why: &str) {
    let mut files = Vec::new();
    collect_rs(dir, &mut files);
    files.sort();
    let mut violations = Vec::new();
    for path in &files {
        let stripped = strip_comments_and_strings(&read(path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "{label} {}: contains forbidden `{needle}`",
                    path.display()
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

fn assert_absent(forbidden: &[&str], why: &str) {
    assert_absent_in(&host_src_dir(), "axiom-host", forbidden, why);
}

#[test]
fn no_browser_or_js_bindgen_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "axiom-host must not reference browser / JS bindings",
    );
}

#[test]
fn no_dom_canvas_or_browser_globals() {
    assert_absent(
        &[
            "HtmlCanvas",
            "HtmlElement",
            "OffscreenCanvas",
            "document.",
            "window.",
            "navigator.",
        ],
        "axiom-host must not reference DOM/canvas/browser globals",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "webgpu", "WebGpu", "WebGL", "webgl", "GPUDevice"],
        "axiom-host must not reference WebGPU/WebGL",
    );
}

#[test]
fn no_animation_frame_or_performance_now() {
    assert_absent(
        &["requestAnimationFrame", "performance.now"],
        "axiom-host must not call browser frame/clock APIs",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant::now", "chrono"],
        "axiom-host must not read wall-clock time",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "axiom-host must not use randomness",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-host must emit structured records, not print",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-host must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static"],
        "axiom-host must not use global mutable state",
    );
}

#[test]
fn no_renderer_or_shader_concepts() {
    assert_absent(
        &[
            "::Renderer",
            "::RenderPipeline",
            "::Shader",
            "::ShaderModule",
            "::Material",
            "::Mesh",
            "::Texture",
            "::Swapchain",
        ],
        "axiom-host must not absorb renderer / shader / material concepts",
    );
}

#[test]
fn no_higher_engine_layer_concepts() {
    assert_absent(
        &[
            "::World",
            "::Scene",
            "::SceneGraph",
            "::Asset",
            "::AssetLoader",
            "::Physics",
            "::RigidBody",
            "::Collider",
            "::Animation",
            "::Animator",
            "::Audio",
            "::SoundSource",
            "::InputState",
            "::KeyCode",
            "::MouseButton",
            "::Gamepad",
            "::Plugin",
            "::EditorPanel",
            "::GameLoop",
            "rapier",
            "wgpu",
            "winit",
            "egui",
            "bevy",
        ],
        "axiom-host must not import any higher-layer engine concept",
    );
}

#[test]
fn no_utils_or_helpers_modules() {
    for path in host_source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert_ne!(name, "utils", "axiom-host must not have a `utils` module");
        assert_ne!(
            name, "helpers",
            "axiom-host must not have a `helpers` module"
        );
        assert_ne!(name, "common", "axiom-host must not have a `common` module");
        assert_ne!(name, "misc", "axiom-host must not have a `misc` module");
    }
}

#[test]
fn lib_exports_are_curated_set() {
    let lib = read(&host_src_dir().join("lib.rs"));
    let mut actual: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    actual.sort();

    let mut expected: Vec<&str> = vec![
        "pub use host_api::HostApi;",
        "pub use host_boundary_config::HostBoundaryConfig;",
        "pub use host_error::HostError;",
        "pub use host_error_code::HostErrorCode;",
        "pub use host_frame_input::HostFrameInput;",
        "pub use host_frame_report::HostFrameReport;",
        "pub use host_lifecycle_signal::HostLifecycleSignal;",
        "pub use host_lifecycle_state::HostLifecycleState;",
        "pub use host_result::HostResult;",
        "pub use pixels::Pixels;",
        "pub use host_skip_reason::HostSkipReason;",
        "pub use host_step_driver::HostStepDriver;",
        "pub use host_step_plan::HostStepPlan;",
        "pub use host_viewport::HostViewport;",
        // Presentation boundary.
        "pub use host_adapter_request::HostAdapterRequest;",
        "pub use host_alpha_mode::HostAlphaMode;",
        "pub use host_color_format::HostColorFormat;",
        "pub use host_device_profile::HostDeviceProfile;",
        "pub use host_device_request::HostDeviceRequest;",
        "pub use host_present_mode::HostPresentMode;",
        "pub use host_presentation_report::HostPresentationReport;",
        "pub use host_presentation_request::HostPresentationRequest;",
        "pub use host_presentation_status::HostPresentationStatus;",
        "pub use host_presentation_target::HostPresentationTarget;",
        "pub use host_power_preference::HostPowerPreference;",
        "pub use host_surface_descriptor::HostSurfaceDescriptor;",
        "pub use host_surface_handle::HostSurfaceHandle;",
    ];
    expected.sort();

    assert_eq!(
        actual, expected,
        "axiom-host's lib.rs public exports must match the curated set; \
         update both lib.rs and this test together"
    );
}

#[test]
fn axiom_kernel_does_not_import_axiom_host() {
    assert_absent_in(
        &sibling_src_dir("axiom-kernel"),
        "axiom-kernel",
        &["axiom_host", "axiom-host"],
        "axiom-kernel (Layer 00) must not import axiom-host (Layer 03)",
    );
}

#[test]
fn axiom_runtime_does_not_import_axiom_host() {
    assert_absent_in(
        &sibling_src_dir("axiom-runtime"),
        "axiom-runtime",
        &["axiom_host", "axiom-host"],
        "axiom-runtime (Layer 01) must not import axiom-host (Layer 03)",
    );
}

#[test]
fn axiom_math_does_not_import_axiom_host() {
    assert_absent_in(
        &sibling_src_dir("axiom-math"),
        "axiom-math",
        &["axiom_host", "axiom-host"],
        "axiom-math (Layer 02) must not import axiom-host (Layer 03)",
    );
}

#[test]
fn host_only_imports_legal_lower_layers() {
    let mut illegal = Vec::new();
    for path in host_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for line in stripped.lines() {
            let trimmed = line.trim();
            if !trimmed.contains("axiom_") {
                continue;
            }
            for chunk in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if chunk.starts_with("axiom_")
                    && chunk != "axiom_kernel"
                    && chunk != "axiom_runtime"
                    && chunk != "axiom_host"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-host may only import axiom-kernel and axiom-runtime:\n{}",
        illegal.join("\n")
    );
}
