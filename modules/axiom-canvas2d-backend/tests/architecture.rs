//! Per-module architecture-boundary tests for axiom-canvas2d-backend.

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

#[test]
fn module_toml_exists() {
    assert!(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("module.toml")
        .is_file());
}

#[test]
fn lib_rs_exports_only_the_facade() {
    let text = read(&src_dir().join("lib.rs"));
    let actual: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("pub ") && !l.starts_with("pub(crate)"))
        .collect();
    assert_eq!(
        actual,
        vec!["pub use canvas2d_backend_api::Canvas2dBackendApi;"]
    );
}

#[test]
fn pure_rasterizer_core_has_no_browser_apis() {
    // The whole rasterizer (projection, triangle assembly, depth sort, colour,
    // op emission) is a pure native core with zero browser coupling. Only the
    // platform edge — the wasm-gated `live_canvas_binding.rs` and the facade's
    // `#[cfg(wasm32)]` `attach_canvas` arm in `canvas2d_backend_api.rs` — may
    // name web-sys/canvas APIs. Every other file must be clean.
    let edge = ["live_canvas_binding.rs", "canvas2d_backend_api.rs"];
    let needles = [
        "web_sys",
        "wasm_bindgen",
        "CanvasRenderingContext2d",
        "HtmlCanvasElement",
        "get_context",
        "ImageData",
        "put_image_data",
        "Clamped",
    ];
    let mut violations = Vec::new();
    for path in source_files() {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if edge.contains(&name) {
            continue;
        }
        // Strip `//` line comments so doc prose that *names* a browser API (e.g.
        // describing what the wasm arm does) does not trip the scan — only real
        // code references count.
        let code: String = read(&path)
            .lines()
            .map(|line| line.split("//").next().unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in needles {
            if code.contains(needle) {
                violations.push(format!("{}: contains `{needle}`", path.display()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "the pure rasterizer core must contain no browser APIs:\n{}",
        violations.join("\n")
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
