//! Architecture-boundary tests for the `axiom-recording` engine module.
//!
//! The workspace `xtask` checker enforces the global module law (allowed layers,
//! no module-to-module deps, single facade). These per-module tests scan this
//! crate's `src/` tree for forbidden tokens so internal regressions fail at
//! `cargo test` time — the recorder must stay an isolated, deterministic,
//! browser-free, kernel-only module that treats every artifact as opaque bytes.

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
        .expect("repo root is two levels above modules/axiom-recording")
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
    assert!(!files.is_empty(), "expected recording source files");
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
                    "axiom-recording {}: contains forbidden `{}`",
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
        "expected modules/axiom-recording/module.toml"
    );
    let raw = fs::read_to_string(&manifest).unwrap();
    let stripped = strip_comments_and_strings(&raw);
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-recording must declare `allowed_modules = []`"
    );
    // The `kind` value is a string literal, so check the raw manifest text.
    assert!(
        raw.contains("kind = \"engine-module\""),
        "axiom-recording must be an isolated engine module"
    );
}

#[test]
fn lib_rs_exports_exactly_one_facade_and_no_extra_surface() {
    let lib = read(&src_dir().join("lib.rs"));
    let public: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    assert_eq!(
        public,
        vec!["pub use recording_api::RecordingApi;"],
        "axiom-recording's lib.rs must publicly export exactly one facade: RecordingApi \
         (every other type — captures, timeline, mode, report, artifact kind — is returned opaquely)"
    );
}

#[test]
fn lib_rs_declares_no_public_modules() {
    // Modules are private; only the single `pub use` facade leaks out. A `pub mod`
    // would widen the surface past the one-facade rule.
    let lib = strip_comments_and_strings(&read(&src_dir().join("lib.rs")));
    assert!(
        !lib.contains("pub mod "),
        "axiom-recording must not declare any `pub mod` — its modules are private"
    );
}

// ---------- legal dependencies only ----------

#[test]
fn imports_only_the_kernel_layer() {
    // The recorder builds ONLY on the kernel (FrameIndex/Tick/KernelError). It is
    // allowed to name itself and the repo-wide `axiom_zones` support crate.
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
                    && chunk != "axiom_recording"
                    && chunk != "axiom_zones"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-recording may only import the kernel layer:\n{}",
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
        .filter(|name| name != "axiom_recording")
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
        "axiom-recording must not depend on any other module:\n{}",
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
        "axiom-recording must not depend on apps or tools:\n{}",
        violations.join("\n")
    );
}

// ---------- source hygiene ----------

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
            "requestAnimationFrame",
            "document.",
            "window.",
            "navigator.",
            "canvas",
        ],
        "axiom-recording must not reference browser/DOM/GPU APIs",
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
            "RandomState",
        ],
        "axiom-recording must be deterministic: no wall-clock time, no randomness",
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
        "axiom-recording must contain no console output or placeholder macros",
    );
}

#[test]
fn no_global_mutable_state_or_file_io() {
    assert_absent(
        &[
            "static mut",
            "lazy_static",
            "once_cell",
            "std::fs",
            "OpenOptions",
            "File::",
        ],
        "axiom-recording must not use global mutable state or file IO (no save-to-disk)",
    );
}

#[test]
fn no_render_scene_input_or_pixel_concepts() {
    // The recorder treats every artifact as opaque bytes. It must own NO concept
    // of scenes, renderers, GPU, input, pixels, screenshots, or compression — the
    // explicitly out-of-scope features.
    assert_absent(
        &[
            "winit",
            "rapier",
            "egui",
            "image::",
            "png",
            "::Mesh",
            "::Texture",
            "::Shader",
            "::Pixel",
            "::Rgba",
            "::Framebuffer",
            "::SceneGraph",
            "::Renderable",
            "::KeyCode",
            "::MouseButton",
            "screenshot",
            "flate2",
            "zstd",
            "Deflate",
        ],
        "axiom-recording must own no render/scene/input/pixel/screenshot/compression concepts",
    );
}

#[test]
fn no_utils_or_helpers_modules() {
    for path in source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(
                name, banned,
                "axiom-recording must not have a `{banned}` module"
            );
        }
    }
}
