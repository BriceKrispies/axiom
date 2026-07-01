//! Guards the scope boundary of the Axiom Coverage Law: asserts that
//! [`SANCTIONED_IGNORE_REGEX`] excludes no layer/module source path, and that
//! every gate script applies exactly that pattern exactly once (so the
//! coverage ignore list can't be quietly widened to hide engine code).
//!
//! The pattern is evaluated with the `regex` crate — the same engine llvm-cov
//! uses — so the check reflects what the tool actually does, not an
//! approximation.

use std::path::{Path, PathBuf};

use regex::Regex;

use crate::violation::{CheckReport, Violation, ViolationKind};

/// The ONE ignore pattern the coverage gate is permitted to use. It excludes
/// the apps (composition leaves), repo tooling (the `xtask` crate and anything
/// under `tools/`), and the `axiom-zones` build-time support crate from the 100%
/// count, and nothing else. Mirrored verbatim in `scripts/coverage.sh` and
/// `scripts/coverage.ps1`.
pub const SANCTIONED_IGNORE_REGEX: &str = r"[/\\](xtask|apps|axiom-zones|tools)[/\\]";

/// Gate scripts, relative to the repo root. Each must apply exactly the
/// sanctioned ignore, once.
const COVERAGE_SCRIPTS: &[&str] = &["scripts/coverage.sh", "scripts/coverage.ps1"];

/// The llvm-cov flag that introduces a filename ignore pattern.
const IGNORE_FLAG: &str = "--ignore-filename-regex";

/// A workspace-level pseudo-name for violations not tied to one layer.
const SCOPE: &str = "<coverage>";

/// Verify the coverage gate's ignore scope. Pure and deterministic.
///
/// Does nothing when no gate script is present (synthetic fixtures and
/// alternate roots have no `scripts/`), so the real repo is the only thing this
/// governs.
pub fn check(
    root: &Path,
    layer_dirs: &[(String, PathBuf)],
    module_dirs: &[(String, PathBuf)],
    report: &mut CheckReport,
) {
    let present: Vec<PathBuf> = COVERAGE_SCRIPTS
        .iter()
        .map(|rel| root.join(rel))
        .filter(|p| p.is_file())
        .collect();

    (!present.is_empty()).then(|| {
        // A compile failure here is an xtask programming error; surface it as
        // a violation rather than panicking so the checker stays infallible.
        let re = Regex::new(SANCTIONED_IGNORE_REGEX);

        re.as_ref().err().into_iter().for_each(|err| {
            report.push(Violation::new(
                ViolationKind::CoverageIgnoreScriptDrift,
                SCOPE,
                format!(
                    "sanctioned coverage ignore `{SANCTIONED_IGNORE_REGEX}` does not compile: {err}"
                ),
            ));
        });

        re.as_ref().ok().into_iter().for_each(|re| {
            // (1) The sanctioned pattern must exclude no engine path.
            [("layer", layer_dirs), ("module", module_dirs)]
                .into_iter()
                .flat_map(|(label, dirs)| dirs.iter().map(move |pair| (label, pair)))
                .map(|(label, (name, src_dir))| (label, name, engine_probe_path(root, src_dir)))
                .filter(|(_, _, probe)| re.is_match(probe))
                .for_each(|(label, name, probe)| {
                    report.push(Violation::new(
                        ViolationKind::CoverageIgnoreExcludesEngine,
                        name.clone(),
                        format!(
                            "coverage ignore `{SANCTIONED_IGNORE_REGEX}` excludes {label} `{name}` \
                             (matches `{probe}`); the gate may exclude only apps and tooling, never a \
                             layer or module"
                        ),
                    ));
                });

            // (2) Every present script must apply exactly the sanctioned ignore,
            // once. A script we cannot read contributes nothing.
            present
                .iter()
                .filter_map(|path| {
                    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
                    std::fs::read_to_string(path).ok().map(|text| (rel, text))
                })
                .map(|(rel, text)| {
                    // Count real usages only: a `#`-comment that merely mentions
                    // the flag must not trip (or mask) the check.
                    let code = strip_hash_comments(&text);
                    let flag_uses = code.matches(IGNORE_FLAG).count();
                    let has_sanctioned = code.contains(SANCTIONED_IGNORE_REGEX);
                    (rel, flag_uses, has_sanctioned)
                })
                .filter(|&(_, flag_uses, has_sanctioned)| (flag_uses != 1) | !has_sanctioned)
                .for_each(|(rel, flag_uses, has_sanctioned)| {
                    report.push(
                        Violation::new(
                            ViolationKind::CoverageIgnoreScriptDrift,
                            SCOPE,
                            format!(
                                "coverage script `{}` must apply the sanctioned ignore \
                                 `{SANCTIONED_IGNORE_REGEX}` exactly once via `{IGNORE_FLAG}` (found \
                                 {flag_uses} flag use(s); sanctioned pattern present: {has_sanctioned}); \
                                 widening the ignore list to hide engine code is forbidden",
                                rel.display()
                            ),
                        )
                        .at(rel, 1),
                    );
                });
        });
    });
}

/// A representative source path for an engine crate, normalized the way the
/// coverage tool sees paths: a leading separator before the category directory
/// and forward slashes, so a pattern like `[/\\]modules[/\\]` is correctly
/// recognized as matching `modules`.
fn engine_probe_path(root: &Path, src_dir: &Path) -> String {
    let rel = src_dir.strip_prefix(root).unwrap_or(src_dir);
    let normalized = rel.to_string_lossy().replace('\\', "/");
    format!("/{normalized}/lib.rs")
}

/// Strip `#` line comments (shell and PowerShell both use `#`), returning only
/// the executable text. Crude but sufficient: these scripts contain no `#`
/// inside strings.
fn strip_hash_comments(text: &str) -> String {
    text.lines()
        .map(|line| line.find('#').map_or(line, |i| &line[..i]))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    type EngineDirs = Vec<(String, PathBuf)>;

    /// Build a temp root with a `scripts/coverage.sh` whose ignore line is
    /// `ignore_line`, plus one layer crate and one module crate at the standard
    /// locations.
    fn setup(tag: &str, ignore_line: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("axiom_xtask_covscope_{tag}"));
        let _ = fs::remove_dir_all(&root);
        let scripts = root.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(
            scripts.join("coverage.sh"),
            format!("#!/usr/bin/env bash\n{ignore_line}\n"),
        )
        .unwrap();
        fs::create_dir_all(root.join("crates/axiom-kernel/src")).unwrap();
        fs::create_dir_all(root.join("modules/axiom-scene/src")).unwrap();
        root
    }

    fn dirs(root: &Path) -> (EngineDirs, EngineDirs) {
        (
            vec![("kernel".into(), root.join("crates/axiom-kernel/src"))],
            vec![("scene".into(), root.join("modules/axiom-scene/src"))],
        )
    }

    #[test]
    fn sanctioned_pattern_excludes_apps_tooling_and_support_only() {
        let re = Regex::new(SANCTIONED_IGNORE_REGEX).unwrap();
        assert!(re.is_match("/apps/axiom-demo/src/lib.rs"));
        assert!(re.is_match("/crates/xtask/src/main.rs"));
        assert!(re.is_match("/crates/axiom-zones/src/lib.rs"));
        assert!(re.is_match("/tools/axiom-netcode-relay/src/main.rs"));
        assert!(!re.is_match("/crates/axiom-kernel/src/lib.rs"));
        assert!(!re.is_match("/modules/axiom-scene/src/scene.rs"));
    }

    #[test]
    fn real_sanctioned_line_passes() {
        let root = setup(
            "ok",
            "exclude=(--ignore-filename-regex '[/\\\\](xtask|apps|axiom-zones|tools)[/\\\\]')",
        );
        let (layers, modules) = dirs(&root);
        let mut report = CheckReport::default();
        check(&root, &layers, &modules, &mut report);
        assert!(!report.has_kind(ViolationKind::CoverageIgnoreExcludesEngine));
        assert!(!report.has_kind(ViolationKind::CoverageIgnoreScriptDrift));
    }

    #[test]
    fn no_scripts_present_is_skipped() {
        let root = std::env::temp_dir().join("axiom_xtask_covscope_none");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let (layers, modules) = dirs(&root);
        let mut report = CheckReport::default();
        check(&root, &layers, &modules, &mut report);
        assert!(report.is_ok());
    }

    #[test]
    fn script_missing_sanctioned_pattern_is_drift() {
        let root = setup(
            "drift_missing",
            "exclude=(--ignore-filename-regex '[/\\\\]modules[/\\\\]')",
        );
        let (layers, modules) = dirs(&root);
        let mut report = CheckReport::default();
        check(&root, &layers, &modules, &mut report);
        assert!(report.has_kind(ViolationKind::CoverageIgnoreScriptDrift));
    }

    #[test]
    fn script_with_second_ignore_flag_is_drift() {
        let root = setup(
            "drift_extra",
            "exclude=(--ignore-filename-regex '[/\\\\](xtask|apps|axiom-zones|tools)[/\\\\]' \
             --ignore-filename-regex '[/\\\\]modules[/\\\\]')",
        );
        let (layers, modules) = dirs(&root);
        let mut report = CheckReport::default();
        check(&root, &layers, &modules, &mut report);
        assert!(report.has_kind(ViolationKind::CoverageIgnoreScriptDrift));
    }

    #[test]
    fn flag_mentioned_only_in_comment_does_not_satisfy_the_gate() {
        let root = setup("comment", "# uses --ignore-filename-regex somewhere");
        let (layers, modules) = dirs(&root);
        let mut report = CheckReport::default();
        check(&root, &layers, &modules, &mut report);
        assert!(report.has_kind(ViolationKind::CoverageIgnoreScriptDrift));
    }

    #[test]
    fn module_placed_under_an_excluded_dir_is_flagged() {
        let root = setup(
            "engine_excluded",
            "exclude=(--ignore-filename-regex '[/\\\\](xtask|apps|axiom-zones|tools)[/\\\\]')",
        );
        let layers = vec![("kernel".to_string(), root.join("crates/axiom-kernel/src"))];
        let modules = vec![("stray".to_string(), root.join("apps/stray/src"))];
        let mut report = CheckReport::default();
        check(&root, &layers, &modules, &mut report);
        assert!(report.has_kind(ViolationKind::CoverageIgnoreExcludesEngine));
    }
}
