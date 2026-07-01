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
    /// Flags structs in **engine code** (layers under `crates/` and modules under
    /// `modules/`) that have more than [`MAX_FIELDS`] named or tuple fields.
    /// ### Why is this bad?
    /// A struct with dozens of fields is a design smell — it is doing too much,
    /// knows too much, or has not been divided into focused sub-types. In Axiom's
    /// strict layered architecture, god-structs leak responsibilities across
    /// boundaries and make the data model opaque to future agents. The limit is a
    /// forcing function: if you need more fields, restructure first.
    /// ### Example
    /// ```rust
    /// // BAD — one struct owns 26 unrelated knobs
    /// struct WorldState {
    ///     f0: u8, f1: u8, f2: u8, f3: u8, f4: u8,
    ///     f5: u8, f6: u8, f7: u8, f8: u8, f9: u8,
    ///     f10: u8, f11: u8, f12: u8, f13: u8, f14: u8,
    ///     f15: u8, f16: u8, f17: u8, f18: u8, f19: u8,
    ///     f20: u8, f21: u8, f22: u8, f23: u8, f24: u8,
    ///     f25: u8,
    /// }
    /// ```
    /// Use instead:
    /// ```rust
    /// // GOOD — cluster related fields into named sub-structs
    /// struct PhysicsState { position: Vec3, velocity: Vec3, mass: f32 }
    /// struct RenderState  { color: [f32; 4], visible: bool }
    /// struct WorldState   { physics: PhysicsState, render: RenderState, ... }
    /// ```
    pub ENGINE_NO_LARGE_STRUCTS,
    Warn,
    "engine struct has too many fields"
}

/// Maximum number of fields allowed on a single engine struct.
const MAX_FIELDS: usize = 24;

impl<'tcx> LateLintPass<'tcx> for EngineNoLargeStructs {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        let ItemKind::Struct(_, _, data) = item.kind else {
            return;
        };
        let n = data.fields().len();
        if n <= MAX_FIELDS {
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
            ENGINE_NO_LARGE_STRUCTS,
            item.span,
            format!(
                "engine struct has {n} fields, which exceeds the limit of {MAX_FIELDS}"
            ),
            None,
            "group related fields into focused sub-structs; a god-struct is a design smell",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
