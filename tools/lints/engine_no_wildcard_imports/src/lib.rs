#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::is_engine_file;
use rustc_hir::{Item, ItemKind, UseKind};
use rustc_lint::{LateContext, LateLintPass};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags glob (`use foo::*`) import statements in **non-test engine code** —
    /// the layer crates under `crates/` (except `xtask` and `axiom-zones`) and the
    /// modules under `modules/`. Apps, tooling, and all test code are exempt.
    ///
    /// This includes `pub use foo::*` re-exports as well as private glob imports.
    ///
    /// ### Why is this bad?
    ///
    /// Wildcard imports hide which symbols a module actually uses. In an agentic
    /// codebase where dozens of agents read and write engine code cold, a
    /// `use foo::*` forces every reader to mentally expand the glob to know what
    /// names are in scope. Specific imports (`use foo::{A, B}`) make the symbol
    /// set greppable: you can search for `A` or `B` and find every use site.
    /// Glob imports also make unintended symbol capture invisible — a new item
    /// added to `foo` silently enters scope everywhere the glob appears.
    ///
    /// Axiom's engine is designed to survive agent-driven development. That
    /// requires every name in scope to be explicitly visible, not hidden behind
    /// a glob.
    ///
    /// ### Example
    ///
    /// ```rust
    /// use std::collections::*;   // ← banned: which types are in scope?
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// use std::collections::{BTreeMap, BTreeSet};   // explicit, greppable
    /// ```
    pub ENGINE_NO_WILDCARD_IMPORTS,
    Warn,
    "glob (`use foo::*`) imports in engine code"
}

impl<'tcx> LateLintPass<'tcx> for EngineNoWildcardImports {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        let ItemKind::Use(_, UseKind::Glob) = item.kind else {
            return;
        };
        // Skip globs synthesised by the compiler (e.g. the implicit prelude).
        if item.span.from_expansion() {
            return;
        }
        // Test code (inside `#[test]` fns or `#[cfg(test)]` modules) may use globs.
        if is_in_test(cx.tcx, item.hir_id()) {
            return;
        }
        // Only fire for engine-spine source files.
        if !is_engine_file(cx, item.span) {
            return;
        }
        span_lint_and_help(
            cx,
            ENGINE_NO_WILDCARD_IMPORTS,
            item.span,
            "glob import (`use foo::*`) is banned in engine code",
            None,
            "import the specific items you use so the symbol set is greppable: `use foo::{A, B}`",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
