//! Architecture-boundary hygiene for the `axiom-bend-it` app: the manifest
//! matches what is consumed, the deterministic core is browser-free and
//! wall-clock-free, no placeholder macros or junk-drawer modules exist, and no
//! engine layer or module depends on this composition leaf.

use std::fs;
use std::path::{Path, PathBuf};

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

/// Strip `//` comments and string/char literals, so a word inside a doc comment
/// can neither fabricate nor mask a violation.
fn strip(text: &str) -> String {
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

/// Everything except the sanctioned wasm edge (`src/web/`).
fn core_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&app_root().join("src"), &mut files);
    files.retain(|p| !p.components().any(|c| c.as_os_str() == "web"));
    assert!(files.len() > 20, "the app has its full module tree");
    files.sort();
    files
}

fn assert_absent_in_core(forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in core_sources() {
        let stripped = strip(&read(&path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!("{}: contains `{}`", path.display(), needle));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

#[test]
fn the_manifest_lists_exactly_what_is_consumed() {
    let text = fs::read_to_string(app_root().join("app.toml")).expect("app.toml exists");
    for required in [
        "\"kernel\"", "\"math\"", "\"host\"", "\"runtime\"", "\"engine\"", "\"figure\"",
        "\"input\"", "\"agent\"", "\"windowing\"", "\"debug-overlay\"",
    ] {
        assert!(text.contains(required), "app.toml must list {required}");
    }
    // It must not claim capabilities it does not use.
    for forbidden in ["\"physics\"", "\"scene\"", "\"render\"", "\"webgpu\"", "\"animation\""] {
        assert!(!text.contains(forbidden), "app.toml must not list {forbidden}");
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
            "HtmlCanvas",
            "OffscreenCanvas",
            "document.",
            "navigator.",
        ],
        "everything outside src/web/ is headless",
    );
}

#[test]
fn the_deterministic_core_has_no_wall_clock_and_no_ambient_randomness() {
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
        "the game is a pure function of its commands: no clock, no ambient randomness",
    );
}

#[test]
fn there_are_no_placeholders_and_no_console_output() {
    assert_absent_in_core(
        &["todo!", "unimplemented!", "dbg!", "println!", "eprintln!"],
        "no placeholders or console output in the game core",
    );
}

#[test]
fn there_are_no_junk_drawer_modules() {
    for path in core_sources() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(name, banned, "no `{banned}` module in bend-it");
        }
    }
}

#[test]
fn only_the_declared_engine_crates_are_imported() {
    let allowed = [
        "axiom",
        "axiom_kernel",
        "axiom_math",
        "axiom_figure",
        "axiom_input",
        "axiom_agent",
        "axiom_runtime",
        "axiom_host",
        "axiom_windowing",
        "axiom_debug_overlay",
        "axiom_bend_it",
    ];
    let mut files = Vec::new();
    collect_rs(&app_root().join("src"), &mut files);
    let mut illegal = Vec::new();
    for path in files {
        for chunk in strip(&read(&path)).split(|c: char| !c.is_alphanumeric() && c != '_') {
            if (chunk.starts_with("axiom_") || chunk == "axiom") && !allowed.contains(&chunk) {
                illegal.push(format!("{}: {}", path.display(), chunk));
            }
        }
    }
    assert!(illegal.is_empty(), "undeclared engine import:\n{}", illegal.join("\n"));
}

#[test]
fn no_layer_or_module_depends_on_this_app() {
    let mut hits = Vec::new();
    for dir in ["crates", "modules"] {
        let mut files = Vec::new();
        collect_rs(&repo_root().join(dir), &mut files);
        for path in files {
            if read(&path).contains("axiom_bend_it") {
                hits.push(path.display().to_string());
            }
        }
    }
    assert!(
        hits.is_empty(),
        "no layer/module may depend on a composition leaf:\n{}",
        hits.join("\n")
    );
}

/// The one file measured against a different bar, and why.
///
/// `tuning.rs` exists to be *the* place every gameplay number lives. Splitting
/// it to satisfy a line count would defeat the only reason it exists and leave a
/// reader hunting two files for "what can I change". It is a declaration table
/// with a documented field per number, not a system, so the ownership heuristic
/// below tells us nothing useful about it. This is a deliberate, named exception
/// — not a growing list.
const DECLARATION_TABLES: [&str; 1] = ["tuning"];

#[test]
fn source_files_stay_narrowly_owned() {
    // The repo's slice-placement heuristic flags ≥300-line app files. Measured
    // on PRODUCTION lines: the rule is about how much behaviour one file owns,
    // and counting a file's own tests against it would put a price on testing it
    // thoroughly.
    let mut over = Vec::new();
    for path in core_sources() {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if DECLARATION_TABLES.contains(&name.as_str()) {
            continue;
        }
        let text = read(&path);
        let lines = text
            .split("#[cfg(test)]")
            .next()
            .unwrap_or("")
            .lines()
            .count();
        if lines >= 300 {
            over.push(format!("{}: {lines} production lines", path.display()));
        }
    }
    assert!(over.is_empty(), "files must stay narrowly owned:\n{}", over.join("\n"));
}

#[test]
fn the_drawing_layer_can_only_produce_a_shot_intent() {
    // The seam the whole design rests on: reading a drawing yields a ShotIntent
    // and nothing else. If it ever learns to build a trajectory, move the ball,
    // or step the session, the promise that the ball goes where you drew stops
    // being structural and becomes a habit.
    let mut files = Vec::new();
    collect_rs(&app_root().join("src").join("stroke"), &mut files);
    let mut leaks = Vec::new();
    for path in files {
        let text = strip(&read(&path));
        let production = text.split("#[cfg(test)]").next().unwrap_or("");
        for needle in ["Trajectory", "ResolvedShot", "Ball", "Keeper", "session.step"] {
            if production.contains(needle) {
                leaks.push(format!("{}: reaches for `{}`", path.display(), needle));
            }
        }
    }
    assert!(
        leaks.is_empty(),
        "the drawing layer may only produce a ShotIntent:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn the_same_drawing_is_read_the_same_way_every_time() {
    // "The same swipe always produces the same kick" is a promise about the
    // reading, so nothing in it may consult a clock, a random source, or an
    // iteration order that is not its own.
    let mut files = Vec::new();
    collect_rs(&app_root().join("src").join("stroke"), &mut files);
    for path in files {
        let text = strip(&read(&path));
        for needle in ["HashMap", "HashSet", "rand", "Instant", "SystemTime"] {
            assert!(
                !text.contains(needle),
                "{}: the reading must be deterministic, found `{}`",
                path.display(),
                needle
            );
        }
    }
}

#[test]
fn nothing_downstream_of_the_trajectory_can_rewrite_it() {
    // `shot::trajectory` is the single producer of the authored path. Ball
    // flight and the keeper may READ it; neither may construct one, because a
    // second producer is exactly how "the ball follows what you drew" quietly
    // becomes "the ball follows what you drew, mostly".
    for file in ["ball.rs", "keeper.rs", "keeper_read.rs"] {
        let path = app_root().join("src").join("play").join(file);
        let text = strip(&read(&path));
        let production = text.split("#[cfg(test)]").next().unwrap_or("");
        assert!(
            !production.contains("Trajectory::build")
                && !production.contains("ResolvedShot::build"),
            "{file} builds a trajectory of its own"
        );
    }
}
