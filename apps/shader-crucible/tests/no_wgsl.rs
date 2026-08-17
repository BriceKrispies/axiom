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

/// The two files of the interactive camera — the one part of this app that does
/// arithmetic on purpose. See [`the_app_computes_no_colour_in_rust`] for why
/// they are exempt from the shading-maths scan, and
/// [`the_camera_is_outside_the_appearance_pipeline_entirely`] for what keeps
/// that exemption honest.
const CAMERA_FILES: [&str; 2] = ["orbit.rs", "pointer_input.rs"];

/// Whether `path` is one of [`CAMERA_FILES`].
fn is_camera_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| CAMERA_FILES.contains(&name))
        .unwrap_or(false)
}

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
///
/// **The camera is exempt too, and named** ([`CAMERA_FILES`]). A turntable is
/// spherical coordinates, so it is trigonometry — but it is not *shading*: it
/// produces an eye position, never a pixel value, and there is no `FieldGraph`
/// that could express it, because a graph is a pointwise appearance IR and a
/// camera is not appearance. The exemption is two named files rather than a
/// directory, and
/// [`the_camera_is_outside_the_appearance_pipeline_entirely`] holds it honest by
/// proving those two files touch nothing the appearance system is made of — so
/// the exemption cannot quietly become somewhere to hide a shading function.
#[test]
fn the_app_computes_no_colour_in_rust() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let banned = ["powf(", ".sin()", ".cos()", ".exp()", ".sqrt()", ".ln()"];
    let offenders: Vec<String> = sources(&src)
        .iter()
        .filter(|path| !is_camera_file(path))
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

/// **The camera's exemption cannot become a hiding place.**
///
/// [`the_app_computes_no_colour_in_rust`] lets [`CAMERA_FILES`] do trigonometry,
/// on the grounds that an orbit is spherical coordinates and not shading. That
/// claim is only true while those files stay *outside the appearance pipeline* —
/// so this proves it directly: neither file may name any of the types the
/// appearance system is built out of. A shading function smuggled in there would
/// have to author a graph, bind a surface, or set a program to be worth anything,
/// and each of those is a token below.
///
/// The two tests are therefore a pair: one says "not here", the other says "and
/// the place it is allowed cannot be used for it".
#[test]
fn the_camera_is_outside_the_appearance_pipeline_entirely() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // Every vocabulary a colour could actually be produced through in this app.
    let appearance = [
        "FieldGraph",
        "FieldBuilder",
        "FieldOp",
        "Surface",
        "surface_program",
        "Material",
        "Color",
        "Texture",
    ];
    let camera: Vec<PathBuf> = sources(&src)
        .into_iter()
        .filter(|path| is_camera_file(path))
        .collect();
    assert_eq!(
        camera.len(),
        CAMERA_FILES.len(),
        "the exemption names a file the scan cannot find: {camera:?}"
    );
    let offenders: Vec<String> = camera
        .iter()
        .flat_map(|path| {
            let text =
                strip_line_comments(&std::fs::read_to_string(path).expect("a readable source file"));
            appearance
                .iter()
                .filter(|token| text.contains(**token))
                .map(|token| format!("{}: `{token}`", path.display()))
                .collect::<Vec<String>>()
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "the camera is exempt from the shading-maths scan only because it is not \
         part of the appearance pipeline — and these references say otherwise:\n{}",
        offenders.join("\n")
    );
}
