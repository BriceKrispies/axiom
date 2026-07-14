//! Architecture-boundary hygiene for the `axiom-end-zone` app: the app.toml
//! matches consumption, the deterministic core is browser-free and
//! wall-clock-free, no placeholder macros or junk-drawer modules exist, and
//! no engine layer/module depends on this composition leaf.

use std::fs;
use std::path::{Path, PathBuf};

fn app_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn app_src() -> PathBuf {
    app_root().join("src")
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

/// Deterministic-core sources: everything except the sanctioned wasm edge
/// (the `src/web/` directory — DOM presenter, storage adapter, gamepad,
/// tones; all `cfg(target_arch = "wasm32")`).
fn core_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&app_src(), &mut files);
    files.retain(|p| {
        let is_edge = p.components().any(|c| c.as_os_str() == "web")
            || p.file_name().and_then(|n| n.to_str()) == Some("web.rs");
        !is_edge
    });
    assert!(files.len() > 30, "the app has its full module tree");
    files.sort();
    files
}

fn assert_absent_in_core(forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in core_sources() {
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
fn app_toml_lists_only_consumed_layers_and_modules() {
    let text = fs::read_to_string(app_root().join("app.toml")).expect("app.toml exists");
    for required in [
        "\"kernel\"",
        "\"math\"",
        "\"host\"",
        "\"runtime\"",
        "\"interface\"",
        "\"layout\"",
        "\"engine\"",
        "\"physics\"",
        "\"figure\"",
        "\"input\"",
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
        "\"animation\"",
        "\"agent\"",
    ] {
        assert!(
            !text.contains(forbidden),
            "app.toml must not list {forbidden}"
        );
    }
}

#[test]
fn the_deterministic_core_is_browser_free() {
    assert_absent_in_core(
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "wgpu",
            "WebGpu",
            "WebGL",
            "GPUDevice",
            "HtmlCanvas",
            "OffscreenCanvas",
            "document.",
            "navigator.",
        ],
        "everything outside web.rs is headless",
    );
}

#[test]
fn the_deterministic_core_has_no_wall_clock_or_ambient_randomness() {
    assert_absent_in_core(
        &[
            "std::time",
            "SystemTime",
            "Instant::now",
            "chrono",
            "rand::",
            "thread_rng",
            "getrandom",
            "fastrand",
            "Date::now",
        ],
        "the sim is deterministic: no wall clock, no ambient randomness",
    );
}

#[test]
fn no_placeholder_or_console_macros() {
    assert_absent_in_core(
        &[
            "todo!",
            "unimplemented!",
            "dbg!",
            "println!",
            "eprintln!",
            "panic!(",
        ],
        "no placeholders or console output in the app core",
    );
}

#[test]
fn no_unwrap_or_expect_in_production_paths() {
    // Production sources are unwrap/expect free; `unwrap_or*` fallbacks are
    // fine (they cannot panic) and inline #[cfg(test)] modules are exempt.
    let mut violations = Vec::new();
    for path in core_sources() {
        let text = read(&path);
        let production = text.split("#[cfg(test)]").next().unwrap_or("");
        let stripped = strip_comments_and_strings(production);
        let mut scan = stripped.as_str();
        while let Some(at) = scan.find(".unwrap") {
            let rest = &scan[at + ".unwrap".len()..];
            if rest.starts_with("()") {
                violations.push(format!("{}: .unwrap()", path.display()));
            }
            scan = rest;
        }
        if stripped.contains(".expect(") {
            violations.push(format!("{}: .expect(", path.display()));
        }
    }
    assert!(
        violations.is_empty(),
        "no unwrap/expect in production paths:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in core_sources() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, banned, "no `{banned}` module in end-zone");
        }
    }
}

#[test]
fn only_declared_engine_crates_are_imported() {
    let allowed = [
        "axiom",
        "axiom_kernel",
        "axiom_math",
        "axiom_host",
        "axiom_runtime",
        "axiom_interface",
        "axiom_layout",
        "axiom_physics",
        "axiom_figure",
        "axiom_input",
        "axiom_windowing",
        "axiom_debug_overlay",
        "axiom_end_zone",
    ];
    let mut files = Vec::new();
    collect_rs(&app_src(), &mut files);
    let mut illegal = Vec::new();
    for path in files {
        for chunk in strip_comments_and_strings(&read(&path))
            .split(|c: char| !c.is_alphanumeric() && c != '_')
        {
            if chunk.starts_with("axiom_") || chunk == "axiom" {
                if !allowed.contains(&chunk) {
                    illegal.push(format!("{}: {}", path.display(), chunk));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "undeclared engine import:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn no_layer_or_module_depends_on_this_app() {
    let mut hits = Vec::new();
    for dir in ["crates", "modules"] {
        let mut files = Vec::new();
        collect_rs(&repo_root().join(dir), &mut files);
        for path in files {
            if read(&path).contains("axiom_end_zone") {
                hits.push(path.display().to_string());
            }
        }
    }
    assert!(
        hits.is_empty(),
        "no layer/module may depend on end-zone:\n{}",
        hits.join("\n")
    );
}

#[test]
fn source_files_stay_narrowly_owned() {
    // The repo's slice-placement heuristic flags ≥300-line geometry-dense app
    // files; End Zone commits to staying under it everywhere.
    let mut over = Vec::new();
    for path in core_sources() {
        let lines = read(&path).lines().count();
        if lines >= 300 {
            over.push(format!("{}: {lines} lines", path.display()));
        }
    }
    assert!(
        over.is_empty(),
        "files must stay narrowly owned:\n{}",
        over.join("\n")
    );
}
