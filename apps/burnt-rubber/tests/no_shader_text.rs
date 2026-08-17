//! **Requirement 7 of the field-system slice: no raw WGSL is authored by the
//! app.**
//!
//! The point of authoring the tarmac's grain as an
//! `axiom_field::FieldGraph` is that the app describes *what the surface is*
//! and the backend decides how to compute it. That claim is only worth
//! something if the app cannot quietly keep a shader in its back pocket, so
//! this test scans **every file in `apps/burnt-rubber`** — Rust, JavaScript,
//! HTML, CSS, TOML, the lot — for the vocabulary a hand-written shader is made
//! of, and fails naming the file and the line.
//!
//! It is deliberately a **text scan over the whole app directory** rather than a
//! Rust-only scan: the browser bundle's page and its glue JS could host a
//! `createShader` call just as easily as a `const SHADER: &str` could, and both
//! would be exactly the violation this requirement forbids.
//!
//! Two things it is *not*:
//!
//! * It is not a lint on the engine. `modules/axiom-gpu-backend` owns every
//!   shader string in the repository and is supposed to.
//! * It is not an anti-mention rule. This file itself names every marker it
//!   scans for, which is why it excludes itself; the markers are chosen to be
//!   shader **syntax** (`@vertex`, `gl_Position`, `fn fs_main`) rather than
//!   English words, so prose about shading in `asphalt_field.rs`'s
//!   documentation cannot trip it.

use std::fs;
use std::path::{Path, PathBuf};

/// The syntax a hand-authored shader is made of, in both of the languages this
/// engine's two live backends could accept.
///
/// Each entry is matched case-insensitively against the file's text.
const SHADER_MARKERS: &[&str] = &[
    // WGSL
    "@vertex",
    "@fragment",
    "@compute",
    "@group(",
    "@builtin(",
    "var<uniform>",
    "var<storage",
    "textureSample(",
    "fn vs_main",
    "fn fs_main",
    // GLSL / WebGL
    "gl_Position",
    "gl_FragColor",
    "precision mediump",
    "precision highp",
    "attribute vec",
    "varying vec",
    "createShader",
    "shaderSource",
    // The languages by name, as a source-of-truth marker for a `.wgsl` blob
    // pasted into a string literal.
    ".wgsl",
];

/// Files this scan skips: itself (it names every marker), and build outputs that
/// are not authored by the app.
fn skipped(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    text.ends_with("tests/no_shader_text.rs")
        || text.contains("/web/pkg/")
        || text.contains("/target/")
}

fn files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if !skipped(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn burnt_rubber_authors_no_shader_text() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scanned = files(root);
    // A scan that found nothing to scan is a green test that proves nothing.
    assert!(
        scanned.len() > 50,
        "the shader scan only reached {} files under {}",
        scanned.len(),
        root.display()
    );

    let mut violations: Vec<String> = Vec::new();
    for path in &scanned {
        let Ok(text) = fs::read_to_string(path) else {
            continue; // a binary golden; it holds no authored source
        };
        let lower = text.to_ascii_lowercase();
        for marker in SHADER_MARKERS {
            let needle = marker.to_ascii_lowercase();
            if let Some(offset) = lower.find(&needle) {
                let line = lower[..offset].matches('\n').count() + 1;
                violations.push(format!(
                    "{}:{line} contains shader syntax `{marker}`",
                    path.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "apps/burnt-rubber authors shader text. The app describes surfaces as \
         field graphs; every shader string in this engine belongs in \
         modules/axiom-gpu-backend.\n{}",
        violations.join("\n")
    );
}

/// The scan is only worth running if it can actually fail — a marker list that
/// no longer matches anything is a test that has quietly stopped working.
#[test]
fn the_scan_would_catch_a_shader_if_one_were_authored() {
    let sample = "@group(0) @binding(0) var<uniform> u: Camera;\n@vertex fn vs_main() {}";
    let lower = sample.to_ascii_lowercase();
    let hits = SHADER_MARKERS
        .iter()
        .filter(|marker| lower.contains(&marker.to_ascii_lowercase()))
        .count();
    assert!(hits >= 4, "the marker list matched only {hits} of a real shader");
}
