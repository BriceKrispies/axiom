//! The semantic *slice* checks: `check-slices` and `check-slice-placement`.
//!
//! These sit on top of the structural Layer/Module/Coverage/Branchless laws and
//! prove **end-to-end slice health** — the property the structural checks
//! cannot express. They add assertions only; they weaken no existing rule.
//!
//! * [`check_slices`] validates every `slice.toml`: a determinism test exists,
//!   each committed golden `.bin` exists AND hashes to its recorded SHA-256
//!   (closing the trust-on-first-use hole where a deleted/regenerated golden is
//!   silently re-blessed), an optional reference image likewise hash-pins, the
//!   `harness_entry` symbol is a real public export, and — when the slice
//!   declares a live `harness` — it is registered in axiom-shot's renderable
//!   registry (so a slice is not "runnable but un-harnessable").
//!
//! * [`check_slice_placement`] flags engine render logic hiding in an app:
//!   a large `apps/` source file that is a dense mesh/instance/matrix
//!   data-transform (exposes geometry-producing `pub fn`s) that does NOT
//!   genuinely compose modules (touches ≤1 module facade) — code that belongs
//!   in a coverage+branchless feature module, not the coverage-exempt app tier.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::cargo_metadata::load as load_cargo_metadata;
use crate::rust_source::{collect_rs_files, find_public_export, strip_line_comments};
use crate::slice_manifest::{load_slice_manifests, SliceManifest};
use crate::violation::{CheckReport, Violation, ViolationKind};

/// Run every `slice.toml` semantic check rooted at `root`. Pushes violations
/// into `report`.
pub fn check_slices(root: &Path, report: &mut CheckReport) {
    let (manifests, parse_errors) = load_slice_manifests(root);

    parse_errors.iter().for_each(|err| {
        let rel = relativize(root, &err.path);
        report.push(
            Violation::new(
                ViolationKind::SliceManifestInvalid,
                rel.display().to_string(),
                format!("could not parse slice manifest: {}", err.message),
            )
            .at(rel, 1),
        );
    });

    // The set of real workspace package names (for the `crate_name` sanity
    // check). `cargo metadata` may be unavailable for a synthetic fixture with
    // no `Cargo.toml`; then the crate-name check is skipped (every other check
    // still runs).
    let known_crates: Option<BTreeSet<String>> = load_cargo_metadata(root)
        .ok()
        .map(|graph| graph.packages.iter().map(|p| p.name.clone()).collect());

    // The axiom-shot registry source, scanned once for declared harness names.
    let registry_text = read_shot_registry_text(root);

    manifests.iter().for_each(|m| {
        check_one_slice(root, m, known_crates.as_ref(), &registry_text, report);
    });
}

fn check_one_slice(
    root: &Path,
    m: &SliceManifest,
    known_crates: Option<&BTreeSet<String>>,
    registry_text: &str,
    report: &mut CheckReport,
) {
    let name = &m.slice.name;
    let manifest_rel = relativize(root, &m.dir.join("slice.toml"));

    // Sanity: `crate_name` names a real workspace package.
    known_crates
        .filter(|known| !known.contains(&m.slice.crate_name))
        .into_iter()
        .for_each(|_| {
            report.push(
                Violation::new(
                    ViolationKind::SliceCrateUnknown,
                    name,
                    format!(
                        "slice `{name}` declares crate_name = `{}` but no workspace package by that \
                         name exists",
                        m.slice.crate_name
                    ),
                )
                .at(manifest_rel.clone(), 1),
            );
        });

    // (a) A determinism test target exists.
    (!m.determinism_test_path().is_file()).then(|| {
        report.push(
            Violation::new(
                ViolationKind::SliceDeterminismTestMissing,
                name,
                format!(
                    "slice `{name}` declares determinism_test = `{}`, but its test file \
                     `tests/{}.rs` does not exist; a slice must carry a fixed-scenario \
                     replay-equal + perturbed-differs determinism test",
                    m.slice.determinism_test, m.slice.determinism_test
                ),
            )
            .at(relativize(root, &m.determinism_test_path()), 1),
        );
    });

    // (b) Each declared golden exists AND matches its recorded SHA-256.
    m.goldens.iter().for_each(|g| {
        let path = m.dir.join(&g.path);
        check_hashed_file(
            root,
            name,
            &path,
            &g.sha256,
            ViolationKind::SliceGoldenMissing,
            ViolationKind::SliceGoldenHashMismatch,
            "golden",
            report,
        );
    });

    // (c) The reference image (optional) exists AND matches its recorded hash.
    m.reference.iter().for_each(|r| {
        let path = m.dir.join(&r.path);
        check_hashed_file(
            root,
            name,
            &path,
            &r.sha256,
            ViolationKind::SliceReferenceMissing,
            ViolationKind::SliceReferenceHashMismatch,
            "reference image",
            report,
        );
    });

    // (d) The harness_entry symbol is a real public export of the slice's crate,
    // and (when a live harness is declared) the slice is registered in axiom-shot.
    let has_entry = crate_public_exports(&m.src_dir()).contains(&m.slice.harness_entry);
    (!has_entry).then(|| {
        report.push(
            Violation::new(
                ViolationKind::SliceHarnessEntryMissing,
                name,
                format!(
                    "slice `{name}` declares harness_entry = `{}`, but no public export by that \
                     name exists in `{}`; name the slice's renderable core symbol",
                    m.slice.harness_entry, m.slice.crate_name
                ),
            )
            .at(manifest_rel.clone(), 1),
        );
    });

    m.slice.harness.iter().for_each(|harness| {
        (!registry_declares(registry_text, harness)).then(|| {
            report.push(
                Violation::new(
                    ViolationKind::SliceHarnessNotRegistered,
                    name,
                    format!(
                        "slice `{name}` declares harness = `{harness}`, but that name is not \
                         registered in axiom-shot's slice registry \
                         (tools/axiom-shot/src/registry.rs); register it so the slice is \
                         renderable by name"
                    ),
                )
                .at(manifest_rel.clone(), 1),
            );
        });
    });
}

/// Assert a committed file exists and hashes to `expected_sha256` (lowercase
/// hex). Pushes the missing-kind violation if absent, the mismatch-kind
/// violation if the hash differs.
#[allow(clippy::too_many_arguments)]
fn check_hashed_file(
    root: &Path,
    slice_name: &str,
    path: &Path,
    expected_sha256: &str,
    missing_kind: ViolationKind,
    mismatch_kind: ViolationKind,
    noun: &str,
    report: &mut CheckReport,
) {
    let rel = relativize(root, path);
    // Build at most one violation, then push it once, so `report` is borrowed
    // a single time (a two-closure `map_or_else` would alias it).
    let violation = std::fs::read(path).ok().map_or_else(
        || {
            Some(
                Violation::new(
                    missing_kind,
                    slice_name,
                    format!(
                        "slice `{slice_name}` declares {noun} `{}`, but the file is missing; a \
                         deleted/regenerated artifact must be re-recorded in slice.toml",
                        rel.display()
                    ),
                )
                .at(rel.clone(), 1),
            )
        },
        |bytes| {
            let actual = hex_sha256(&bytes);
            (actual != expected_sha256.trim().to_lowercase()).then(|| {
                Violation::new(
                    mismatch_kind,
                    slice_name,
                    format!(
                        "slice `{slice_name}` {noun} `{}` hashes to {actual} but slice.toml \
                         records {}; the committed artifact drifted from its pinned hash",
                        rel.display(),
                        expected_sha256.trim().to_lowercase()
                    ),
                )
                .at(rel.clone(), 1)
            })
        },
    );
    violation.into_iter().for_each(|v| report.push(v));
}

/// The lowercase hex SHA-256 of `bytes`.
pub fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Every public export symbol name in a crate's `src/` tree (comment-stripped).
fn crate_public_exports(src_dir: &Path) -> BTreeSet<String> {
    collect_rs_files(src_dir)
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok())
        .flat_map(|text| public_export_names(&strip_line_comments(&text)))
        .collect()
}

/// The names introduced by `pub <kw> NAME` / `pub use ... NAME;` lines in one
/// source text. A lightweight sweep reusing [`find_public_export`] per candidate
/// identifier taken from each `pub` line.
fn public_export_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|raw| raw.trim_start().strip_prefix("pub "))
        .filter_map(|rest| {
            // The candidate name is the last identifier-ish token on the item
            // head (covers `pub fn foo(`, `pub struct Foo<`, `pub use a::Foo;`,
            // `pub use a::X as Foo;`). `find_public_export` then confirms it.
            candidate_name(rest)
        })
        .filter(|name| find_public_export(text, name).is_some())
        .collect()
}

/// The exported name a `pub `-stripped item head introduces, if any.
fn candidate_name(rest: &str) -> Option<String> {
    let rest = rest.trim_start();
    // `pub use ... NAME;` / `pub use ... as NAME;`
    let from_use = rest.strip_prefix("use ").map(|use_body| {
        let body = use_body.trim().trim_end_matches(';').trim();
        body.rsplit_once(" as ")
            .map(|(_, alias)| alias.trim())
            .unwrap_or_else(|| body.rsplit("::").next().unwrap_or(body).trim())
            .trim_end_matches(['{', '}', '*'])
            .trim()
            .to_string()
    });
    from_use
        .or_else(|| {
            // `pub <kw> NAME ...`: skip the item keyword, take the next identifier.
            const KEYWORDS: &[&str] = &[
                "fn", "struct", "enum", "trait", "type", "const", "static", "union", "mod",
            ];
            KEYWORDS.iter().find_map(|kw| {
                rest.strip_prefix(kw)
                    .filter(|after| after.chars().next().is_some_and(char::is_whitespace))
                    .and_then(|after| first_ident(after.trim_start()))
                    .map(str::to_string)
            })
        })
        .filter(|name| !name.is_empty())
}

fn first_ident(s: &str) -> Option<&str> {
    let len = s
        .bytes()
        .position(|b| !(b.is_ascii_alphanumeric() | (b == b'_')))
        .unwrap_or(s.len());
    (len != 0).then(|| &s[..len])
}

/// The axiom-shot renderable-slice registry source (all `.rs` under
/// `tools/axiom-shot/src`), concatenated and comment-stripped. Empty when the
/// tool is absent (e.g. a synthetic fixture).
fn read_shot_registry_text(root: &Path) -> String {
    let src = root.join("tools").join("axiom-shot").join("src");
    collect_rs_files(&src)
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok())
        .map(|t| strip_line_comments(&t))
        .collect::<Vec<String>>()
        .join("\n")
}

/// Whether the axiom-shot registry source registers a slice under `harness`
/// (its name appears as a `"harness"` string literal).
fn registry_declares(registry_text: &str, harness: &str) -> bool {
    registry_text.contains(&format!("\"{harness}\""))
}

// --- check-slice-placement -------------------------------------------------

/// Minimum line count for a source file to be considered a "large" data
/// transform. Below this an app file is small glue, not a hidden engine.
const PLACEMENT_MIN_LINES: usize = 300;

/// Minimum count of geometry-math tokens for a file to read as a dense
/// mesh/instance/matrix transform (not one incidental mention).
const PLACEMENT_MIN_GEOMETRY_HITS: usize = 12;

/// Run the slice-placement check: flag every `apps/` source file that
/// is a large, pure mesh/instance/matrix data-transform with no module-facade
/// call. Pushes [`ViolationKind::SlicePlacementEngineLogicInApp`] per offender.
pub fn check_slice_placement(root: &Path, report: &mut CheckReport) {
    ["apps"]
        .into_iter()
        .map(|sub| root.join(sub))
        .flat_map(|dir| collect_rs_files(&dir))
        .filter(|p| !is_test_path(p))
        .for_each(|path| {
            std::fs::read_to_string(&path)
                .ok()
                .map(|text| strip_line_comments(&text))
                .filter(|text| is_hidden_engine_transform(text))
                .into_iter()
                .for_each(|_| {
                    let rel = relativize(root, &path);
                    report.push(
                        Violation::new(
                            ViolationKind::SlicePlacementEngineLogicInApp,
                            rel.display().to_string(),
                            format!(
                                "`{}` is a large pure mesh/instance/matrix data-transform with \
                                 public geometry-producing fns and no module-facade call — \
                                 engine render logic hiding in an app. Extract it into a \
                                 coverage+branchless feature module",
                                rel.display()
                            ),
                        )
                        .at(rel.clone(), 1),
                    );
                });
        });
}

/// Whether `path` is under a `tests/` directory or is a `*_test.rs`/test file
/// (placement targets runtime app source, not test harnesses).
fn is_test_path(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_str() == Some("tests"))
}

/// The heuristic for "engine render logic hiding in an app": a large file that
/// (1) defines ≥1 public geometry-producing fn, (2) is dense with
/// mesh/instance/matrix tokens, and (3) makes no module-facade (`*Api::`) call.
fn is_hidden_engine_transform(text: &str) -> bool {
    let line_count = text.lines().count();
    let large = line_count >= PLACEMENT_MIN_LINES;

    // Exposes a public surface (geometry-producing `pub fn`s). The geometry
    // property is carried by `dense` below, since a builder's *return type*
    // (`RenderData`, `Vec<Instance>`) rarely names a math type on its signature
    // line — the geometry lives in the body.
    let has_public_fn = text
        .lines()
        .any(|line| line.trim_start().starts_with("pub fn "));

    // Dense with geometry math (matrices / vectors / mesh+instance building),
    // not one incidental mention.
    let geometry_hits = GEOMETRY_TOKENS
        .iter()
        .map(|tok| text.matches(tok).count())
        .sum::<usize>();
    let dense = geometry_hits >= PLACEMENT_MIN_GEOMETRY_HITS;

    // Genuine module composition wires SEVERAL distinct engine facades
    // (`SceneApi::`, `RenderApi::`, …) together. A large geometry transform that
    // touches at most one facade is not composing modules — it is engine render
    // math living in an app (e.g. a 1188-line neutral-render builder with a lone
    // incidental `TerrainMeshApi::` call). More than one distinct facade means
    // the file is real composition glue and is left alone.
    let composes_modules = distinct_facade_count(text) > 1;

    large & has_public_fn & dense & !composes_modules
}

const GEOMETRY_TOKENS: &[&str] = &[
    "Mat4", "Mat3", "Vec3", "Vec4", "mesh", "instance", "vertices", "[f32", "world",
];

/// The number of DISTINCT `*Api::` module-facade names called in `text` (an
/// uppercase-led identifier ending in `Api`, immediately followed by `::`, e.g.
/// `RenderApi::`). A substring like `capi::` never counts.
fn distinct_facade_count(text: &str) -> usize {
    text.match_indices("Api::")
        .filter_map(|(idx, _)| {
            let ident: String = text[..idx]
                .bytes()
                .rev()
                .take_while(|b| b.is_ascii_alphanumeric() | (*b == b'_'))
                .collect::<Vec<u8>>()
                .into_iter()
                .rev()
                .map(char::from)
                .collect();
            // Uppercase-led (a real facade type, not a lowercase module path).
            ident
                .chars()
                .next()
                .filter(|c| c.is_ascii_uppercase())
                .map(|_| format!("{ident}Api"))
        })
        .collect::<BTreeSet<String>>()
        .len()
}

fn relativize(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        // SHA-256("abc")
        assert_eq!(
            hex_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn public_export_names_finds_items_and_reexports() {
        let text = "pub fn build_app() {}\n\
                    pub struct Foo;\n\
                    pub use inner::Bar as Baz;\n\
                    pub(crate) fn hidden() {}\n";
        let names = public_export_names(text);
        assert!(names.contains(&"build_app".to_string()));
        assert!(names.contains(&"Foo".to_string()));
        assert!(names.contains(&"Baz".to_string()));
        assert!(!names.contains(&"hidden".to_string()));
    }

    #[test]
    fn registry_declares_matches_string_literal() {
        let text = "SliceEntry { name: \"retro-fps\", build: ... }";
        assert!(registry_declares(text, "retro-fps"));
        assert!(!registry_declares(text, "generia"));
    }

    #[test]
    fn facade_count_counts_distinct_uppercase_facades() {
        assert_eq!(distinct_facade_count("let r = RenderApi::new();"), 1);
        assert_eq!(
            distinct_facade_count("RenderApi::new(); SceneApi::new(); RenderApi::step();"),
            2
        );
        // A lowercase module lookalike never counts.
        assert_eq!(distinct_facade_count("let x = capi::thing();"), 0);
    }

    #[test]
    fn hidden_engine_transform_flags_big_geometry_transform_without_facade() {
        let body = std::iter::repeat(
            "let m = Mat4::from_cols(); let v = Vec3::ZERO; // mesh instance world [f32; 16]",
        )
        .take(400)
        .collect::<Vec<_>>()
        .join("\n");
        let text = format!("pub fn build_mesh() -> Vec3 {{\n{body}\n}}\n");
        assert!(is_hidden_engine_transform(&text));
    }

    #[test]
    fn hidden_engine_transform_ignores_files_that_compose_several_facades() {
        // Genuine composition glue wires MULTIPLE distinct module facades; that
        // is left alone even when large and geometry-dense.
        let body = std::iter::repeat("let m = Mat4::default(); // mesh instance world [f32")
            .take(400)
            .collect::<Vec<_>>()
            .join("\n");
        let text = format!(
            "pub fn build_mesh() -> Vec3 {{\n\
             let r = RenderApi::new(); let s = SceneApi::new(); let g = ResourcesApi::new();\n\
             {body}\n}}\n"
        );
        assert!(!is_hidden_engine_transform(&text));
    }

    #[test]
    fn hidden_engine_transform_flags_a_lone_incidental_facade_call() {
        // The build.rs case: 1000+ lines of neutral-render math with ONE
        // incidental facade call is still engine logic hiding in an app.
        let body = std::iter::repeat("let m = Mat4::default(); // mesh instance world [f32")
            .take(400)
            .collect::<Vec<_>>()
            .join("\n");
        let text = format!(
            "pub fn build_mesh() -> Vec3 {{\nlet t = TerrainMeshApi::mesh();\n{body}\n}}\n"
        );
        assert!(is_hidden_engine_transform(&text));
    }

    #[test]
    fn hidden_engine_transform_ignores_small_files() {
        let text = "pub fn build_mesh() -> Vec3 { Mat4::default(); Vec3::ZERO }\n";
        assert!(!is_hidden_engine_transform(text));
    }
}
