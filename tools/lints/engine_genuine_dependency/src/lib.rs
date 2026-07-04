#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_middle;

use std::collections::BTreeSet;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use rustc_hir::def_id::{CRATE_DEF_ID, LOCAL_CRATE};
use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{HirId, Path};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::TyCtxt;

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// For a layer crate (one with a `layer.toml`), flags every entry in
    /// `depends_on` that the crate never actually uses in **non-test** code. A
    /// declared dependency must be referenced by a resolved path to that crate
    /// somewhere outside `#[cfg(test)]` / `#[test]`.
    ///
    /// ### Why is this bad?
    ///
    /// The Axiom Layer Law makes layers a directed acyclic graph in which **every
    /// declared dependency is genuinely adapted**. A `depends_on` entry the layer
    /// does not use is a *ceremonial dependency* — it fakes an adapter
    /// relationship the code does not have, which is exactly the kind of shortcut
    /// the engine forbids. The `xtask` checker enforces the graph's shape (the
    /// edges are real and acyclic); this lint, with real type information,
    /// enforces that each declared edge is actually used.
    ///
    /// ### Example
    ///
    /// ```toml
    /// # layer.toml
    /// depends_on = ["kernel", "math"]   # but nothing in the crate names axiom_math
    /// ```
    ///
    /// Either genuinely use `axiom_math`, or remove `"math"` from `depends_on`
    /// (and the dependency from `Cargo.toml`).
    pub ENGINE_GENUINE_DEPENDENCY,
    Warn,
    "a layer declares a `depends_on` dependency it never uses in non-test code"
}

impl<'tcx> LateLintPass<'tcx> for EngineGenuineDependency {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        let Some(info) = read_layer_info() else {
            return;
        };
        if info.depends_on.is_empty() {
            return;
        }
        // `--all-targets` compiles a layer's integration tests / examples as
        // their own crates, sharing the layer's `CARGO_MANIFEST_DIR`; skip any
        // target whose crate name isn't the layer's own library crate.
        if cx.tcx.crate_name(LOCAL_CRATE).as_str() != info.self_prefix {
            return;
        }
        let declared = info.depends_on;

        let mut used: BTreeSet<String> = BTreeSet::new();
        let mut visitor = UsedCrates { cx, used: &mut used };
        cx.tcx.hir_walk_toplevel_module(&mut visitor);

        let span = cx.tcx.def_span(CRATE_DEF_ID);
        for dep in unused_dependencies(&declared, &used) {
            span_lint_and_help(
                cx,
                ENGINE_GENUINE_DEPENDENCY,
                span,
                format!(
                    "this layer declares `depends_on = [.. \"{dep}\" ..]` but never uses \
                     `axiom_{dep}` in non-test code"
                ),
                None,
                "remove the dependency from `depends_on` and `Cargo.toml`, or genuinely use it \
                 (a declared-but-unused dependency is a ceremonial dependency)",
            );
        }
    }
}

/// The bits of a `layer.toml` this lint needs: the layer's own import prefix
/// (to recognise its library crate) and its declared dependencies.
struct LayerInfo {
    self_prefix: String,
    depends_on: Vec<String>,
}

/// Read the current crate's `layer.toml`, if it is a layer.
///
/// `CARGO_MANIFEST_DIR` is set by cargo in the compiler's environment, so it
/// points at the crate being compiled. A crate with no `layer.toml` (a module,
/// app, tool, or a lint `ui` fixture) returns `None` and is skipped.
fn read_layer_info() -> Option<LayerInfo> {
    let dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let text = std::fs::read_to_string(std::path::Path::new(&dir).join("layer.toml")).ok()?;
    let value = toml::from_str::<toml::Value>(&text).ok()?;
    let layer = value.get("layer")?;
    let name = layer.get("name")?.as_str()?;
    let crate_name = layer
        .get("crate_name")
        .and_then(|c| c.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("axiom-{name}"));
    Some(LayerInfo {
        self_prefix: crate_name.replace('-', "_"),
        depends_on: parse_depends_on(&value),
    })
}

/// Parse `[layer] depends_on = [...]` from already-parsed `layer.toml`.
fn parse_depends_on(value: &toml::Value) -> Vec<String> {
    value
        .get("layer")
        .and_then(|layer| layer.get("depends_on"))
        .and_then(|deps| deps.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The declared dependencies whose import prefix (`axiom_<name>`) is absent from
/// the set of crates actually referenced. A layer name may contain hyphens (e.g.
/// `proc-core`), but the compiler reports crate names with underscores
/// (`axiom_proc_core`), so the dependency name is normalized the same way before
/// the lookup.
fn unused_dependencies(declared: &[String], used: &BTreeSet<String>) -> Vec<String> {
    declared
        .iter()
        .filter(|dep| !used.contains(&format!("axiom_{}", dep.replace('-', "_"))))
        .cloned()
        .collect()
}

/// Records the name of every extern crate referenced by a resolved path in
/// non-test code.
struct UsedCrates<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    used: &'a mut BTreeSet<String>,
}

impl<'tcx> Visitor<'tcx> for UsedCrates<'_, 'tcx> {
    type NestedFilter = rustc_middle::hir::nested_filter::All;

    fn maybe_tcx(&mut self) -> TyCtxt<'tcx> {
        self.cx.tcx
    }

    fn visit_path(&mut self, path: &Path<'tcx>, id: HirId) -> Self::Result {
        if !is_in_test(self.cx.tcx, id) {
            if let Some(def_id) = path.res.opt_def_id() {
                if def_id.krate != LOCAL_CRATE {
                    self.used
                        .insert(self.cx.tcx.crate_name(def_id.krate).to_string());
                }
            }
        }
        intravisit::walk_path(self, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> toml::Value {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn parse_depends_on_reads_the_array() {
        let v = value("[layer]\nname = \"math\"\ndepends_on = [\"kernel\", \"runtime\"]\n");
        assert_eq!(parse_depends_on(&v), vec!["kernel", "runtime"]);
    }

    #[test]
    fn parse_depends_on_empty_and_missing() {
        assert!(parse_depends_on(&value("[layer]\ndepends_on = []\n")).is_empty());
        assert!(parse_depends_on(&value("[layer]\nname = \"kernel\"\n")).is_empty());
    }

    #[test]
    fn unused_dependencies_flags_only_the_absent_ones() {
        let declared = vec!["kernel".to_string(), "math".to_string()];
        let mut used = BTreeSet::new();
        used.insert("axiom_kernel".to_string());
        // `math` is declared but `axiom_math` is not referenced.
        assert_eq!(unused_dependencies(&declared, &used), vec!["math".to_string()]);
        used.insert("axiom_math".to_string());
        assert!(unused_dependencies(&declared, &used).is_empty());
    }

    #[test]
    fn unused_dependencies_normalizes_hyphenated_layer_names() {
        // A hyphenated layer (`proc-core`) is referenced as `axiom_proc_core`.
        let declared = vec!["proc-core".to_string()];
        let mut used = BTreeSet::new();
        assert_eq!(unused_dependencies(&declared, &used), vec!["proc-core".to_string()]);
        used.insert("axiom_proc_core".to_string());
        assert!(unused_dependencies(&declared, &used).is_empty());
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
