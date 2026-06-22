//! Architecture-boundary tests for the `axiom-sim-crucible` app.
//!
//! Domain scenario names (`cat`, `beer`, `paw`, `tavern`) are ALLOWED in this app
//! and its tests; they are forbidden from leaking into the reusable substrate
//! (`axiom-sim-core`, `axiom-ecs`). These tests also enforce app hygiene and the
//! "no planning-milestone names in structure" rule.

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

// ---------- app manifest ----------

#[test]
fn app_toml_exists_and_lists_only_consumed_layers_and_modules() {
    let manifest = app_root().join("app.toml");
    let text = fs::read_to_string(&manifest).expect("app.toml exists");
    assert!(text.contains("\"ecs\""), "app.toml lists the ecs layer");
    assert!(
        text.contains("\"sim-core\""),
        "app.toml lists the sim-core module"
    );
    // It must not claim browser/render/scene modules it does not use.
    for forbidden in ["\"scene\"", "\"render\"", "\"webgpu\"", "\"windowing\""] {
        assert!(
            !text.contains(forbidden),
            "app.toml must not list `{forbidden}`"
        );
    }
}

// ---------- app hygiene ----------

#[test]
fn no_browser_gpu_or_dom() {
    assert_absent_in_app(
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "wgpu",
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
        "the crucible is headless: no browser/GPU/DOM",
    );
}

#[test]
fn no_wall_clock_or_randomness() {
    assert_absent_in_app(
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
    assert_absent_in_app(
        &["todo!", "unimplemented!"],
        "no placeholder architecture in the crucible",
    );
}

#[test]
fn no_illegal_substrate_imports() {
    // The app may import only axiom-ecs and axiom-sim-core (+ std).
    let mut illegal = Vec::new();
    for path in app_sources() {
        for chunk in strip_comments_and_strings(&read(&path))
            .split(|c: char| !c.is_alphanumeric() && c != '_')
        {
            if chunk.starts_with("axiom_")
                && chunk != "axiom_ecs"
                && chunk != "axiom_sim_core"
                && chunk != "axiom_sim_crucible"
            {
                illegal.push(format!("{}: {}", path.display(), chunk));
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "app may only import axiom-ecs and axiom-sim-core:\n{}",
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

#[test]
fn no_phase_milestone_naming_in_structure() {
    // A phase is a planning milestone, not an engine concept. No source FILE may be
    // named after a phase, and no phase-numbered IDENTIFIER may appear in code.
    let mut violations = Vec::new();
    for path in app_sources() {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if stem.contains("phase") {
            violations.push(format!(
                "source file named after a phase: {}",
                path.display()
            ));
        }
        let stripped = strip_comments_and_strings(&read(&path)).to_ascii_lowercase();
        let bytes = stripped.as_bytes();
        for (index, _) in stripped.match_indices("phase") {
            let rest = &bytes[index + "phase".len()..];
            let direct_digit = rest.first().is_some_and(|c| c.is_ascii_digit());
            let separated_digit =
                rest.first() == Some(&b'_') && rest.get(1).is_some_and(|c| c.is_ascii_digit());
            if direct_digit || separated_digit {
                violations.push(format!(
                    "phase-milestone identifier in code: {}",
                    path.display()
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "a phase is a planning milestone, not an engine concept:\n{}",
        violations.join("\n")
    );
}

// ---------- the tick loop is data-driven ----------

#[test]
fn tick_loop_has_no_hardcoded_consequence_branches() {
    // The driver's `tick` function must be boring: step the scheduler, run the
    // actions due this tick, apply the boundary. It must NOT branch on a scenario
    // tick or inline the contact/grooming consequence (interaction/transfer/effect)
    // — those belong in scenario actions and the generic executor.
    let lib = read(&app_src().join("lib.rs"));
    let start = lib.find("fn tick(").expect("the driver has a tick fn");
    let rest = &lib[start..];
    let end = rest
        .find("fn execute(")
        .expect("the execute fn follows tick");
    // Compact whitespace so the scan is robust to rustfmt line-wrapping.
    let tick_body: String = rest[..end].chars().filter(|c| !c.is_whitespace()).collect();
    for needle in [
        "TICK_CONTACT",
        "TICK_GROOM",
        "record_surface_interaction",
        "apply_transfer",
        "apply_material_effects",
        "KIND_CONTACT",
        "KIND_GROOM",
        "KIND_INGESTION",
        "KIND_INTOX",
    ] {
        assert!(
            !tick_body.contains(needle),
            "the tick loop must be data-driven, but inlines the consequence: found `{needle}` in tick()"
        );
    }
    assert!(
        tick_body.contains("self.schedule"),
        "the tick loop must run the due actions from the schedule"
    );
}

// ---------- scenario names stay in the app ----------

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
fn scenario_domain_names_do_not_leak_into_reusable_substrate() {
    // The proof must NOT bake a `cat`/`beer`/`tavern` special case into the
    // substrate. `beer`/`tavern` are scenario-only tokens with no substring
    // collisions in the substrate, so a raw scan is a sound leak detector.
    // (`cat`/`paw` are deliberately not scanned — they collide with `catalog`
    // and `spawn` in legitimate substrate code.)
    let mut hits = scan_raw(
        repo_root()
            .join("modules")
            .join("axiom-sim-core")
            .join("src"),
        &["beer", "tavern"],
    );
    hits.extend(scan_raw(
        repo_root().join("crates").join("axiom-ecs").join("src"),
        &["beer", "tavern"],
    ));
    assert!(
        hits.is_empty(),
        "scenario domain names leaked into reusable substrate:\n{}",
        hits.join("\n")
    );
}

#[test]
fn no_layer_or_module_depends_on_this_app() {
    let mut hits = scan_raw(repo_root().join("crates"), &["axiom_sim_crucible"]);
    hits.extend(scan_raw(
        repo_root().join("modules"),
        &["axiom_sim_crucible"],
    ));
    assert!(
        hits.is_empty(),
        "no layer/module may depend on the crucible app:\n{}",
        hits.join("\n")
    );
}
