//! Guards the scope boundary of the Axiom Coverage Law.
//!
//! The 100% coverage gate (`scripts/coverage.sh` / `scripts/coverage.ps1`)
//! excludes apps and repo tooling from the count via llvm-cov's
//! `--ignore-filename-regex`. That ignore list is the one place the gate can be
//! quietly widened: a future agent could append a layer or module path and
//! "earn" 100% by hiding code instead of testing it.
//!
//! This module makes that impossible to do silently. xtask OWNS the sanctioned
//! ignore pattern and asserts two things mechanically:
//!
//!   1. The sanctioned pattern excludes NO layer or module source path — only
//!      apps and tooling. If the constant is ever edited to swallow engine
//!      code, this fires.
//!   2. Every gate script applies exactly that sanctioned pattern, exactly
//!      once. If a script is edited to use a wider pattern, or to add a second
//!      ignore flag, this fires.
//!
//! To change what the gate excludes you must edit [`SANCTIONED_IGNORE_REGEX`]
//! here AND both scripts, and the new pattern must still exclude no engine
//! path. There is no quiet path through.
//!
//! The pattern is evaluated with the `regex` crate — the same engine llvm-cov
//! uses — so the check reflects what the tool actually does, not an
//! approximation.

use std::path::{Path, PathBuf};

use regex::Regex;

use crate::violation::{CheckReport, Violation, ViolationKind};

/// The ONE ignore pattern the coverage gate is permitted to use. It excludes
/// the apps (composition leaves) and the `xtask`/`tools` tooling from the 100%
/// count, and nothing else. Mirrored verbatim in `scripts/coverage.sh` and
/// `scripts/coverage.ps1`.
pub const SANCTIONED_IGNORE_REGEX: &str = r"[/\\](xtask|apps)[/\\]";

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
    if present.is_empty() {
        return;
    }

    // The sanctioned pattern is a constant, so a compile failure is an xtask
    // programming error. Surface it as a violation rather than panicking so the
    // checker stays infallible.
    let re = match Regex::new(SANCTIONED_IGNORE_REGEX) {
        Ok(re) => re,
        Err(err) => {
            report.push(Violation::new(
                ViolationKind::CoverageIgnoreScriptDrift,
                SCOPE,
                format!(
                    "sanctioned coverage ignore `{SANCTIONED_IGNORE_REGEX}` does not compile: {err}"
                ),
            ));
            return;
        }
    };

    // (1) The sanctioned pattern must exclude no engine (layer/module) path.
    for (label, dirs) in [("layer", layer_dirs), ("module", module_dirs)] {
        for (name, src_dir) in dirs {
            let probe = engine_probe_path(root, src_dir);
            if re.is_match(&probe) {
                report.push(Violation::new(
                    ViolationKind::CoverageIgnoreExcludesEngine,
                    name.clone(),
                    format!(
                        "coverage ignore `{SANCTIONED_IGNORE_REGEX}` excludes {label} `{name}` \
                         (matches `{probe}`); the gate may exclude only apps and tooling, never a \
                         layer or module"
                    ),
                ));
            }
        }
    }

    // (2) Every present script must apply exactly the sanctioned ignore, once.
    for path in &present {
        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        // Count real usages only: a `#`-comment that merely mentions the flag
        // must not trip (or mask) the check.
        let code = strip_hash_comments(&text);
        let flag_uses = code.matches(IGNORE_FLAG).count();
        let has_sanctioned = code.contains(SANCTIONED_IGNORE_REGEX);
        if flag_uses != 1 || !has_sanctioned {
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
        }
    }
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
        .map(|line| match line.find('#') {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// `(name, src_dir)` pairs for a set of engine crates.
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
    fn sanctioned_pattern_excludes_apps_and_tools_only() {
        let re = Regex::new(SANCTIONED_IGNORE_REGEX).unwrap();
        assert!(re.is_match("/apps/axiom-demo/src/lib.rs"));
        assert!(re.is_match("/crates/xtask/src/main.rs"));
        assert!(!re.is_match("/crates/axiom-kernel/src/lib.rs"));
        assert!(!re.is_match("/modules/axiom-scene/src/scene.rs"));
    }

    #[test]
    fn real_sanctioned_line_passes() {
        let root = setup(
            "ok",
            "exclude=(--ignore-filename-regex '[/\\\\](xtask|apps)[/\\\\]')",
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
        // A script that ignores something else entirely (no sanctioned pattern).
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
        // The sanctioned pattern is present, but a smuggled second ignore would
        // let engine code be hidden. Two flag uses must fail.
        let root = setup(
            "drift_extra",
            "exclude=(--ignore-filename-regex '[/\\\\](xtask|apps)[/\\\\]' \
             --ignore-filename-regex '[/\\\\]modules[/\\\\]')",
        );
        let (layers, modules) = dirs(&root);
        let mut report = CheckReport::default();
        check(&root, &layers, &modules, &mut report);
        assert!(report.has_kind(ViolationKind::CoverageIgnoreScriptDrift));
    }

    #[test]
    fn flag_mentioned_only_in_comment_does_not_satisfy_the_gate() {
        // A comment mentioning the flag is not a real usage: flag_uses == 0.
        let root = setup("comment", "# uses --ignore-filename-regex somewhere");
        let (layers, modules) = dirs(&root);
        let mut report = CheckReport::default();
        check(&root, &layers, &modules, &mut report);
        assert!(report.has_kind(ViolationKind::CoverageIgnoreScriptDrift));
    }

    #[test]
    fn module_placed_under_an_excluded_dir_is_flagged() {
        // Contrived: a module whose source sits under `apps/` would be silently
        // excluded by the sanctioned pattern. The engine-exclusion guard catches
        // it regardless of how the misplacement happened.
        let root = setup(
            "engine_excluded",
            "exclude=(--ignore-filename-regex '[/\\\\](xtask|apps)[/\\\\]')",
        );
        let layers = vec![("kernel".to_string(), root.join("crates/axiom-kernel/src"))];
        let modules = vec![("stray".to_string(), root.join("apps/stray/src"))];
        let mut report = CheckReport::default();
        check(&root, &layers, &modules, &mut report);
        assert!(report.has_kind(ViolationKind::CoverageIgnoreExcludesEngine));
    }
}
