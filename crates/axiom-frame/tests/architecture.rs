//! Mechanical architecture enforcement for Axiom Layer 04 (axiom-frame).

use std::fs;
use std::path::{Path, PathBuf};

fn frame_src_dir() -> PathBuf {
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

fn frame_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&frame_src_dir(), &mut files);
    assert!(!files.is_empty(), "expected frame source files");
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("source must be valid UTF-8")
}

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
    assert_absent_in(&frame_src_dir(), "axiom-frame", forbidden, why);
}

#[test]
fn no_browser_or_js_bindgen_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "axiom-frame must not reference browser / JS bindings",
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
        "axiom-frame must not reference DOM/canvas/browser globals",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "webgpu", "WebGpu", "WebGL", "webgl", "GPUDevice"],
        "axiom-frame must not reference WebGPU/WebGL",
    );
}

#[test]
fn no_animation_frame_or_performance_now() {
    assert_absent(
        &["requestAnimationFrame", "performance.now"],
        "axiom-frame must not call browser frame/clock APIs",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant::now", "chrono"],
        "axiom-frame must not read wall-clock time",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "axiom-frame must not use randomness",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-frame must emit structured records, not print",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-frame must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static"],
        "axiom-frame must not use global mutable state",
    );
}

#[test]
fn no_renderer_or_shader_concepts() {
    assert_absent(
        &[
            "::Renderer",
            "::RenderPipeline",
            "::RenderGraph",
            "::Shader",
            "::ShaderModule",
            "::Material",
            "::Mesh",
            "::Texture",
            "::Swapchain",
        ],
        "axiom-frame must not absorb renderer / shader / material concepts",
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
        "axiom-frame must not import any higher-layer engine concept",
    );
}

#[test]
fn no_utils_or_helpers_modules() {
    for path in frame_source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert_ne!(name, "utils", "axiom-frame must not have a `utils` module");
        assert_ne!(name, "helpers", "axiom-frame must not have a `helpers` module");
        assert_ne!(name, "common", "axiom-frame must not have a `common` module");
        assert_ne!(name, "misc", "axiom-frame must not have a `misc` module");
    }
}

#[test]
fn lib_exports_are_curated_set() {
    let lib = read(&frame_src_dir().join("lib.rs"));
    let mut actual: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    actual.sort();

    let mut expected: Vec<&str> = vec![
        "pub use frame_api::FrameApi;",
        "pub use engine_frame::EngineFrame;",
        "pub use frame_builder::FrameBuilder;",
        "pub use frame_command::FrameCommand;",
        "pub use frame_command_queue::FrameCommandQueue;",
        "pub use frame_context::FrameContext;",
        "pub use frame_diagnostics::FrameDiagnostics;",
        "pub use frame_error::FrameError;",
        "pub use frame_error_code::FrameErrorCode;",
        "pub use frame_lifecycle_state::FrameLifecycleState;",
        "pub use frame_result::FrameResult;",
        "pub use frame_step_summary::FrameStepSummary;",
        "pub use frame_system_report::FrameSystemReport;",
        "pub use frame_timing::FrameTiming;",
        "pub use frame_viewport::FrameViewport;",
    ];
    expected.sort();

    assert_eq!(
        actual, expected,
        "axiom-frame's lib.rs public exports must match the curated set; \
         update both lib.rs and this test together"
    );
}

#[test]
fn axiom_kernel_does_not_import_axiom_frame() {
    assert_absent_in(
        &sibling_src_dir("axiom-kernel"),
        "axiom-kernel",
        &["axiom_frame", "axiom-frame"],
        "axiom-kernel (Layer 00) must not import axiom-frame (Layer 04)",
    );
}

#[test]
fn axiom_runtime_does_not_import_axiom_frame() {
    assert_absent_in(
        &sibling_src_dir("axiom-runtime"),
        "axiom-runtime",
        &["axiom_frame", "axiom-frame"],
        "axiom-runtime (Layer 01) must not import axiom-frame (Layer 04)",
    );
}

#[test]
fn axiom_math_does_not_import_axiom_frame() {
    assert_absent_in(
        &sibling_src_dir("axiom-math"),
        "axiom-math",
        &["axiom_frame", "axiom-frame"],
        "axiom-math (Layer 02) must not import axiom-frame (Layer 04)",
    );
}

#[test]
fn axiom_host_does_not_import_axiom_frame() {
    assert_absent_in(
        &sibling_src_dir("axiom-host"),
        "axiom-host",
        &["axiom_frame", "axiom-frame"],
        "axiom-host (Layer 03) must not import axiom-frame (Layer 04)",
    );
}

#[test]
fn frame_only_imports_legal_lower_layers() {
    let mut illegal = Vec::new();
    for path in frame_source_files() {
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
                    && chunk != "axiom_math"
                    && chunk != "axiom_host"
                    && chunk != "axiom_frame"
                    // axiom-zones is the build-time zone-marker Support crate.
                    && chunk != "axiom_zones"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-frame may only import axiom-kernel, axiom-runtime, axiom-math, and axiom-host:\n{}",
        illegal.join("\n")
    );
}
