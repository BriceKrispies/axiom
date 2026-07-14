//! Architecture-boundary tests for the `axiom-dev-harness` app.
//!
//! The harness is a thin browser host for the `axiom-debug-overlay` module:
//! it mounts the overlay via the module's measured-diagnostics driver and owns
//! no overlay logic itself. These tests pin that thinness — the app imports
//! only the overlay module, claims only what it consumes in `app.toml`, and is
//! a true composition leaf that nothing in the engine spine depends on.

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

fn assert_absent_in_app(forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in app_sources() {
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
    assert!(
        text.contains("\"debug-overlay\""),
        "app.toml lists the debug-overlay module"
    );
    // It must not claim render/scene/presentation modules it does not use.
    for forbidden in ["\"scene\"", "\"render\"", "\"webgpu\"", "\"windowing\""] {
        assert!(
            !text.contains(forbidden),
            "app.toml must not list `{forbidden}`"
        );
    }
}

#[test]
fn no_placeholder_macros() {
    assert_absent_in_app(
        &["todo!", "unimplemented!"],
        "no placeholder architecture in the harness",
    );
}

#[test]
fn no_illegal_engine_imports() {
    // The harness is a thin overlay host: it may import only the
    // debug-overlay module (+ std and the wasm glue crates).
    let mut illegal = Vec::new();
    for path in app_sources() {
        for chunk in strip_comments_and_strings(&read(&path))
            .split(|c: char| !c.is_alphanumeric() && c != '_')
        {
            if chunk.starts_with("axiom_")
                && chunk != "axiom_debug_overlay"
                && chunk != "axiom_dev_harness"
            {
                illegal.push(format!("{}: {}", path.display(), chunk));
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "app may only import axiom-debug-overlay:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn overlay_logic_stays_in_the_module() {
    // The measured-diagnostics driver graduated into the module facade
    // (`DebugOverlayApi::mount_with_measured_diagnostics`). The harness must
    // stay thin: no re-grown local measurement loop poking the granular
    // diagnostics setters or raw browser probes.
    assert_absent_in_app(
        &[
            "set_frame",
            "set_backends",
            "set_counters",
            "set_fallback",
            "set_visibility",
            "request_animation_frame",
            "web_sys",
            "js_sys",
        ],
        "the harness mounts the module's driver; it measures nothing itself",
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in app_sources() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, banned, "no `{banned}` module in the harness");
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
    let mut hits = scan_raw(repo_root().join("crates"), &["axiom_dev_harness"]);
    hits.extend(scan_raw(repo_root().join("modules"), &["axiom_dev_harness"]));
    assert!(
        hits.is_empty(),
        "no layer/module may depend on the dev-harness app:\n{}",
        hits.join("\n")
    );
}
