//! Architecture-boundary tests for the `axiom-physics` engine module.
//!
//! The workspace `xtask` checker enforces the global Module Law (allowed
//! layers, no module-to-module deps, single facade). These per-module tests are
//! the second line of defence: they scan this crate's `src/` tree for forbidden
//! tokens so module-internal regressions fail at `cargo test` time. Tests are
//! exempt from the Branchless Law, so this file uses ordinary control flow.

use std::fs;
use std::path::{Path, PathBuf};

// Facade-only imports for the determinism/ordering checks below. Like
// `tests/integration.rs`, these tests drive the module solely through its public
// facade plus the regular kernel/math/runtime value types that cross it; the
// rich return types (snapshot, material) stay sealed and are never named.
use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::PhysicsApi;
use axiom_runtime::RuntimeStep;

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
        .expect("repo root is two levels above modules/axiom-physics")
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
    assert!(!files.is_empty(), "expected physics source files");
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
                    "axiom-physics {}: contains forbidden `{}`",
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

// ---------- manifest + facade ----------

#[test]
fn module_toml_exists_and_is_isolated() {
    let manifest = module_root().join("module.toml");
    assert!(
        manifest.is_file(),
        "expected modules/axiom-physics/module.toml"
    );
    let stripped = strip_comments_and_strings(&fs::read_to_string(&manifest).unwrap());
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-physics must declare `allowed_modules = []`"
    );
}

#[test]
fn lib_rs_exports_one_facade_plus_identity_vocabulary() {
    // Module Law #8: exactly one behavioral facade (PhysicsApi), plus the
    // identity vocabulary (the handle types). All other public exports forbidden.
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
        vec!["pub use physics_api::PhysicsApi;"],
        "axiom-physics must expose exactly one behavioral facade (PhysicsApi)"
    );
    let id_lines = pub_uses
        .iter()
        .filter(|line| line.contains("ids::"))
        .count();
    assert_eq!(
        id_lines, 1,
        "axiom-physics re-exports its identity vocabulary via exactly one `pub use ids::{{…}}` line"
    );
}

// ---------- legal layer imports only ----------

#[test]
fn physics_imports_only_legal_layers() {
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
                    && chunk != "axiom_math"
                    && chunk != "axiom_physics"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-physics may only import axiom-kernel, axiom-runtime, axiom-math:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn physics_imports_no_other_modules() {
    let modules_dir = repo_root().join("modules");
    let other_modules: Vec<String> = fs::read_dir(&modules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
        .filter(|name| name != "axiom_physics")
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
        "axiom-physics must not depend on any other module:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_layer_imports_axiom_physics() {
    for layer in [
        "axiom-kernel",
        "axiom-runtime",
        "axiom-math",
        "axiom-host",
        "axiom-frame",
        "axiom-ecs",
    ] {
        let src = repo_root().join("crates").join(layer).join("src");
        assert_absent_in_other(
            src,
            layer,
            &["axiom_physics", "axiom-physics"],
            &format!("layer `{layer}` must not import axiom-physics"),
        );
    }
}

// ---------- source hygiene: platform / determinism / foreign concepts ----------

#[test]
fn no_browser_or_js_bindgen_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "axiom-physics must not reference browser / JS bindings",
    );
}

#[test]
fn no_dom_canvas_or_browser_globals() {
    assert_absent(
        &[
            "HtmlCanvas",
            "canvas",
            "requestAnimationFrame",
            "document.",
            "window.",
            "navigator.",
        ],
        "axiom-physics must not reference DOM/canvas/browser globals",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "WebGpu", "WebGPU", "WebGL", "webgl", "GPUDevice"],
        "axiom-physics must not reference WebGPU/WebGL",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant", "chrono"],
        "axiom-physics must read no wall-clock time — only the explicit fixed step",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "axiom-physics must use no randomness",
    );
}

#[test]
fn no_threads_or_async_runtimes() {
    assert_absent(
        &["thread::spawn", "tokio", "async_std", "std::net", "std::process"],
        "axiom-physics must not spawn threads, use async runtimes, or touch net/process",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-physics must not print to the console",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-physics must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static", "once_cell", "OnceLock"],
        "axiom-physics must use no global mutable state",
    );
}

#[test]
fn no_nondeterministic_hash_collections() {
    assert_absent(
        &["HashMap", "HashSet"],
        "axiom-physics state must use ordered Vec storage, never hash iteration",
    );
}

#[test]
fn no_foreign_engine_subsystem_concepts() {
    // Physics legitimately owns RigidBody/Collider nouns; it must not absorb the
    // concepts owned by *other* subsystems (scene/render/asset/input/animation/
    // audio/plugin/editor), nor reference foreign engine crates.
    assert_absent(
        &[
            "axiom_scene",
            "axiom_render",
            "axiom_assets",
            "axiom_input",
            "axiom_ecs",
            "Renderable",
            "Mesh",
            "Animator",
            "Skeleton",
            "AudioSource",
            "KeyCode",
            "EditorPanel",
            "rapier",
            "nalgebra",
            "glam",
            "bevy",
        ],
        "axiom-physics must not absorb scene/render/asset/input/animation/audio/plugin/editor concepts or external engines",
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc", "shared", "prelude"] {
            assert_ne!(name, banned, "axiom-physics must not have a `{banned}` module");
        }
    }
}

/// Deterministic state must use insertion-ordered `Vec` storage. The existing
/// `HashMap`/`HashSet` scan bans hash iteration; this widens the net to the
/// *ordered* associative/linked collections too. A `BTreeMap`/`BTreeSet` would
/// iterate in key order — which silently reorders relative to insertion and so
/// hides the canonical handle-allocation order the snapshots depend on — and a
/// `LinkedList` is a non-contiguous, replay-fragile container. None belong in a
/// module whose snapshots must be byte-identical across runs.
#[test]
fn no_btreemap_or_other_unordered_or_linked_collections() {
    assert_absent(
        &["BTreeMap", "BTreeSet", "LinkedList"],
        "axiom-physics state must use insertion-ordered Vec storage, never \
         key-ordered BTree* or a LinkedList",
    );
}

/// Guard against an orphan source module: a `src/*.rs` file added to the tree
/// but never wired into `lib.rs`. Every top-level source file (other than
/// `lib.rs` itself) must have a matching `mod <stem>;` declaration in `lib.rs`,
/// so the public facade is the genuine single entry point to the whole crate.
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

// ---------- facade-level determinism / ordering proofs ----------

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn meters(v: f32) -> Meters {
    Meters::new(v).unwrap()
}

/// `overlap_sphere` must return its overlapped bodies in strictly ascending
/// handle order with no duplicates: a deterministic, canonical result regardless
/// of how the colliders were inserted. Three static bodies at distinct positions,
/// each given a sphere collider, all overlapped by one wide query sphere, must
/// come back sorted and unique.
#[test]
fn overlap_sphere_results_are_sorted_and_unique() {
    let mut api = PhysicsApi::new();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    for x in [0.0_f32, 2.0, 4.0] {
        let body = api
            .create_static_body(Transform::from_translation(Vec3::new(x, 0.0, 0.0)))
            .unwrap();
        api.attach_sphere_collider(body, meters(1.0), material, false)
            .unwrap();
    }
    // A wide query sphere centred among the three overlaps all of them.
    let hits = api.overlap_sphere(Vec3::new(2.0, 0.0, 0.0), meters(20.0));
    assert_eq!(hits.len(), 3, "all three collidered bodies must be found");
    assert!(
        hits.windows(2).all(|p| p[0] < p[1]),
        "overlap_sphere results must be strictly ascending (sorted + de-duplicated): {hits:?}"
    );
}

/// Two independently-built worlds, fed the identical scene and the identical
/// sequence of fixed steps, must produce equal snapshots — the core replay
/// invariant, asserted here through the public facade with a `RuntimeStep`
/// constructed exactly as a production caller would.
#[test]
fn identical_worlds_replay_to_identical_snapshots() {
    fn fixed_step() -> RuntimeStep {
        RuntimeStep::new(FrameIndex::new(0), Tick::new(0), 16_666_667, 0)
    }
    let run = || {
        let mut api = PhysicsApi::new();
        let ground = api.create_static_body(Transform::IDENTITY).unwrap();
        let ball = api
            .create_dynamic_body(
                Transform::from_translation(Vec3::new(0.0, 5.0, 0.0)),
                ratio(1.0),
            )
            .unwrap();
        let material = PhysicsApi::material(ratio(0.5), ratio(0.3), ratio(1.0)).unwrap();
        api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), material, false)
            .unwrap();
        api.attach_sphere_collider(ball, meters(0.5), material, false)
            .unwrap();
        api.apply_force(ball, Vec3::new(1.0, 0.0, 0.5)).unwrap();
        for _ in 0..120 {
            api.step(fixed_step()).unwrap();
        }
        api.snapshot()
    };
    assert_eq!(
        run(),
        run(),
        "two identical worlds + identical steps must replay to equal snapshots"
    );
}
