//! Architecture-boundary tests for the `axiom-draw2d` engine module.
//!
//! The workspace `xtask` checker enforces the global Module Law (allowed
//! layers, no module-to-module deps, single facade). These per-module tests are
//! the second line of defence: they scan this crate's `src/` tree for forbidden
//! tokens so module-internal regressions fail at `cargo test` time. Tests are
//! exempt from the Branchless Law, so this file uses ordinary control flow.

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
        .expect("repo root is two levels above modules/axiom-draw2d")
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
    assert!(!files.is_empty(), "expected draw2d source files");
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
                    "axiom-draw2d {}: contains forbidden `{}`",
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
    assert!(manifest.is_file(), "expected modules/axiom-draw2d/module.toml");
    let stripped = strip_comments_and_strings(&fs::read_to_string(&manifest).unwrap());
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-draw2d must declare `allowed_modules = []`"
    );
}

#[test]
fn lib_rs_exports_exactly_one_facade_plus_its_id_vocabulary() {
    // Module Law #8: exactly one behavioral facade (Draw2dApi), plus — and only —
    // the module's `ids` identity vocabulary (`pub use ids::{…}`). The 2D *draw
    // contract's* value vocabulary stays host-owned (relocated to axiom-host so
    // the render backends that depend on host can name it); callers reach it via
    // `use axiom_host::{…}`. The one module-owned vocabulary is the particle
    // surface's nouns (EmitterId handle + EmitterConfig recipe), which the facade
    // returns/accepts and a caller must be able to name — exactly the sanctioned
    // `ids` exemption — the SpriteAnimation flip-book recipe (§10.2) joins it for
    // the same reason (the sampler accepts it, so a caller must be able to name it).
    let lib = read(&src_dir().join("lib.rs"));
    let pub_items: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub use") || line.starts_with("pub "))
        .collect();
    assert_eq!(
        pub_items,
        vec![
            "pub use draw2d_api::Draw2dApi;",
            "pub use ids::{EmitterConfig, EmitterId, SpriteAnimation};",
        ],
        "axiom-draw2d's lib.rs must expose the Draw2dApi facade plus only its `ids` vocabulary"
    );
}

// ---------- legal layer imports only ----------

#[test]
fn draw2d_imports_only_legal_layers() {
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
                    && chunk != "axiom_host"
                    && chunk != "axiom_draw2d"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-draw2d may only import axiom-kernel, axiom-math, and axiom-host:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn draw2d_imports_no_other_modules() {
    let modules_dir = repo_root().join("modules");
    let other_modules: Vec<String> = fs::read_dir(&modules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
        .filter(|name| name != "axiom_draw2d")
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
        "axiom-draw2d must not depend on any other module:\n{}",
        violations.join("\n")
    );
}

// ---------- source hygiene ----------

#[test]
fn no_browser_or_js_bindgen_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "axiom-draw2d must not reference browser / JS bindings",
    );
}

#[test]
fn no_dom_canvas_or_browser_globals() {
    assert_absent(
        &["HtmlCanvas", "canvas", "requestAnimationFrame", "document.", "window."],
        "axiom-draw2d must not reference DOM/canvas/browser globals — rasterization is the backends' job",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "WebGpu", "WebGPU", "WebGL", "webgl", "GPUDevice"],
        "axiom-draw2d must not reference WebGPU/WebGL — it rasterizes nothing",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant", "chrono"],
        "axiom-draw2d must read no wall-clock time — presentation dt is passed in by the app",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "axiom-draw2d must use no randomness",
    );
}

#[test]
fn no_console_printing_or_placeholder_macros() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!", "todo!", "unimplemented!"],
        "axiom-draw2d must not print or contain placeholder architecture",
    );
}

#[test]
fn no_foreign_engine_subsystem_concepts() {
    // draw2d owns 2D-draw nouns; it must not absorb other subsystems' concepts.
    assert_absent(
        &[
            "axiom_scene",
            "axiom_render",
            "axiom_physics",
            "axiom_assets",
            "axiom_ecs",
            "RigidBody",
            "Collider",
            "Skeleton",
            "AudioSource",
            "KeyCode",
            "EditorPanel",
            "lyon",
            "wgpu",
        ],
        "axiom-draw2d must not absorb scene/render/physics/asset/input/audio/editor concepts or external rasterizers",
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc", "shared", "prelude"] {
            assert_ne!(name, banned, "axiom-draw2d must not have a `{banned}` module");
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

// ---------- presentation exclusion (structural) ----------

/// The whole surface is presentation class (SPEC-04 §6/§17.5): nothing it
/// produces may be read back into a sim-class API. Structurally, the facade's
/// *only* way to extract draw state is `finish(&mut self) -> Draw2dList`, which
/// consumes the frame and yields the neutral list. There must be **no immutable
/// (`&self`) getter that returns accumulated draw state** — the only read-only
/// methods are *pure functions of their arguments* that read no stored draw
/// state: `measure_text` (a metric of caller-supplied input) and `target_texture`
/// (the handle naming a render target's surface, a pure function of the id). In
/// particular there is **no** getter that returns particle or accumulated-command
/// state. This is the no-read-back proof.
#[test]
fn facade_has_no_sim_readable_draw_state_getter() {
    let src = strip_comments_and_strings(&read(&src_dir().join("draw2d_api.rs")));
    // Each `pub fn` signature runs from "pub fn" to the body's opening brace.
    let mut immutable_readers = Vec::new();
    for chunk in src.split("pub fn ").skip(1) {
        let sig_end = chunk.find('{').unwrap_or(chunk.len());
        let sig = &chunk[..sig_end];
        let name: String = chunk
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        // `&self` (immutable borrow) but not `&mut self`.
        if sig.contains("&self") {
            immutable_readers.push(name);
        }
    }
    immutable_readers.sort();
    assert_eq!(
        immutable_readers,
        vec!["measure_text".to_string(), "target_texture".to_string()],
        "the only immutable-self facade methods may be the pure `measure_text` and \
         `target_texture` (each a pure function of its arguments, reading no stored draw \
         state); any other `&self` getter would be a sim-readable read-back path into \
         presentation draw state"
    );
    // And the sole list producer consumes the frame via `&mut self`.
    assert!(
        src.contains("pub fn finish(&mut self) -> Draw2dList"),
        "finish must consume the frame (&mut self) and yield the neutral Draw2dList"
    );
}
