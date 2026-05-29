//! Per-module architecture-boundary tests for axiom-resources.

use std::fs;
use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("src directory must exist") {
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
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("module.toml");
    assert!(manifest.is_file());
}

#[test]
fn lib_rs_exports_only_resources_api() {
    let text = read(&src_dir().join("lib.rs"));
    let actual: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("pub ") && !l.starts_with("pub(crate)"))
        .collect();
    assert_eq!(actual, vec!["pub use resources_api::ResourcesApi;"]);
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
        "axiom-resources must not reference browser/WebGPU/DOM",
    );
}

#[test]
fn no_other_module_imports() {
    assert_absent(
        &[
            "axiom_scene",
            "axiom-scene",
            "axiom_render",
            "axiom-render",
            "axiom_webgpu",
            "axiom-webgpu",
        ],
        "axiom-resources must not import any other module",
    );
}

#[test]
fn no_host_layer_import() {
    assert_absent(
        &["axiom_host", "axiom-host"],
        "axiom-resources should not depend on axiom-host",
    );
}

#[test]
fn no_forbidden_debug_macros() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!", "todo!", "unimplemented!"],
        "axiom-resources must not use debug-print macros",
    );
}

#[test]
fn no_asset_loading_or_physics_or_audio_or_input() {
    assert_absent(
        &[
            "std::fs",
            "::AssetLoader",
            "::Physics",
            "::Animator",
            "::Audio",
            "::InputState",
        ],
        "axiom-resources must not absorb asset/physics/animation/audio/input concepts",
    );
}

#[test]
fn no_utils_or_helpers_modules() {
    for p in source_files() {
        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for forbidden in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, forbidden);
        }
    }
}
