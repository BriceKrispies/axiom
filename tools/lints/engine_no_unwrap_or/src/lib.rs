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
    ///
    /// Flags `.unwrap_or(..)` in **non-test engine code** — the layer crates
    /// under `crates/` (except `xtask` and the `axiom-zones` support crate) and
    /// the modules under `modules/`. Apps, tooling, and all test code are
    /// exempt, exactly as for `engine_no_branching` and `no_unwrap_in_engine`.
    ///
    /// ### Why is this bad?
    ///
    /// `.unwrap_or(b)` is a **value-level branch wearing a combinator's
    /// clothes**: it selects between the carried value and a fallback on the
    /// `Option`/`Result` discriminant. `engine_no_branching` cannot see it —
    /// there is no `if`/`match` in the HIR — so it is exactly where the
    /// Branchless Law's pressure escapes to. Worse, the fallback is an
    /// *eagerly evaluated* default chosen at the use site, which is where
    /// absence gets papered over: a missing entry, an out-of-range index, or a
    /// failed lookup silently becomes `0`, `Default::default()`, or an identity
    /// value, and the surrounding code can no longer tell "absent" from
    /// "genuinely this value".
    ///
    /// The structural fix is to remove the optionality rather than to default
    /// it: make the producer total (return the value, not `Option<value>`),
    /// carry the discriminant in the data contract, or push the fallback down
    /// into the lower layer that actually owns the default. Where a fallback is
    /// genuinely part of the contract, name it — a domain method whose signature
    /// says what the default *means* — instead of an anonymous `.unwrap_or(0)`
    /// at the call site.
    ///
    /// ### Example
    ///
    /// ```rust
    /// let speed = table.get(&id).copied().unwrap_or(0.0);
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// // the producer is total — there is nothing to unwrap
    /// let speed = table.speed_of(id);
    /// // or the fallback is named, and owned by the layer that defines it
    /// let speed = table.speed_or_rest(id);
    /// ```
    pub ENGINE_NO_UNWRAP_OR,
    Warn,
    "`.unwrap_or(..)` (a value-level branch) in non-test engine code"
}

/// The method this lint bans. Only the eager, value-taking `unwrap_or` — the
/// lazy `unwrap_or_else` and `unwrap_or_default` are a deliberately separate
/// question and are **not** flagged here.
const BANNED: &str = "unwrap_or";

impl<'tcx> LateLintPass<'tcx> for EngineNoUnwrapOr {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::MethodCall(seg, ..) = expr.kind else {
            return;
        };
        if seg.ident.name.as_str() != BANNED {
            return;
        }
        // Don't blame the call site for an `unwrap_or` a macro expanded into it.
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
            ENGINE_NO_UNWRAP_OR,
            seg.ident.span,
            "`.unwrap_or(..)` is a value-level branch; it is banned in non-test engine code",
            None,
            "make the producer total, carry the discriminant in the data contract, or push the named default down into the layer that owns it",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
