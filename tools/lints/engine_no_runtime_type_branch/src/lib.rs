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
    /// Flags runtime type reflection APIs — `downcast_ref`, `downcast_mut`,
    /// `downcast`, and `TypeId::of` — in **non-test engine code** (layer crates
    /// under `crates/` and modules under `modules/`). Apps, tooling, and all test
    /// code (`#[test]` functions and `#[cfg(test)]` modules) are exempt.
    ///
    /// > **Note:** Detection of bare `dyn Any` trait bounds is out of scope for
    /// > Tier-1. This lint catches the *call sites* where the runtime type branch
    /// > actually executes; trait-bound auditing is a separate, harder analysis.
    ///
    /// ### Why is this bad?
    ///
    /// Axiom's engine is built on a static, deterministic data model: every datum
    /// has a concrete, statically-known type at the call site, and every dispatch
    /// path is decided at compile time. `Any`/`TypeId`/`downcast` punches a hole
    /// in that model:
    ///
    /// - It makes control flow depend on the *runtime identity* of a type, not on
    ///   its statically declared structure — which is exactly the hidden branching
    ///   that makes engine code hard to reason about, test, and replay.
    /// - A `downcast_ref` that silently returns `None` for an unexpected type can
    ///   swallow bugs rather than surfacing them as compile-time or invariant errors.
    /// - `TypeId` values are not stable across compiler versions or build sessions,
    ///   so any serialisation / replay path that leaks them is non-deterministic.
    ///
    /// ### Example
    ///
    /// ```rust
    /// use std::any::Any;
    ///
    /// fn handle(value: &dyn Any) {
    ///     if let Some(n) = value.downcast_ref::<u32>() {
    ///         // runtime type branch — banned in engine code
    ///     }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// // Use an explicit enum or a typed dispatch table so the compiler knows all
    /// // cases and can enforce exhaustiveness:
    /// enum Value { Int(u32), Float(f32) }
    ///
    /// fn handle(value: &Value) {
    ///     match value {
    ///         Value::Int(n) => { /* … */ }
    ///         Value::Float(f) => { /* … */ }
    ///     }
    /// }
    /// ```
    pub ENGINE_NO_RUNTIME_TYPE_BRANCH,
    Warn,
    "runtime type reflection (`Any`/`TypeId`/`downcast`) in engine code"
}

/// Method names that perform a runtime downcast or type-identity check.
const BANNED_METHODS: &[&str] = &["downcast_ref", "downcast_mut", "downcast", "type_id"];

/// Path-call suffixes for runtime type identity.
const BANNED_PATHS: &[&str] = &["TypeId::of"];

impl<'tcx> LateLintPass<'tcx> for EngineNoRuntimeTypeBranch {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let hit = match expr.kind {
            ExprKind::MethodCall(seg, ..) => BANNED_METHODS.contains(&seg.ident.name.as_str()),
            ExprKind::Call(callee, _) => {
                if let ExprKind::Path(ref q) = callee.kind {
                    if let Some(did) = cx.qpath_res(q, callee.hir_id).opt_def_id() {
                        let p = cx.tcx.def_path_str(did);
                        BANNED_PATHS.iter().any(|b| p.ends_with(b))
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        if !hit {
            return;
        }
        // Don't blame the call site for a downcast a macro expanded into it.
        if expr.span.from_expansion() {
            return;
        }
        // Tests (and `#[cfg(test)]` helpers) may use type reflection freely.
        if is_in_test(cx.tcx, expr.hir_id) {
            return;
        }
        if !is_engine_file(cx, expr.span) {
            return;
        }
        span_lint_and_help(
            cx,
            ENGINE_NO_RUNTIME_TYPE_BRANCH,
            expr.span,
            "runtime type reflection is banned in non-test engine code; it defeats the engine's static, deterministic data model".to_string(),
            None,
            "use an explicit enum or typed dispatch instead of `Any`/`TypeId`/`downcast`",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
