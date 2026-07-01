#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

use std::collections::HashSet;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test_function;
use rustc_hir::intravisit::{FnKind, Visitor, walk_expr};
use rustc_hir::attrs::AttributeKind;
use rustc_hir::{Attribute, Body, Expr, ExprKind, FnDecl, QPath};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::hir::nested_filter;
use rustc_middle::ty::TyCtxt;
use rustc_span::Span;
use rustc_span::def_id::{DefId, LocalDefId};
use rustc_span::hygiene::{ExpnKind, MacroKind};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags any `#[test]` function whose body contains no assertion. In **strict
    /// mode** (this lint's policy) an assertion is exactly one of:
    /// - an `assert!`/`assert_eq!`/`assert_ne!`/`debug_assert*!` (or
    ///   `panic!`/`unreachable!`) macro, anywhere in the body or a closure in it;
    /// - the `#[should_panic]` attribute (the expected panic *is* the check);
    /// - a call to a helper function that itself asserts — resolved
    ///   **semantically** by following the call into the helper's body (a few
    ///   levels deep), or a helper whose name contains `assert`.
    ///
    /// A bare `.unwrap()`/`.expect()` or `?` does **not** count: it proves the
    /// value wasn't the error variant, not that the behavior under test is right.
    ///
    /// ### Why is this bad?
    ///
    /// A test that executes code but asserts nothing is "coverage theater": it
    /// moves the coverage number without proving any behavior. Axiom's Coverage
    /// Law explicitly forbids "tests that execute code without asserting on its
    /// behavior". Such a test passes even if the code under it returns garbage.
    ///
    /// ### Example
    ///
    /// ```rust
    /// #[test]
    /// fn builds_a_thing() {
    ///     let _ = Thing::new();
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// #[test]
    /// fn builds_a_thing() {
    ///     assert_eq!(Thing::new().count(), 0);
    /// }
    /// ```
    pub TEST_WITHOUT_ASSERTION,
    Warn,
    "a `#[test]` function whose body contains no assertion"
}

/// Bang-macros that count as an assertion when found anywhere in the test body
/// (matched through the macro-expansion backtrace, so this is unaffected by the
/// macro having already been expanded into `if !cond { panic!(..) }`).
const ASSERT_MACROS: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "panic",
    "unreachable",
];

/// How many levels of helper calls to follow before giving up. Bounds the
/// semantic resolution so a deep/recursive helper graph can't loop or stall.
const MAX_HELPER_DEPTH: u32 = 4;

impl<'tcx> LateLintPass<'tcx> for TestWithoutAssertion {
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        kind: FnKind<'tcx>,
        _decl: &'tcx FnDecl<'tcx>,
        body: &'tcx Body<'tcx>,
        span: Span,
        def_id: LocalDefId,
    ) {
        if matches!(kind, FnKind::Closure) {
            return;
        }
        // Under `--test` the `#[test]` attribute macro is *consumed* (replaced by
        // a generated `#[rustc_test_marker]` const), so we can't match the raw
        // attribute; `is_in_test_function` matches the marker-name set instead.
        if !is_in_test_function(cx.tcx, body.value.hir_id) {
            return;
        }
        // Lint the test function itself, never a helper nested inside it (e.g. an
        // inline `impl Trait for S { fn run() {..} }`): if this fn's parent is
        // already inside a test fn, it's such a nested helper — skip it.
        let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
        if is_in_test_function(cx.tcx, cx.tcx.parent_hir_id(hir_id)) {
            return;
        }
        // `#[should_panic]` is lowered to `AttributeKind::ShouldPanic` (no longer
        // a raw `should_panic` path), so match the parsed kind, not the attribute.
        if is_should_panic(cx.tcx, hir_id) {
            return;
        }

        let mut finder = AssertionFinder {
            cx,
            visited: HashSet::new(),
            depth: 0,
            found: false,
        };
        finder.visit_body(body);
        if !finder.found {
            span_lint_and_help(
                cx,
                TEST_WITHOUT_ASSERTION,
                span,
                "this `#[test]` function contains no assertion",
                None,
                "assert on an observable result, or delete the test if it proves nothing",
            );
        }
    }
}

/// Is `hir_id` a `#[should_panic]` test? The attribute is lowered to a parsed
/// `AttributeKind::ShouldPanic`, so we match that rather than the path name.
fn is_should_panic(tcx: TyCtxt<'_>, hir_id: rustc_hir::HirId) -> bool {
    tcx.hir_attrs(hir_id)
        .iter()
        .any(|attr| matches!(attr, Attribute::Parsed(AttributeKind::ShouldPanic { .. })))
}

/// Walks a test body — including nested closure bodies — looking for the first
/// expression that constitutes an assertion. Follows calls into local helper
/// bodies (bounded by `MAX_HELPER_DEPTH` and `visited`). Stops once one is found.
struct AssertionFinder<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    /// Helper fns already being scanned, to break call cycles.
    visited: HashSet<LocalDefId>,
    depth: u32,
    found: bool,
}

impl<'tcx> Visitor<'tcx> for AssertionFinder<'_, 'tcx> {
    type NestedFilter = nested_filter::OnlyBodies;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.cx.tcx
    }

    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if self.found {
            return;
        }
        if self.expr_is_assertion(expr) {
            self.found = true;
            return;
        }
        walk_expr(self, expr);
    }
}

impl<'a, 'tcx> AssertionFinder<'a, 'tcx> {
    /// Is this single expression an assertion (strict policy)?
    fn expr_is_assertion(&mut self, expr: &Expr<'tcx>) -> bool {
        // An assertion/panic macro anywhere in this expression's expansion chain.
        for expn in expr.span.macro_backtrace() {
            if let ExpnKind::Macro(MacroKind::Bang, name) = expn.kind
                && ASSERT_MACROS.contains(&name.as_str())
            {
                return true;
            }
        }
        if let ExprKind::Call(callee, _) = expr.kind {
            return self.call_is_assertion(callee);
        }
        false
    }

    /// A direct function call: an assertion if the callee is named `*assert*`, or
    /// if it resolves to a local function whose own body asserts.
    fn call_is_assertion(&mut self, callee: &Expr<'tcx>) -> bool {
        let ExprKind::Path(QPath::Resolved(_, path)) = callee.kind else {
            return false;
        };
        if let Some(seg) = path.segments.last()
            && seg.ident.name.as_str().contains("assert")
        {
            return true;
        }
        path.res
            .opt_def_id()
            .is_some_and(|def_id| self.called_fn_asserts(def_id))
    }

    /// Does the local function `def_id` assert somewhere in its own body? Bounded
    /// by depth and a visited set so recursion always terminates.
    fn called_fn_asserts(&mut self, def_id: DefId) -> bool {
        if self.depth >= MAX_HELPER_DEPTH {
            return false;
        }
        // Non-local helpers (std, other crates) have no HIR body to inspect.
        let Some(local) = def_id.as_local() else {
            return false;
        };
        if !self.visited.insert(local) {
            return false;
        }
        let Some(body_id) = self.cx.tcx.hir_node_by_def_id(local).body_id() else {
            return false;
        };
        let body = self.cx.tcx.hir_body(body_id);
        let mut sub = AssertionFinder {
            cx: self.cx,
            visited: std::mem::take(&mut self.visited),
            depth: self.depth + 1,
            found: false,
        };
        sub.visit_body(body);
        self.visited = std::mem::take(&mut sub.visited);
        sub.found
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
