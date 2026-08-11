#![feature(rustc_private)]
#![warn(unused_extern_crates)]

//! `engine_no_retained_state` — the Axiom State Law.
//!
//! > Persistent information may exist only as explicit data passed into and
//! > returned from engine computations. Executable engine machinery may not
//! > secretly retain, mutate, initialize, synchronize, cache, or otherwise
//! > remember information across invocations.
//!
//! Explicit state *data* is legal; hidden retained state in engine *behavior*
//! is not. See `README.md` next to this crate for the full law, the legal /
//! illegal catalogue, and the proof boundary.

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

mod category;
mod prohibited;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::is_in_test;
use engine_lint_helpers::is_engine_file;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::{
    Block, BlockCheckMode, ClosureKind, CoroutineDesugaring, CoroutineKind,
    CoroutineSource, Expr, ExprKind, FieldDef, FnDecl, FnRetTy, ForeignItem, ForeignItemKind,
    HirId, ImplItem, ImplItemKind, Item, ItemKind, Node, TraitItem, TraitItemKind, UnsafeSource,
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::hygiene::{ExpnKind, MacroKind};
use rustc_span::Span;

use category::Category;
use prohibited::{
    behavior_probe, behavioral_trait, find, mut_ref_probe, raw_pointer_probe, state_carrier_probe,
    Descent,
};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Enforces the Axiom **State Law** over the reusable engine spine — the
    /// layer crates under `crates/*/src` and the modules under `modules/*/src`
    /// (apps, tools, `xtask`, `axiom-zones`, and all test code are out of
    /// scope):
    ///
    /// > Persistent information may exist only as explicit data passed into and
    /// > returned from engine computations. Executable engine machinery may not
    /// > secretly retain, mutate, initialize, synchronize, cache, or otherwise
    /// > remember information across invocations.
    ///
    /// Twelve categories are detected, each named in its diagnostic:
    /// `static-storage`, `thread-local-storage`, `interior-mutability`,
    /// `shared-state-ownership`, `mutable-engine-api`,
    /// `retained-execution-state`, `stateful-callback-boundary`,
    /// `opaque-behavior-state`, `generic-behavior-state`,
    /// `stateful-trait-implementation`, `drop-side-effect-hole`, and
    /// `unsafe-state-escape`.
    ///
    /// Detection is semantic: types are inspected as **resolved**
    /// `rustc_middle::ty::Ty`, so `type Hidden = RefCell<World>` nested inside
    /// `Option<Box<Hidden>>` is caught exactly like a written `RefCell`.
    ///
    /// ### Why is this bad?
    ///
    /// Retained state is what makes engine behavior unreplayable. A value the
    /// caller cannot see, did not pass in, and does not get back means the same
    /// call with the same arguments can produce two different answers — which
    /// defeats determinism, replay, snapshotting, and every test that depends on
    /// them. Explicit state *data* has none of these problems: it is visible in
    /// the signature, ownable by the app, and diffable between ticks.
    ///
    /// ### Example
    ///
    /// ```rust
    /// // illegal — hidden retained state
    /// static CURRENT_SCORE: AtomicU32 = AtomicU32::new(0);
    /// struct Engine { score: RefCell<u32> }
    /// impl Engine { pub fn update(&mut self, input: Input) {} }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// // legal — explicit state data in, explicit state data out
    /// pub struct BaseballState { pub score: u32, pub inning: u8 }
    /// pub fn step(state: &BaseballState, input: &Input) -> BaseballState { state.clone() }
    /// ```
    pub ENGINE_NO_RETAINED_STATE,
    Warn,
    "hidden, retained, ambient, or internally mutable state in engine code"
}

// ---------------------------------------------------------------------------
// scope + reporting
// ---------------------------------------------------------------------------

/// True if this span belongs to non-test engine spine source that the state law
/// governs. Written-by-hand code only: a macro expansion is attributed to the
/// macro, not to the engine file that invoked it (`thread_local!` is the one
/// deliberate exception, handled by [`thread_local_callsite`]).
fn in_scope(cx: &LateContext<'_>, span: Span, hir_id: HirId) -> bool {
    !span.from_expansion() && is_engine_file(cx, span) && !is_in_test(cx.tcx, hir_id)
}

/// Emit one finding. Every diagnostic names the exact construct, its category,
/// why it can retain or conceal state, and the concrete stateless rewrite.
fn report(cx: &LateContext<'_>, span: Span, category: Category, construct: &str, why: &str) {
    span_lint_and_help(
        cx,
        ENGINE_NO_RETAINED_STATE,
        span,
        format!("[{}] {construct}", category.slug()),
        None,
        format!("{why}; {}", category.rewrite()),
    );
}

/// If `span` was produced by a `thread_local!` expansion, the invocation site in
/// engine source. `thread_local!` is the one macro whose expansion this lint
/// attributes back to its caller: the `static` it generates is precisely the
/// ambient per-thread slot the law forbids, and the engine author wrote the
/// invocation.
fn thread_local_callsite(span: Span) -> Option<Span> {
    span.macro_backtrace()
        .any(|expn| {
            matches!(expn.kind, ExpnKind::Macro(MacroKind::Bang, name) if name.as_str() == "thread_local")
        })
        .then(|| span.source_callsite())
}

// ---------------------------------------------------------------------------
// type-position checks
// ---------------------------------------------------------------------------

/// Which rules apply at a type position.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Surface {
    /// The position is on a public engine boundary, so the boundary rules
    /// (`mutable-engine-api`, `stateful-callback-boundary`,
    /// `opaque-behavior-state`, public `Future` exposure) apply on top of the
    /// always-on carrier rules.
    public_boundary: bool,
}

/// Check one declared type position (a field, a parameter, a return type).
///
/// The carrier rules (state-carrying std types, raw pointers) apply regardless
/// of visibility: a *private* field holding a `RefCell` is exactly the hidden
/// retention the law is about. The boundary rules apply only where
/// `surface.public_boundary` holds — a private helper taking `&mut Vec<Effect>`
/// to construct its output is explicitly legal.
///
/// At most one finding per position, in priority order, so a single
/// `&mut Arc<Mutex<T>>` reports the innermost, most specific cause once.
fn check_type_position<'tcx>(
    cx: &LateContext<'tcx>,
    span: Span,
    ty: Ty<'tcx>,
    surface: Surface,
    what: &str,
) {
    let tcx: TyCtxt<'tcx> = cx.tcx;
    if let Some(found) = find(tcx, ty, Descent::Structural, &state_carrier_probe) {
        report(
            cx,
            span,
            found.category,
            &format!("{what} contains {}", found.describe()),
            "this type exists to hold information that outlives, or mutates outside of, the \
             call that touches it",
        );
        return;
    }
    if let Some(found) = find(tcx, ty, Descent::Structural, &raw_pointer_probe) {
        report(
            cx,
            span,
            found.category,
            &format!("{what} contains the raw pointer {}", found.describe()),
            "a raw pointer aliases memory the compiler cannot track, so nothing proves the \
             pointee is not retained and mutated behind the API",
        );
        return;
    }
    if !surface.public_boundary {
        return;
    }
    if let Some(found) = find(tcx, ty, Descent::Structural, &behavior_probe) {
        report(
            cx,
            span,
            found.category,
            &format!("{what} exposes {} on a public engine boundary", found.describe()),
            "this hands executable behavior across the boundary, and that behavior may capture \
             and retain state the engine cannot see",
        );
        return;
    }
    if let Some(found) = find(tcx, ty, Descent::Structural, &mut_ref_probe) {
        report(
            cx,
            span,
            found.category,
            &format!("{what} takes or returns {}", found.describe()),
            "a public API that writes through a mutable reference mutates caller-owned state in \
             place, so the new state never appears as a value",
        );
    }
}

/// Check a `const`/`static`/alias type, where the whole composed value is the
/// subject and there is no inner declaration to attribute the finding to.
fn check_composed_type<'tcx>(cx: &LateContext<'tcx>, span: Span, ty: Ty<'tcx>, what: &str) {
    if let Some(found) = find(cx.tcx, ty, Descent::Transitive, &state_carrier_probe) {
        report(
            cx,
            span,
            found.category,
            &format!("{what} contains {}", found.describe()),
            "this type exists to hold information that outlives, or mutates outside of, the \
             call that touches it",
        );
    }
}

// ---------------------------------------------------------------------------
// signature checks
// ---------------------------------------------------------------------------

/// Check a function signature, pairing each HIR type (for its span) with the
/// corresponding **resolved** signature type (for its identity).
fn check_signature<'tcx>(
    cx: &LateContext<'tcx>,
    def_id: LocalDefId,
    decl: &'tcx FnDecl<'tcx>,
    surface: Surface,
    is_async: bool,
) {
    let sig = cx.tcx.fn_sig(def_id).instantiate_identity().skip_binder();
    decl.inputs
        .iter()
        .zip(sig.inputs().iter())
        .for_each(|(hir_ty, mid_ty)| {
            check_type_position(cx, hir_ty.span, *mid_ty, surface, "parameter type");
        });
    // An `async fn`'s return type is a compiler-synthesized `impl Future`
    // wrapping what the author wrote. Reporting it would be the third diagnostic
    // for one construct (the header already fires, as does the desugared body),
    // so the `async fn` itself is the single finding.
    if is_async {
        return;
    }
    if let FnRetTy::Return(hir_ty) = decl.output {
        check_type_position(cx, hir_ty.span, sig.output(), surface, "return type");
    }
}

/// Rule 9: a public generic parameter constrained by a behavioral trait
/// (`Fn`/`FnMut`/`FnOnce`/`Future`). Data generics (`StateTable<K, V>`) carry no
/// such bound and are untouched.
///
/// Only the item's *own* predicates are read (not the parent impl's), so a
/// method is not blamed for its impl block's bounds.
fn check_generic_behavior(cx: &LateContext<'_>, def_id: LocalDefId) {
    cx.tcx
        .predicates_of(def_id)
        .predicates
        .iter()
        .for_each(|(clause, span)| {
            let ty::ClauseKind::Trait(pred) = clause.kind().skip_binder() else {
                return;
            };
            if !matches!(pred.self_ty().kind(), ty::Param(_)) {
                return;
            }
            let Some(kind) = behavioral_trait(cx.tcx, pred.def_id()) else {
                return;
            };
            let trait_name = cx.tcx.def_path_str(pred.def_id());
            let why: &str = match kind {
                Category::RetainedExecutionState => {
                    "a caller-supplied future retains execution state between polls, inside \
                     engine computation"
                }
                _ => {
                    "a caller-supplied closure can capture and mutate arbitrary hidden state \
                     inside engine computation"
                }
            };
            report(
                cx,
                *span,
                Category::GenericBehaviorState,
                &format!("public generic parameter is bound by `{trait_name}`"),
                why,
            );
        });
}

/// True if `def_id`'s declared visibility is `pub`.
fn is_public(cx: &LateContext<'_>, def_id: LocalDefId) -> bool {
    cx.tcx.visibility(def_id.to_def_id()).is_public()
}

/// Run every public-boundary rule that keys off a function's own signature and
/// generics, plus the always-on carrier rules.
fn check_fn_like<'tcx>(
    cx: &LateContext<'tcx>,
    def_id: LocalDefId,
    sig: &rustc_hir::FnSig<'tcx>,
    public_boundary: bool,
) {
    check_signature(
        cx,
        def_id,
        sig.decl,
        Surface { public_boundary },
        sig.header.is_async(),
    );
    if public_boundary {
        check_generic_behavior(cx, def_id);
    }
}

/// Rule 6 and rule 12 as they appear in a function *header*: `async fn` is a
/// state machine; `unsafe fn` is an escape from the whole law.
fn check_fn_header(cx: &LateContext<'_>, header: rustc_hir::FnHeader, span: Span) {
    if header.is_async() {
        report(
            cx,
            span,
            Category::RetainedExecutionState,
            "`async fn` compiles to a state machine",
            "the generated future holds its locals and resume point across every `.await`, which \
             is retained execution state by construction",
        );
    }
    if header.is_unsafe() {
        report(
            cx,
            span,
            Category::UnsafeStateEscape,
            "`unsafe fn` opens an escape from the state law",
            "inside an unsafe fn the compiler stops proving aliasing and initialization, so \
             retention and mutation become unprovable",
        );
    }
}

// ---------------------------------------------------------------------------
// the pass
// ---------------------------------------------------------------------------

impl<'tcx> LateLintPass<'tcx> for EngineNoRetainedState {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        // `thread_local!` is handled before the from-expansion gate: the static
        // it generates is macro-authored, but the invocation is the engine's.
        if matches!(item.kind, ItemKind::Static(..)) {
            if let Some(callsite) = thread_local_callsite(item.span) {
                if is_engine_file(cx, callsite) && !is_in_test(cx.tcx, item.hir_id()) {
                    report(
                        cx,
                        callsite,
                        Category::ThreadLocalStorage,
                        "`thread_local!` declares ambient per-thread storage",
                        "a thread-local slot is read and written by code that never names it in \
                         a signature, so the same call can observe different values on different \
                         threads",
                    );
                }
                return;
            }
        }
        if !in_scope(cx, item.span, item.hir_id()) {
            return;
        }
        let def_id = item.owner_id.def_id;
        match item.kind {
            ItemKind::Static(..) => report(
                cx,
                item.span,
                Category::StaticStorage,
                "`static` item declares process-wide storage",
                "a static outlives every call, so information written into it is remembered \
                 across invocations without appearing in any signature",
            ),
            ItemKind::Const(..) => check_composed_type(
                cx,
                item.span,
                cx.tcx.type_of(def_id).instantiate_identity(),
                "`const` item type",
            ),
            ItemKind::TyAlias(..) => {
                let ty = cx.tcx.type_of(def_id).instantiate_identity();
                check_composed_type(cx, item.span, ty, "type alias");
                if is_public(cx, def_id) {
                    if let Some(found) = find(cx.tcx, ty, Descent::Structural, &behavior_probe) {
                        report(
                            cx,
                            item.span,
                            found.category,
                            &format!("public type alias exposes {}", found.describe()),
                            "an alias published from the engine is part of its boundary, and this \
                             one names executable behavior whose captured state is invisible",
                        );
                    }
                }
            }
            ItemKind::Fn { sig, .. } => {
                check_fn_header(cx, sig.header, item.span);
                check_fn_like(cx, def_id, &sig, is_public(cx, def_id));
            }
            ItemKind::Impl(imp) => {
                // Only a trait impl can be `unsafe`, so the safety flag lives on
                // the trait-impl header.
                let Some(header) = imp.of_trait else {
                    return;
                };
                if header.safety.is_unsafe() {
                    report(
                        cx,
                        item.span,
                        Category::UnsafeStateEscape,
                        "`unsafe impl` asserts a property the compiler cannot check",
                        "an unsafe impl vouches for invariants (often about shared mutation) that \
                         nothing verifies",
                    );
                }
                let Some(trait_did) = header.trait_ref.trait_def_id() else {
                    return;
                };
                let name = cx.tcx.opt_item_name(trait_did).map(|n| n.to_string());
                match name.as_deref() {
                    Some("Drop") => report(
                        cx,
                        item.span,
                        Category::DropSideEffectHole,
                        "custom `Drop` impl runs engine behavior at an invisible moment",
                        "destruction is scheduled by the compiler, so anything this body commits, \
                         flushes, or records happens at a point no caller wrote",
                    ),
                    Some(kind @ ("Future" | "Iterator" | "DoubleEndedIterator")) => report(
                        cx,
                        item.span,
                        Category::StatefulTraitImplementation,
                        &format!("engine-defined `{kind}` impl is a resumable state machine"),
                        "each call to this trait's method advances progression stored in `self`, \
                         which is retained state by definition",
                    ),
                    _ => {}
                }
            }
            // ItemKind::Trait(Constness, IsAuto, Safety, ImplRestriction, Ident,
            // Generics, GenericBounds, [TraitItemId]) on nightly-2026-04-16.
            ItemKind::Trait(_, _, safety, ..) if safety.is_unsafe() => report(
                cx,
                item.span,
                Category::UnsafeStateEscape,
                "`unsafe trait` declares an unchecked contract",
                "implementors assert invariants no compiler check enforces, which is where hidden \
                 mutation hides",
            ),
            ItemKind::ForeignMod { .. } => report(
                cx,
                item.span,
                Category::UnsafeStateEscape,
                "`extern` block imports foreign machinery",
                "foreign code is entirely outside the compiler's model, so it may retain and \
                 mutate anything with no evidence available to the engine",
            ),
            _ => {}
        }
    }

    fn check_foreign_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx ForeignItem<'tcx>) {
        if !in_scope(cx, item.span, item.hir_id()) {
            return;
        }
        if matches!(item.kind, ForeignItemKind::Static(..)) {
            report(
                cx,
                item.span,
                Category::StaticStorage,
                "`extern` static declares process-wide storage owned outside Rust",
                "a foreign static outlives every call and is mutable by code the compiler never \
                 sees",
            );
        }
    }

    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, impl_item: &'tcx ImplItem<'tcx>) {
        if !in_scope(cx, impl_item.span, impl_item.hir_id()) {
            return;
        }
        let ImplItemKind::Fn(sig, _) = impl_item.kind else {
            return;
        };
        check_fn_header(cx, sig.header, impl_item.span);
        let def_id = impl_item.owner_id.def_id;
        // A trait impl's signature is the trait's contract, not this crate's
        // choice, so the *boundary* rules are checked where the trait is
        // declared (an engine-defined `pub trait` with `&mut self` is flagged at
        // its declaration). The carrier rules still apply here: a trait impl
        // that stores an `Arc` is retained state wherever it was mandated.
        let public_boundary = is_public(cx, def_id) && !in_trait_impl(cx, def_id);
        check_fn_like(cx, def_id, &sig, public_boundary);
    }

    fn check_trait_item(&mut self, cx: &LateContext<'tcx>, trait_item: &'tcx TraitItem<'tcx>) {
        if !in_scope(cx, trait_item.span, trait_item.hir_id()) {
            return;
        }
        let TraitItemKind::Fn(sig, _) = trait_item.kind else {
            return;
        };
        check_fn_header(cx, sig.header, trait_item.span);
        let def_id = trait_item.owner_id.def_id;
        // A trait item inherits its trait's visibility; the trait is the
        // boundary an engine-defined contract publishes.
        let public_boundary = is_public(cx, cx.tcx.local_parent(def_id));
        check_fn_like(cx, def_id, &sig, public_boundary);
    }

    fn check_field_def(&mut self, cx: &LateContext<'tcx>, field: &'tcx FieldDef<'tcx>) {
        if !in_scope(cx, field.span, field.hir_id) {
            return;
        }
        let ty = cx.tcx.type_of(field.def_id).instantiate_identity();
        let public_boundary = is_public(cx, field.def_id);
        check_type_position(
            cx,
            field.span,
            ty,
            Surface { public_boundary },
            "field type",
        );
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if !in_scope(cx, expr.span, expr.hir_id) {
            return;
        }
        let ExprKind::Closure(closure) = expr.kind else {
            return;
        };
        // `CoroutineSource::Fn` is the body an `async fn` desugars into — that
        // construct is already reported once at its header. Only an `async`
        // block or `async` closure the author actually wrote is reported here.
        if matches!(
            closure.kind,
            ClosureKind::Coroutine(CoroutineKind::Desugared(
                CoroutineDesugaring::Async,
                CoroutineSource::Block | CoroutineSource::Closure
            ))
        ) {
            report(
                cx,
                expr.span,
                Category::RetainedExecutionState,
                "`async` block compiles to a state machine",
                "the generated future holds its captured environment and resume point across \
                 every `.await`, which is retained execution state by construction",
            );
        }
    }

    fn check_block(&mut self, cx: &LateContext<'tcx>, block: &'tcx Block<'tcx>) {
        if !in_scope(cx, block.span, block.hir_id) {
            return;
        }
        if matches!(
            block.rules,
            BlockCheckMode::UnsafeBlock(UnsafeSource::UserProvided)
        ) {
            report(
                cx,
                block.span,
                Category::UnsafeStateEscape,
                "`unsafe` block suspends the guarantees the state law rests on",
                "inside it, aliasing and initialization are unchecked, so retention and mutation \
                 through a shared reference become possible and unprovable",
            );
        }
    }
}

/// True if `def_id` is an item of an `impl Trait for Type` block.
fn in_trait_impl(cx: &LateContext<'_>, def_id: LocalDefId) -> bool {
    let parent = cx.tcx.local_parent(def_id);
    matches!(
        cx.tcx.hir_node_by_def_id(parent),
        Node::Item(Item { kind: ItemKind::Impl(imp), .. }) if imp.of_trait.is_some()
    )
}

/// The UI suite: every `ui/**/*.rs` fixture is compiled through the lint driver
/// and its diagnostics compared against the `.stderr` beside it. A fixture with
/// no `.stderr` must produce **no** findings — that is how the compile-pass
/// (legal) fixtures assert the law does not over-fire.
///
/// `--edition=2024` matches the engine's own edition; without it the fixtures
/// compile as Rust 2015 and `async fn` / `unsafe extern` fail to parse before
/// the lint ever runs.
#[test]
fn ui() {
    dylint_testing::ui::Test::src_base(env!("CARGO_PKG_NAME"), "ui")
        .rustc_flags(["--edition=2024"])
        .run();
}
