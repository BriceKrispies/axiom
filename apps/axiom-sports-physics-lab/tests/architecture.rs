//! Architecture-boundary tests for the `axiom-sports-physics-lab` app.
//!
//! The lab is a composition leaf: a deterministic, native-tested core plus one
//! `wasm32` browser edge (`web.rs` + `overlay.rs`). These tests pin the app's
//! declared surface (app.toml), keep the browser/DOM strings confined to the
//! wasm edge, keep the core deterministic, and prove no engine layer or module
//! ever grows a dependency on this app.

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

/// The two files that ARE the browser edge: everything else is the pure core.
fn is_wasm_edge(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name == "web.rs" || name == "overlay.rs"
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
    for required in [
        "\"kernel\"",
        "\"math\"",
        "\"host\"",
        "\"runtime\"",
        "\"recipe\"",
        "\"proc-texture\"",
        "\"engine\"",
        "\"physics\"",
        "\"fp-controller\"",
        "\"figure\"",
        "\"windowing\"",
        "\"debug-overlay\"",
    ] {
        assert!(text.contains(required), "app.toml lists {required}");
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
fn browser_and_dom_strings_stay_on_the_wasm_edge() {
    // The pure core (everything but web.rs/overlay.rs) is engine-agnostic and
    // native-tested: no browser, GPU, or DOM references.
    let core: Vec<PathBuf> = app_sources()
        .into_iter()
        .filter(|p| !is_wasm_edge(p))
        .collect();
    assert!(!core.is_empty(), "expected pure-core source files");
    assert_absent(
        &core,
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "webgpu",
            "WebGpu",
            "WebGL",
            "GPUDevice",
            "HtmlCanvas",
            "OffscreenCanvas",
            "document.",
            "window.",
            "navigator.",
        ],
        "the lab core is headless: browser/GPU/DOM strings live only in web.rs/overlay.rs",
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
        "the lab is deterministic: no wall-clock time, no randomness",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &app_sources(),
        &["todo!", "unimplemented!"],
        "no placeholder architecture in the lab",
    );
}

#[test]
fn no_illegal_engine_imports() {
    // The app may import only the layers/modules its app.toml declares (+ std).
    const ALLOWED: [&str; 13] = [
        "axiom",
        "axiom_kernel",
        "axiom_math",
        "axiom_host",
        "axiom_runtime",
        "axiom_recipe",
        "axiom_proc_texture",
        "axiom_physics",
        "axiom_fp_controller",
        "axiom_figure",
        "axiom_windowing",
        "axiom_debug_overlay",
        "axiom_sports_physics_lab",
    ];
    let mut illegal = Vec::new();
    for path in app_sources() {
        for chunk in strip_comments_and_strings(&read(&path))
            .split(|c: char| !c.is_alphanumeric() && c != '_')
        {
            if chunk.starts_with("axiom_") && !ALLOWED.contains(&chunk) {
                illegal.push(format!("{}: {}", path.display(), chunk));
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "app imports only its declared layers/modules:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in app_sources() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, banned, "no `{banned}` module in the lab");
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
    let mut hits = scan_raw(repo_root().join("crates"), &["axiom_sports_physics_lab"]);
    hits.extend(scan_raw(
        repo_root().join("modules"),
        &["axiom_sports_physics_lab"],
    ));
    assert!(
        hits.is_empty(),
        "no layer/module may depend on the sports-physics-lab app:\n{}",
        hits.join("\n")
    );
}
