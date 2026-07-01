//! Architecture-boundary tests for the `axiom-agent` engine module.
//! The workspace `xtask` checker enforces the global Module Law (allowed layers,
//! no module-to-module deps, single facade). These per-module tests are the
//! second line of defence: they scan this crate's `src/` tree for forbidden
//! tokens so module-internal regressions fail at `cargo test` time. Tests are
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
        .expect("repo root is two levels above modules/axiom-agent")
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
    assert!(!files.is_empty(), "expected agent source files");
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("source must be valid UTF-8")
}

/// Strip `//` line comments and string/char literal contents so a token that
/// merely appears in prose or a message cannot mask or fabricate a violation.
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
                    "axiom-agent {}: contains forbidden `{}`",
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
    assert!(manifest.is_file(), "expected modules/axiom-agent/module.toml");
    let raw = fs::read_to_string(&manifest).unwrap();
    let stripped = strip_comments_and_strings(&raw);
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-agent must declare `allowed_modules = []`"
    );
    assert!(
        raw.contains("name = \"agent\""),
        "axiom-agent module must declare name = \"agent\""
    );
    assert!(
        raw.contains("kind = \"engine-module\""),
        "axiom-agent must be an engine-module"
    );
}

#[test]
fn lib_rs_exports_exactly_one_facade() {
    let lib = read(&src_dir().join("lib.rs"));
    let pub_uses: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub use") || line.starts_with("pub mod"))
        .collect();
    assert_eq!(
        pub_uses,
        vec!["pub use agent_api::AgentApi;"],
        "axiom-agent must expose exactly one public facade (AgentApi) and nothing else"
    );
    assert!(
        !lib.contains("ids::"),
        "axiom-agent exposes no identity-vocabulary line — only AgentApi"
    );
}


#[test]
fn agent_imports_only_legal_layers() {
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
                    && chunk != "axiom_runtime"
                    && chunk != "axiom_agent"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-agent may only import axiom-kernel and axiom-runtime:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn agent_imports_no_other_modules() {
    let modules_dir = repo_root().join("modules");
    let other_modules: Vec<String> = fs::read_dir(&modules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
        .filter(|name| name != "axiom_agent")
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
        "axiom-agent must not depend on any other module:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_layer_imports_axiom_agent() {
    for layer in ["axiom-kernel", "axiom-runtime", "axiom-math", "axiom-host", "axiom-frame", "axiom-ecs"] {
        let src = repo_root().join("crates").join(layer).join("src");
        assert_absent_in_other(
            src,
            layer,
            &["axiom_agent", "axiom-agent"],
            &format!("layer `{layer}` must not import axiom-agent"),
        );
    }
}


#[test]
fn no_browser_or_js_bindgen_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "axiom-agent must not reference browser / JS bindings",
    );
}

#[test]
fn no_dom_canvas_or_browser_globals() {
    assert_absent(
        &["HtmlCanvas", "canvas", "requestAnimationFrame", "document.", "window.", "navigator."],
        "axiom-agent must not reference DOM/canvas/browser globals",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "WebGpu", "WebGPU", "WebGL", "webgl", "GPUDevice"],
        "axiom-agent must not reference WebGPU/WebGL",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant", "chrono"],
        "axiom-agent must read no wall-clock time — only the explicit RuntimeStep tick",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "axiom-agent must use no randomness",
    );
}

#[test]
fn no_threads_or_async_runtimes() {
    assert_absent(
        &["thread::spawn", "tokio", "async_std", "std::net", "std::process"],
        "axiom-agent must not spawn threads, use async runtimes, or touch net/process",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-agent must not print to the console",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-agent must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static", "once_cell", "OnceLock"],
        "axiom-agent must use no global mutable state",
    );
}

#[test]
fn no_unordered_or_linked_collections() {
    assert_absent(
        &["HashMap", "HashSet", "BTreeMap", "BTreeSet", "LinkedList"],
        "axiom-agent state must use insertion-ordered Vec storage, never hash/btree/linked collections",
    );
}

#[test]
fn no_foreign_engine_subsystem_concepts() {
    assert_absent(
        &[
            "axiom_scene",
            "axiom_render",
            "axiom_resources",
            "axiom_physics",
            "axiom_input",
            "axiom_ecs",
            "axiom_math",
            "Renderable",
            "Mesh",
            "RigidBody",
            "Collider",
            "KeyCode",
            "EditorPanel",
            "rapier",
            "nalgebra",
            "glam",
            "bevy",
        ],
        "axiom-agent must not absorb scene/render/physics/input/ecs concepts or external engines",
    );
}

#[test]
fn no_prohibited_ai_concepts() {
    assert_absent(
        &[
            "navmesh",
            "pathfind",
            "Pathfind",
            "BehaviorTree",
            "behavior_tree",
            "UtilityAi",
            "utility_ai",
            "Planner",
            "planner",
            "neural",
            "Neural",
            "tensor",
            "Tensor",
            "Llm",
            "llm",
        ],
        "axiom-agent must contain no pathfinding/navmesh/behavior-tree/utility-AI/planner/ML/neural/LLM concepts",
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc", "shared", "prelude"] {
            assert_ne!(name, banned, "axiom-agent must not have a `{banned}` module");
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
