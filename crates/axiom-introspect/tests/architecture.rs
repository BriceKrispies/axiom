//! Mechanical architecture enforcement for Axiom Layer 05 (axiom-introspect).

use std::fs;
use std::path::{Path, PathBuf};

fn introspect_src_dir() -> PathBuf {
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

fn introspect_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&introspect_src_dir(), &mut files);
    assert!(!files.is_empty(), "expected introspect source files");
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
    assert_absent_in(&introspect_src_dir(), "axiom-introspect", forbidden, why);
}

#[test]
fn no_browser_or_js_bindgen_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "axiom-introspect must not reference browser / JS bindings",
    );
}

#[test]
fn no_dom_canvas_or_browser_globals() {
    assert_absent(
        &[
            "HtmlCanvas",
            "OffscreenCanvas",
            "document.",
            "window.",
            "navigator.",
        ],
        "axiom-introspect must not reference DOM/canvas/browser globals",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "webgpu", "WebGpu", "WebGL", "webgl", "GPUDevice"],
        "axiom-introspect must not reference WebGPU/WebGL",
    );
}

#[test]
fn no_wall_clock_time_or_randomness() {
    assert_absent(
        &[
            "std::time",
            "SystemTime",
            "Instant::now",
            "rand::",
            "thread_rng",
            "getrandom",
        ],
        "axiom-introspect must not read wall-clock time or use randomness",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-introspect must emit structured data, not print",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-introspect must contain no placeholder architecture",
    );
}

#[test]
fn no_utils_or_helpers_modules() {
    for path in introspect_source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert_ne!(
            name, "utils",
            "axiom-introspect must not have a `utils` module"
        );
        assert_ne!(
            name, "helpers",
            "axiom-introspect must not have a `helpers` module"
        );
        assert_ne!(
            name, "common",
            "axiom-introspect must not have a `common` module"
        );
        assert_ne!(
            name, "misc",
            "axiom-introspect must not have a `misc` module"
        );
    }
}

#[test]
fn lib_exports_are_curated_set() {
    let lib = read(&introspect_src_dir().join("lib.rs"));
    let mut actual: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    actual.sort();

    let mut expected: Vec<&str> = vec![
        "pub use introspect_api::IntrospectApi;",
        "pub use frame_history::FrameHistory;",
        "pub use frame_report::FrameReport;",
        "pub use metric_report::MetricReport;",
        "pub use system_report::SystemReport;",
        "pub use world_report::WorldReport;",
    ];
    expected.sort();

    assert_eq!(
        actual, expected,
        "axiom-introspect's lib.rs public exports must match the curated set; \
         update both lib.rs and this test together"
    );
}

#[test]
fn lower_layers_do_not_import_axiom_introspect() {
    for layer in [
        "axiom-kernel",
        "axiom-runtime",
        "axiom-math",
        "axiom-host",
        "axiom-frame",
        "axiom-ecs",
    ] {
        assert_absent_in(
            &sibling_src_dir(layer),
            layer,
            &["axiom_introspect", "axiom-introspect"],
            "no lower layer may import axiom-introspect (Layer 06)",
        );
    }
}

#[test]
fn introspect_only_imports_legal_lower_layers() {
    let mut illegal = Vec::new();
    for path in introspect_source_files() {
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
                    && chunk != "axiom_ecs"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-introspect may only import axiom-kernel/runtime/math/host/frame:\n{}",
        illegal.join("\n")
    );
}
