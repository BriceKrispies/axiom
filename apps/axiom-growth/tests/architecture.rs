//! Architecture-boundary tests for the `axiom-growth` app.
//!
//! Growth is a composition leaf (a browser demo with native agent/visual-target
//! drivers): it may compose the layers and modules its `app.toml` declares, and
//! nothing engine-side may ever depend on it. These tests enforce app hygiene —
//! the import whitelist, no placeholder macros, no junk-drawer modules — and the
//! leaf property.

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
    // The demo's real spine: planet worldgen, streaming, the umbrella engine
    // module, and the browser presentation arm.
    for expected in [
        "\"planetgen\"",
        "\"streaming\"",
        "\"engine\"",
        "\"windowing\"",
        "\"debug-overlay\"",
        "\"geosphere\"",
    ] {
        assert!(text.contains(expected), "app.toml lists {expected}");
    }
    // It must not claim modules other demos used but growth does not.
    for forbidden in [
        "\"physics\"",
        "\"input\"",
        "\"fp-controller\"",
        "\"scatter\"",
        "\"world\"",
        "\"recording\"",
    ] {
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
        "no placeholder architecture in the growth app",
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in app_sources() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, banned, "no `{banned}` module in the growth app");
        }
    }
}

#[test]
fn no_illegal_engine_imports() {
    // The app may import only the axiom crates its Cargo.toml (all targets and
    // features) declares — exactly the app.toml layer/module lists.
    const ALLOWED: &[&str] = &[
        // this crate (tests/bins name it)
        "axiom_growth",
        // layers
        "axiom_kernel",
        "axiom_math",
        "axiom_host",
        "axiom_entropy",
        "axiom_noise",
        "axiom_geosphere",
        "axiom_space",
        "axiom_proc_validate",
        "axiom_introspect",
        "axiom_interface",
        // modules (the umbrella `axiom` crate has no `axiom_` prefix)
        "axiom_biome",
        "axiom_planetgen",
        "axiom_streaming",
        "axiom_terrain_mesh",
        "axiom_agent_harness",
        "axiom_perception",
        "axiom_gpu_backend",
        "axiom_canvas2d_backend",
        "axiom_windowing",
        "axiom_debug_overlay",
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
        "app imports an axiom crate outside its declared set:\n{}",
        illegal.join("\n")
    );
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
    let mut hits = scan_raw(repo_root().join("crates"), &["axiom_growth"]);
    hits.extend(scan_raw(repo_root().join("modules"), &["axiom_growth"]));
    assert!(
        hits.is_empty(),
        "no layer/module may depend on the growth app:\n{}",
        hits.join("\n")
    );
}
