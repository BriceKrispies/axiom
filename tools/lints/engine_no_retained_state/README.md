# `engine_no_retained_state` — the Axiom State Law

> **Persistent information may exist only as explicit data passed into and
> returned from engine computations. Executable engine machinery may not
> secretly retain, mutate, initialize, synchronize, cache, or otherwise remember
> information across invocations.**

This is a zero-tolerance architectural law, on the same footing as the Layer Law,
the Module Law, the Coverage Law and the Branchless Law.

## The one distinction that matters

> **Explicit state data is legal. Hidden retained state in engine behavior is
> illegal.**

The law is not "games cannot have state" and it is not "structs are bad". A game
is nothing *but* state. The law says where that state may live: in values the
caller owns, hands in, and gets back — never inside the machinery.

### Legal

```rust
// A struct that IS the state. This is the entire point.
pub struct BaseballState {
    pub score: u32,
    pub inning: u8,
    pub ball: BallState,
}

// State in, state out. Nothing is remembered between calls.
pub fn step(state: &BaseballState, input: &Input) -> BaseballState {
    let mut next = state.clone();   // local mutation building the return value
    next.score += 1;
    next
}

// Local mutation to construct an output.
pub fn build_effects(input: &Input) -> Vec<Effect> {
    let mut effects = Vec::new();
    effects.push(Effect::Score);
    collect_more(&mut effects);     // a PRIVATE helper may take `&mut` to build output
    effects
}

fn collect_more(out: &mut Vec<Effect>) { out.push(Effect::Cheer); }

const FIXED_STEP_NS: u64 = 16_666_667;          // compile-time value
const MAX_PLAYERS: usize = 64;

pub struct Rules { pub score_for: fn(&BaseballState) -> u32 }  // plain fn pointer
pub struct StateTable<K, V> { pub keys: Vec<K>, pub values: Vec<V> }  // data generic
```

`Vec`, arrays, tuples, `Box`, `String`, `BTreeMap` and every other ordinary owned
container are explicit data. *Data contains values* is not *the engine secretly
retains state*.

### Illegal

```rust
static CURRENT_SCORE: AtomicU32 = AtomicU32::new(0);   // static-storage
thread_local! { static TICK: Cell<u32> = ...; }        // thread-local-storage
struct Engine { score: RefCell<u32> }                  // interior-mutability
struct Engine { world: Arc<World> }                    // shared-state-ownership

impl Engine {
    pub fn update(&mut self, input: Input) { }         // mutable-engine-api
    pub fn get_mut(&mut self) -> &mut World { }        // mutable-engine-api
}

pub async fn load() -> u32 { }                         // retained-execution-state
pub fn on(f: Box<dyn Fn(Input)>) { }                   // stateful-callback-boundary
pub fn render(r: &dyn Renderer) { }                    // opaque-behavior-state
pub fn apply<F: Fn(Input)>(f: F) { }                   // generic-behavior-state
impl Iterator for Counter { }                          // stateful-trait-implementation
impl Drop for Session { }                              // drop-side-effect-hole
unsafe { *ptr }                                        // unsafe-state-escape
```

The rewrite is always the same shape: replace
`fn step(&mut self, input: Input)` with
`fn step(state: &State, input: &Input) -> State`. Instead of `Mutex<Cache>`,
either make the cache an explicit input the caller passes in, or move the
imperative retained resource **out** of the engine and into the app tier, which
is allowed to own it.

## The twelve categories

Every diagnostic is prefixed with its category, which is also the token the audit
groups on.

| Category | What it flags |
|---|---|
| `static-storage` | Every user-declared `static`, **including immutable ones** and `extern` statics. `const` stays legal. |
| `thread-local-storage` | `thread_local!` and `std::thread::LocalKey`. |
| `interior-mutability` | `UnsafeCell`, `SyncUnsafeCell`, `Cell`, `RefCell`, `OnceCell`, `LazyCell`, `OnceLock`, `LazyLock`, `Mutex`, `RwLock`, `ReentrantLock`, `Once`, and **every** standard atomic (`Atomic*`, present and future). |
| `shared-state-ownership` | `Rc`, `Arc`, `rc::Weak`, `sync::Weak`. `Box<T>` and `Vec<T>` are *not* flagged. |
| `mutable-engine-api` | A **public** signature taking `&mut T` / `&mut self`, or returning `&mut T`, including through wrappers (`Option<&mut T>`, `&mut [T]`, tuples). Private `&mut` is legal. |
| `retained-execution-state` | `async fn`, author-written `async` blocks/closures, and `Future` on a public boundary. |
| `stateful-callback-boundary` | `dyn Fn`/`FnMut`/`FnOnce` (boxed or by reference) and `impl Fn*` on a public boundary — parameters, returns, and public fields. A plain `fn(Input) -> Output` pointer is legal. |
| `opaque-behavior-state` | `dyn Trait` on a public boundary: parameters, returns, public fields, public aliases. Static dispatch through a concrete type is untouched. |
| `generic-behavior-state` | A **public** generic parameter bounded by `Fn`/`FnMut`/`FnOnce`/`AsyncFn*`/`Future`/`IntoFuture`. Data generics (`StateTable<K, V>`) are legal. |
| `stateful-trait-implementation` | An engine-defined `impl` of `Future`, `Iterator`, or `DoubleEndedIterator`. Consuming an iterator from `Vec::iter()` or a dependency is legal. |
| `drop-side-effect-hole` | A custom `Drop` impl written by engine code. |
| `unsafe-state-escape` | `unsafe` blocks, `unsafe fn`, `unsafe trait`, `unsafe impl`, raw pointers in signatures and fields, `extern` blocks. |

## How it works

The primary detector is a **semantic late-pass dylint**, not a source scanner. It
reads HIR and resolved `rustc_middle::ty::Ty`, so nothing is matched by spelling.

### Recursive / aliased type detection

`src/prohibited.rs` is the reusable inspector. `find(tcx, ty, descent, probe)`
walks a composed type looking for a component a probe rejects. Because the input
is a **resolved** `Ty`, aliases are already expanded before the walk begins:

```rust
type Hidden = RefCell<World>;
struct Something { value: Option<Box<Hidden>> }   // caught: Option<Box<RefCell<World>>>

type Shared<T> = Arc<T>;
struct Other { value: Shared<World> }             // caught: Arc<World>
```

Classification is by resolved `DefId`: the defining crate must be `core` /
`alloc` / `std` and the item name must match, so a re-export or a rename
(`use std::sync::Mutex as Lock`) resolves to the same answer, and a user type
called `Cell` in an engine crate does not collide. Atomics are matched
generically (name starts with `Atomic`, defined in the `atomic` module), so
`AtomicU32` — today an alias for `Atomic<u32>` — and every atomic added in a
future toolchain are covered without a list to maintain.

The walk descends through references, raw pointers, slices, arrays, patterns,
tuples, alias arguments, and the generic arguments of any ADT (which is what
covers `Option<T>`, `Result<T, E>`, `Box<T>`, `Vec<T>` and every engine generic).
A function pointer's signature is deliberately **not** descended: `fn(Input) ->
Output` is explicitly legal.

Termination is guaranteed two ways: a **visited set keyed by the interned `Ty`**
(the compiler's own identity for a fully-resolved type, so a recursive ADT such
as `struct Chain { next: Option<Box<Chain>>, slot: Cell<u32> }` revisits a key
and stops) plus a depth cap.

There are two descent depths, and the difference is deliberate:

* **`Structural`** — the type's own structure only. Used for **declaration
  positions**: struct/enum fields and function signatures. A prohibited type is
  therefore reported once, at the declaration that actually *introduces* it, and
  not again at every type that transitively contains that declaration. That is
  what makes the audit a work list rather than a fan-out.
* **`Transitive`** — `Structural` plus the field types of crate-local ADTs. Used
  for `const` / `static` item types and `type` aliases, where the whole composed
  value is the subject and there is no inner declaration to attribute the finding
  to. (A `const` holding interior mutability is the classic "every use is a fresh
  copy" trap.)

### Scope

In scope: the reusable engine spine — `crates/<layer>/src/**` and
`modules/<module>/src/**`. Out of scope: `apps/` (composition leaves, which are
*allowed* to own the current explicit state snapshot and imperative
host/platform resources), `tools/`, `xtask`, the `axiom-zones` support crate, and
all test code (`#[test]` functions, `#[cfg(test)]` modules, and whole
`tests/` / `examples/` / `benches/` files).

Scoping reuses `engine_lint_helpers::is_engine_file` — the same classifier every
other lint in the rulebook uses — rather than a second copy of the tier rules.
(The `xtask` crate's `PackageClass` machinery is the canonical *manifest-level*
classifier, but it lives in the root workspace; `tools/lints/` is a separate
workspace pinned to a `rustc_private` nightly and cannot depend on it. The two
agree on the tier boundary by construction: `is_engine_file` keys on exactly the
`crates/` and `modules/` directories the Module Law defines.)

No engine layer or module is exempt. If a spine crate contains retained state
today, that is a violation to inventory, not a reason to carve it out.

### Preventing suppression of the law

A dylint cannot refuse its own `#[allow]`: a lint level attribute is applied by
rustc before the pass runs, and an unknown lint name is at most a warning in a
plain `cargo build`. So suppression is closed by one narrow source check in the
architecture checker — `crates/xtask/src/hygiene.rs`,
`ViolationKind::SourceHygieneStateLawSuppression`:

* **naming `engine_no_retained_state` at all** inside a layer or module is a
  violation. Engine code has no legitimate reason to write the name, so a bare
  identifier match closes every spelling at once — `#[allow(...)]`,
  `#[expect(...)]`, `#![allow(...)]` at crate level, and
  `#[cfg_attr(feature = "x", allow(...))]`;
* `allow(warnings)` / `expect(warnings)` — the indirect route that would silence
  the law without ever naming it.

It scans raw text (not comment-stripped): a commented-out suppression is still a
sign the law is being worked around. This is deliberately the **only**
source-scanning rule the State Law adds; it exists solely because self-suppression
is not something a lint can mechanically refuse. Every other rule is semantic.

## What this lint can and cannot prove

It is a **structural floor**, not a proof of purity. Do not read a clean run as
"the engine is provably stateless."

**What it proves.** Within the code the compiler actually compiles, no spine
declaration introduces a standard-library state carrier, no public boundary hands
out mutable references / trait objects / callbacks / futures, and no `static`,
`unsafe`, `Drop`, `Future`/`Iterator` impl, or `extern` block exists.

**What it does not prove — the honest list:**

1. **Foreign types are opaque.** For a type from an external dependency, rustc
   exposes the definition but nothing about whether its *behavior* retains state.
   A dependency type that internally holds a `Mutex` is not visible as such
   unless the `Mutex` appears in a field type the engine names. The lint does
   **not** silently claim such a type is safe — it simply cannot see inside it.
   Closing this needs a dependency purity allowlist/audit, which is deliberately
   **not** built here. The design is ready for it: `state_carrier` in
   `src/prohibited.rs` is a single `DefId → Category` function, and an allowlist
   would extend exactly that one place.
2. **`cfg`-gated code is only checked on arms that are compiled.** A native
   `cargo check` never sees `#[cfg(target_arch = "wasm32")]` bodies. That is why
   `scripts/retained_state_audit.py` runs the rulebook a *second* time against
   `--target wasm32-unknown-unknown` — it found 78 findings invisible to the
   native run, including the only `static-storage` and `thread-local-storage`
   sites in the repo. Arms behind a **non-default feature** (e.g.
   `axiom-gpu-backend`'s `offscreen`) are still unscanned; this is the same scope
   hole the coverage gate has.
3. **Retention is inferred from declared type positions.** The lint reports
   fields, signatures, `const`/`static` types and aliases — not every expression.
   That is sound for *retention*: to remember something you must store it, and to
   store it you need a field or a static; to leak it you need a return type. All
   three are declared type positions. A purely invocation-local
   `let scratch = RefCell::new(0)` is not reported, and per the law's own
   examples it is not a violation.
4. **`impl Trait` over an ordinary trait is not flagged.** The rules as specified
   ban `dyn Trait` on public boundaries and engine-defined `Iterator` impls;
   `-> impl Iterator<Item = T>` is neither, and it is the spine's dominant
   branchless idiom. It does conceal a state machine, and widening the law to
   cover it is a deliberate future amendment, not an oversight.
5. **Trait-impl method signatures are checked where the trait is declared.** An
   `impl Trait for Type` method is not held to the public-boundary rules, because
   its signature is the trait's contract, not this crate's choice. An
   engine-defined `pub trait` with `&mut self` *is* flagged, at its declaration.
   A foreign trait's `&mut` (e.g. `Display::fmt`'s `&mut Formatter`) is not.
6. **Visibility is declared visibility.** `pub fn` inside a private module counts
   as public, matching the rest of the rulebook
   (`engine_no_unitless_float_public_api`). This over-reports rather than
   under-reports.

## Running

```sh
# The lint's own UI suite (from this crate):
cargo test

# The whole rulebook over the workspace — the repo's canonical invocation,
# used by scripts/dylint-gate.sh and .github/workflows/ci.yml (from repo root):
cargo dylint --all -- --all-targets

# Regenerate the current-repository inventory (native + wasm32 arms):
uv run scripts/retained_state_audit.py     # -> docs/audits/retained-state-audit.md
```

### Enforcement status — read this before "fixing the gate"

`engine_no_retained_state` has **no entry in `tools/lints/dylint-baseline.txt`,
by design**. The gate treats a missing entry as a ceiling of zero, so it fails on
this lint today. That is the correct state: the acceptable count for a
zero-tolerance law is zero, and the engine is not there yet
(`docs/audits/retained-state-audit.md` records **787** distinct source sites across 49
spine packages). The gate itself prints a different, smaller number —
`engine_no_retained_state = 49` — because it counts *compilation units that
emitted the lint*, not findings; both numbers are correct measures of different
things.

Do **not** resolve that failure by adding a baseline number, an `#[allow]`, or a
scope carve-out. Resolve it by removing retained state. Migration is a separate,
later task; making the violations mechanically visible — and impossible to
introduce accidentally once migration begins — is what this lint is for.

## Layout

```text
src/lib.rs         the late lint pass: item/impl-item/trait-item/field/expr/block hooks
src/category.rs    the twelve categories, their slugs, and their rewrite directions
src/prohibited.rs  the reusable resolved-type inspector (walker + probes + classifier)
ui/modules/m/src/  compile-fail fixtures (one per rule) + the compile-pass legal fixture
ui/apps/a/src/     the out-of-scope app fixture (must produce zero findings)
```
