#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::{in_zone, is_engine_file, markers};
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags wall-clock / monotonic time reads (`Instant::now`,
    /// `SystemTime::now`) inside a `#[sim]` zone (a function or module marked
    /// with `axiom_zones::sim`).
    ///
    /// ### Why is this bad?
    ///
    /// Deterministic simulation must be a pure function of its seeded inputs.
    /// Reading the wall clock makes a tick non-reproducible — the same inputs
    /// replay to a different result. Time must enter the sim as an explicit
    /// input (a fixed tick / step), never be sampled from the environment.
    ///
    /// ### Example
    ///
    /// ```rust
    /// #[axiom_zones::sim]
    /// fn step() {
    ///     let now = std::time::Instant::now(); // non-deterministic
    /// }
    /// ```
    pub NO_TIME_IN_SIM,
    Warn,
    "wall-clock / monotonic time read inside a `#[sim]` zone"
}

/// Time-source associated functions that read the environment clock. Matched by
/// def-path suffix so the `std`/`core` prefix doesn't matter.
const BANNED_TIME: &[&str] = &["Instant::now", "SystemTime::now"];

impl<'tcx> LateLintPass<'tcx> for NoTimeInSim {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::Call(callee, _) = expr.kind else {
            return;
        };
        let ExprKind::Path(ref qpath) = callee.kind else {
            return;
        };
        let Some(def_id) = cx.qpath_res(qpath, callee.hir_id).opt_def_id() else {
            return;
        };
        let path = cx.tcx.def_path_str(def_id);
        if !BANNED_TIME.iter().any(|banned| path.ends_with(banned)) {
            return;
        }
        if expr.span.from_expansion() {
            return;
        }
        if is_in_test(cx.tcx, expr.hir_id) {
            return;
        }
        if !is_engine_file(cx, expr.span) {
            return;
        }
        if !in_zone(cx, expr.hir_id, markers::SIM) {
            return;
        }
        span_lint_and_help(
            cx,
            NO_TIME_IN_SIM,
            expr.span,
            "reading the clock inside a `#[sim]` zone makes the simulation non-deterministic",
            None,
            "take time as an explicit seeded input (a fixed tick / step), never sample it here",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
