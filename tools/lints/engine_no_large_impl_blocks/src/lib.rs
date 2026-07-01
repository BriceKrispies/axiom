#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::is_engine_file;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};

dylint_linting::declare_late_lint! {
    /// ### What it does
    /// Flags `impl` blocks in **engine code** (layers under `crates/` and modules
    /// under `modules/`) that have more than [`MAX_ITEMS`] associated items
    /// (methods, associated constants, and associated types combined).
    /// ### Why is this bad?
    /// An `impl` block with dozens of methods is a god-object smell. It means one
    /// type is carrying too many responsibilities, making the code hard to reason
    /// about, hard to test in isolation, and hard for future agents to navigate.
    /// In Axiom's strict layered architecture, each type should own a focused,
    /// well-bounded capability. When an impl block exceeds the limit, split the
    /// behavior into focused traits or break the type into smaller, more
    /// purposeful types.
    /// ### Example
    /// ```rust
    /// // BAD — one impl block with 32 methods signals a god object
    /// struct Engine;
    /// impl Engine {
    ///     fn init(&self) {}
    ///     fn update(&self) {}
    ///     fn render(&self) {}
    ///     // ... 29 more methods ...
    /// }
    /// ```
    /// Use instead:
    /// ```rust
    /// // GOOD — split into focused traits
    /// trait Lifecycle { fn init(&self); fn update(&self); }
    /// trait Renderer  { fn render(&self); }
    /// struct Engine;
    /// impl Lifecycle for Engine { fn init(&self) {} fn update(&self) {} }
    /// impl Renderer  for Engine { fn render(&self) {} }
    /// ```
    pub ENGINE_NO_LARGE_IMPL_BLOCKS,
    Warn,
    "engine impl block has too many items"
}

/// Maximum associated items allowed in one engine `impl` block.
const MAX_ITEMS: usize = 30;

impl<'tcx> LateLintPass<'tcx> for EngineNoLargeImplBlocks {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        let ItemKind::Impl(imp) = item.kind else {
            return;
        };
        let n = imp.items.len();
        if n <= MAX_ITEMS {
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
            ENGINE_NO_LARGE_IMPL_BLOCKS,
            item.span,
            format!(
                "engine impl block has {n} items, which exceeds the limit of {MAX_ITEMS}"
            ),
            None,
            "split behavior into focused traits or smaller types; a large impl block is a god-object smell",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
