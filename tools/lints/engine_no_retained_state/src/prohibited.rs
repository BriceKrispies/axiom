//! The reusable resolved-type inspector behind the state law.
//!
//! Everything here works on `rustc_middle::ty::Ty` — the **resolved** type after
//! alias expansion — never on written syntax. That is what makes
//!
//! ```ignore
//! type Hidden = RefCell<World>;
//! struct Something { value: Option<Box<Hidden>> }
//!
//! type Shared<T> = Arc<T>;
//! struct Other { value: Shared<World> }
//! ```
//!
//! detectable: by the time the walker sees the field, its type is
//! `Option<Box<RefCell<World>>>` / `Arc<World>` and the `RefCell` / `Arc`
//! `DefId` is right there. A source scanner would see `Hidden` and `Shared<T>`
//! and miss both.
//!
//! [`find`] is the single entry point: it walks a composed type looking for a
//! component that satisfies a probe. Recursion is bounded two ways — a visited
//! set keyed by the interned `Ty` (so a recursive ADT such as
//! `struct Node { next: Option<Box<Node>> }` terminates) and a depth cap.

use std::collections::HashSet;

use rustc_hir::def_id::DefId;
use rustc_middle::ty::{self, Ty, TyCtxt};

use crate::category::Category;

/// How far [`find`] descends through a composed type.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Descent {
    /// Walk the type's own structure only: references, raw pointers, slices,
    /// arrays, tuples, and the generic arguments of an ADT (`Option<T>`,
    /// `Result<T, E>`, `Box<T>`, `Vec<T>`, and every engine-defined generic).
    ///
    /// Used for **declaration positions** — struct/enum fields and function
    /// signatures — so a prohibited type is reported once, at the declaration
    /// that actually introduces it, rather than again at every type that
    /// transitively contains that declaration.
    Structural,
    /// `Structural`, plus the field types of crate-local ADTs.
    ///
    /// Used where the whole composed value is the subject and there is no
    /// separate declaration to attribute the finding to: `const` and `static`
    /// item types (a `const` holding a `RefCell` several structs deep is the
    /// classic "fresh copy per use" trap) and `type` aliases.
    Transitive,
}

/// A prohibited component found inside a composed type.
pub struct TypeFinding {
    /// The category the component falls under.
    pub category: Category,
    /// The exact offending component, rendered by the compiler and therefore
    /// fully qualified: `std::cell::RefCell<World>`, `std::rc::Weak<World>`,
    /// `std::sync::atomic::Atomic<u32>` (the generic an `AtomicU32` alias
    /// resolves to), `&mut World`. Diagnostics quote this so the reader is told
    /// *which* nested piece is the problem — and which crate's type it is —
    /// rather than merely that the outer type is unclean.
    pub component: String,
}

impl TypeFinding {
    /// The component rendered for a diagnostic, already quoted.
    pub fn describe(&self) -> String {
        format!("`{}`", self.component)
    }
}

/// Recursion cap. Only reached by pathological generic nesting; the visited set
/// handles ordinary recursive ADTs on its own.
const MAX_DEPTH: usize = 32;

/// Walk `ty` and everything `descent` says it is made of, returning the first
/// component for which `probe` reports a finding.
///
/// The visited set is keyed by the interned `Ty`, which is the compiler's own
/// identity for a fully-resolved type — two spellings of the same type are one
/// key, and a cycle revisits a key and stops.
pub fn find<'tcx, P>(
    tcx: TyCtxt<'tcx>,
    ty: Ty<'tcx>,
    descent: Descent,
    probe: &P,
) -> Option<TypeFinding>
where
    P: Fn(TyCtxt<'tcx>, Ty<'tcx>) -> Option<TypeFinding>,
{
    let mut visited = HashSet::new();
    walk(tcx, ty, descent, &mut visited, 0, probe)
}

fn walk<'tcx, P>(
    tcx: TyCtxt<'tcx>,
    ty: Ty<'tcx>,
    descent: Descent,
    visited: &mut HashSet<Ty<'tcx>>,
    depth: usize,
    probe: &P,
) -> Option<TypeFinding>
where
    P: Fn(TyCtxt<'tcx>, Ty<'tcx>) -> Option<TypeFinding>,
{
    if depth > MAX_DEPTH || !visited.insert(ty) {
        return None;
    }
    if let Some(finding) = probe(tcx, ty) {
        return Some(finding);
    }
    children(tcx, ty, descent)
        .into_iter()
        .find_map(|child| walk(tcx, child, descent, visited, depth + 1, probe))
}

/// The component types of `ty` that the law considers part of it.
///
/// A function pointer's signature is deliberately **not** a child: `fn(Input) ->
/// Output` is explicitly legal — it carries no captured environment — and
/// descending into it would flag a pointer to a function that merely mentions a
/// prohibited type. `dyn Trait` is likewise a leaf here; the behavior probe
/// classifies it directly.
fn children<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>, descent: Descent) -> Vec<Ty<'tcx>> {
    match *ty.kind() {
        ty::Adt(adt, args) => {
            let mut out: Vec<Ty<'tcx>> = args.types().collect();
            // Descend into crate-local ADT fields only in `Transitive` mode, and
            // only for local defs: a foreign ADT's private fields are not part
            // of this crate's declared shape (see the proof boundary in
            // README.md).
            if descent == Descent::Transitive && adt.did().is_local() {
                out.extend(
                    adt.all_fields()
                        .map(|field| tcx.type_of(field.did).instantiate(tcx, args)),
                );
            }
            out
        }
        ty::Ref(_, inner, _) => vec![inner],
        ty::RawPtr(inner, _) => vec![inner],
        ty::Slice(inner) => vec![inner],
        ty::Array(inner, _) => vec![inner],
        ty::Pat(inner, _) => vec![inner],
        ty::Tuple(list) => list.iter().collect(),
        ty::Alias(alias) => alias.args.types().collect(),
        _ => Vec::new(),
    }
}

/// The name of `def_id`'s item when it is defined by `core` / `alloc` / `std`,
/// else `None`.
///
/// Keyed on the resolved `DefId`'s defining crate, so a re-export or a rename
/// (`use std::sync::Mutex as Lock;`) resolves to the same answer.
fn std_item_name(tcx: TyCtxt<'_>, def_id: DefId) -> Option<String> {
    let krate = tcx.crate_name(def_id.krate);
    matches!(krate.as_str(), "core" | "alloc" | "std")
        .then(|| tcx.opt_item_name(def_id).map(|name| name.to_string()))
        .flatten()
}

/// Classify a standard-library ADT as a state carrier, by resolved definition.
///
/// This is the semantic core of rules 2–4: identity is the `DefId`, so aliases,
/// renames and nesting cannot evade it.
pub fn state_carrier(tcx: TyCtxt<'_>, def_id: DefId) -> Option<Category> {
    let name = std_item_name(tcx, def_id)?;
    match name.as_str() {
        // Interior mutability: every std type whose entire purpose is mutating
        // through a shared reference.
        "UnsafeCell" | "SyncUnsafeCell" | "Cell" | "RefCell" | "OnceCell" | "LazyCell"
        | "OnceLock" | "LazyLock" | "Mutex" | "RwLock" | "ReentrantLock" | "Once" => {
            Some(Category::InteriorMutability)
        }
        // Shared ownership with a hidden lifecycle. `Box<T>` and `Vec<T>` are
        // deliberately absent: they are plain owned data.
        "Rc" | "Arc" | "Weak" => Some(Category::SharedStateOwnership),
        // `thread_local!` lowers to a `LocalKey` static; a hand-written
        // `LocalKey` is the same ambient per-thread slot.
        "LocalKey" => Some(Category::ThreadLocalStorage),
        // Every atomic, present and future: `AtomicBool`, `AtomicU8`..`AtomicU64`,
        // `AtomicUsize`, the signed set, `AtomicPtr`. Matched by name *and* by
        // living in the `atomic` module, so a user type called `AtomicThing`
        // outside std cannot collide (std_item_name already required core/std).
        other => (other.starts_with("Atomic") && tcx.def_path_str(def_id).contains("atomic"))
            .then_some(Category::InteriorMutability),
    }
}

/// Probe: is this component a standard-library state carrier?
pub fn state_carrier_probe<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Option<TypeFinding> {
    let ty::Adt(adt, _) = ty.kind() else {
        return None;
    };
    let def_id = adt.did();
    state_carrier(tcx, def_id).map(|category| TypeFinding {
        category,
        component: format!("{ty}"),
    })
}

/// Probe: is this component a raw pointer?
pub fn raw_pointer_probe<'tcx>(_tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Option<TypeFinding> {
    matches!(ty.kind(), ty::RawPtr(..)).then(|| TypeFinding {
        category: Category::UnsafeStateEscape,
        component: format!("{ty}"),
    })
}

/// Probe: is this component a `&mut` reference?
pub fn mut_ref_probe<'tcx>(_tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Option<TypeFinding> {
    matches!(ty.kind(), ty::Ref(_, _, rustc_hir::Mutability::Mut)).then(|| TypeFinding {
        category: Category::MutableEngineApi,
        component: format!("{ty}"),
    })
}

/// The category a *behavioral* std trait falls under, or `None` for an ordinary
/// trait.
pub fn behavioral_trait(tcx: TyCtxt<'_>, trait_def_id: DefId) -> Option<Category> {
    let name = std_item_name(tcx, trait_def_id)?;
    match name.as_str() {
        "Fn" | "FnMut" | "FnOnce" | "AsyncFn" | "AsyncFnMut" | "AsyncFnOnce" => {
            Some(Category::StatefulCallbackBoundary)
        }
        "Future" | "IntoFuture" => Some(Category::RetainedExecutionState),
        _ => None,
    }
}

/// Probe: does this component hand arbitrary behavior across a boundary?
///
/// `dyn Trait` is classified by its principal trait — an `Fn` family object is a
/// callback, a `Future` object is retained execution state, anything else is an
/// opaque behavior hole. An `impl Trait` opaque is classified only when its
/// bounds name a behavioral trait; a bare `impl Trait` over an ordinary trait is
/// out of the law's stated scope (see README.md, "What the lint does not prove").
pub fn behavior_probe<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Option<TypeFinding> {
    match *ty.kind() {
        ty::Dynamic(preds, ..) => {
            let category = preds
                .principal_def_id()
                .and_then(|did| behavioral_trait(tcx, did))
                .unwrap_or(Category::OpaqueBehaviorState);
            Some(TypeFinding {
                category,
                component: format!("{ty}"),
            })
        }
        ty::Alias(ty::AliasTy {
            kind: ty::AliasTyKind::Opaque { def_id },
            ..
        }) => tcx
            .explicit_item_bounds(def_id)
            .skip_binder()
            .iter()
            .find_map(|(clause, _)| match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(pred) => behavioral_trait(tcx, pred.def_id()),
                _ => None,
            })
            .map(|category| TypeFinding {
                category,
                component: format!("{ty}"),
            }),
        _ => None,
    }
}
