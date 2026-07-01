#![feature(rustc_private)]
#![warn(unused_extern_crates)]

//! Shared building blocks for Axiom's dylint rulebook: [`is_engine_file`] decides
//! whether a span is in the reusable engine spine, and [`in_zone`] /
//! [`item_has_marker`] / [`def_named`] detect an `axiom_zones` marker `const`
//! (named via [`markers`]) so no lint hard-codes the marker strings.

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_span;

use rustc_hir::def_id::DefId;
use rustc_hir::{ExprKind, HirId, Item, ItemKind, Node, StmtKind};
use rustc_lint::LateContext;
use rustc_span::{FileName, Span};

/// The marker `const` names that `axiom_zones` injects into a zoned item. Each
/// `#[axiom_zones::<zone>]` attribute re-emits its item with
/// `const __engine_zone_<zone>: () = ();` prepended; `#[escape_hatch]` injects a
/// `const __engine_escape_hatch_reason: &str = "...";`. Lints detect the zone by
/// these names, so they live in one place rather than as string literals
/// scattered across the rulebook.
pub mod markers {
    /// `#[sim]` — deterministic simulation zone.
    pub const SIM: &str = "__engine_zone_sim";
    /// `#[hot_path]` — per-frame / per-tick work.
    pub const HOT_PATH: &str = "__engine_zone_hot_path";
    /// `#[strict]` — branchless / primitive zone.
    pub const STRICT: &str = "__engine_zone_strict";
    /// `#[supervisor]` — an unbounded `loop` is permitted here.
    pub const SUPERVISOR: &str = "__engine_zone_supervisor";
    /// `#[escape_hatch(reason = "...")]` — a documented, deliberate exception.
    pub const ESCAPE_HATCH_REASON: &str = "__engine_escape_hatch_reason";
}

/// True if `span` is in engine *source*: under `crates/<layer>/src/...` or
/// `modules/<module>/src/...`, excluding the `xtask` tool and the `axiom-zones`
/// support crate. The `src` requirement is what exempts integration tests,
/// benches, and examples.
pub fn is_engine_file(cx: &LateContext<'_>, span: Span) -> bool {
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

/// True if `span`'s source path has a directory component exactly equal to one
/// of `dirs`. Matches whole path components, so `"axiom-math"` does not match a
/// hypothetical `axiom-math-extra`.
pub fn is_in_crate_dir(cx: &LateContext<'_>, span: Span, dirs: &[&str]) -> bool {
    let FileName::Real(name) = cx.tcx.sess.source_map().span_to_filename(span) else {
        return false;
    };
    let Some(path) = name.local_path() else {
        return false;
    };
    let path = path.to_string_lossy().replace('\\', "/");
    path.split('/').any(|component| dirs.contains(&component))
}

/// Is `hir_id` inside a zone whose marker `const` is `marker`? Walks the
/// enclosing item chain (functions and inline modules) for the injected marker.
///
/// Pass one of the [`markers`] constants, e.g.
/// `in_zone(cx, expr.hir_id, markers::SIM)`.
pub fn in_zone(cx: &LateContext<'_>, hir_id: HirId, marker: &str) -> bool {
    cx.tcx
        .hir_parent_iter(hir_id)
        .any(|(_id, node)| matches!(node, Node::Item(item) if item_has_marker(cx, item, marker)))
}

/// Does `item` (a fn or inline mod) declare the zone marker `const` directly?
pub fn item_has_marker(cx: &LateContext<'_>, item: &Item<'_>, marker: &str) -> bool {
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

/// Is the item at `def_id` named exactly `name`? Uses `opt_item_name` rather
/// than `item_name`, which panics on a nameless def (e.g. a `use` re-export).
pub fn def_named(cx: &LateContext<'_>, def_id: DefId, name: &str) -> bool {
    cx.tcx
        .opt_item_name(def_id)
        .is_some_and(|item| item.as_str() == name)
}
