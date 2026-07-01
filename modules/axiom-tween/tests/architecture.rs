//! Architecture-boundary tests for the `axiom-tween` engine module.
//! The workspace `xtask` checker enforces the global Module Law; these per-module
//! tests are the second line of defence, scanning this crate's `src/` tree for
//! forbidden tokens so module-internal regressions fail at `cargo test` time.
//! Tests are exempt from the Branchless Law, so this file uses ordinary control
//! flow.

use std::fs;
use std::path::{Path, PathBuf};

use axiom_tween::{Ease, TweenApi, TweenSpec, TweenValue};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn module_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("repo root is two levels above modules/axiom-tween")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("directory must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&src_dir(), &mut files);
    assert!(!files.is_empty(), "expected tween source files");
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

fn assert_absent(forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "axiom-tween {}: contains forbidden `{}`",
                    path.display(),
                    needle
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

fn assert_absent_in_other(dir: PathBuf, label: &str, forbidden: &[&str], why: &str) {
    let mut files = Vec::new();
    if dir.is_dir() {
        collect_rs(&dir, &mut files);
    }
    files.sort();
    let mut violations = Vec::new();
    for path in &files {
        let stripped = strip_comments_and_strings(&read(path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "{label} {}: contains forbidden `{}`",
                    path.display(),
                    needle
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}


#[test]
fn module_toml_exists_and_is_isolated() {
    let manifest = module_root().join("module.toml");
    assert!(manifest.is_file(), "expected modules/axiom-tween/module.toml");
    let stripped = strip_comments_and_strings(&fs::read_to_string(&manifest).unwrap());
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-tween must declare `allowed_modules = []`"
    );
}

#[test]
fn lib_rs_exports_one_facade_plus_identity_vocabulary() {
    let lib = read(&src_dir().join("lib.rs"));
    let pub_uses: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub use"))
        .collect();
    let facades: Vec<&str> = pub_uses
        .iter()
        .copied()
        .filter(|line| !line.contains("ids::"))
        .collect();
    assert_eq!(
        facades,
        vec!["pub use tween_api::TweenApi;"],
        "axiom-tween must expose exactly one behavioral facade (TweenApi)"
    );
    let id_lines = pub_uses.iter().filter(|line| line.contains("ids::")).count();
    assert_eq!(
        id_lines, 1,
        "axiom-tween re-exports its value vocabulary via exactly one `pub use ids::{{…}}` line"
    );
}


#[test]
fn tween_imports_only_the_kernel() {
    let mut illegal = Vec::new();
    for path in source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for line in stripped.lines() {
            let trimmed = line.trim();
            if !trimmed.contains("axiom_") {
                continue;
            }
            for chunk in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if chunk.starts_with("axiom_") && chunk != "axiom_kernel" && chunk != "axiom_tween" {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-tween may only import axiom-kernel:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn tween_imports_no_other_modules() {
    let modules_dir = repo_root().join("modules");
    let other_modules: Vec<String> = fs::read_dir(&modules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
        .filter(|name| name != "axiom_tween")
        .collect();
    assert!(!other_modules.is_empty(), "expected sibling modules to exist");
    let mut violations = Vec::new();
    for path in source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        let tokens: std::collections::HashSet<&str> = stripped
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .collect();
        for other in &other_modules {
            if tokens.contains(other.as_str()) {
                violations.push(format!("{}: references other module `{}`", path.display(), other));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "axiom-tween must not depend on any other module:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_layer_imports_axiom_tween() {
    for layer in ["axiom-kernel", "axiom-math", "axiom-interface", "axiom-frame"] {
        let src = repo_root().join("crates").join(layer).join("src");
        assert_absent_in_other(
            src,
            layer,
            &["axiom_tween", "axiom-tween"],
            &format!("layer `{layer}` must not import axiom-tween"),
        );
    }
}


#[test]
fn no_browser_platform_or_renderer_apis() {
    assert_absent(
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "wasm-bindgen",
            "HtmlCanvas",
            "canvas",
            "requestAnimationFrame",
            "document.",
            "window.",
            "wgpu",
            "WebGPU",
            "WebGL",
        ],
        "axiom-tween is a pure arithmetic core — no browser, platform, or renderer APIs",
    );
}

#[test]
fn no_wall_clock_time_or_randomness() {
    assert_absent(
        &[
            "std::time",
            "SystemTime",
            "Instant",
            "chrono",
            "rand::",
            "thread_rng",
            "getrandom",
        ],
        "axiom-tween advances on a fed-in elapsed interval — no wall clock, no randomness",
    );
}

#[test]
fn no_threads_async_or_console() {
    assert_absent(
        &[
            "thread::spawn",
            "tokio",
            "async_std",
            "std::net",
            "std::process",
            "println!",
            "eprintln!",
            "print!",
            "eprint!",
            "dbg!",
            "todo!",
            "unimplemented!",
        ],
        "axiom-tween must not spawn threads, use async/net/process, print, or stub",
    );
}

#[test]
fn no_global_mutable_or_nondeterministic_collections() {
    assert_absent(
        &[
            "static mut",
            "lazy_static",
            "once_cell",
            "OnceLock",
            "HashMap",
            "HashSet",
            "BTreeMap",
            "BTreeSet",
        ],
        "axiom-tween must use no global mutable state and no map collections — an ordered Vec table only",
    );
}

#[test]
fn no_ui_or_foreign_subsystem_concepts() {
    assert_absent(
        &[
            "axiom_interface",
            "axiom_scene",
            "axiom_render",
            "Panel",
            "Widget",
            "DrawList",
            "Renderable",
        ],
        "axiom-tween must own no UI or rendering concepts — it animates a number, nothing more",
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc", "shared", "prelude"] {
            assert_ne!(name, banned, "axiom-tween must not have a `{banned}` module");
        }
    }
}

#[test]
fn every_source_module_is_declared_in_lib_rs() {
    let lib = strip_comments_and_strings(&read(&src_dir().join("lib.rs")));
    let mut missing = Vec::new();
    for entry in fs::read_dir(src_dir()).expect("src dir must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem == "lib" {
            continue;
        }
        let decl = format!("mod {stem};");
        if !lib.contains(&decl) {
            missing.push(format!("{stem} (expected `{decl}` in lib.rs)"));
        }
    }
    assert!(
        missing.is_empty(),
        "every src/*.rs file must be declared in lib.rs — orphan modules:\n{}",
        missing.join("\n")
    );
}


#[test]
fn identical_tweens_replay_to_identical_samples() {
    let run = || {
        let mut api = TweenApi::new();
        api.start(TweenSpec {
            from: TweenValue::new(0.0),
            to: TweenValue::new(100.0),
            duration_nanos: 1000,
            ease: Ease::QuadInOut,
        });
        [200u64, 200, 200, 200, 200]
            .into_iter()
            .map(|step| api.advance(step).first().map(|s| s.value.get()))
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}
