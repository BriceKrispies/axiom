#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::is_engine_file;
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags `.unwrap()` (and `unwrap_err` / `unwrap_unchecked`) in **non-test
    /// engine code** — the layer crates under `crates/` (except the `xtask` tool)
    /// and the modules under `modules/`. Apps, tooling, and all test code (`#[test]`
    /// functions and `#[cfg(test)]` modules) are exempt.
    ///
    /// ### Why is this bad?
    ///
    /// `.unwrap()` is an *undocumented* panic. Axiom's engine handles failure
    /// explicitly through its kernel result/error types; an unannounced panic on
    /// the hot path is a determinism and robustness hazard, and it hides which
    /// invariants the code actually depends on. `.expect("<why it can't fail>")`
    /// is the sanctioned escape hatch for a genuinely-impossible case, because it
    /// documents the invariant at the call site.
    ///
    /// ### Example
    ///
    /// ```rust
    /// let v = table.lookup(id).unwrap();
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// let v = table.lookup(id)?;                          // propagate the error
    /// let v = table.lookup(id).expect("id was just inserted"); // documented invariant
    /// ```
    pub UNWRAP_IN_ENGINE,
    Warn,
    "`.unwrap()` in non-test engine (layer/module) code"
}

/// Panicking unwrap-family methods. The non-panicking combinators
/// (`unwrap_or`, `unwrap_or_else`, `unwrap_or_default`) are deliberately absent.
const BANNED: &[&str] = &["unwrap", "unwrap_err", "unwrap_unchecked"];

impl<'tcx> LateLintPass<'tcx> for UnwrapInEngine {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::MethodCall(seg, ..) = expr.kind else {
            return;
        };
        if !BANNED.contains(&seg.ident.name.as_str()) {
            return;
        }
        // Don't blame the call site for an unwrap a macro expanded into it.
        if expr.span.from_expansion() {
            return;
        }
        // Tests (and `#[cfg(test)]` helpers) may unwrap freely.
        if is_in_test(cx.tcx, expr.hir_id) {
            return;
        }
        if !is_engine_file(cx, expr.span) {
            return;
        }
        span_lint_and_help(
            cx,
            UNWRAP_IN_ENGINE,
            seg.ident.span,
            "this `unwrap`-family call panics; it is banned in non-test engine code",
            None,
            "propagate the error with `?`, or use `.expect(\"<why it can't fail>\")` for a documented invariant",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
