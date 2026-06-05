//! Architecture-boundary tests for the `axiom-scene` engine module.
//!
//! The workspace's `xtask` checker enforces the global module law
//! (allowed layers, no module-to-module deps, etc.). These per-module
//! tests are the second line of defence: they scan this crate's source
//! tree for forbidden tokens so module-internal regressions fail the
//! build at `cargo test` time.

use std::fs;
use std::path::{Path, PathBuf};

fn scene_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn scene_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("repo root is two levels above modules/axiom-scene")
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

fn scene_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&scene_src_dir(), &mut files);
    assert!(!files.is_empty(), "expected scene source files");
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
    for path in scene_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "axiom-scene {}: contains forbidden `{}`",
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

// ---------- manifest presence ----------

#[test]
fn module_toml_exists() {
    let manifest = scene_root().join("module.toml");
    assert!(
        manifest.is_file(),
        "expected modules/axiom-scene/module.toml to exist"
    );
}

#[test]
fn module_toml_has_empty_allowed_modules() {
    let manifest = scene_root().join("module.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    // Look for the literal empty-list form, comment-stripped.
    let stripped = strip_comments_and_strings(&text);
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-scene's module.toml must declare `allowed_modules = []`"
    );
}

#[test]
fn lib_rs_exports_only_scene_api() {
    let lib = read(&scene_src_dir().join("lib.rs"));
    let actual: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    assert_eq!(
        actual,
        vec!["pub use scene_api::SceneApi;"],
        "axiom-scene's lib.rs must publicly export exactly one item: SceneApi"
    );
}

// ---------- legal layer imports only ----------

#[test]
fn scene_imports_only_legal_layers() {
    let mut illegal = Vec::new();
    for path in scene_source_files() {
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
                    && chunk != "axiom_frame"
                    // The scene is an ECS world (Layer 05): nodes are entities,
                    // node facts are component columns.
                    && chunk != "axiom_ecs"
                    // axiom_host appears only in #[cfg(test)] blocks because it
                    // is a dev-dependency; allow it here too.
                    && chunk != "axiom_host"
                    && chunk != "axiom_scene"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-scene may only import axiom-kernel, axiom-runtime, axiom-math, axiom-frame, \
         axiom-ecs (and axiom-host as a dev-dependency in tests):\n{}",
        illegal.join("\n")
    );
}

#[test]
fn scene_imports_no_other_modules() {
    // Modules live under modules/; reject any reference to another
    // module's crate name. Today there are no other modules to refer to,
    // so this scan is a tripwire for when one is added.
    let modules_dir = repo_root().join("modules");
    if !modules_dir.is_dir() {
        return;
    }
    let other_modules: Vec<String> = fs::read_dir(&modules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
        .filter(|name| name != "axiom_scene")
        .collect();
    if other_modules.is_empty() {
        return;
    }
    let mut violations = Vec::new();
    for path in scene_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        // Match whole import-prefix identifiers, not raw substrings: a module's
        // crate prefix (`axiom_render`, or the umbrella's bare `axiom`) is a
        // distinct token. A substring check would flag legal layer imports like
        // `axiom_math` as references to the `axiom` umbrella — the same
        // identifier-token split the legal-layers test above relies on.
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
        "axiom-scene must not depend on any other module:\n{}",
        violations.join("\n")
    );
}

// ---------- lower layers must not import scene ----------

#[test]
fn no_layer_imports_axiom_scene() {
    for layer in [
        "axiom-kernel",
        "axiom-runtime",
        "axiom-math",
        "axiom-host",
        "axiom-frame",
    ] {
        let src = repo_root().join("crates").join(layer).join("src");
        assert_absent_in_other(
            src,
            layer,
            &["axiom_scene", "axiom-scene"],
            &format!("layer `{layer}` must not import axiom-scene"),
        );
    }
}

#[test]
fn no_app_imports_axiom_scene_unless_app_manifest_allows_it() {
    // For every app under `apps/`, its `app.toml` MUST list `"scene"` in
    // `allowed_modules` if its source imports `axiom_scene`. The xtask
    // checker enforces this with `AppDependsOnModuleNotAllowed`; this is
    // a fast in-crate sanity check.
    let apps_dir = repo_root().join("apps");
    if !apps_dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(&apps_dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_dir() {
            continue;
        }
        let app_src = path.join("src");
        if !app_src.is_dir() {
            continue;
        }
        let mut imports_scene = false;
        let mut sources = Vec::new();
        collect_rs(&app_src, &mut sources);
        for src in &sources {
            let stripped = strip_comments_and_strings(&read(src));
            if stripped.contains("axiom_scene") {
                imports_scene = true;
                break;
            }
        }
        if !imports_scene {
            continue;
        }
        let app_manifest = path.join("app.toml");
        let manifest_text = fs::read_to_string(&app_manifest).unwrap_or_default();
        // Do not strip strings here — the manifest's `allowed_modules`
        // list IS strings. Just drop `#` line comments.
        let no_comments: String = manifest_text
            .lines()
            .map(|l| match l.find('#') {
                Some(i) => &l[..i],
                None => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            no_comments.contains("\"scene\""),
            "app `{}` imports axiom_scene but its app.toml does not list \"scene\" in \
             allowed_modules",
            path.display()
        );
    }
}

// ---------- source hygiene ----------

#[test]
fn no_browser_or_js_bindgen_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "axiom-scene must not reference browser / JS bindings",
    );
}

#[test]
fn no_dom_canvas_or_browser_globals() {
    assert_absent(
        &[
            "HtmlCanvas",
            "HtmlElement",
            "OffscreenCanvas",
            "document.",
            "window.",
            "navigator.",
        ],
        "axiom-scene must not reference DOM/canvas/browser globals",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "webgpu", "WebGpu", "WebGL", "webgl", "GPUDevice"],
        "axiom-scene must not reference WebGPU/WebGL",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant::now", "chrono"],
        "axiom-scene must not read wall-clock time",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "axiom-scene must not use randomness",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-scene must emit structured records via kernel sinks, not print",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-scene must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static"],
        "axiom-scene must not use global mutable state",
    );
}

#[test]
fn no_asset_or_file_loading_concepts() {
    assert_absent(
        &[
            "::AssetLoader",
            "::AssetServer",
            "std::fs",
            "std::path::Path::new(",
            "OpenOptions",
            "::FileReader",
        ],
        "axiom-scene must not load assets or open files",
    );
}

#[test]
fn no_physics_animation_audio_input_plugin_editor_or_gameplay_concepts() {
    assert_absent(
        &[
            "::Physics",
            "::RigidBody",
            "::Collider",
            "::Animator",
            "::Skeleton",
            "::Audio",
            "::SoundSource",
            "::InputState",
            "::KeyCode",
            "::MouseButton",
            "::Gamepad",
            "::Plugin",
            "::EditorPanel",
            "::GameLoop",
            "rapier",
            "winit",
            "egui",
            "bevy",
        ],
        "axiom-scene must not absorb physics/animation/audio/input/plugin/editor/gameplay concepts",
    );
}

#[test]
fn no_utils_or_helpers_modules() {
    for path in scene_source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert_ne!(name, "utils", "axiom-scene must not have a `utils` module");
        assert_ne!(name, "helpers", "axiom-scene must not have a `helpers` module");
        assert_ne!(name, "common", "axiom-scene must not have a `common` module");
        assert_ne!(name, "misc", "axiom-scene must not have a `misc` module");
    }
}
