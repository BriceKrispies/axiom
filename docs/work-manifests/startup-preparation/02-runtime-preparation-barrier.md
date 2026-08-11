# 02 — Runtime Preparation Barrier

## Mission

Add `Runtime::prepare`, add `RuntimeErrorCode::PreparationFailed`, and **close
the barrier**: change `Runtime::start` to require `Prepared` instead of
`Initialized`. Because that change breaks compilation for every existing
lifecycle sequence, this manifest also fixes the **8 call sites that live inside
`runtime.rs` itself**, so the crate compiles and `main` stays green in a single
atomic commit. The other 15 sites belong to manifest `05` and `06`.

## Architectural owner

- **Package:** `crates/axiom-runtime`
- **Classification:** Layer (`runtime`)
- **Facade:** `Runtime` — `crates/axiom-runtime/src/runtime.rs`
- **Why here:** `Runtime::step` is the operation being gated and it lives here.
  A gate anywhere else could be bypassed by calling `Runtime::step` directly.

## Depends on

**`01-runtime-preparation-primitive.md` — must be merged first.** You need
`PreparationSchedule`, its `pub(crate) execute`, and `RuntimeState::Prepared`.

## Parallel safety

This manifest is the head of an **atomic landing group**:

> **`02` + `05` + `06` are developed in parallel but land together as one merge.**

The `start()` change breaks 15 call sites outside this crate. Those live in
files `02` does not own, so they cannot be fixed here — and fixing them
separately would leave `main` red between commits. Resolution: agents for `05`
and `06` **branch from `02`'s branch, not from `main`**, write against the frozen
contract below, and the orchestrator merges all three together. This is the
stacked-branch model the repo already uses for fan-out work.

Also concurrent, but independently landable: **`03`** (touches no `.rs`).

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-runtime/src/runtime.rs` | modify (656 lines) |
| `crates/axiom-runtime/src/runtime_error_code.rs` | modify (53 lines) |

## Files allowed to modify

Only the two above.

## Files forbidden to modify

- `crates/axiom-runtime/src/lib.rs`, `preparation_task.rs`,
  `preparation_schedule.rs`, `runtime_state.rs` — owned by `01`
- `crates/axiom-runtime/src/runtime_scheduler.rs` — owned by `05` (it holds one
  breaking call site at `:404`)
- `crates/axiom-runtime/tests/**` — owned by `04` and `05`
- `crates/axiom-runtime/layer.toml`, `ARCHITECTURE.md` — owned by `03`
- Everything outside `crates/axiom-runtime/src/`

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `crates/axiom-runtime/src/runtime.rs:79-155` | The four existing transitions + the `step` gate. Copy the idiom exactly |
| `crates/axiom-runtime/src/runtime.rs:291-572` | `mod tests` — contains 6 of your 8 call sites |
| `crates/axiom-runtime/src/runtime.rs:574-656` | `mod cov` — contains the other 2 |
| `crates/axiom-runtime/src/runtime_error.rs:23,33` | `RuntimeError::new(code, &'static str)` and `with_kernel`. **There is no constructor that wraps a `RuntimeError`** |
| `crates/axiom-runtime/src/runtime_scheduler.rs:133-152` | The `try_fold` + `ControlFlow` table idiom |

## Contract consumed

From `01`, verbatim:

```rust
pub trait PreparationTask { fn prepare(&mut self) -> RuntimeResult<()>; }
pub struct PreparationSchedule { … }
impl PreparationSchedule {
    pub fn new() -> Self;
    pub fn push(&mut self, name: &'static str, task: Box<dyn PreparationTask>);
    pub(crate) fn execute(&mut self) -> Option<(&'static str, RuntimeError)>;
}
pub enum RuntimeState { …, Prepared = 6 }
```

## Contract produced

```rust
// crates/axiom-runtime/src/runtime.rs
impl Runtime {
    /// Initialized -> Prepared (all tasks Ok) or Failed (any task Err).
    pub fn prepare(&mut self, schedule: PreparationSchedule) -> RuntimeResult<()>;
}

// crates/axiom-runtime/src/runtime_error_code.rs
pub enum RuntimeErrorCode { …, PreparationFailed = 8 }
```

**Behavioural contract every downstream manifest relies on:**
`start()` accepts exactly `{Prepared, Paused}`. The migration idiom for any
existing sequence is `initialize(); prepare(PreparationSchedule::new())?; start();`.

## Implementation instructions

1. **`runtime_error_code.rs`** — append `PreparationFailed = 8`. Extend the
   discriminant test to pin it.
2. **`runtime.rs`** — add `prepare` beside the other transitions, branchless:

```rust
pub fn prepare(&mut self, schedule: PreparationSchedule) -> RuntimeResult<()> {
    (self.state == RuntimeState::Initialized)
        .then_some(schedule)
        .map_or(Err(invalid_transition("prepare requires Initialized")),
                |s| self.run_preparation(s))
}

#[axiom_zones::sim]
fn run_preparation(&mut self, mut schedule: PreparationSchedule) -> RuntimeResult<()> {
    let failure = schedule.execute();          // schedule dropped at end of fn
    self.state = [RuntimeState::Prepared, RuntimeState::Failed]
        [usize::from(failure.is_some())];
    // Keep BOTH facts: which task failed (name) and why (its own code).
    failure.map_or(Ok(()), |(name, cause)| Err(RuntimeError::new(cause.code(), name)))
}
```
   The schedule is taken **by value** and dropped inside `run_preparation` —
   that is what makes "temporary work can die" a guarantee.
3. **`start()`** — change the accepted set from
   `(Initialized) | (Paused)` to `(Prepared) | (Paused)`; update the error
   message to `"start requires Prepared or Paused"`.
4. **`stop()`** — add `Prepared` to its accepted set.
5. **Add `#[axiom_zones::sim]` to all three of `prepare`, `run_preparation` and
   `run_one_step`.** `engine_lint_helpers::in_zone`
   (`tools/lints/engine_lint_helpers/src/lib.rs:84`) walks the HIR parent chain
   and `item_has_marker` matches only `ItemKind::Fn` and **inline** `ItemKind::Mod`
   — so a marker on `prepare` covers `prepare`'s body **and nothing else**.
   Marking only `prepare` would leave `run_preparation` (where the work actually
   happens) unguarded, which is precisely the trap `runtime.rs:143` falls into
   today: it is the crate's only marker and `run_one_step` sits outside it.
6. **Fix the 8 in-file call sites** at `runtime.rs:305, 319, 334, 335, 356, 506,
   566, 600` (with their paired `start()` at `:306, 321, 325, 342, 357, 507, 567,
   601`). Most take the mechanical insertion. **Two must be rewritten, not
   patched:**
   - `double_initialize_is_rejected` (around `:335`) — still valid, but re-read it
   - `start_without_initialize_is_rejected` (around `:342`) — **its premise
     changes**. Rewrite it as `start_without_preparation_is_rejected`: after
     `initialize()`, `start()` must return `InvalidLifecycleTransition`. This is
     the barrier's headline test; make it say so.

## Required behavior

- `initialize(); prepare(empty)` → `Prepared`, `Ok(())`.
- `initialize(); start()` → `InvalidLifecycleTransition`, state stays
  `Initialized`. **This is the barrier.**
- `initialize(); prepare(..); start()` → `Running`; `step()` then succeeds.
- A failing task → state `Failed`; `prepare` returns an error whose **message is
  the failing task's name** and whose **code is the task's own code**; `start()`
  afterwards returns `InvalidLifecycleTransition`.
- `prepare()` from `Created`, `Prepared`, `Running`, `Paused`, `Failed` or
  `Stopped` → `InvalidLifecycleTransition`. **Preparation runs exactly once per
  launch.**
- `Prepared → Stopped` is legal.
- `Running → pause() → start()` still works from `Paused` with no schedule, and
  re-runs no task.
- 100 `step()` calls after `Running` leave a counting task's run count at 1.

## Error behavior

`InvalidLifecycleTransition` for every illegal transition (existing code). A
preparation failure propagates the **task's own code** with the **task's name**
as the message — the runtime does not overwrite the diagnosis.
`RuntimeErrorCode::PreparationFailed` exists as the vocabulary a *task* uses when
it has no better code (README §7 shows the worked example); the runtime itself
never manufactures it. `step()` from a non-`Running` state still returns
`StepWhileNotRunning`, unchanged.

## Determinism requirements

Sequential, single-pass, first-failure-wins. No clock (enforced by the `#[sim]`
marker you add). No async. Branchless.

## Tests

Inline in `runtime.rs`'s existing `mod tests` / `mod cov`:

- `preparation_runs_before_running` ★
- `running_cannot_begin_before_preparation_completes` ★ (the rewritten
  `start_without_preparation_is_rejected`)
- `successful_preparation_permits_the_transition` ★
- `failed_preparation_blocks_the_transition` ★
- `a_failing_task_stops_the_remaining_tasks`
- `the_error_names_the_failing_task_and_keeps_its_code`
- `preparation_runs_exactly_once_per_launch` ★
- `an_empty_schedule_prepares_immediately`
- `stepping_does_not_rerun_preparation` ★
- `preparation_is_rejected_before_initialize`
- `preparation_is_rejected_from_terminal_states`
- `stop_is_legal_from_prepared`
- `pause_and_resume_do_not_reenter_preparation`
- `a_failed_preparation_leaves_the_step_gate_closed`

## Architecture validation

Same constraints as `01`: kernel-only imports, no platform substrings,
branchless, no `unwrap`, 100% coverage. Note `runtime.rs` is **656 lines** —
`engine_no_large_files` fires at 1000, and `engine_no_large_functions` at 120
lines per fn (cap 2 for the whole workspace, already consumed elsewhere — keep
`prepare` and `run_preparation` small).

## Performance considerations

`prepare` runs once per launch. On `wasm32` it blocks the main thread for its
duration — which is **exactly what happens today**, so it is not a regression.
Do not add a budget or a yield; see README §16.

## Documentation changes

Doc comments on `prepare`, on the changed `start`/`stop`, and on
`PreparationFailed`. `ARCHITECTURE.md` belongs to `03`.

## Completion criteria

- [ ] `Runtime::prepare` exists with the exact contract signature
- [ ] `start()` accepts exactly `{Prepared, Paused}`
- [ ] `stop()` additionally accepts `Prepared`
- [ ] `PreparationFailed = 8`
- [ ] `#[axiom_zones::sim]` on `prepare`, `run_preparation` **and** `run_one_step`
- [ ] All 8 in-file call sites fixed; `start_without_initialize_is_rejected`
      rewritten as a barrier test
- [ ] `cargo test -p axiom-runtime` green
- [ ] Branchless, fully covered

## Validation commands

```sh
cargo test -p axiom-runtime
bash scripts/dylint-gate.sh          # run alone — never concurrently with another gate
```

**On your own branch, `cargo build --workspace` will fail**: 15 call sites in
crates you do not own are still broken. That is correct and expected — they are
`05`'s and `06`'s. Do **not** reach outside your owned files to fix them, and do
not weaken `start()` to make the build pass. `cargo test --workspace` is a
gate for the *merged group*, run by the orchestrator, not by you.

Enumerate the breakage precisely and hand it to the orchestrator:

```sh
cargo build --workspace 2>&1 | grep -E '^error' -A 3
```

## Deliverable to orchestrator

Report: commit hash; both file paths; test output tail; the exact list of
remaining broken call sites outside this crate (so `05`/`06` can be dispatched);
confirmation that `start()`'s accepted set is `{Prepared, Paused}`; deviations.
