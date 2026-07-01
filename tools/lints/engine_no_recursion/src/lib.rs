#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::is_engine_file;
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_hir::intravisit::{self, FnKind, Visitor};
use rustc_hir::{Body, Expr, ExprKind, FnDecl};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::TypeckResults;
use rustc_span::Span;

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags a function in **non-test engine code** — the layer crates under
    /// `crates/` (except the `xtask` tool) and the modules under `modules/` —
    /// whose own body contains a call that resolves back to itself, i.e. a
    /// **direct (self-) recursive call**. The call may be a plain function call
    /// (`down(n - 1)`) or a method call that dispatches to the same `DefId`.
    /// Apps, tooling, and all test code (`#[test]` functions and `#[cfg(test)]`
    /// modules) are exempt.
    ///
    /// **Scope: direct self-recursion only.** Indirect / mutual recursion (`a`
    /// calls `b`, `b` calls `a`) is *not* detected by this Tier-1 lint and is a
    /// possible future enhancement (it requires whole-program call-graph
    /// analysis rather than a single-body walk).
    ///
    /// ### Why is this bad?
    ///
    /// Unbounded recursion risks stack overflow and is non-obvious about its
    /// bound — the recursion depth is implicit, scattered across call sites, and
    /// invisible at the function signature. Axiom forbids that on the runtime
    /// path: every loop on the engine spine must have an explicit, inspectable
    /// bound. An explicit `loop`/`while` (or a worklist/stack you push and pop)
    /// makes the bound and the working-set size first-class and reviewable.
    ///
    /// ### Example
    ///
    /// ```rust
    /// fn count(n: u32) -> u32 {
    ///     if n == 0 { 0 } else { count(n - 1) } // recursive call: flagged
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// fn count(mut n: u32) -> u32 {
    ///     let mut acc = 0;
    ///     while n != 0 { acc += 1; n -= 1; } // explicit, bounded loop
    ///     acc
    /// }
    /// ```
    pub ENGINE_NO_RECURSION,
    Warn,
    "direct recursive call in non-test engine (layer/module) code"
}

/// Walks one function body for the first call resolving to `me` (the
/// function's own `DefId`). Deliberately does not descend into nested bodies
/// (closures, nested `fn`s) — those get their own `check_fn` invocation.
struct SelfCallFinder<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    me: DefId,
    typeck: &'tcx TypeckResults<'tcx>,
    hit: Option<Span>,
}

impl<'a, 'tcx> Visitor<'tcx> for SelfCallFinder<'a, 'tcx> {
    fn visit_expr(&mut self, ex: &'tcx Expr<'tcx>) {
        if self.hit.is_some() {
            return;
        }
        let callee = match ex.kind {
            ExprKind::Call(c, _) => match c.kind {
                ExprKind::Path(ref q) => self.cx.qpath_res(q, c.hir_id).opt_def_id(),
                _ => None,
            },
            ExprKind::MethodCall(..) => self.typeck.type_dependent_def_id(ex.hir_id),
            _ => None,
        };
        if callee == Some(self.me) && !ex.span.from_expansion() {
            self.hit = Some(ex.span);
            return;
        }
        intravisit::walk_expr(self, ex);
    }
}

impl<'tcx> LateLintPass<'tcx> for EngineNoRecursion {
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        _kind: FnKind<'tcx>,
        _decl: &'tcx FnDecl<'tcx>,
        body: &'tcx Body<'tcx>,
        span: Span,
        def_id: LocalDefId,
    ) {
        if span.from_expansion() {
            return;
        }
        if !is_engine_file(cx, span) {
            return;
        }
        let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
        if is_in_test(cx.tcx, hir_id) {
            return;
        }
        let typeck = cx.tcx.typeck(def_id);
        let mut finder = SelfCallFinder {
            cx,
            me: def_id.to_def_id(),
            typeck,
            hit: None,
        };
        finder.visit_body(body);
        if let Some(call_span) = finder.hit {
            span_lint_and_help(
                cx,
                ENGINE_NO_RECURSION,
                call_span,
                "this function calls itself; direct recursion is banned in non-test engine code"
                    .to_string(),
                None,
                "rewrite as an explicit bounded loop, or an explicit worklist/stack you push and pop",
            );
        }
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
