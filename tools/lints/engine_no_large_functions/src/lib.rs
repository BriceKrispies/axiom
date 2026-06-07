#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_span;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::is_engine_file;
use rustc_hir::intravisit::FnKind;
use rustc_hir::{Body, FnDecl};
use rustc_lint::{LateContext, LateLintPass};
use rustc_span::Span;
use rustc_span::def_id::LocalDefId;

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags engine functions whose source span exceeds [`MAX_LINES`] lines.
    /// Closures and `#[test]` / `#[cfg(test)]` functions are exempt. Only
    /// source files on the engine spine (`crates/<layer>/src/` and
    /// `modules/<module>/src/`) are checked; apps and tooling are ignored.
    ///
    /// ### Why is this bad?
    ///
    /// A function that runs for more than 120 lines is doing too many things.
    /// It hides its responsibilities from the next reader — human or AI agent —
    /// behind a wall of sequential logic that is hard to test, hard to reason
    /// about, and nearly impossible to reuse. The Axiom Coverage Law requires
    /// every branch to be reachable; a 200-line function with eight nested
    /// conditions is evidence that the design, not the tests, is broken.
    /// Extract helper functions. Name each responsibility. An over-long body
    /// is a junk drawer wearing a `fn` keyword.
    ///
    /// ### Example
    ///
    /// ```rust
    /// // FLAGGED — 121+ lines of sequential logic in one function:
    /// fn do_everything(state: &mut World) {
    ///     // ... 150 lines ...
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// fn do_everything(state: &mut World) {
    ///     update_transforms(state);
    ///     resolve_collisions(state);
    ///     flush_events(state);
    /// }
    ///
    /// fn update_transforms(state: &mut World) { /* focused, testable */ }
    /// fn resolve_collisions(state: &mut World) { /* focused, testable */ }
    /// fn flush_events(state: &mut World) { /* focused, testable */ }
    /// ```
    pub ENGINE_NO_LARGE_FUNCTIONS,
    Warn,
    "engine function exceeds the line-count budget"
}

/// Maximum source lines for one engine function body. TUNABLE — keep as a
/// named const; the orchestrator verifies the real engine stays under it.
const MAX_LINES: usize = 120;

impl<'tcx> LateLintPass<'tcx> for EngineNoLargeFunctions {
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        kind: FnKind<'tcx>,
        _decl: &'tcx FnDecl<'tcx>,
        _body: &'tcx Body<'tcx>,
        span: Span,
        def_id: LocalDefId,
    ) {
        // Only real named functions / methods — skip closures (their size is
        // accounted for by the enclosing named function).
        if let FnKind::Closure = kind {
            return;
        }
        // Macro-generated code can be arbitrarily long; blame the macro author,
        // not the call site.
        if span.from_expansion() {
            return;
        }
        // Measure the function's span in source lines.
        let sm = cx.tcx.sess.source_map();
        let lo = sm.lookup_char_pos(span.lo()).line;
        let hi = sm.lookup_char_pos(span.hi()).line;
        let lines = hi.saturating_sub(lo) + 1;
        if lines <= MAX_LINES {
            return;
        }
        // Only engine spine source — crates/<layer>/src/ and modules/<mod>/src/.
        if !is_engine_file(cx, span) {
            return;
        }
        // Exempt test functions; long #[test] bodies are normal in this repo
        // (they set up elaborate state for a single assertion).
        let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
        if is_in_test(cx.tcx, hir_id) {
            return;
        }
        span_lint_and_help(
            cx,
            ENGINE_NO_LARGE_FUNCTIONS,
            span,
            format!(
                "this engine function is {lines} lines long; the limit is {MAX_LINES}"
            ),
            None,
            "extract helper functions and name each responsibility; \
             an over-long function hides its design from future agents",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
