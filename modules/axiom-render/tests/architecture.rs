//! Per-module architecture-boundary tests for axiom-render.

use std::fs;
use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("src dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&src_dir(), &mut files);
    assert!(!files.is_empty());
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("utf-8")
}

fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_str = false;
    let mut in_char = false;
    while let Some(c) = chars.next() {
        if in_str {
            if c == '\\' {
                chars.next();
                continue;
            }
            if c == '"' {
                in_str = false;
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
            in_str = true;
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

fn assert_absent(needles: &[&str], why: &str) {
    let mut v = Vec::new();
    for p in source_files() {
        let s = strip(&read(&p));
        for n in needles {
            if s.contains(n) {
                v.push(format!("{}: contains `{}`", p.display(), n));
            }
        }
    }
    assert!(v.is_empty(), "{why}\n{}", v.join("\n"));
}

#[test]
fn module_toml_exists() {
    assert!(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("module.toml")
        .is_file());
}

#[test]
fn lib_rs_exports_only_render_api() {
    let text = read(&src_dir().join("lib.rs"));
    let actual: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("pub ") && !l.starts_with("pub(crate)"))
        .collect();
    assert_eq!(actual, vec!["pub use render_api::RenderApi;"]);
}

#[test]
fn no_browser_or_webgpu() {
    assert_absent(
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "wasm-bindgen",
            "wgpu",
            "webgpu",
            "WebGL",
            "webgl",
            "requestAnimationFrame",
            "document.",
            "window.",
            "canvas",
        ],
        "axiom-render must not reference browser/WebGPU/DOM",
    );
}

#[test]
fn no_scene_or_resources_or_webgpu_or_other_module_imports() {
    assert_absent(
        &[
            "axiom_scene",
            "axiom-scene",
            "axiom_resources",
            "axiom-resources",
            "axiom_webgpu",
            "axiom-webgpu",
        ],
        "axiom-render must not import scene/resources/webgpu",
    );
}

#[test]
fn host_dependency_is_limited_to_the_neutral_frame_packet() {
    // render may name host's neutral FramePacket types but must not become a
    // host/presentation consumer (facade, stepping, surface APIs).
    assert_absent(
        &[
            "HostApi",
            "HostStepDriver",
            "HostStepPlan",
            "HostBoundaryConfig",
            "HostFrameInput",
            "HostFrameReport",
            "HostLifecycleSignal",
            "HostPresentationRequest",
            "HostPresentationTarget",
            "HostPresentationReport",
            "HostSurfaceHandle",
            "HostSurfaceDescriptor",
            "HostAdapterRequest",
            "HostDeviceRequest",
            "HostViewport",
        ],
        "axiom-render may name host's neutral FramePacket types but must not \
         consume host's presentation/stepping/surface APIs",
    );
}

#[test]
fn no_forbidden_debug_macros() {
    assert_absent(
        &[
            "println!",
            "eprintln!",
            "print!",
            "eprint!",
            "dbg!",
            "todo!",
            "unimplemented!",
        ],
        "axiom-render must not use debug-print macros",
    );
}

#[test]
fn no_higher_engine_concepts() {
    assert_absent(
        &[
            "std::fs",
            "::AssetLoader",
            "::Physics",
            "::Animator",
            "::Audio",
            "::InputState",
            "::Scene",
        ],
        "axiom-render must not absorb scene/asset/physics/animation/audio/input concepts",
    );
}

#[test]
fn capture_boundary_has_no_pixels_clock_randomness_or_global_state() {
    assert_absent(
        &[
            "screenshot",
            "read_back",
            "readback",
            "map_async",
            "get_current_texture",
            "TextureView",
            "Framebuffer",
            "Instant",
            "SystemTime",
            "std::time",
            "chrono",
            "rand::",
            "thread_rng",
            "getrandom",
            "random()",
            "static mut",
            "lazy_static",
        ],
        "axiom-render's capture boundary must be pixel-free and deterministic",
    );
}

#[test]
fn no_utils_modules() {
    for p in source_files() {
        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for forbidden in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, forbidden);
        }
    }
}
