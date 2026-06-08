#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_span;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::{is_engine_file, is_in_crate_dir};
use rustc_hir::def::Res;
use rustc_hir::{FnSig, ImplItem, ImplItemKind, Item, ItemKind, Node, PrimTy, QPath, Ty, TyKind};
use rustc_lint::{LateContext, LateLintPass};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Flags a naked `f32` / `f64` on the **public surface** of engine code —
    /// the layer crates under `crates/` (except `xtask`) and the modules under
    /// `modules/`. Three surfaces are checked: the parameter and return types of
    /// a `pub fn` (free function), the same for a `pub` method in an *inherent*
    /// `impl` block (e.g. `Vec3::new`, a `fn distance(self) -> f32` accessor),
    /// and a `pub` field of a `pub` struct. A single layer of reference
    /// (`&f32`, `&mut f64`) is peeled before the check. Methods of a *trait*
    /// `impl` are intentionally skipped — their signature is dictated by the
    /// trait, not by this crate, so the unit decision isn't ours to make there.
    /// The inherent methods of a **quantity newtype** (a struct that is itself a
    /// single `f32`/`f64` field, e.g. `Pixels(f32)`) are also skipped: that
    /// type's own `new(f32)` / `get() -> f32` are the boundary where a raw scalar
    /// enters/leaves the quantity, not a unitless leak — the same shape as the
    /// kernel's `Ratio::new`/`get`.
    /// The **scalar-floor crates** `axiom-kernel` and `axiom-math` are skipped
    /// entirely: the kernel owns the dimensioned-scalar primitives (whose
    /// constructors take a raw `f32` by definition) plus serialization /
    /// telemetry, and math is the dimensionless linear-algebra layer — a raw
    /// `f32` is the correct type there, not a missing unit.
    ///
    /// ### Why is this bad?
    ///
    /// A bare float carries no unit. `set_speed(speed: f32)` does not say whether
    /// `speed` is meters per second, units per tick, or degrees per frame — the
    /// caller has to guess, and a wrong guess compiles cleanly and produces a
    /// silent physics bug. Axiom prefers unit newtypes (`Meters`, `Seconds`,
    /// `Radians`, `MetersPerSecond`, ...) so the unit is part of the type: the
    /// compiler rejects mismatched units, the public API documents itself, and a
    /// future agent reading the signature cannot misread the contract. Private
    /// items, function bodies, and local variables are out of scope — only the
    /// public surface that other crates and apps build against is constrained.
    ///
    /// ### Example
    ///
    /// ```rust
    /// pub fn set_speed(speed: f32) {}        // unitless — what unit is `speed`?
    /// pub fn area() -> f64 { 0.0 }           // unitless return
    /// pub struct Body { pub mass: f32 }      // unitless public field
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// pub fn set_speed(speed: MetersPerSecond) {}
    /// pub fn area() -> SquareMeters { SquareMeters(0.0) }
    /// pub struct Body { pub mass: Kilograms }
    /// ```
    pub ENGINE_NO_UNITLESS_FLOAT_PUBLIC_API,
    Warn,
    "naked f32/f64 in a public engine API"
}

/// The "scalar floor": the crates where a raw `f32` / `f64` is the *correct*
/// type, not a missing unit, so this lint must stay silent there.
///
/// - `axiom-kernel` owns the dimensioned-scalar primitives themselves
///   (`Meters`/`Radians`/`Ratio` constructors take a raw `f32` by definition),
///   plus serialization (`write_f32`) and telemetry — genuine raw-scalar
///   boundaries.
/// - `axiom-math` is the dimensionless linear-algebra layer: `Vec3::new`, `dot`,
///   `length`, `distance` are dimensionless by construction.
///
/// Everything above this floor should carry a unit/ratio type, so it is flagged.
/// This is the rule being precise about where raw scalars belong — not an
/// exemption to dodge the rule.
const SCALAR_FLOOR: &[&str] = &["axiom-kernel", "axiom-math"];

/// True if this HIR type is a bare `f32` / `f64` primitive, optionally behind a
/// single layer of reference (`&f32`, `&mut f64`).
fn is_bare_float(ty: &Ty<'_>) -> bool {
    match ty.kind {
        TyKind::Path(QPath::Resolved(_, path)) => {
            matches!(path.res, Res::PrimTy(PrimTy::Float(_)))
        }
        TyKind::Ref(_, mut_ty) => is_bare_float(mut_ty.ty),
        _ => false,
    }
}

impl<'tcx> LateLintPass<'tcx> for EngineNoUnitlessFloatPublicApi {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        if is_in_test(cx.tcx, item.hir_id()) {
            return;
        }
        if !is_engine_file(cx, item.span) {
            return;
        }
        // The scalar-floor crates (kernel, math) are where raw f32 is correct.
        if is_in_crate_dir(cx, item.span, SCALAR_FLOOR) {
            return;
        }
        // Only the public surface is the concern. A private fn / struct can use
        // bare floats freely.
        let def_id = item.owner_id.def_id;
        if !cx.tcx.visibility(def_id.to_def_id()).is_public() {
            return;
        }

        match item.kind {
            // ItemKind::Fn { sig, generics, body, .. } on nightly-2026-04-16.
            ItemKind::Fn { sig, .. } => check_fn_sig(cx, &sig),
            // ItemKind::Struct(_ident, _generics, variant_data) — data is third.
            ItemKind::Struct(_, _, data) => {
                for field in data.fields() {
                    // A private field of a public struct is out of scope.
                    if cx.tcx.visibility(field.def_id.to_def_id()).is_public()
                        && is_bare_float(field.ty)
                    {
                        emit(cx, field.span);
                    }
                }
            }
            _ => {}
        }
    }

    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, impl_item: &'tcx ImplItem<'tcx>) {
        // Only methods/associated functions carry a signature to inspect.
        let ImplItemKind::Fn(sig, _) = impl_item.kind else {
            return;
        };
        if impl_item.span.from_expansion() {
            return;
        }
        if is_in_test(cx.tcx, impl_item.hir_id()) {
            return;
        }
        if !is_engine_file(cx, impl_item.span) {
            return;
        }
        // The scalar-floor crates (kernel, math) are where raw f32 is correct.
        if is_in_crate_dir(cx, impl_item.span, SCALAR_FLOOR) {
            return;
        }
        let def_id = impl_item.owner_id.def_id;
        // Only the public surface is the concern.
        if !cx.tcx.visibility(def_id.to_def_id()).is_public() {
            return;
        }
        // The parent of an impl item is its `impl` block.
        let parent = cx.tcx.local_parent(def_id);
        if let Node::Item(parent_item) = cx.tcx.hir_node_by_def_id(parent) {
            if let ItemKind::Impl(imp) = parent_item.kind {
                // Skip trait-impl methods: the signature is the trait's contract,
                // not a free choice of this crate. (`impl Trait for T` sets
                // `of_trait`; an inherent `impl T` leaves it `None`.)
                if imp.of_trait.is_some() {
                    return;
                }
                // Skip the inherent methods of a *quantity newtype* — a struct
                // that is itself a single `f32`/`f64` field (e.g. `Pixels(f32)`,
                // `Angle { radians: f32 }`). Such a type IS a float quantity, so
                // its own constructor (`new(f32)`) and accessor (`get() -> f32`)
                // are the boundary where a raw scalar enters/leaves the type, not
                // a unitless leak — exactly like the kernel's `Ratio::new`/`get`.
                if impl_self_is_float_newtype(cx, &imp) {
                    return;
                }
            }
        }
        check_fn_sig(cx, &sig);
    }
}

/// True if `imp`'s `Self` type is a struct with exactly one field whose type is a
/// bare `f32`/`f64` — a float quantity newtype. The type is always local (you can
/// only write an inherent impl for a local type), so its definition is in HIR.
fn impl_self_is_float_newtype(cx: &LateContext<'_>, imp: &rustc_hir::Impl<'_>) -> bool {
    let TyKind::Path(QPath::Resolved(_, path)) = imp.self_ty.kind else {
        return false;
    };
    let Some(local) = path.res.opt_def_id().and_then(|d| d.as_local()) else {
        return false;
    };
    let Node::Item(item) = cx.tcx.hir_node_by_def_id(local) else {
        return false;
    };
    let ItemKind::Struct(_, _, data) = item.kind else {
        return false;
    };
    let fields = data.fields();
    fields.len() == 1 && is_bare_float(fields[0].ty)
}

/// Flag every bare-float parameter and the bare-float return type of `sig`.
fn check_fn_sig(cx: &LateContext<'_>, sig: &FnSig<'_>) {
    for input in sig.decl.inputs {
        if is_bare_float(input) {
            emit(cx, input.span);
        }
    }
    if let rustc_hir::FnRetTy::Return(ret) = sig.decl.output {
        if is_bare_float(ret) {
            emit(cx, ret.span);
        }
    }
}

fn emit(cx: &LateContext<'_>, span: rustc_span::Span) {
    span_lint_and_help(
        cx,
        ENGINE_NO_UNITLESS_FLOAT_PUBLIC_API,
        span,
        "naked floating-point type in a public engine API".to_string(),
        None,
        "wrap it in a unit newtype (e.g. `Meters`, `Seconds`, `Radians`) so the unit is part of the type",
    );
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
