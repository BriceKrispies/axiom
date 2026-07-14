//! Architecture-boundary tests for the `axiom-retro-fps` app.
//!
//! The app is a composition leaf: a deterministic, natively-tested game core
//! (`lib.rs` + `level.rs`) with a thin wasm-only browser arm (`web.rs`,
//! `overlay.rs`) and feature-gated native agent bins (`src/bin/`). These tests
//! pin that shape: browser APIs stay in the wasm arm, wall-clock time stays out
//! of the deterministic core, imports stay within the app.toml surface, and no
//! layer or module depends back on this app.

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

/// The wasm-only browser arm: the two files allowed to touch browser APIs.
fn is_wasm_arm(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name == "web.rs" || name == "overlay.rs"
}

/// The native agent server bins: real-time relay servers, allowed wall-clock
/// timeouts (they are outside the deterministic game core).
fn is_agent_bin(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_str() == Some("bin"))
}

#[test]
fn app_toml_exists_and_lists_only_consumed_layers_and_modules() {
    let manifest = app_root().join("app.toml");
    let text = fs::read_to_string(&manifest).expect("app.toml exists");
    for required in [
        "\"kernel\"",
        "\"runtime\"",
        "\"interface\"",
        "\"engine\"",
        "\"windowing\"",
        "\"debug-overlay\"",
        "\"agent\"",
        "\"perception\"",
    ] {
        assert!(text.contains(required), "app.toml lists {required}");
    }
    // It must not claim modules it does not use.
    for forbidden in ["\"physics\"", "\"input\"", "\"scene\"", "\"render\"", "\"webgpu\""] {
        assert!(
            !text.contains(forbidden),
            "app.toml must not list `{forbidden}`"
        );
    }
}

#[test]
fn browser_apis_stay_in_the_wasm_arm() {
    // The deterministic core and the native agent code must never touch the
    // browser; only the wasm-only arm (`web.rs`, `overlay.rs`) may. (`wgpu` is
    // deliberately not scanned: the agent bins' offscreen renderer is native
    // GPU behind the `agent-render` feature, not a browser API.)
    let mut violations = Vec::new();
    for path in app_sources() {
        if is_wasm_arm(&path) {
            continue;
        }
        let stripped = strip_comments_and_strings(&read(&path));
        for needle in [
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "HtmlCanvas",
            "OffscreenCanvas",
            "document.",
            "navigator.",
        ] {
            if stripped.contains(needle) {
                violations.push(format!(
                    "{}: contains browser API `{}`",
                    path.display(),
                    needle
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "browser APIs belong in the wasm-only arm (web.rs / overlay.rs):\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_deterministic_core_has_no_wall_clock_or_randomness() {
    // The game core is a pure function of (tick, inputs). Wall-clock time is
    // allowed only in the native agent server bins (HTTP/WebSocket timeouts).
    let mut violations = Vec::new();
    for path in app_sources() {
        if is_agent_bin(&path) {
            continue;
        }
        let stripped = strip_comments_and_strings(&read(&path));
        for needle in [
            "std::time",
            "SystemTime",
            "Instant::now",
            "chrono",
            "rand::",
            "thread_rng",
            "getrandom",
            "fastrand",
        ] {
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
        "the game core is deterministic: no wall-clock time, no randomness:\n{}",
        violations.join("\n")
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
        "no placeholder architecture in the app:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_illegal_engine_imports() {
    // The app may import only the layers/modules its app.toml declares: the
    // engine umbrella (`axiom`), kernel, runtime, interface, agent, perception,
    // windowing, and debug-overlay (+ std and the external server/codec deps).
    let allowed = [
        "axiom_kernel",
        "axiom_runtime",
        "axiom_interface",
        "axiom_agent",
        "axiom_perception",
        "axiom_windowing",
        "axiom_debug_overlay",
        "axiom_retro_fps",
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
        "app imports must stay within the app.toml surface:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in app_sources() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, banned, "no `{banned}` module in the app");
        }
    }
}

#[test]
fn no_layer_or_module_depends_on_this_app() {
    let mut hits = Vec::new();
    for dir in ["crates", "modules"] {
        let mut files = Vec::new();
        collect_rs(&repo_root().join(dir), &mut files);
        for path in &files {
            if read(path).contains("axiom_retro_fps") {
                hits.push(format!("{}: contains `axiom_retro_fps`", path.display()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "no layer/module may depend on the retro-fps app:\n{}",
        hits.join("\n")
    );
}
