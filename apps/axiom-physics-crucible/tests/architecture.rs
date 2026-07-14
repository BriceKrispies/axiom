//! Architecture-boundary tests for the `axiom-physics-crucible` app.
//!
//! The crucible is a composition leaf: it may compose the engine umbrella, the
//! physics module under test, and (on wasm32 only) the windowing/debug-overlay
//! platform arm. These tests enforce app hygiene: the manifest lists only what
//! the app consumes, browser/platform code stays confined to the wasm arm
//! (`web.rs` / `overlay.rs`), the simulation stays deterministic, and nothing
//! in the reusable engine depends back on this app.

use std::fs;
use std::path::{Path, PathBuf};

fn app_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn app_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    app_root()
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("repo root is two levels up")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).expect("readable dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("utf-8 source")
}

fn strip_comments_and_strings(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let (mut in_string, mut in_char) = (false, false);
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if in_char {
            if c == '\\' {
                chars.next();
            } else if c == '\'' {
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
        match c {
            '"' => in_string = true,
            '\'' => in_char = true,
            _ => out.push(c),
        }
    }
    out
}

fn app_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&app_src(), &mut files);
    assert!(!files.is_empty(), "expected app source files");
    files.sort();
    files
}

/// Whether a source file is part of the sanctioned wasm32 platform arm (the
/// live browser driver + the overlay mount), which legitimately touches
/// wasm-bindgen / web-sys / windowing.
fn is_wasm_arm(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name == "web.rs" || name == "overlay.rs"
}

fn assert_absent_in_native_app(forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in app_sources() {
        if is_wasm_arm(&path) {
            continue;
        }
        let stripped = strip_comments_and_strings(&read(&path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "{}: contains forbidden `{}`",
                    path.display(),
                    needle
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

#[test]
fn app_toml_exists_and_lists_only_consumed_layers_and_modules() {
    let manifest = app_root().join("app.toml");
    let text = fs::read_to_string(&manifest).expect("app.toml exists");
    for required in [
        "\"kernel\"",
        "\"runtime\"",
        "\"engine\"",
        "\"physics\"",
        "\"windowing\"",
        "\"debug-overlay\"",
    ] {
        assert!(
            text.contains(required),
            "app.toml lists the consumed {required} layer/module"
        );
    }
    // It must not claim modules it does not use.
    for forbidden in [
        "\"scene\"",
        "\"render\"",
        "\"webgpu\"",
        "\"agent\"",
        "\"input\"",
    ] {
        assert!(
            !text.contains(forbidden),
            "app.toml must not list `{forbidden}`"
        );
    }
}

#[test]
fn browser_platform_code_is_confined_to_the_wasm_arm() {
    // The crucible's simulation, stations, report, and render translation are
    // platform-free; only `web.rs` (the live browser driver) and `overlay.rs`
    // (the debug-overlay mount) may speak wasm-bindgen / web-sys / windowing.
    assert_absent_in_native_app(
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen::JsCast",
            "axiom_windowing",
            "HtmlCanvas",
            "OffscreenCanvas",
            "document.",
            "navigator.",
        ],
        "browser/platform code belongs only in the wasm arm (web.rs / overlay.rs)",
    );
}

#[test]
fn no_wall_clock_or_randomness() {
    assert_absent_in_native_app(
        &[
            "std::time",
            "SystemTime",
            "Instant::now",
            "chrono",
            "rand::",
            "thread_rng",
            "getrandom",
            "fastrand",
        ],
        "the crucible is deterministic: no wall-clock time, no randomness",
    );
}

#[test]
fn no_placeholder_macros() {
    let mut violations = Vec::new();
    for path in app_sources() {
        let stripped = strip_comments_and_strings(&read(&path));
        for needle in ["todo!", "unimplemented!"] {
            if stripped.contains(needle) {
                violations.push(format!(
                    "{}: contains forbidden `{}`",
                    path.display(),
                    needle
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "no placeholder architecture in the crucible:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_illegal_engine_imports() {
    // The app may import only the layers/modules its app.toml declares (+ std):
    // axiom (the umbrella), axiom-kernel, axiom-runtime, axiom-physics, and —
    // in the wasm arm — axiom-windowing and axiom-debug-overlay.
    let allowed = [
        "axiom_kernel",
        "axiom_runtime",
        "axiom_physics",
        "axiom_windowing",
        "axiom_debug_overlay",
        "axiom_physics_crucible",
    ];
    let mut illegal = Vec::new();
    for path in app_sources() {
        for chunk in strip_comments_and_strings(&read(&path))
            .split(|c: char| !c.is_alphanumeric() && c != '_')
        {
            if chunk.starts_with("axiom_") && !allowed.contains(&chunk) {
                illegal.push(format!("{}: {}", path.display(), chunk));
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "app may only import its declared layers/modules:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in app_sources() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, banned, "no `{banned}` module in the crucible");
        }
    }
}

fn scan_raw(dir: PathBuf, needles: &[&str]) -> Vec<String> {
    let mut files = Vec::new();
    collect_rs(&dir, &mut files);
    let mut hits = Vec::new();
    for path in &files {
        let text = read(path);
        for needle in needles {
            if text.contains(needle) {
                hits.push(format!("{}: contains `{}`", path.display(), needle));
            }
        }
    }
    hits
}

#[test]
fn no_layer_or_module_depends_on_this_app() {
    let mut hits = scan_raw(repo_root().join("crates"), &["axiom_physics_crucible"]);
    hits.extend(scan_raw(
        repo_root().join("modules"),
        &["axiom_physics_crucible"],
    ));
    assert!(
        hits.is_empty(),
        "no layer/module may depend on the crucible app:\n{}",
        hits.join("\n")
    );
}
