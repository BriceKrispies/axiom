# 01 — Runtime Preparation Primitive

## Mission

Add the two new public types that constitute the entire foundational surface of
startup preparation — `PreparationTask` and `PreparationSchedule` — plus the
`RuntimeState::Prepared` variant they exist to reach, to `crates/axiom-runtime`.
This manifest lands **purely additive** code: it does not change `Runtime`'s
behaviour, does not gate anything, and must not break a single existing call
site. Manifest `02` consumes what you build and closes the barrier.

## Architectural owner

- **Package:** `crates/axiom-runtime`
- **Classification:** Layer (`runtime`), governed by the Layer Law
- **Facade:** `crates/axiom-runtime/src/lib.rs` — 18 private `mod`s + 18
  `pub use` re-exports, strictly one public type per file
- **Why here:** the barrier is a precondition on `Runtime::step`, which lives in
  this crate; `RuntimeState` is already the engine's only construction→running
  state machine. See `README.md` §2.

## Depends on

**None — may begin immediately.**

## Parallel safety

**Nothing may run concurrently with this manifest.** It owns
`crates/axiom-runtime/src/lib.rs`, which every other runtime-crate stream would
otherwise contend for. Wave 1, width 1.

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-runtime/src/preparation_task.rs` | **create** |
| `crates/axiom-runtime/src/preparation_schedule.rs` | **create** |
| `crates/axiom-runtime/src/runtime_state.rs` | modify (61 lines) |
| `crates/axiom-runtime/src/lib.rs` | modify (65 lines) |

## Files allowed to modify

Only the four above.

## Files forbidden to modify

- `crates/axiom-runtime/src/runtime.rs` — owned by `02`
- `crates/axiom-runtime/src/runtime_error_code.rs` — owned by `02`
- `crates/axiom-runtime/layer.toml`, `ARCHITECTURE.md` — owned by `03`
- `crates/axiom-runtime/tests/**` — owned by `04` and `05`
- `crates/axiom-runtime/Cargo.toml`, workspace `Cargo.toml` — **no Cargo change
  is required by this work**
- Everything outside `crates/axiom-runtime/src/`

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `crates/axiom-runtime/src/runtime_scheduler.rs:32-155` | **Partial template.** Copy the hand-written `Debug` (`:36-49`) and the `try_fold`/`ControlFlow` execute idiom (`:133-152`). **Do NOT copy `register` (`:70-107`) or the sort (`:102`)** — README §7 explains why that shape does not transfer |
| `crates/axiom-runtime/src/runtime_system.rs:12` | `RuntimeSystem` — the trait your new trait must be *incompatible* with |
| `crates/axiom-runtime/src/system_outcome.rs` | Outcome shape (you are **not** building one; read it to understand the idiom) |
| `crates/axiom-runtime/src/runtime_state.rs` (all 61 lines) | The enum you are extending, incl. `discriminants_are_stable_and_ordered` at `:41` |
| `crates/axiom-runtime/src/lib.rs` (all 65 lines) | The facade convention |

## Contract consumed

Nothing new from the kernel. The schedule holds only `&'static str` names and
boxed tasks; `HandleId` is deliberately **not** used (README §7).

## Contract produced

This is the **frozen** contract. Downstream manifests are written against it
verbatim. Do not rename, do not add parameters, do not add methods.

```rust
// crates/axiom-runtime/src/preparation_task.rs
pub trait PreparationTask {
    fn prepare(&mut self) -> RuntimeResult<()>;
}

// crates/axiom-runtime/src/preparation_schedule.rs
pub struct PreparationSchedule { /* private: Vec<(&'static str, Box<dyn PreparationTask>)> */ }

impl PreparationSchedule {
    pub fn new() -> Self;
    pub fn push(&mut self, name: &'static str, task: Box<dyn PreparationTask>);
}

impl Default for PreparationSchedule { … }
impl std::fmt::Debug for PreparationSchedule { … }

// crates/axiom-runtime/src/runtime_state.rs
pub enum RuntimeState { … , Prepared = 6 }
```

**`push`, not `register` — and no `HandleId`, no `order: i32`, no `Result`.**
Read README §7 before you start; it explains at length why mirroring
`RuntimeScheduler::register` here would be cargo-culting. In one line: that
registry is long-lived and multi-writer, this schedule is built at one site in a
straight line and immediately consumed, so **push order already is a
deterministic total order**.

Additionally, for manifest `02`'s exclusive use, expose a **crate-private**
executor:

```rust
impl PreparationSchedule {
    /// Run every task in push order, stopping at the first failure.
    /// Returns the failing task's name **and its own error**, or `None`.
    pub(crate) fn execute(&mut self) -> Option<(&'static str, RuntimeError)>;
}
```

Returning the task's `RuntimeError` (which is `Copy`) is what lets `02` report
*which* task failed **and** *why*.

`execute` is `pub(crate)` deliberately: only `Runtime::prepare` may drive a
schedule, and making it public would let a caller run preparation without
touching the lifecycle.

## Implementation instructions

1. **`runtime_state.rs`** — append `Prepared = 6` after `Failed = 5`. **Do not
   renumber**: `raw()` is a stable identity byte surfaced through
   `RuntimeStepRecord::state_after()`.
2. **Drop the `PartialOrd, Ord` derive** from `RuntimeState`. The workspace's
   only consumer of that ordering is the assertion
   `RuntimeState::Created < RuntimeState::Running` at `runtime_state.rs:45` — a
   test asserting the derive it tests — and `RuntimeState` is a
   `BTreeMap`/`BTreeSet` key nowhere. Replace that line with
   `assert_eq!(RuntimeState::Prepared.raw(), 6);` and extend the test to pin all
   seven discriminants. Add a doc line stating `raw()` is identity, not order.
3. **`preparation_task.rs`** — the trait, with a module doc (required:
   `engine_require_module_docs` is at cap 0) explaining that the zero-argument
   signature is what makes a task un-registerable as a `RuntimeSystem`.
4. **`preparation_schedule.rs`** — transcribe `runtime_scheduler.rs`:
   - a private `Registered { name, task }` entry struct;
   - `push` — an infallible `Vec::push`. **No duplicate detection, no sort.**
     There is nothing to detect (one writer) and nothing to sort (push order is
     the order);
   - a hand-written `Debug` mirroring `runtime_scheduler.rs:36-49` (required —
     `Box<dyn PreparationTask>` is not `Debug` and
     `crates/axiom-runtime/Cargo.toml:18` sets
     `missing_debug_implementations = "warn"`);
   - `pub(crate) fn execute(&mut self) -> Option<(&'static str, RuntimeError)>`
     as a `try_fold` over `iter_mut()`, selecting continuation with
     `[ControlFlow::Continue(()), ControlFlow::Break((name, err))][usize::from(failed)]`.
5. **`lib.rs`** — add two `mod` lines and two `pub use` lines, in the existing
   alphabetical position. Update the module-doc sentence that enumerates the
   layer's lifecycle so it mentions preparation.

## Required behavior

- Tasks run in **push order**: pushing `a`, `b`, `c` runs `a`, `b`, `c`.
- `execute` stops at the first failing task and returns its name **and its
  error**; later tasks do not run.
- `execute` returns `None` when every task succeeded, including for an empty
  schedule.
- `RuntimeState::Prepared.raw() == 6`; every other discriminant is unchanged.

## Error behavior

`push` is infallible — there is no duplicate to detect with a single writer.
`execute` reports failure by returning `Some((name, error))`, preserving the
task's own `RuntimeError` (which is `Copy`) so `02` can report both which task
failed and why. It never panics.

## Determinism requirements

- Total order by **push order**; no key, no sort, no tie-breaker.
- No clock, no randomness, no iteration over a `HashMap`.
- Single-pass, single-threaded. No `async`, no threads — `crates/axiom-runtime`
  contains zero async today and must keep it that way.

## Tests

Inline `#[cfg(test)] mod tests` at the bottom of each new file.

- `tasks_run_in_push_order` — via a shared trace
  (`Rc<RefCell<Vec<&'static str>>>`), the idiom at `runtime_scheduler.rs:264`
- `execute_stops_at_the_first_failure_and_reports_it` — asserts both the name
  and the propagated error code
- `execute_on_an_empty_schedule_succeeds`
- `the_schedule_and_its_debug_are_constructible` — covers `new`, `Default` and
  the hand-written `Debug` (a region the Coverage Law will otherwise flag)
- `discriminants_are_stable` (in `runtime_state.rs`) — all seven pinned

## Architecture validation

- New code must reference **only** `axiom_kernel`. Four sibling layers
  mechanically assert `crates/axiom-runtime/src` contains no `axiom_frame`,
  `axiom_host`, `axiom_math` or `axiom_ecs` token
  (`crates/axiom-frame/tests/architecture.rs:308` and siblings).
- `runtime` is **not** in `PLATFORM_FACING_LAYERS` (`crates/xtask/src/hygiene.rs:52`),
  so the substrings `web_sys js_sys wasm_bindgen WebGPU WebGL
  requestAnimationFrame window. document. canvas` are all banned — note
  **`canvas` is banned as a bare substring**.
- No `println!`/`eprintln!`/`dbg!`/`todo!`/`unimplemented!`; no file named
  `utils`/`helpers`/`common`/`misc`; no `#[coverage(off)]`.
- **Branchless** (`engine_no_branching` cap 0): no `if`/`match`/`for`/`while`/
  `&&`/`||`/`?`/`if let`. Also cap 0: `no_unwrap_in_engine`,
  `engine_no_recursion`, `engine_no_wildcard_imports`,
  `engine_require_module_docs`. File ≤1000 lines, fn ≤120 lines.
- **100% coverage** — `crates/` is fully inside the gate.

## Performance considerations

None. Registration is O(n log n) on a handful of entries, once per launch.

## Documentation changes

Only the `lib.rs` module-doc sentence and the new files' own module docs.
`ARCHITECTURE.md` belongs to manifest `03`.

## Completion criteria

- [ ] Two new files exist, each with a module doc and inline tests
- [ ] `RuntimeState::Prepared = 6`; `PartialOrd, Ord` derive removed; all seven
      discriminants pinned
- [ ] `lib.rs` exports exactly two new symbols
- [ ] `pub(crate) execute` returns `Option<(&'static str, RuntimeError)>`
- [ ] `cargo test -p axiom-runtime` green
- [ ] **No existing call site changed** — the crate still compiles for every
      current consumer
- [ ] Branchless and fully covered

## Validation commands

```sh
cargo test -p axiom-runtime
cargo build --workspace
cargo run -p xtask -- check-architecture
```

## Deliverable to orchestrator

Report: commit hash; the four file paths; `cargo test -p axiom-runtime` output
tail; confirmation that the public surface is exactly two new symbols;
confirmation that no other crate needed a change; any deviation or blocker.
