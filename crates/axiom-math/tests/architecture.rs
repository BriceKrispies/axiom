//! Mechanical architecture enforcement for axiom-math (an Axiom layer).
//!
//! These tests scan the math layer's own source tree (and the source trees of
//! the layers below it) and fail the build if any hard architecture rule is
//! violated. They are intentionally crude substring scans: the goal is a
//! fast, dependency-free tripwire, not a parser.
//!
//! This file lives under `tests/` (not `src/`) so the forbidden patterns it
//! searches *for* never trip the scan of themselves.

use std::fs;
use std::path::{Path, PathBuf};

fn math_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn kernel_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("axiom-kernel")
        .join("src")
}

fn runtime_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("axiom-runtime")
        .join("src")
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

fn math_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&math_src_dir(), &mut files);
    assert!(!files.is_empty(), "expected math source files");
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("source must be valid UTF-8")
}

/// Strip `//` line comments and string-literal contents so a forbidden token
/// that appears only inside a doc comment or string literal can't fail the
/// scan.
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
            // Consume to end of line, but keep the newline so line counts
            // (and forbidden-pattern positions) remain meaningful.
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

fn assert_absent_in(dir: &Path, label: &str, forbidden: &[&str], why: &str) {
    let mut files = Vec::new();
    collect_rs(dir, &mut files);
    files.sort();
    let mut violations = Vec::new();
    for path in &files {
        let stripped = strip_comments_and_strings(&read(path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "{label} {}: contains forbidden `{needle}`",
                    path.display()
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

fn assert_absent(forbidden: &[&str], why: &str) {
    assert_absent_in(&math_src_dir(), "axiom-math", forbidden, why);
}

#[test]
fn no_browser_or_js_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "Math.random"],
        "axiom-math must not reference browser / JS APIs",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "webgpu", "WebGpu", "WebGL", "webgl"],
        "axiom-math must not reference WebGPU/WebGL",
    );
}

#[test]
fn no_dom_or_browser_globals() {
    assert_absent(
        &["window.", "document.", "HtmlCanvas", "HtmlElement"],
        "axiom-math must not reference DOM globals",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant::now", "chrono"],
        "axiom-math must not read wall-clock time",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "axiom-math must not use randomness",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-math must emit structured records via kernel sinks, not print",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-math must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static"],
        "axiom-math must not use global mutable state",
    );
}

#[test]
fn no_engine_layer_concepts_higher_than_math() {
    // Any of these would mean math has started absorbing concepts that belong
    // to higher layers. Use word-boundary-ish patterns to avoid false hits
    // (e.g. don't flag plain "Plane" because of "plane.rs").
    let forbidden = &[
        "::World",
        "::Scene",
        "::Renderer",
        "::Material",
        "::Mesh",
        "::Asset",
        "::AssetLoader",
        "::Physics",
        "::RigidBody",
        "::Collider",
        "::Animation",
        "::Animator",
        "::Audio",
        "::SoundSource",
        "::InputState",
        "::KeyCode",
        "::MouseButton",
        "::Plugin",
        "::EditorPanel",
        "::GameLoop",
        "::Schedule",
        "rapier",
        "wgpu",
        "winit",
        "egui",
        "bevy",
    ];
    assert_absent(
        forbidden,
        "axiom-math must not import a layer it does not declare in depends_on",
    );
}

#[test]
fn no_utils_module() {
    for path in math_source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert_ne!(name, "utils", "axiom-math must not have a `utils` module");
        assert_ne!(
            name, "helpers",
            "axiom-math must not have a `helpers` module"
        );
        assert_ne!(name, "common", "axiom-math must not have a `common` module");
        assert_ne!(name, "misc", "axiom-math must not have a `misc` module");
    }
}

#[test]
fn lib_exports_exactly_math_api() {
    let lib = read(&math_src_dir().join("lib.rs"));
    let mut actual: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    actual.sort();

    // `MathApi` is the primary facade; the rest are the workhorse value
    // types future layers and modules must be able to *name* (store,
    // construct, match on). Any change to this set requires explicit
    // justification in ARCHITECTURE.md — mismatches fail the build so
    // accidental surface widening is caught.
    let mut expected: Vec<&str> = vec![
        "pub use math_api::MathApi;",
        "pub use approx_eq::ApproxEq;",
        "pub use epsilon::Epsilon;",
        "pub use scalar::Scalar;",
        "pub use math_error::MathError;",
        "pub use math_error_code::MathErrorCode;",
        "pub use math_result::MathResult;",
        "pub use mat3::Mat3;",
        "pub use mat4::Mat4;",
        "pub use quat::Quat;",
        "pub use transform::Transform;",
        "pub use vec2::Vec2;",
        "pub use vec3::Vec3;",
        "pub use vec4::Vec4;",
        "pub use aabb::Aabb;",
        "pub use frustum::Frustum;",
        "pub use plane::Plane;",
        "pub use plane_side::PlaneSide;",
        "pub use ray::Ray;",
        "pub use sphere::Sphere;",
    ];
    expected.sort();

    assert_eq!(
        actual, expected,
        "axiom-math's lib.rs public exports must match the curated set; \
         update both lib.rs and this test together"
    );
}

#[test]
fn axiom_kernel_does_not_import_axiom_math() {
    assert_absent_in(
        &kernel_src_dir(),
        "axiom-kernel",
        &["axiom_math", "axiom-math"],
        "axiom-kernel must not import axiom-math",
    );
}

#[test]
fn axiom_runtime_does_not_import_axiom_math() {
    assert_absent_in(
        &runtime_src_dir(),
        "axiom-runtime",
        &["axiom_math", "axiom-math"],
        "axiom-runtime must not import axiom-math",
    );
}

#[test]
fn math_only_imports_declared_dependencies() {
    // axiom-math may only import axiom-kernel and axiom-runtime.
    let mut illegal = Vec::new();
    for path in math_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        // Any `axiom_` crate prefix that isn't kernel/runtime is illegal.
        // (axiom_math itself is the layer's own prefix; it's allowed
        // internally — but a self-import like `use axiom_math::...` would
        // already be flagged by the architecture checker as a self-reference.)
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
                    // axiom-zones is the build-time zone-marker Support crate.
                    && chunk != "axiom_zones"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-math may only import axiom-kernel and axiom-runtime:\n{}",
        illegal.join("\n")
    );
}
