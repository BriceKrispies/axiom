# 06 — Composition Root: Authoring Becomes Preparation

## Mission

Make `modules/axiom` — the umbrella that every `App`-based application is built
from — route through the preparation barrier, and fix the root defect this whole
programme exists to correct: `RunningApp::realize` currently calls
`runtime.start()` **before** it authors the scene, so the runtime reports
`Running` for an application whose meshes do not yet exist. The fix is not to
move a line; it is to express **scene authoring as a preparation task**, so the
ordering becomes structurally unreinstatable.

## Architectural owner

- **Package:** `modules/axiom` (the umbrella feature module)
- **Classification:** Feature module — the composition root
- **Facade:** `App` / `RunningApp`, `modules/axiom/src/app.rs`
- **Why here:** `RunningApp::realize` (`app.rs:324`) is the only place in the
  engine that calls `Runtime::initialize` + `Runtime::start` for a real
  application. It is where the transition into the frame loop is owned, so it is
  where the composition of the preparation phase belongs. The runtime owns the
  *phase*; this module owns *what the phase contains*.

## Depends on

**`02-runtime-preparation-barrier.md`** — and **branch from `02`'s branch, not
`main`**. You are part of the atomic landing group.

## Parallel safety

Concurrent with `02` (group head), `05` (other group member) and `03`. Your file
set is disjoint from all three — in particular `05` is explicitly forbidden from
touching `modules/axiom/src/app.rs`, which holds the 23rd broken call site and is
yours.

## Files owned

| Path | Action |
|---|---|
| `modules/axiom/src/app.rs` | modify (536 lines) |
| `modules/axiom/src/app_tests.rs` | modify (566 lines) |
| `modules/axiom/src/app/preparation.rs` | **create** |
| `modules/axiom/src/prelude.rs` | modify — **one line** |

## Files allowed to modify

Only the four above.

## Files forbidden to modify

- `modules/axiom/src/app/{authoring,frame,queries,components,dynamic_world,render_look,resources}.rs`
  — none of them needs to change
- `crates/axiom-runtime/**` — `01`, `02`, `04`, `05`
- `modules/axiom/module.toml`, `Cargo.toml` — `axiom-runtime` is already a
  dependency; **no manifest change is required**
- Any `apps/**` file — `07` onward
- `tools/axiom-shot/src/registry.rs` — zero-edit, but note it is a build tripwire

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `modules/axiom/src/app.rs:324-385` | `RunningApp::realize` — the defect. `initialize()` `:330`, `start()` `:334`, `Self::author(...)` `:353` |
| `modules/axiom/src/app.rs:386-434` | `Self::author` — the work that becomes a preparation task. Returns `AuthoredScene` |
| `modules/axiom/src/app.rs:436` | `reauthor` — **also calls `Self::author`**. Must keep working; it runs *after* `Running` and must not go through preparation |
| `modules/axiom/src/app.rs:82-260` | `App` struct + builder — where `prepare_with` and the `preparation` field go |
| `modules/axiom/src/app.rs:42-67` | The 7 `mod` declarations — your new `mod preparation;` joins them |
| `modules/axiom/src/app.rs:404-413` | Setup-time mesh/material id assignment (`(i + 1) as u64`). **Registration order is load-bearing** |
| `modules/axiom/src/app/frame.rs:46-47, 58-62, 102` | `tick` = `step` + `render`; `step` drives `Runtime::step` via `driver.drive`; `render` touches the runtime **zero** times |

## Contract consumed

From `01`/`02`, verbatim: `PreparationTask`, `PreparationSchedule::{new, push}`,
`Runtime::prepare`, `RuntimeState::Prepared`.

## Contract produced

```rust
// modules/axiom/src/app.rs
impl App {
    /// Contribute a startup preparation task. Tasks run in the order they are
    /// added, after the engine's own AuthorTask.
    pub fn prepare_with(mut self, name: &'static str,
                        task: Box<dyn PreparationTask>) -> Self;
}

// modules/axiom/src/prelude.rs   (one added line)
pub use axiom_runtime::PreparationTask;
```

**No order key and no reserved band.** `realize` pushes `AuthorTask` first, then
drains `app.preparation` in call order — so an app *cannot* get in front of the
engine's task. That is strictly stronger than a documented band, which relies on
everyone honouring a convention. See README §7.

The `prelude.rs` line is load-bearing: only **9 of 31 apps** Cargo-depend on
`axiom-runtime`, and adding that dependency purely to name the trait would be a
ceremonial dependency (forbidden). Re-exporting through the umbrella's single
facade — which already re-exports five layers — makes `prepare_with` usable by
every app with no manifest change.

## Implementation instructions

1. **`modules/axiom/src/app/preparation.rs` (new).** Define a crate-private
   `AuthorTask` implementing `PreparationTask`. It performs exactly what
   `Self::author(app.setup, aspect)` does today and writes its `AuthoredScene`
   into an `Rc<RefCell<Option<AuthoredScene>>>` the caller holds.

   `Self::author` is a private associated fn used by both `realize` and
   `reauthor`. **Do not delete it** — `reauthor` still needs it. `AuthorTask`
   should call it.

2. **`App`** — add a private field holding the contributed tasks, e.g.
   `preparation: Vec<(&'static str, Box<dyn PreparationTask>)>`,
   and the `prepare_with` builder method. `App` has a hand-written `Debug`
   (`app.rs:251-260`) — extend it; `Box<dyn PreparationTask>` is not `Debug`.

3. **`RunningApp::realize`** — reorder to:

```rust
runtime.initialize().expect("runtime initialize cannot fail");

let authored_cell = Rc::new(RefCell::new(None));
let mut schedule = PreparationSchedule::new();
schedule.push("axiom/author",
              Box::new(AuthorTask { setup: app.setup, aspect,
                                    out: Rc::clone(&authored_cell) }));
app.preparation
    .into_iter()
    .for_each(|(name, task)| schedule.push(name, task));

runtime.prepare(schedule).expect("app preparation succeeds");
runtime.start().expect("a prepared runtime starts");

let authored = authored_cell.borrow_mut().take().expect("preparation authored the scene");
```

4. **`modules/axiom` is inside the branchless spine.** `app.rs` contains **zero**
   `?;` today and uses `.expect(…)`; `realize` returns `Self`, not a `Result`, so
   `?` cannot compile there anyway. Use `.expect(…)` and `try_fold`, never `?`,
   never `if`/`match`/`for`.

5. **Do not change `reauthor`.** It runs after `Running` and must keep calling
   `Self::author` directly. Preparation is a launch-time phase, not a rebuild
   mechanism.

6. **Preserve authoring order exactly.** Mesh and material ids are `Vec::len() + 1`
   assigned at registration (`app.rs:404-413`, `app/authoring.rs:73,116,132`).
   Running the same authoring closure inside a task rather than inline must
   produce byte-identical id assignment. **The golden run is the detector.**

## Required behavior

- `RunningApp::realize` leaves the runtime `Running` **with a non-empty authored
  scene** — meshes, materials and renderables all populated.
- The author task runs strictly before `start()`.
- An app that calls `prepare_with` has its task run after `AuthorTask` and before
  `start()`, in the order it was added.
- `reauthor` still works after `Running`, unchanged.
- `App::build()` on an app with **no** `prepare_with` calls still works: the
  schedule contains only `AuthorTask`.

## Error behavior

`realize` returns `Self` and cannot propagate. Use `.expect` with messages that
name the failure: a preparation failure here is a programming error at
composition time, not a recoverable runtime condition. Do **not** swallow a
`prepare` failure and continue — that would defeat the barrier.

## Determinism requirements

- Push order is the order; no ids, no order key, no name hashing.
- Authoring order — and therefore mesh/material id assignment — **must not
  change**. This is the single highest-risk property in this manifest.
- No clock, no randomness added.

## Tests

In `modules/axiom/src/app_tests.rs`:

- `realize_leaves_the_runtime_running_with_an_authored_scene`
- `the_author_task_runs_before_start` — a trace proving ordering
- `an_app_preparation_task_runs_after_authoring` — via `prepare_with`
- `app_preparation_tasks_run_in_the_order_they_were_added`
- `an_app_task_cannot_run_before_the_author_task`
- `reauthor_still_works_after_running`
- `an_app_with_no_preparation_tasks_still_realizes`

## Architecture validation

- `modules/axiom` is a **feature module** — the Branchless Law and the 100%
  coverage gate both apply to `modules/**`. New non-test code must be branchless
  and fully covered.
- No new Cargo dependency and no `module.toml` change: `axiom-runtime` is already
  declared.
- Module Law #8 (one public facade) is unaffected — `prepare_with` is a method on
  the existing `App`, not a new top-level export.

## Performance considerations

Authoring moves from inline to inside a task; the work is identical and runs
once. No per-frame change. On `wasm32` it remains on the main thread, exactly as
today.

## Documentation changes

Doc comments on `prepare_with` (stating the reserved band) and on the reordered
`realize` explaining that authoring **is** preparation and why.

## Completion criteria

- [ ] `App::prepare_with(name, task)` exists — no order key
- [ ] `AuthorTask` pushed first, in a new `app/preparation.rs`
- [ ] `prelude.rs` re-exports `PreparationTask` (one line)
- [ ] `realize` is `initialize → prepare → start`; `start()` is unreachable
      before authoring completes
- [ ] `Self::author` still exists and `reauthor` still uses it
- [ ] `modules/axiom/src/app.rs`'s single broken call site is repaired here
- [ ] No `?` introduced; branchless in non-test code
- [ ] `cargo test -p axiom` green
- [ ] **`cargo test -p axiom-burnt-rubber --test agent_golden` green with
      unchanged bytes** — this is the acceptance test that authoring order did
      not move

## Validation commands

```sh
cargo test -p axiom
cargo test --workspace
cargo test -p axiom-burnt-rubber --test agent_golden
cargo run -p xtask -- check-architecture
```

**On your own branch `cargo test --workspace` will be RED**, because the other
member of the landing group has not merged yet. That is correct. Do **not** reach
into files you do not own to make it green — that is exactly the write conflict
the group exists to avoid. `cargo test --workspace` is an orchestrator-only,
post-merge gate.

Note the breakage is **behavioural, not a compile error**: `Runtime::start` keeps
its signature and only its accepted states change, so failures appear as
`.unwrap()` panics in tests, never as build errors. Never enumerate the remaining
work with `cargo build`.


## Deliverable to orchestrator

Report: commit hash; three file paths; test output; **explicit confirmation that
all 15 Burnt Rubber golden files are byte-unchanged** and that `AXIOM_REGOLD` was
never set; the reserved-order-band contract as implemented; deviations.
