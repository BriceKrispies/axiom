# 04 — Runtime Preparation Integration Tests and Surface Lock

## Mission

Prove the preparation lifecycle end-to-end from *outside* the crate, and pin the
new public surface so it cannot silently widen. Two things nothing else owns:
a generic-application integration test that never names an engine-domain concept,
and `crates/axiom-runtime`'s **first** `tests/architecture.rs` — the crate has
none today, so its public surface is currently unlocked.

## Architectural owner

- **Package:** `crates/axiom-runtime`
- **Classification:** Layer (`runtime`) integration tests
- **Why here:** these tests consume the crate through its public facade only,
  which is precisely what makes them integration tests rather than unit tests.
  All three files are **new**, so this manifest cannot collide with anyone.

## Depends on

**The `02` + `05` + `06` atomic landing group must be merged.** You test real
behaviour through the public API, so it must exist and the workspace must build.

## Parallel safety

Concurrent with `07` and anything after it. Owns only new files.

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-runtime/tests/preparation_lifecycle.rs` | **create** |
| `crates/axiom-runtime/tests/preparation.rs` | **create** |
| `crates/axiom-runtime/tests/architecture.rs` | **create** |

## Files allowed to modify

Only the three above.

## Files forbidden to modify

- **All of `crates/axiom-runtime/src/**`.** If a test cannot be written without
  changing `src`, that is a design defect — **stop and report it**. Do not widen
  a public API to make a test reachable (the Coverage Law names this explicitly).
- `crates/axiom-runtime/tests/integration.rs` — owned by `05`
- `crates/axiom-runtime/layer.toml`, `ARCHITECTURE.md` — owned by `03`
- Everything outside `crates/axiom-runtime/tests/`

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `crates/axiom-kernel/tests/architecture.rs:113-173` | **The template for the surface lock.** `lib_exports_are_curated_set` reads `src/lib.rs`, collects every trimmed line starting with `"pub "` and not `"pub(crate)"`, sorts, and `assert_eq!`s against a hard-coded sorted `Vec<&str>` of the **verbatim source lines including the trailing `;`** |
| `crates/axiom-math/tests/architecture.rs` | The non-root-layer variant, with `X_only_imports_declared_dependencies` |
| `crates/axiom-runtime/tests/integration.rs` | The existing integration style |
| `crates/axiom-kernel/src/deterministic_rng.rs` | `DeterministicRng::seeded` — for the generic product in your lifecycle test |

## Contract consumed

From `01` and `02`, verbatim — `PreparationTask`, `PreparationSchedule::{new,
push}`, `Runtime::prepare`, `RuntimeState::Prepared`,
`RuntimeErrorCode::PreparationFailed`. No `HandleId`, no order key.

## Contract produced

None. Tests only.

## Implementation instructions

### `tests/preparation_lifecycle.rs` — the generic-application proof

The whole point is that it names **no** engine-domain concept: no mesh, no
texture, no scene, no course. A deterministic table stands in for any product.

```rust
struct BuildTable { seed: u64, out: Rc<RefCell<Option<Vec<u64>>>> }

impl PreparationTask for BuildTable {
    fn prepare(&mut self) -> RuntimeResult<()> {
        let mut rng = DeterministicRng::seeded(self.seed);
        *self.out.borrow_mut() = Some((0..1024).map(|_| rng.next_u64()).collect());
        Ok(())
    }
}

#[test]
fn a_generic_application_prepares_then_runs() {
    let table = Rc::new(RefCell::new(None));
    let mut runtime = Runtime::new(RuntimeConfig::new(16_666_667)).unwrap();
    runtime.initialize().unwrap();

    let mut schedule = PreparationSchedule::new();
    schedule.push("table", Box::new(BuildTable { seed: 42, out: Rc::clone(&table) }));

    assert!(table.borrow().is_none(), "nothing is built before prepare");
    assert!(runtime.step().is_err(), "and the frame loop is closed");

    runtime.prepare(schedule).unwrap();
    assert_eq!(runtime.state(), RuntimeState::Prepared);
    assert_eq!(table.borrow().as_ref().unwrap().len(), 1024);

    runtime.start().unwrap();
    (0..10).for_each(|_| { runtime.step().unwrap(); });
    assert_eq!(table.borrow().as_ref().unwrap().len(), 1024, "stepping did not rebuild it");
}
```

Note the product cell is `Rc<RefCell<Option<T>>>`, **never a defaultable bare
`T`** — see README §8. Also add:

- `the_frame_loop_is_closed_until_prepared` — `step()` errs at `Created`,
  `Initialized` and `Prepared`; succeeds only at `Running`
- `a_generic_application_with_a_failing_task_never_runs` — product absent, state
  `Failed`, `start()` and `step()` both err

### `tests/preparation.rs` — determinism and ownership

- `equivalent_inputs_produce_equivalent_prepared_output` — two runtimes, two
  identically-seeded schedules of the same tasks, byte-equal products
- `temporary_preparation_data_is_discarded_at_the_barrier` — a task holding a
  `Drop`-counting scratch value has it dropped by the time `prepare()` returns,
  while the product it wrote survives
- `products_reach_the_caller_without_passing_through_the_runtime` — the caller's
  cell holds the product; `Runtime` exposes no accessor for it
- `a_task_that_reads_an_unwritten_product_fails_the_phase` — **push a consumer
  before its producer**; it finds `None`, returns `Err`, and the runtime lands in
  `Failed`. **Never a panic** — this is the test that pins README §8's
  `ok_or_else`-not-`expect` rule, and it is the exact hazard `07`'s `MeshTask`
  (which reads the course cell) lives with

### `tests/architecture.rs` — the surface lock

Model on `crates/axiom-kernel/tests/architecture.rs:113-173`. Two tests minimum:

1. `lib_exports_are_curated_set` — the verbatim-line equality assertion. After
   `01`, `src/lib.rs` has **20** `pub use` lines. List all 20.
2. `start_accepts_exactly_prepared_or_paused` — behavioural: `start()` errs from
   `Created`, `Initialized`, `Stopped` and `Failed`, and succeeds from `Prepared`
   and from `Paused`.

> Do **not** attempt "the only public path to `Running` is `start()`". That is a
> whole-crate reachability property a `#[test]` cannot observe. The behavioural
> accepted-set test above *is* the writable form of that intent.

Optionally add the sibling-layer hygiene tests the other layer crates carry
(`no_browser_or_js_apis`, `no_wall_clock_time`, `no_randomness`,
`no_console_printing`, `no_global_mutable_state`, `no_utils_module`) plus
`runtime_only_imports_declared_dependencies` (allowlist: `axiom_kernel`,
`axiom_zones`). These duplicate xtask/dylint coverage but match the convention of
every other layer crate.

## Required behavior

Every test asserts observable behaviour. No test may execute code without
asserting on its result — `test_without_assertion` is a dylint at cap 0.

## Error behavior

Tests assert on `RuntimeErrorCode` values, not on message strings, except for
`the_error_names_the_failing_task`-style checks which may assert the message
equals the registered task name.

## Determinism requirements

No clock, no unseeded randomness, no `HashMap` iteration. `DeterministicRng` is
seeded explicitly.

## Tests

The files above are entirely tests. Count: ≥3 in `preparation_lifecycle.rs`,
≥4 in `preparation.rs`, ≥2 in `architecture.rs`.

## Architecture validation

Test files are **exempt** from the branchless law and from
`is_engine_file`-scoped dylints (`tools/lints/engine_lint_helpers/src/lib.rs:43`
requires a `src` component). Write natural Rust — do **not** contort tests to be
branchless. They are also outside the coverage gate as *subjects*, though they
contribute coverage of `src`.

## Performance considerations

None.

## Documentation changes

None.

## Completion criteria

- [ ] Three new files, no `src` change
- [ ] `lib_exports_are_curated_set` pins all 20 `pub use` lines verbatim
- [ ] `start_accepts_exactly_prepared_or_paused` passes
- [ ] `a_task_that_reads_an_unwritten_product_fails_the_phase` proves `Failed`,
      not a panic
- [ ] The generic test names no engine-domain concept
- [ ] `cargo test -p axiom-runtime` green

## Validation commands

```sh
cargo test -p axiom-runtime
cargo test -p axiom-runtime --test architecture
cargo test -p axiom-runtime --test preparation_lifecycle
```

## Deliverable to orchestrator

Report: commit hash; three file paths; test names and count; the 20 pinned
export lines; confirmation that `src/` is untouched; deviations.
