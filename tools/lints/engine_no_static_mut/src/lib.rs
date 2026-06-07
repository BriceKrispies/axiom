#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::is_engine_file;
use rustc_hir::{Item, ItemKind, Mutability};
use rustc_lint::{LateContext, LateLintPass};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags `static mut` declarations in **non-test engine code** — the layer
    /// crates under `crates/` (except `xtask`) and the modules under `modules/`.
    /// Apps, tooling, and all test code are exempt.
    ///
    /// ### Why is this bad?
    ///
    /// `static mut` is process-global mutable state. It breaks determinism
    /// (two ticks can observe different values), breaks reentrancy (concurrent
    /// or reentrant access is instant UB in Rust), and hides state that should
    /// flow explicitly through the runtime. Axiom's engine is built around
    /// explicit ownership and typed handles — state belongs in a data structure
    /// threaded through the call graph, not in a global slot.
    ///
    /// ### Example
    ///
    /// ```rust
    /// static mut COUNTER: u32 = 0; // banned
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// // Thread owned state through the runtime, e.g.:
    /// struct EngineState { counter: u32 }
    /// // or use a typed handle exposed through the layer API.
    /// ```
    pub ENGINE_NO_STATIC_MUT,
    Warn,
    "`static mut` / global mutable state in engine code"
}

impl<'tcx> LateLintPass<'tcx> for EngineNoStaticMut {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        // On nightly-2026-04-16 the shape is:
        //   ItemKind::Static(Mutability, Ident, &'hir Ty<'hir>, BodyId)
        // Mutability is the first field.
        let ItemKind::Static(mutability, _ident, _ty, _body) = item.kind else {
            return;
        };
        if mutability != Mutability::Mut {
            return;
        }
        if item.span.from_expansion() {
            return;
        }
        if is_in_test(cx.tcx, item.hir_id()) {
            return;
        }
        if !is_engine_file(cx, item.span) {
            return;
        }
        span_lint_and_help(
            cx,
            ENGINE_NO_STATIC_MUT,
            item.span,
            "`static mut` is banned in engine code: it breaks determinism and reentrancy",
            None,
            "thread owned state through the runtime or use a typed handle instead of process-global mutable state",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
