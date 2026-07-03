//! Architecture-boundary tests for the `axiom-animation` engine module.
//!
//! The workspace `xtask` checker enforces the global Module Law (allowed layers,
//! no module-to-module deps, single facade). These per-module tests are the
//! second line of defence: they scan this crate's `src/` tree for forbidden
//! tokens so a module-internal regression fails at `cargo test` time. Tests are
//! exempt from the Branchless Law, so this file uses ordinary control flow.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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
        .expect("repo root is two levels above modules/axiom-animation")
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
    assert!(!files.is_empty(), "expected animation source files");
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("source must be valid UTF-8")
}

/// Strip `//` line comments and string/char literals so a token that appears
/// only in prose (e.g. a "kicker" example in a doc comment) never trips a scan.
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
                    "axiom-animation {}: contains forbidden `{}`",
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
                violations.push(format!("{label} {}: contains `{}`", path.display(), needle));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

#[test]
fn module_toml_exists_and_is_isolated() {
    let manifest = module_root().join("module.toml");
    assert!(manifest.is_file(), "expected modules/axiom-animation/module.toml");
    let stripped = strip_comments_and_strings(&fs::read_to_string(&manifest).unwrap());
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-animation must declare `allowed_modules = []`"
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
        vec!["pub use animation_api::AnimationApi;"],
        "axiom-animation must expose exactly one behavioral facade (AnimationApi)"
    );
    let id_lines = pub_uses.iter().filter(|line| line.contains("ids::")).count();
    assert_eq!(
        id_lines, 1,
        "axiom-animation re-exports its identity vocabulary via exactly one `pub use ids::{{…}}` line"
    );
}

#[test]
fn animation_imports_only_kernel_and_math() {
    let mut illegal = Vec::new();
    for path in source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for line in stripped.lines() {
            let trimmed = line.trim();
            if !trimmed.contains("axiom_") {
                continue;
            }
            for chunk in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if chunk.starts_with("axiom_")
                    && chunk != "axiom_kernel"
                    && chunk != "axiom_math"
                    && chunk != "axiom_animation"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-animation may only import axiom-kernel and axiom-math:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn animation_imports_no_other_modules() {
    let modules_dir = repo_root().join("modules");
    let other_modules: Vec<String> = fs::read_dir(&modules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
        .filter(|name| name != "axiom_animation")
        .collect();
    assert!(!other_modules.is_empty(), "expected sibling modules to exist");
    let mut violations = Vec::new();
    for path in source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        let tokens: HashSet<&str> = stripped
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
        "axiom-animation must not depend on any other module:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_layer_imports_axiom_animation() {
    for layer in ["axiom-kernel", "axiom-math", "axiom-runtime", "axiom-frame", "axiom-host"] {
        let src = repo_root().join("crates").join(layer).join("src");
        assert_absent_in_other(
            src,
            layer,
            &["axiom_animation", "axiom-animation"],
            &format!("layer `{layer}` must not import axiom-animation"),
        );
    }
}

#[test]
fn no_browser_gpu_or_platform_apis() {
    assert_absent(
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "wgpu",
            "WebGpu",
            "WebGPU",
            "WebGL",
            "canvas",
            "requestAnimationFrame",
            "document.",
            "window.",
        ],
        "axiom-animation must reference no browser / GPU / platform APIs",
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
            "random()",
            "getrandom",
        ],
        "axiom-animation must read no wall-clock time and use no randomness",
    );
}

#[test]
fn no_threads_console_or_placeholders() {
    assert_absent(
        &[
            "thread::spawn",
            "tokio",
            "println!",
            "eprintln!",
            "print!",
            "eprint!",
            "dbg!",
            "todo!",
            "unimplemented!",
        ],
        "axiom-animation must not spawn threads, print, or contain placeholder macros",
    );
}

#[test]
fn no_global_mutable_state_or_unordered_collections() {
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
            "LinkedList",
        ],
        "axiom-animation must use no global mutable state and only insertion-ordered Vec storage",
    );
}

#[test]
fn no_foreign_subsystem_or_gameplay_concepts() {
    // Animation legitimately owns Skeleton / Bone / Pose / Clip nouns. It must
    // not absorb the concepts owned by *other* subsystems, reference foreign
    // engine crates, or carry any game/domain (meaning) vocabulary.
    assert_absent(
        &[
            // other engine subsystems (mechanism owned elsewhere)
            "axiom_scene",
            "axiom_render",
            "axiom_resources",
            "axiom_physics",
            "axiom_input",
            "axiom_audio",
            "Renderable",
            "RigidBody",
            "Collider",
            "AudioSource",
            "KeyCode",
            "EditorPanel",
            // external animation/physics engines
            "rapier",
            "nalgebra",
            "glam",
            "bevy",
            // gameplay / domain meaning — must never leak into a core mechanism
            "soccer",
            "forest",
            "kicker",
            "goalie",
            "enemy",
            "quest",
            "inventory",
            "weapon",
        ],
        "axiom-animation must own only animation mechanism — no foreign subsystem, \
         external engine, or gameplay/domain concept",
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc", "shared", "prelude"] {
            assert_ne!(name, banned, "axiom-animation must not have a `{banned}` module");
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
