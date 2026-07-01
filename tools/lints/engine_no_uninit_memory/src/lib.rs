#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::is_engine_file;
use rustc_hir::{Expr, ExprKind, QPath, TyKind};
use rustc_lint::{LateContext, LateLintPass};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags calls to uninitialized or zero-initialized memory APIs —
    /// `std::mem::zeroed`, `std::mem::uninitialized`, `MaybeUninit::uninit`,
    /// `MaybeUninit::zeroed`, `MaybeUninit::uninit_array`, and
    /// `MaybeUninit::assume_init` — in non-test engine code (layer crates under
    /// `crates/` and modules under `modules/`). Apps, tooling, and all test
    /// code are exempt.
    ///
    /// ### Why is this bad?
    ///
    /// Uninitialized memory is one of the most dangerous primitives in unsafe
    /// Rust. `mem::uninitialized` was deprecated because it trivially produces
    /// undefined behavior. `mem::zeroed` is only sound for types where
    /// all-zeros is a valid bit pattern — a constraint that is invisible at the
    /// call site and easy to violate during refactoring. Direct `MaybeUninit`
    /// usage scatters unreviewed unsafe initialization logic across the engine
    /// codebase, making correctness audits impractical.
    ///
    /// In Axiom's engine the rule is: **values are fully initialized before
    /// use**. If a storage primitive genuinely needs uninit memory for
    /// performance (e.g. a fixed-capacity arena), that need is encapsulated in
    /// one reviewed primitive — it is not spread across engine call sites.
    ///
    /// ### Example
    ///
    /// ```rust
    /// // Flagged — zeroed bytes may not be valid for `MyStruct`.
    /// let s: MyStruct = unsafe { std::mem::zeroed() };
    ///
    /// // Flagged — `MaybeUninit` scattered through engine code is an audit hazard.
    /// let x = core::mem::MaybeUninit::<u32>::uninit();
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// // Provide a real `Default` impl or a named constructor.
    /// let s = MyStruct::default();
    ///
    /// // If a storage primitive genuinely requires uninit memory, encapsulate it
    /// // in one reviewed type rather than calling MaybeUninit at each site.
    /// ```
    pub ENGINE_NO_UNINIT_MEMORY,
    Warn,
    "uninitialized/zeroed/MaybeUninit memory in engine code"
}

/// Free-function (path-call) forms that produce uninitialized or zero memory.
/// Matched by def-path suffix so the `std`/`core` prefix doesn't matter.
const BANNED_PATH: &[&str] = &[
    "mem::zeroed",
    "mem::uninitialized",
    "MaybeUninit::uninit",
    "MaybeUninit::zeroed",
    "MaybeUninit::uninit_array",
];

/// The associated-function name suffixes used on `MaybeUninit<T>` via turbofish
/// (`MaybeUninit::<T>::uninit()` etc.). Matched against the last path segment when
/// the callee is a `TypeRelative` QPath whose type is `MaybeUninit`.
const BANNED_MAYBEUNINIT_FNS: &[&str] = &["uninit", "zeroed", "uninit_array"];

const LINT_MESSAGE: &str =
    "uninitialized or zero-initialized memory is banned in non-test engine code";
const LINT_HELP: &str =
    "construct a fully-initialized value (e.g. `Default::default()` or a named constructor); \
     if a reviewed storage primitive genuinely needs uninit memory, encapsulate it there \
     rather than calling these APIs at each site";

impl<'tcx> LateLintPass<'tcx> for EngineNoUninitMemory {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }
        if is_in_test(cx.tcx, expr.hir_id) {
            return;
        }
        if !is_engine_file(cx, expr.span) {
            return;
        }

        match expr.kind {
            // Path-call form: mem::zeroed(), MaybeUninit::uninit(), etc.
            ExprKind::Call(callee, _) => {
                let ExprKind::Path(ref qpath) = callee.kind else {
                    return;
                };
                let banned = match qpath {
                    // Resolved path, e.g. `std::mem::zeroed()`; def_path_str
                    // returns a string like "core::mem::zeroed".
                    QPath::Resolved(..) => {
                        let Some(def_id) = cx.qpath_res(qpath, callee.hir_id).opt_def_id()
                        else {
                            return;
                        };
                        let path = cx.tcx.def_path_str(def_id);
                        BANNED_PATH.iter().any(|b| path.ends_with(b))
                    }
                    // Type-relative path, e.g. `MaybeUninit::<u32>::uninit()`:
                    // TypeRelative(ty, seg) where `ty` is the HIR Ty for
                    // `MaybeUninit<T>` and `seg.ident` is the assoc-fn name.
                    QPath::TypeRelative(ty, seg) => {
                        if !BANNED_MAYBEUNINIT_FNS.contains(&seg.ident.name.as_str()) {
                            return;
                        }
                        let TyKind::Path(QPath::Resolved(_, path)) = ty.kind else {
                            return;
                        };
                        let last_seg = path.segments.last().map(|s| s.ident.name.as_str());
                        last_seg == Some("MaybeUninit")
                    }
                };
                if !banned {
                    return;
                }
                span_lint_and_help(
                    cx,
                    ENGINE_NO_UNINIT_MEMORY,
                    expr.span,
                    LINT_MESSAGE,
                    None,
                    LINT_HELP,
                );
            }

            ExprKind::MethodCall(seg, _recv, _, _) => {
                if seg.ident.name.as_str() != "assume_init" {
                    return;
                }
                span_lint_and_help(
                    cx,
                    ENGINE_NO_UNINIT_MEMORY,
                    expr.span,
                    LINT_MESSAGE,
                    None,
                    LINT_HELP,
                );
            }

            _ => {}
        }
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
