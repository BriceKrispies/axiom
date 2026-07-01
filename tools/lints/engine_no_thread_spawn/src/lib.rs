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
    /// Flags direct OS thread spawning (`std::thread::spawn` and
    /// `std::thread::Builder::spawn`) in **non-test engine code** — the layer
    /// crates under `crates/` (except `xtask` and `axiom-zones`) and the modules
    /// under `modules/`. Apps, tooling, and all test code are exempt.
    /// ### Why is this bad?
    /// Axiom is a **WASM-first, deterministic** engine. Raw OS threads break both
    /// properties at once:
    /// - **WASM**: `std::thread::spawn` does not exist in the standard WASM target
    ///   (`wasm32-unknown-unknown`). Calling it from engine code makes the crate
    ///   non-portable and will fail to compile for the primary target.
    /// - **Determinism**: OS thread scheduling is non-deterministic. Engine code
    ///   that touches threads cannot be replayed, fuzz-tested, or simulated
    ///   repeatably. Concurrency must flow through the engine's runtime/scheduler,
    ///   which controls ordering and is testable.
    /// If work parallelism is genuinely needed, it must go through the engine's
    /// sanctioned scheduler API, not the raw OS thread primitive.
    /// ### Example
    /// ```rust
    /// // FLAGGED — raw thread spawn in engine code
    /// let _ = std::thread::spawn(|| do_work());
    /// // FLAGGED — Builder variant
    /// let _ = std::thread::Builder::new().spawn(|| do_work());
    /// ```
    /// Use instead:
    /// ```rust
    /// // Submit work through the engine's runtime/scheduler (when it exists).
    /// runtime.schedule(|| do_work());
    /// ```
    pub ENGINE_NO_THREAD_SPAWN,
    Warn,
    "direct OS thread/task spawning in engine code"
}

impl<'tcx> LateLintPass<'tcx> for EngineNoThreadSpawn {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        // Resolve the called def_id for BOTH a free-fn path call (thread::spawn)
        // and a method call (Builder::spawn).
        let def_id = match expr.kind {
            ExprKind::Call(callee, _) => {
                if let ExprKind::Path(ref qpath) = callee.kind {
                    cx.qpath_res(qpath, callee.hir_id).opt_def_id()
                } else {
                    None
                }
            }
            ExprKind::MethodCall(..) => {
                cx.typeck_results().type_dependent_def_id(expr.hir_id)
            }
            _ => None,
        };
        let Some(def_id) = def_id else { return };
        let path = cx.tcx.def_path_str(def_id);
        if !(path.ends_with("thread::spawn") || path.ends_with("Builder::spawn")) {
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
        span_lint_and_help(
            cx,
            ENGINE_NO_THREAD_SPAWN,
            expr.span,
            "direct OS thread spawning is banned in engine code",
            None,
            "the engine is WASM-first and deterministic — raw threads break both; \
             spawn work through the engine's runtime/scheduler instead",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
