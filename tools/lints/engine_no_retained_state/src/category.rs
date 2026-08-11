//! The diagnostic categories of the Axiom State Law.
//!
//! Every finding this lint emits carries exactly one category. The slug is the
//! stable, greppable token used in messages and in the audit inventory
//! (`docs/retained-state-audit.md`); the rewrite text is the concrete stateless
//! direction the diagnostic hands the reader, so no message is ever a vague
//! "state is bad".

/// One category of hidden / retained state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Category {
    /// Any user-declared `static` item (mutable or not).
    StaticStorage,
    /// `thread_local!` and `std::thread::LocalKey`.
    ThreadLocalStorage,
    /// `Cell` / `RefCell` / `UnsafeCell` / `OnceLock` / `Mutex` / atomics / ...
    InteriorMutability,
    /// `Rc` / `Arc` / `rc::Weak` / `sync::Weak`.
    SharedStateOwnership,
    /// A public API that mutates caller-owned state in place (`&mut`).
    MutableEngineApi,
    /// `async fn`, async blocks, and `Future` on a public boundary.
    RetainedExecutionState,
    /// `Fn` / `FnMut` / `FnOnce` callbacks crossing a public boundary.
    StatefulCallbackBoundary,
    /// `dyn Trait` on a public boundary.
    OpaqueBehaviorState,
    /// A public generic parameter bounded by a behavioral trait.
    GenericBehaviorState,
    /// An engine-defined `Future` / `Iterator` / `DoubleEndedIterator` impl.
    StatefulTraitImplementation,
    /// A custom `Drop` impl defined by engine code.
    DropSideEffectHole,
    /// `unsafe` blocks/fns/impls, raw pointers, `extern` blocks.
    UnsafeStateEscape,
}

impl Category {
    /// The stable token that names this category in diagnostics and in the audit.
    pub const fn slug(self) -> &'static str {
        match self {
            Category::StaticStorage => "static-storage",
            Category::ThreadLocalStorage => "thread-local-storage",
            Category::InteriorMutability => "interior-mutability",
            Category::SharedStateOwnership => "shared-state-ownership",
            Category::MutableEngineApi => "mutable-engine-api",
            Category::RetainedExecutionState => "retained-execution-state",
            Category::StatefulCallbackBoundary => "stateful-callback-boundary",
            Category::OpaqueBehaviorState => "opaque-behavior-state",
            Category::GenericBehaviorState => "generic-behavior-state",
            Category::StatefulTraitImplementation => "stateful-trait-implementation",
            Category::DropSideEffectHole => "drop-side-effect-hole",
            Category::UnsafeStateEscape => "unsafe-state-escape",
        }
    }

    /// The concrete stateless rewrite direction offered with every diagnostic.
    pub const fn rewrite(self) -> &'static str {
        match self {
            Category::StaticStorage => {
                "use `const` for a compile-time value, or pass the datum in as an explicit \
                 parameter and return the updated copy: `fn step(state: &State, input: &Input) \
                 -> State`"
            }
            Category::ThreadLocalStorage => {
                "thread-local storage is ambient per-thread state the caller cannot see; thread \
                 the datum through the call as `fn step(state: &State, input: &Input) -> State`"
            }
            Category::InteriorMutability => {
                "make the mutation explicit: take the value as `&State` and return the next \
                 `State`, or move the imperative resource outside the stateless engine boundary \
                 (into the app) and pass the data it produces in as an input"
            }
            Category::SharedStateOwnership => {
                "shared ownership hides who may mutate and when the value dies; own the data \
                 outright (`Box<T>`, `Vec<T>`, a plain field) or pass it by reference for the \
                 duration of one call"
            }
            Category::MutableEngineApi => {
                "replace `fn step(&mut self, input: Input)` with `fn step(state: &State, input: \
                 &Input) -> State` so the new state is an explicit return value, not an \
                 invisible edit to the caller's memory"
            }
            Category::RetainedExecutionState => {
                "a future retains its execution state between polls; express the work as a pure \
                 step over explicit data (`fn step(state: &State, input: &Input) -> State`) and \
                 let the app own the asynchronous driver"
            }
            Category::StatefulCallbackBoundary => {
                "a closure can capture arbitrary hidden state; take a plain function pointer \
                 (`fn(Input) -> Output`) or, better, take the data the callback would have \
                 produced as an explicit input"
            }
            Category::OpaqueBehaviorState => {
                "a trait object hides an implementation that may retain state the engine cannot \
                 prove; dispatch statically through a concrete type, or replace the behavior \
                 with the explicit data it yields"
            }
            Category::GenericBehaviorState => {
                "a behavioral bound lets a caller smuggle arbitrary executable state into engine \
                 computation; keep data generics (`StateTable<K, V>`) and drop the behavioral \
                 bound in favor of an explicit data parameter"
            }
            Category::StatefulTraitImplementation => {
                "these traits are state machines by definition — each call advances hidden \
                 progression; return the finished explicit data (a `Vec<T>` or a state snapshot) \
                 instead of a resumable object"
            }
            Category::DropSideEffectHole => {
                "`Drop` runs behavior the caller never wrote and cannot see; make the teardown an \
                 explicit call that consumes the value and returns the resulting data"
            }
            Category::UnsafeStateEscape => {
                "unsafe code can retain and mutate state behind the compiler's back, so the state \
                 law cannot be proven around it; express the operation in safe Rust over explicit \
                 data, or move the imperative resource out to the app tier"
            }
        }
    }
}
