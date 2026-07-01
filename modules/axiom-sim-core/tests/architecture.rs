//! Architecture-boundary tests for the `axiom-sim-core` engine module.
//!
//! The workspace `xtask` checker enforces the global module law (allowed layers,
//! no module-to-module deps, single facade). These per-module tests scan this
//! crate's `src/` tree for forbidden tokens so internal regressions fail at
//! `cargo test` time.

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
        .expect("repo root is two levels above modules/axiom-sim-core")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("src directory must exist") {
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
    assert!(!files.is_empty(), "expected sim-core source files");
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
                    "axiom-sim-core {}: contains forbidden `{}`",
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
    assert!(
        manifest.is_file(),
        "expected modules/axiom-sim-core/module.toml"
    );
    let stripped = strip_comments_and_strings(&fs::read_to_string(&manifest).unwrap());
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-sim-core must declare `allowed_modules = []`"
    );
}

// Matches a whole `ids` path segment (not a substring like `fluids::`), mirroring
// the `xtask` gate's identity-vocabulary exemption.
fn is_id_vocabulary_export(line: &str) -> bool {
    line.starts_with("pub use ")
        && line["pub use ".len()..]
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|segment| segment == "ids")
}

#[test]
fn lib_rs_exports_one_facade_plus_identity_vocabulary() {
    let lib = read(&src_dir().join("lib.rs"));
    let public: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    let facades: Vec<&str> = public
        .iter()
        .copied()
        .filter(|line| !is_id_vocabulary_export(line))
        .collect();
    let id_vocab_lines = public
        .iter()
        .copied()
        .filter(|line| is_id_vocabulary_export(line))
        .count();
    assert_eq!(
        facades,
        vec!["pub use facade::SimCoreApi;"],
        "axiom-sim-core's lib.rs must publicly export exactly one behavioral facade: SimCoreApi"
    );
    assert_eq!(
        id_vocab_lines, 1,
        "axiom-sim-core re-exports its identity vocabulary via exactly one `pub use ids::{{…}}` line"
    );
}

#[test]
fn imports_only_legal_layers() {
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
                    && chunk != "axiom_ecs"
                    && chunk != "axiom_sim_core"
                    && chunk != "axiom_zones"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-sim-core may only import axiom-kernel and axiom-ecs:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn imports_no_other_modules() {
    let modules_dir = repo_root().join("modules");
    if !modules_dir.is_dir() {
        return;
    }
    let other_modules: Vec<String> = fs::read_dir(&modules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
        .filter(|name| name != "axiom_sim_core")
        .collect();
    let mut violations = Vec::new();
    for path in source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        let tokens: std::collections::HashSet<&str> = stripped
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .collect();
        for other in &other_modules {
            if tokens.contains(other.as_str()) {
                violations.push(format!(
                    "{}: references other module `{}`",
                    path.display(),
                    other
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "axiom-sim-core must not depend on any other module:\n{}",
        violations.join("\n")
    );
}

#[test]
fn imports_no_apps_or_tools() {
    let mut violations = Vec::new();
    for base in ["apps", "tools"] {
        let dir = repo_root().join(base);
        if !dir.is_dir() {
            continue;
        }
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
            .collect();
        for path in source_files() {
            let tokens: std::collections::HashSet<String> =
                strip_comments_and_strings(&read(&path))
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .map(str::to_string)
                    .collect();
            for name in &names {
                if tokens.contains(name) {
                    violations.push(format!("{}: references {base} `{}`", path.display(), name));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "axiom-sim-core must not depend on apps or tools:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_browser_gpu_or_dom() {
    assert_absent(
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "wasm-bindgen",
            "wgpu",
            "webgpu",
            "WebGpu",
            "WebGL",
            "webgl",
            "GPUDevice",
            "HtmlCanvas",
            "OffscreenCanvas",
            "document.",
            "window.",
            "navigator.",
        ],
        "axiom-sim-core must not reference browser/DOM/GPU APIs",
    );
}

#[test]
fn no_wall_clock_or_randomness() {
    assert_absent(
        &[
            "std::time",
            "SystemTime",
            "Instant::now",
            "chrono",
            "rand::",
            "thread_rng",
            "random()",
            "fastrand",
            "getrandom",
        ],
        "axiom-sim-core must be deterministic: no wall-clock time, no randomness",
    );
}

#[test]
fn no_console_or_placeholders() {
    assert_absent(
        &[
            "println!",
            "eprintln!",
            "print!",
            "eprint!",
            "dbg!",
            "todo!",
            "unimplemented!",
        ],
        "axiom-sim-core must contain no console output or placeholder macros",
    );
}

#[test]
fn no_global_mutable_state_or_file_io() {
    assert_absent(
        &["static mut", "lazy_static", "std::fs", "OpenOptions"],
        "axiom-sim-core must not use global mutable state or file IO",
    );
}

#[test]
fn no_domain_render_scene_physics_or_gameplay_concepts() {
    assert_absent(
        &[
            "winit",
            "rapier",
            "egui",
            "bevy",
            "::Mesh",
            "::Texture",
            "::Shader",
            "::RigidBody",
            "::Collider",
            "::Skeleton",
            "::Animator",
            "::SoundSource",
            "::KeyCode",
            "::MouseButton",
            "::SceneGraph",
            "::Renderable",
            "tavern",
            "grooming",
        ],
        "axiom-sim-core must own no render/scene/physics/animation/audio/input/gameplay concepts",
    );
}

#[test]
fn no_utils_or_helpers_modules() {
    for path in source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(
                name, banned,
                "axiom-sim-core must not have a `{banned}` module"
            );
        }
    }
}

#[test]
fn no_phase_milestone_naming_in_structure() {
    // No source file or identifier may be named after a planning phase (e.g.
    // `phase4`); comments/strings are stripped first, so doc prose is unaffected.
    let mut violations = Vec::new();
    for path in source_files() {
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
            let next = rest.first().copied();
            let separated_digit =
                next == Some(b'_') && rest.get(1).copied().is_some_and(|c| c.is_ascii_digit());
            let direct_digit = next.is_some_and(|c| c.is_ascii_digit());
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
