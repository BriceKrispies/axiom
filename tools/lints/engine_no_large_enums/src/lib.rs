#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::is_engine_file;
use rustc_hir::{Item, ItemKind};
use rustc_lint::{LateContext, LateLintPass};

dylint_linting::declare_late_lint! {
    /// ### What it does
    /// Flags `enum` declarations in **non-test engine code** (layer crates under
    /// `crates/` and module crates under `modules/`) that have more than
    /// `MAX_VARIANTS` (currently 24) variants.
    /// ### Why is this bad?
    /// An enum with a very large number of variants is usually a sign that the
    /// type is doing too many jobs. Large enums create broad `match` arms that are
    /// hard to extend, make exhaustive coverage expensive, and often signal that a
    /// single discriminant is encoding what should be a composition of smaller,
    /// focused types. In a game engine this also matters for cache: every branch
    /// arm the optimizer must consider is work, and every match the CPU must
    /// speculate through is heat. A focused enum is fast, legible, and testable.
    /// The usual fix is to split the discriminant space: introduce sub-enums
    /// grouped by semantic category (`InputEvent::Keyboard(…)` +
    /// `InputEvent::Mouse(…)` instead of 30 flat variants), or replace the enum
    /// with a struct carrying a smaller tag plus a payload.
    /// ### Example
    /// ```rust
    /// // BAD — 25 variants, all flat, hard to match and extend
    /// enum EngineEvent {
    ///     KeyPress, KeyRelease, MouseMove, MouseClick, MouseRelease,
    ///     MouseScroll, GamepadButton, GamepadAxis, GamepadConnect,
    ///     GamepadDisconnect, WindowResize, WindowFocus, WindowBlur,
    ///     WindowClose, TouchStart, TouchMove, TouchEnd, TouchCancel,
    ///     NetworkConnect, NetworkDisconnect, NetworkData, NetworkError,
    ///     AudioStart, AudioStop, AudioError,
    /// }
    /// ```
    /// Use instead:
    /// ```rust
    /// // GOOD — sub-enums keep each arm count small and each type focused
    /// enum InputEvent { KeyPress, KeyRelease, MouseMove, /* … */ }
    /// enum NetworkEvent { Connect, Disconnect, Data, Error }
    /// enum AudioEvent { Start, Stop, Error }
    /// enum EngineEvent {
    ///     Input(InputEvent),
    ///     Network(NetworkEvent),
    ///     Audio(AudioEvent),
    /// }
    /// ```
    pub ENGINE_NO_LARGE_ENUMS,
    Warn,
    "engine enum has too many variants"
}

/// Maximum number of variants permitted on a single engine enum before the lint
/// fires. TUNABLE — if the architecture evolves and this bound needs to change,
/// edit this constant; the change is local to this crate.
const MAX_VARIANTS: usize = 24;

impl<'tcx> LateLintPass<'tcx> for EngineNoLargeEnums {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        // ItemKind::Enum(ident, generics, enum_def) on nightly-2026-04-16.
        let ItemKind::Enum(_ident, _generics, enum_def) = item.kind else {
            return;
        };
        let n = enum_def.variants.len();
        if n <= MAX_VARIANTS {
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
            ENGINE_NO_LARGE_ENUMS,
            item.span,
            format!(
                "this enum has {n} variants, which exceeds the engine limit of {MAX_VARIANTS}"
            ),
            None,
            "split into focused sub-enums grouped by semantic category, \
             or replace the discriminant space with a struct + smaller tag",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
