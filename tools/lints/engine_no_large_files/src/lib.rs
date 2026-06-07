#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_span;

use clippy_utils::diagnostics::span_lint_and_help;
use engine_lint_helpers::is_engine_file;
use rustc_lint::{LateContext, LateLintPass};
use rustc_span::{FileName, Span, SyntaxContext};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags **engine source files** (layer crates under `crates/`, module crates
    /// under `modules/`) whose physical line count exceeds [`MAX_LINES`].
    ///
    /// ### Why is this bad?
    ///
    /// Axiom forbids junk-drawer files. A file over the line budget is a
    /// structural smell: it is doing too many things, hiding relationships between
    /// items, and making the engine harder for both humans and agents to
    /// understand. Axiom's agentic-development rules require that every file's
    /// purpose is obvious from its path — a 1 000-line grab-bag violates that
    /// contract.
    ///
    /// Large files also tend to concentrate multiple responsibilities in one
    /// module, which fights Axiom's explicit ownership and dependency direction
    /// rules. Split early; the kernel/layer/module structure exists precisely to
    /// give each concern its own home.
    ///
    /// ### Example
    ///
    /// A source file at `crates/axiom-runtime/src/everything.rs` with 1 200
    /// lines will trigger this warning. Break it into focused modules:
    ///
    /// ```text
    /// crates/axiom-runtime/src/
    ///   scheduler.rs   ← owns the tick-scheduling logic
    ///   step.rs        ← owns the single-step contract
    ///   config.rs      ← owns configuration primitives
    /// ```
    pub ENGINE_NO_LARGE_FILES,
    Warn,
    "engine source file exceeds the line-count budget"
}

/// Maximum physical lines per engine source file. Tunable — adjust here and
/// the test fixture in `ui/modules/m/src/big.rs` must track it. The current
/// real-engine maximum is well under this value; this const prevents new code
/// from quietly blowing past it.
const MAX_LINES: usize = 1000;

impl<'tcx> LateLintPass<'tcx> for EngineNoLargeFiles {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        let sm = cx.tcx.sess.source_map();
        for sf in sm.files().iter() {
            // Only care about real on-disk files, not virtual/macro expansions.
            if !matches!(sf.name, FileName::Real(_)) {
                continue;
            }

            // count_lines() returns the number of line-ending characters seen so
            // far; for a fully-loaded source file this equals the physical line
            // count.
            let lines = sf.count_lines();
            if lines <= MAX_LINES {
                continue;
            }

            // Build a zero-width span at the very start of the file.  We need a
            // Span so we can feed it to `is_engine_file` (which resolves the
            // file path via the SourceMap) and so the diagnostic points at the
            // file.  The fourth argument to `Span::new` is the parent span
            // (None = no parent).
            let span = Span::new(sf.start_pos, sf.start_pos, SyntaxContext::root(), None);

            if !is_engine_file(cx, span) {
                continue;
            }

            span_lint_and_help(
                cx,
                ENGINE_NO_LARGE_FILES,
                span,
                format!(
                    "this engine file has {lines} physical lines, exceeding the {MAX_LINES}-line budget"
                ),
                None,
                "split this file into smaller, single-responsibility modules; \
                 Axiom forbids junk-drawer files and large files hide structure from agents",
            );
        }
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
