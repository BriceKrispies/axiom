#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_span;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use rustc_hir::def_id::DefId;
use rustc_hir::{Expr, ExprKind, HirId, Item, ItemKind, Node, StmtKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_span::FileName;

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags wall-clock / monotonic time reads (`Instant::now`,
    /// `SystemTime::now`) inside a `#[sim]` zone (a function or module marked
    /// with `axiom_zones::sim`).
    ///
    /// ### Why is this bad?
    ///
    /// Deterministic simulation must be a pure function of its seeded inputs.
    /// Reading the wall clock makes a tick non-reproducible — the same inputs
    /// replay to a different result. Time must enter the sim as an explicit
    /// input (a fixed tick / step), never be sampled from the environment.
    ///
    /// ### Example
    ///
    /// ```rust
    /// #[axiom_zones::sim]
    /// fn step() {
    ///     let now = std::time::Instant::now(); // non-deterministic
    /// }
    /// ```
    pub NO_TIME_IN_SIM,
    Warn,
    "wall-clock / monotonic time read inside a `#[sim]` zone"
}

/// Time-source associated functions that read the environment clock. Matched by
/// def-path suffix so the `std`/`core` prefix doesn't matter.
const BANNED_TIME: &[&str] = &["Instant::now", "SystemTime::now"];

/// The marker `axiom_zones::sim` injects into a `#[sim]` fn/module body.
const SIM_MARKER: &str = "__engine_zone_sim";

impl<'tcx> LateLintPass<'tcx> for NoTimeInSim {
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
        if !BANNED_TIME.iter().any(|banned| path.ends_with(banned)) {
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
        if !in_zone(cx, expr.hir_id, SIM_MARKER) {
            return;
        }
        span_lint_and_help(
            cx,
            NO_TIME_IN_SIM,
            expr.span,
            "reading the clock inside a `#[sim]` zone makes the simulation non-deterministic",
            None,
            "take time as an explicit seeded input (a fixed tick / step), never sample it here",
        );
    }
}

/// Is `hir_id` inside a zone whose marker const is `marker`? Walks the enclosing
/// item chain (functions and inline modules) for the injected marker.
fn in_zone(cx: &LateContext<'_>, hir_id: HirId, marker: &str) -> bool {
    cx.tcx
        .hir_parent_iter(hir_id)
        .any(|(_id, node)| matches!(node, Node::Item(item) if item_has_marker(cx, item, marker)))
}

/// Does `item` (a fn or inline mod) declare the zone marker const directly?
fn item_has_marker(cx: &LateContext<'_>, item: &Item<'_>, marker: &str) -> bool {
    match item.kind {
        ItemKind::Fn { body, .. } => {
            let body = cx.tcx.hir_body(body);
            match body.value.kind {
                ExprKind::Block(block, _) => block.stmts.iter().any(|stmt| {
                    matches!(stmt.kind, StmtKind::Item(id) if def_named(cx, id.owner_id.to_def_id(), marker))
                }),
                _ => false,
            }
        }
        ItemKind::Mod(_, m) => m
            .item_ids
            .iter()
            .any(|id| def_named(cx, id.owner_id.to_def_id(), marker)),
        _ => false,
    }
}

/// Is the item at `def_id` named exactly `name`?
fn def_named(cx: &LateContext<'_>, def_id: DefId, name: &str) -> bool {
    cx.tcx.item_name(def_id).as_str() == name
}

/// True if `span` is in engine source: under `crates/<layer>/src/` (except the
/// `xtask` tool and the `axiom-zones` support crate) or `modules/<module>/src/`.
fn is_engine_file(cx: &LateContext<'_>, span: rustc_span::Span) -> bool {
    let FileName::Real(name) = cx.tcx.sess.source_map().span_to_filename(span) else {
        return false;
    };
    let Some(path) = name.local_path() else {
        return false;
    };
    let path = path.to_string_lossy().replace('\\', "/");
    let mut in_engine = false;
    let mut in_src = false;
    let mut excluded = false;
    for component in path.split('/') {
        match component {
            "crates" | "modules" => in_engine = true,
            "src" => in_src = true,
            "xtask" | "axiom-zones" => excluded = true,
            _ => {}
        }
    }
    in_engine && in_src && !excluded
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
