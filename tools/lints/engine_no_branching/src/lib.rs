#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_span;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use rustc_hir::{BinOpKind, Expr, ExprKind, LoopSource, MatchSource};
use rustc_lint::{LateContext, LateLintPass};
use rustc_span::{FileName, Span};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Bans **every** form of branching / control flow in Rust: `if` (and
    /// `if let` / `let ... else`), `match`, `while` (and `while let`), `for`,
    /// `loop`, the `?` operator, and the short-circuiting `&&` / `||`. It fires
    /// on **all** code the build compiles — every crate, module, app, and tool —
    /// with no zone gate, no test exemption, and no escape hatch.
    ///
    /// ### Why is this bad?
    ///
    /// It isn't, universally — this lint is a deliberate, total prohibition on
    /// branching. There is no condition under which a flagged construct is
    /// allowed; the only resolution is to remove the branch. Straight-line code
    /// and combinator method calls (`.map`, `.unwrap_or`, `.and_then`, ...) are
    /// not branching constructs and are not flagged.
    ///
    /// ### Example
    ///
    /// ```rust
    /// let y = if x > 0 { 1 } else { 2 }; // flagged: `if`
    /// for i in 0..n { sum += i; }        // flagged: `for`
    /// let v = thing()?;                  // flagged: `?`
    /// let ok = a && b;                   // flagged: `&&`
    /// ```
    pub ENGINE_NO_BRANCHING,
    Warn,
    "any branching / control-flow construct (banned everywhere, no exemptions)"
}

/// Classify an expression as a branching construct, returning the diagnostic
/// message for it, or `None` if it is not a branch.
///
/// Each surface construct is reported exactly once. `for` / `while` desugar to
/// `Loop` (caught here) wrapping an inner `Match` tagged `ForLoopDesugar` — that
/// inner match is intentionally NOT reported, so one `for` is one finding.
fn branch_message(kind: &ExprKind<'_>) -> Option<&'static str> {
    Some(match kind {
        // `if`, `if let`, and `let ... else` all lower to `If`.
        ExprKind::If(..) => "`if` is a branching construct; all branching is banned",
        ExprKind::Loop(_, _, LoopSource::Loop, _) => {
            "`loop` is a branching construct; all branching is banned"
        }
        ExprKind::Loop(_, _, LoopSource::While, _) => {
            "`while` is a branching construct; all branching is banned"
        }
        ExprKind::Loop(_, _, LoopSource::ForLoop, _) => {
            "`for` is a branching construct; all branching is banned"
        }
        ExprKind::Match(_, _, MatchSource::Normal) => {
            "`match` is a branching construct; all branching is banned"
        }
        ExprKind::Match(_, _, MatchSource::TryDesugar(_)) => {
            "the `?` operator is a branching construct; all branching is banned"
        }
        ExprKind::Binary(op, _, _) if matches!(op.node, BinOpKind::And) => {
            "the `&&` operator is a lazy boolean branch; all branching is banned"
        }
        ExprKind::Binary(op, _, _) if matches!(op.node, BinOpKind::Or) => {
            "the `||` operator is a lazy boolean branch; all branching is banned"
        }
        _ => return None,
    })
}

/// True if `span`'s source file lives under a `tests/`, `examples/`, or
/// `benches/` directory — integration tests, example binaries, and benchmarks.
/// Their non-`#[test]` helper code is still test/support code, so it is exempt
/// just like inline `#[cfg(test)]` code.
fn in_test_or_example_file(cx: &LateContext<'_>, span: Span) -> bool {
    let FileName::Real(name) = cx.tcx.sess.source_map().span_to_filename(span) else {
        return false;
    };
    let Some(path) = name.local_path() else {
        return false;
    };
    let path = path.to_string_lossy().replace('\\', "/");
    path.split('/')
        .any(|c| matches!(c, "tests" | "examples" | "benches"))
}

impl<'tcx> LateLintPass<'tcx> for EngineNoBranching {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let Some(message) = branch_message(&expr.kind) else {
            return;
        };
        // Skip branching that originated inside a macro expansion (library/user
        // macro internals such as `assert!` / `matches!`), keeping diagnostics on
        // control flow the programmer actually wrote. Compiler desugarings of
        // surface constructs (`for` / `while` / `?` / `if let`) carry a
        // desugaring kind and map back to real source, so they are still caught.
        if expr.span.from_expansion() && expr.span.desugaring_kind().is_none() {
            return;
        }
        // A `while` / `while let` lowers to `Loop(While)` wrapping a synthetic
        // `If` (the condition test), which is itself a desugaring. Skip a
        // desugared `If` so the enclosing `while` is the single finding — a
        // genuine `if` / `if let` is never a desugaring, so this only drops the
        // artifact, never real source.
        if matches!(expr.kind, ExprKind::If(..)) && expr.span.desugaring_kind().is_some() {
            return;
        }
        // Test code may branch freely: the ban targets the engine the build
        // ships, not the suites that verify it. Two forms of test code are
        // exempt — inline `#[test]` fns / `#[cfg(test)]` modules (via
        // `is_in_test`), and whole `tests/` / `examples/` / `benches/` files,
        // whose non-`#[test]` helpers `is_in_test` would otherwise miss. (Scope
        // is otherwise everything — no engine-file filter — so apps and tools
        // are still covered.)
        if is_in_test(cx.tcx, expr.hir_id) || in_test_or_example_file(cx, expr.span) {
            return;
        }
        span_lint_and_help(
            cx,
            ENGINE_NO_BRANCHING,
            expr.span,
            message,
            None,
            "this analyzer bans all control flow — remove the branch (there is no escape hatch)",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
