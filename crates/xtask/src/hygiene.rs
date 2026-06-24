//! Centralized source-hygiene scanning for layers and modules.
//!
//! Scans Rust source files under a layer's or module's `src/` directory for:
//! - forbidden macros (`println!`, `eprintln!`, `dbg!`, `todo!`,
//!   `unimplemented!`);
//! - junk-drawer module names (`utils`, `helpers`, `common`, `misc`);
//! - browser/platform API references, unless the crate is on the
//!   platform-facing allowlist (today: only `axiom-host`).
//!
//! Per-layer `tests/architecture.rs` files inside each crate continue to
//! enforce their own per-crate scans. This module runs the centralized
//! version through `cargo xtask check-architecture`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::rust_source::{collect_rs_files, strip_line_comments};
use crate::violation::{CheckReport, Violation, ViolationKind};

/// Forbidden macro identifiers, found inside source as `<ident>!`.
const FORBIDDEN_MACROS: &[&str] = &[
    "println!",
    "eprintln!",
    "print!",
    "eprint!",
    "dbg!",
    "todo!",
    "unimplemented!",
];

/// Junk-drawer module names. Reject any file (or directory module) of
/// these names.
const JUNK_DRAWER_NAMES: &[&str] = &["utils", "helpers", "common", "misc"];

/// Coverage-suppression tokens. Banned everywhere in layers and modules:
/// coverage must be earned by reachable tests, never silenced. Genuinely
/// unreachable defensive code must be refactored away instead.
const COVERAGE_OFF_NEEDLES: &[&str] = &["coverage(off)", "coverage(on)", "coverage_attribute"];

/// Browser / platform API substrings. The scanner uses substring matches
/// so it catches references regardless of casing of the surrounding code.
const BROWSER_API_NEEDLES: &[&str] = &[
    "web_sys",
    "js_sys",
    "wasm_bindgen",
    "WebGPU",
    "WebGL",
    "requestAnimationFrame",
    "window.",
    "document.",
    "canvas",
];

/// Layers that are explicitly allowed to reference platform APIs.
const PLATFORM_FACING_LAYERS: &[&str] = &["host"];

/// Modules that are explicitly allowed to reference platform APIs. Each owns a
/// real `web-sys` / `wgpu` arm compiled only for `wasm32` behind a native-clean
/// facade (Module Law #9): `windowing` drives the run loop + surface request,
/// `gpu-backend` owns the real wgpu device/pipeline/buffers, `canvas2d-backend`
/// the software `CanvasRenderingContext2d` fallback, and `debug-overlay` the
/// developer overlay's DOM binding (keyboard + nodes). Adding another entry here
/// is a deliberate amendment.
const PLATFORM_FACING_MODULES: &[&str] =
    &["windowing", "gpu-backend", "canvas2d-backend", "debug-overlay"];

/// Run the centralized source-hygiene scan against every layer source dir
/// and every module source dir, pushing violations into `report`.
pub fn check(
    layer_dirs: &[(String, PathBuf)],
    module_dirs: &[(String, PathBuf)],
    report: &mut CheckReport,
) {
    layer_dirs.iter().for_each(|(name, dir)| {
        let is_platform_facing = PLATFORM_FACING_LAYERS.contains(&name.as_str());
        scan_one(name, dir, "layer", is_platform_facing, report);
    });
    module_dirs.iter().for_each(|(name, dir)| {
        // Only the sanctioned platform-facing module (windowing) may reference
        // browser APIs; every other module rejects them.
        let is_platform_facing = PLATFORM_FACING_MODULES.contains(&name.as_str());
        scan_one(name, dir, "module", is_platform_facing, report);
    });
}

fn scan_one(
    name: &str,
    src_dir: &Path,
    kind_label: &str,
    is_platform_facing: bool,
    report: &mut CheckReport,
) {
    src_dir.is_dir().then(|| {
        collect_rs_files(src_dir).into_iter().for_each(|path| {
            // Junk-drawer file/dir name (e.g. `utils.rs`).
            path.file_stem()
                .and_then(|s| s.to_str())
                .filter(|stem| JUNK_DRAWER_NAMES.contains(stem))
                .into_iter()
                .for_each(|stem| {
                    report.push(Violation::new(
                        ViolationKind::SourceHygieneJunkDrawerModule,
                        name.to_string(),
                        format!(
                            "{kind_label} `{name}` contains a junk-drawer module `{stem}`: {}; \
                             rename it to something specific",
                            path.display()
                        ),
                    ));
                });

            // A file we cannot read contributes no token scans.
            std::fs::read_to_string(&path)
                .into_iter()
                .for_each(|text| {
                    // Strip `//` line comments so a forbidden token mentioned only
                    // in a comment does not fail the scan.
                    let stripped = strip_line_comments(&text);

                    // Track which (forbidden, kind) we've already reported per file
                    // so a single file doesn't spam the report.
                    let mut reported: BTreeSet<(&str, &str)> = BTreeSet::new();

                    // Coverage-suppression tokens are scanned against the RAW text
                    // (not the comment-stripped form): even a commented-out
                    // `#[coverage(off)]` is a sign the ban is being worked around,
                    // and the attribute itself is the thing we forbid outright.
                    COVERAGE_OFF_NEEDLES
                        .iter()
                        .filter(|needle| text.contains(*needle))
                        // `insert` runs only when `contains` passed (per-file dedup).
                        .filter(|needle| reported.insert((needle, "coverage")))
                        .for_each(|needle| {
                            let v = Violation::new(
                                ViolationKind::SourceHygieneCoverageOff,
                                name.to_string(),
                                format!(
                                    "{kind_label} `{name}` uses banned coverage-suppression `{needle}` in {}; \
                                     cover the code with a reachable test or refactor the dead branch away",
                                    path.display()
                                ),
                            );
                            let v = first_line_containing(&text, needle)
                                .map_or(v.clone(), |line| v.at(path.clone(), line));
                            report.push(v);
                        });

                    FORBIDDEN_MACROS
                        .iter()
                        .filter(|needle| stripped.contains(*needle))
                        .filter(|needle| reported.insert((needle, "macro")))
                        .for_each(|needle| {
                            let v = Violation::new(
                                ViolationKind::SourceHygieneForbiddenMacro,
                                name.to_string(),
                                format!(
                                    "{kind_label} `{name}` uses forbidden macro `{needle}` in {}; \
                                     emit structured records through kernel logging instead",
                                    path.display()
                                ),
                            );
                            let v = first_line_containing(&stripped, needle)
                                .map_or(v.clone(), |line| v.at(path.clone(), line));
                            report.push(v);
                        });

                    // Browser APIs are rejected only for non-platform-facing crates.
                    BROWSER_API_NEEDLES
                        .iter()
                        .filter(|_| !is_platform_facing)
                        .filter(|needle| stripped.contains(*needle))
                        .filter(|needle| reported.insert((needle, "browser")))
                        .for_each(|needle| {
                            let v = Violation::new(
                                ViolationKind::SourceHygieneBrowserApi,
                                name.to_string(),
                                format!(
                                    "{kind_label} `{name}` references platform API `{needle}` in {}; \
                                     only the platform-facing host layer may reference these",
                                    path.display()
                                ),
                            );
                            let v = first_line_containing(&stripped, needle)
                                .map_or(v.clone(), |line| v.at(path.clone(), line));
                            report.push(v);
                        });
                });
        });
    });
}

fn first_line_containing(text: &str, needle: &str) -> Option<usize> {
    text.lines()
        .enumerate()
        .find(|(_, line)| line.contains(needle))
        .map(|(i, _)| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_src(tmp: &Path, name: &str, body: &str) -> PathBuf {
        let dir = tmp.join("src");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        dir
    }

    #[test]
    fn forbidden_macro_is_flagged() {
        let tmp = std::env::temp_dir().join("axiom_xtask_hygiene_macro");
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_src(&tmp, "lib.rs", "fn main() { println!(\"hi\"); }");
        let mut report = CheckReport::default();
        check(&[("test".into(), dir)], &[], &mut report);
        assert!(report.has_kind(ViolationKind::SourceHygieneForbiddenMacro));
    }

    #[test]
    fn junk_drawer_module_is_flagged() {
        let tmp = std::env::temp_dir().join("axiom_xtask_hygiene_junk");
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_src(&tmp, "utils.rs", "pub fn x() {}");
        let mut report = CheckReport::default();
        check(&[("test".into(), dir)], &[], &mut report);
        assert!(report.has_kind(ViolationKind::SourceHygieneJunkDrawerModule));
    }

    #[test]
    fn browser_api_is_flagged_for_non_host() {
        let tmp = std::env::temp_dir().join("axiom_xtask_hygiene_browser");
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_src(&tmp, "lib.rs", "use web_sys::Window;");
        let mut report = CheckReport::default();
        check(&[("notahost".into(), dir)], &[], &mut report);
        assert!(report.has_kind(ViolationKind::SourceHygieneBrowserApi));
    }

    #[test]
    fn browser_api_is_allowed_for_host_layer() {
        let tmp = std::env::temp_dir().join("axiom_xtask_hygiene_host");
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_src(&tmp, "lib.rs", "use web_sys::Window;");
        let mut report = CheckReport::default();
        check(&[("host".into(), dir)], &[], &mut report);
        assert!(!report.has_kind(ViolationKind::SourceHygieneBrowserApi));
    }

    #[test]
    fn coverage_off_attribute_is_banned() {
        let tmp = std::env::temp_dir().join("axiom_xtask_hygiene_covoff");
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_src(&tmp, "lib.rs", "#[coverage(off)]\nfn x() {}");
        let mut report = CheckReport::default();
        check(&[("test".into(), dir)], &[], &mut report);
        assert!(report.has_kind(ViolationKind::SourceHygieneCoverageOff));
    }

    #[test]
    fn forbidden_macro_inside_comment_is_ignored() {
        let tmp = std::env::temp_dir().join("axiom_xtask_hygiene_comment");
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_src(&tmp, "lib.rs", "// println! here is fine\nfn x() {}");
        let mut report = CheckReport::default();
        check(&[("test".into(), dir)], &[], &mut report);
        assert!(!report.has_kind(ViolationKind::SourceHygieneForbiddenMacro));
    }
}
