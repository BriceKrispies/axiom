//! **The crucible authors no WGSL.**
//!
//! The claim the whole appearance system makes is that a visual effect is an
//! authored graph rather than a shading function, and the sharpest evidence for
//! it is that the app demonstrating every capability of the system contains **no
//! shader source at all**. Every pixel it produces comes out of WGSL that
//! `axiom-gpu-backend`'s emitters generated from the graphs in `src/stations/`.
//!
//! This is a grep test, in the same shape as the one `apps/burnt-rubber` ships:
//! it reads every source file of this crate and fails on any token that would
//! indicate hand-written shader source. It is deliberately crude — a text scan
//! is what makes it impossible to satisfy by accident.
//!
//! ## The tokens are WGSL *syntax*, not the word "WGSL"
//!
//! This app discusses WGSL at length — that is half of what it is for — so a
//! test keyed on the word would be a test of its prose. It is keyed instead on
//! things that only ever appear in a shader (`@vertex`, `@group(`,
//! `var<uniform>`, `textureSample`) or in the call that compiles one
//! (`create_shader_module`, `ShaderSource`). None of those can turn up in an
//! English sentence, and none of them can be avoided by an app that genuinely
//! did write a shader.
//!
//! Line comments are stripped first anyway, exactly as `xtask`'s own source
//! scans do it and for the same reason: a comment that merely names a construct
//! must be able neither to fabricate a violation nor to mask one.

use std::path::{Path, PathBuf};

/// Tokens that appear only in WGSL source or in the wgpu call that compiles it.
const SHADER_TOKENS: [&str; 9] = [
    "@vertex",
    "@fragment",
    "@compute",
    "@group(",
    "@location(",
    "@builtin(",
    "var<uniform>",
    "textureSample",
    "create_shader_module",
];

/// `text` with every `//` line comment removed. Crude on purpose: a `//` inside
/// a string literal would also be cut, and this app has none.
fn strip_line_comments(text: &str) -> String {
    text.lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<&str>>()
        .join("\n")
}

/// Every `.rs` file under `dir`, recursively.
fn sources(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|entry| {
            let path = entry.path();
            match path.is_dir() {
                true => sources(&path),
                false => path
                    .extension()
                    .filter(|ext| *ext == "rs")
                    .map(|_| vec![path.clone()])
                    .unwrap_or_default(),
            }
        })
        .collect()
}

#[test]
fn the_app_authors_no_wgsl() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = sources(&src);
    assert!(files.len() >= 12, "the scan found only {} files", files.len());
    let offenders: Vec<String> = files
        .iter()
        .flat_map(|path| {
            let text = strip_line_comments(
                &std::fs::read_to_string(path).expect("a readable source file"),
            );
            SHADER_TOKENS
                .iter()
                .filter(|token| text.contains(**token))
                .map(|token| format!("{}: `{token}`", path.display()))
                .collect::<Vec<String>>()
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "the shader crucible must author no WGSL, and these do:\n{}",
        offenders.join("\n")
    );
}

/// ...and no shading maths either. The app names field **operators**; it never
/// computes a colour in Rust.
///
/// `sin`, `cos`, `pow`, `exp`, `smoothstep` and `mix` all appear in
/// `authoring.rs` as one-line spellings of a `FieldOp` — that is a *name*, not a
/// computation. What is banned is the Rust `f32` **method**, which would mean a
/// pattern was evaluated here instead of authored as a graph.
///
/// Tests are exempt — they legitimately recompute a reference value to check a
/// graph against — so the scan stops at the first `#[cfg(test)]` of each file.
#[test]
fn the_app_computes_no_colour_in_rust() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let banned = ["powf(", ".sin()", ".cos()", ".exp()", ".sqrt()", ".ln()"];
    let offenders: Vec<String> = sources(&src)
        .iter()
        .flat_map(|path| {
            let text = strip_line_comments(
                &std::fs::read_to_string(path).expect("a readable source file"),
            );
            let non_test = text.split("#[cfg(test)]").next().unwrap_or_default().to_string();
            banned
                .iter()
                .filter(|token| non_test.contains(**token))
                .map(|token| format!("{}: `{token}`", path.display()))
                .collect::<Vec<String>>()
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "shading maths in Rust, outside a graph:\n{}",
        offenders.join("\n")
    );
}
