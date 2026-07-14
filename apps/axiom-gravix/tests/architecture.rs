//! Architecture-boundary tests for the `axiom-gravix` app.
//!
//! Gravix is a browser demo with a deterministic native core: the game/physics
//! core must stay headless and deterministic, and only the explicit wasm32 edge
//! (`web.rs`, `overlay.rs`) may touch browser APIs. These tests also enforce app
//! hygiene: the app.toml lists exactly the consumed layers/modules, imports stay
//! inside the declared dependency set, and no engine code depends on this app.

use std::fs;
use std::path::{Path, PathBuf};

/// The wasm32-only source files — the sanctioned browser edge. Everything else
/// under `src/` is the headless, deterministic core.
const WASM_EDGE_FILES: &[&str] = &["web.rs", "overlay.rs"];

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

fn is_wasm_edge(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    WASM_EDGE_FILES.contains(&name)
}

fn assert_absent(paths: &[PathBuf], forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in paths {
        let stripped = strip_comments_and_strings(&read(path));
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
    for layer in ["\"kernel\"", "\"math\"", "\"runtime\""] {
        assert!(text.contains(layer), "app.toml lists the {layer} layer");
    }
    for module in [
        "\"engine\"",
        "\"physics\"",
        "\"terrain-mesh\"",
        "\"windowing\"",
        "\"debug-overlay\"",
    ] {
        assert!(text.contains(module), "app.toml lists the {module} module");
    }
    // It must not claim modules/layers it does not use.
    for forbidden in [
        "\"scene\"",
        "\"render\"",
        "\"webgpu\"",
        "\"input\"",
        "\"fp-controller\"",
        "\"agent\"",
    ] {
        assert!(
            !text.contains(forbidden),
            "app.toml must not list `{forbidden}`"
        );
    }
}

#[test]
fn core_is_headless_no_browser_gpu_or_dom() {
    // Only the explicit wasm32 edge files may reference browser APIs; the game
    // core (physics, course, camera, spin) stays headless and native-testable.
    let core: Vec<PathBuf> = app_sources()
        .into_iter()
        .filter(|p| !is_wasm_edge(p))
        .collect();
    assert_absent(
        &core,
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "HtmlCanvas",
            "OffscreenCanvas",
            "document.",
            "window.",
            "navigator.",
            "requestAnimationFrame",
        ],
        "the gravix core is headless: browser APIs live only in web.rs/overlay.rs",
    );
}

#[test]
fn no_wall_clock_or_randomness() {
    assert_absent(
        &app_sources(),
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
        "gravix is deterministic: no wall-clock time, no unseeded randomness",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &app_sources(),
        &["todo!", "unimplemented!"],
        "no placeholder architecture in gravix",
    );
}

#[test]
fn no_illegal_engine_imports() {
    // The app may import only the layers/modules its app.toml declares (plus
    // the `axiom` umbrella, wasm glue crates, and std).
    let allowed = [
        "axiom_kernel",
        "axiom_math",
        "axiom_runtime",
        "axiom_physics",
        "axiom_terrain_mesh",
        "axiom_windowing",
        "axiom_debug_overlay",
        "axiom_gravix",
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
        "gravix may only import its declared layers/modules:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in app_sources() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, banned, "no `{banned}` module in gravix");
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
    let mut hits = scan_raw(repo_root().join("crates"), &["axiom_gravix"]);
    hits.extend(scan_raw(repo_root().join("modules"), &["axiom_gravix"]));
    assert!(
        hits.is_empty(),
        "no layer/module may depend on the gravix app:\n{}",
        hits.join("\n")
    );
}
