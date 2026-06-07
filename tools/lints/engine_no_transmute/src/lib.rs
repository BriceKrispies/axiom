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
    /// Flags `std::mem::transmute` and `std::mem::transmute_copy` in
    /// **non-test engine code** — the layer crates under `crates/` (except the
    /// `xtask` tool and `axiom-zones`) and the modules under `modules/`. Apps,
    /// tooling, and all test code (`#[test]` functions and `#[cfg(test)]` modules)
    /// are exempt.
    ///
    /// ### Why is this bad?
    ///
    /// `mem::transmute` is a raw memory reinterpretation with no type-system
    /// safety. It bypasses Rust's ownership, alignment, and validity guarantees,
    /// making it trivially easy to produce undefined behaviour. In an engine that
    /// must be deterministic and correct across WASM targets, transmutes are a
    /// reliability and portability hazard — endianness, padding, and ABI
    /// differences can all produce silent corruption.
    ///
    /// Safe alternatives exist for every common use-case:
    /// - **Byte-level reinterpretation**: `f32::from_bits` / `f32::to_bits`,
    ///   `u32::from_le_bytes` / `to_le_bytes`, etc.
    /// - **Numeric widening**: `as` casts.
    /// - **Pod reinterpretation across types**: a reviewed `bytemuck`-style
    ///   wrapper that upholds alignment and size contracts at the type level.
    ///
    /// ### Example
    ///
    /// ```rust
    /// // Reinterpret a u32 bit-pattern as f32 — banned.
    /// let f: f32 = unsafe { std::mem::transmute::<u32, f32>(bits) };
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// let f = f32::from_bits(bits);
    /// ```
    pub ENGINE_NO_TRANSMUTE,
    Warn,
    "ban `mem::transmute` and memory reinterpretation in engine code"
}

/// Memory-reinterpretation functions banned in engine source. Matched by
/// def-path suffix (with a leading `::`) so a user function literally named
/// `my_transmute` does not trigger the lint.
const BANNED: &[&str] = &["::transmute", "::transmute_copy"];

impl<'tcx> LateLintPass<'tcx> for EngineNoTransmute {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::Call(callee, _) = expr.kind else {
            return;
        };
        let ExprKind::Path(ref qpath) = callee.kind else {
            return;
        };
        let Some(def_id) = cx.qpath_res(qpath, callee.hir_id).opt_def_id() else {
            return;
        };
        let path = cx.tcx.def_path_str(def_id);
        if !BANNED.iter().any(|banned| path.ends_with(banned)) {
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
            ENGINE_NO_TRANSMUTE,
            expr.span,
            "`mem::transmute` is raw memory reinterpretation; it is banned in non-test engine code",
            None,
            "use `f32::from_bits`/`to_bits`, `u32::from_le_bytes`/`to_le_bytes`, `as` casts, \
             or a reviewed `bytemuck`-style wrapper instead of raw reinterpretation",
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
