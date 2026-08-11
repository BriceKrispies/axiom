# 07 — Burnt Rubber Preparation Scaffold

## Mission

Create the module structure and schedule assembly that manifests `08`, `09` and
`10` fill in — and **freeze it**, so those three can run genuinely concurrently
without ever contending for a file. This manifest declares all three domain
submodules up front with no-op task bodies, so the app already routes through the
preparation barrier with **behaviour completely unchanged**.

## Architectural owner

- **Package:** `apps/burnt-rubber`
- **Classification:** App — a leaf composition root
- **Why here:** Burnt Rubber is the vertical proof. Its preparation tasks are
  app-owned because everything they generate is a racing concept the engine must
  never learn (`app.toml` already lists `runtime` in `allowed_layers`).

## Depends on

**`06-composition-root-preparation.md`** — you need `App::prepare_with(name, task)`.

## Parallel safety

Nothing runs concurrently with this manifest. It is a wave of one, precisely so
that `08`/`09`/`10` can be a wave of three afterwards.

## Files owned

| Path | Action |
|---|---|
| `apps/burnt-rubber/src/preparation/mod.rs` | **create** — then FROZEN |
| `apps/burnt-rubber/src/preparation/course.rs` | **create** (stub) |
| `apps/burnt-rubber/src/preparation/textures.rs` | **create** (stub) |
| `apps/burnt-rubber/src/preparation/meshes.rs` | **create** (stub) |
| `apps/burnt-rubber/src/lib.rs` | modify — **one line** |

## Files allowed to modify

Only the five above.

## Files forbidden to modify

- **`apps/burnt-rubber/src/app.rs`** — reserved for `11`
- **`apps/burnt-rubber/src/render/mod.rs`** — reserved for `11`
- `apps/burnt-rubber/src/sim/**`, `render/palette.rs`, `render/chunks.rs`,
  `render/scenery_pool.rs`, `render/prop_meshes.rs`, `debug_view.rs`,
  `course/**` — owned by `08`/`09`/`10`
- `apps/burnt-rubber/tests/golden/**`, `slice.toml`, `tests/agent_golden.rs` —
  **the committed baseline; read-only for every manifest**
- `apps/burnt-rubber/app.toml` — `runtime` is already permitted
- `modules/axiom/**`, `crates/**`

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `apps/burnt-rubber/src/course/mod.rs` | The repo's directory-module convention you are copying |
| `apps/burnt-rubber/src/lib.rs:51-90` | The `pub mod` block your one line joins |
| `apps/burnt-rubber/src/app.rs:158-223` | `BurntRubber::with_profile` — what `11` will later wire. **Read it; do not edit it** |
| `apps/burnt-rubber/src/render/mod.rs:85-86, :376-389` | `RaceScene::install` and its fixed install order. **Read it; do not edit it** |

## Contract consumed

`axiom_runtime::PreparationTask` and `App::prepare_with(name, task)` (from `06`).
No `HandleId`, no order key.

## Contract produced

**This is the frozen scaffold. `08`, `09` and `10` write against it verbatim.**

```rust
// apps/burnt-rubber/src/preparation/mod.rs
pub mod course;
pub mod textures;
pub mod meshes;

/// Every product the startup phase yields, shared with the code that consumes it.
/// One cell per domain; each is `Option` so a premature read fails the phase
/// rather than yielding a plausible empty value.
#[derive(Clone, Default)]
pub struct RacePreparation {
    pub course: Rc<RefCell<Option<course::PreparedCourse>>>,
    pub textures: Rc<RefCell<Option<textures::PreparedTextures>>>,
    pub meshes: Rc<RefCell<Option<meshes::PreparedMeshes>>>,
}

impl RacePreparation {
    pub fn new() -> Self;

    /// The three domain tasks, in the order the race needs them built.
    /// The composition root (`11`) passes each to `App::prepare_with`.
    /// Push order IS execution order — there is no order key.
    pub fn tasks(&self, seed: u64, tuning: &Tuning)
        -> [(&'static str, Box<dyn PreparationTask>); 3];
}
```

Each domain file declares its own product type and task, initially inert:

```rust
// preparation/course.rs
#[derive(Debug, Clone, Default)]
pub struct PreparedCourse { /* 08 fills this in */ }

pub struct CourseTask {
    pub seed: u64,
    pub tuning: Tuning,                                  // plan_for(seed, &tuning) needs it
    pub out: Rc<RefCell<Option<PreparedCourse>>>,
}

// preparation/textures.rs
pub struct TextureTask { pub out: Rc<RefCell<Option<PreparedTextures>>> }

// preparation/meshes.rs
pub struct MeshTask {
    /// READ cell: the course task (pushed earlier) fills this.
    pub course: Rc<RefCell<Option<course::PreparedCourse>>>,
    pub tuning: CourseTuning,
    pub out: Rc<RefCell<Option<PreparedMeshes>>>,
}

impl PreparationTask for CourseTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        *self.out.borrow_mut() = Some(PreparedCourse::default());
        Ok(())
    }
}
```

**Why the task structs carry these fields, and why they are frozen here.** An
earlier draft froze `CourseTask { seed, out }` and `MeshTask { out }`, then told
`08` to call `plan_for(self.seed, &tuning)` with no `tuning` in scope, and told
`10` to "accept a `Track` at construction" — which is **impossible**: the schedule
is assembled *before* `Runtime::prepare` runs, and the `Track` does not exist
until `CourseTask` has executed inside that same call. Three manifests would each
have held a fragment of one contract with none able to complete it.

The correct shape is the one above: `MeshTask` holds the **course cell** and
reads it inside `prepare()`, returning `Err` if it is `None` — exactly the
read-before-write protocol README §8 mandates and `04`'s
`a_task_that_reads_an_unwritten_product_fails_the_phase` pins. Note also that
`BurntRubber::with_profile(seed, tuning, …)` takes an **arbitrary caller-supplied
`Tuning`** (`app.rs:158`); it is not a constant, so it must be threaded in.

## Implementation instructions

1. Create `apps/burnt-rubber/src/preparation/` as a directory module with the
   four files above. Add `pub mod preparation;` to `src/lib.rs`.
2. `mod.rs` declares **all three** submodules and returns **all three** tasks
   from `tasks()`, in the fixed push order course → textures → meshes. After this
   manifest `mod.rs` is **frozen**; `08`/`09`/`10` each touch only their own
   domain file.
3. Push order encodes the real dependency: the course produces the `Track` that
   mesh generation reads. `MeshTask` receives the course **cell**, not a `Track`
   — it reads the cell inside `prepare()`. `08`/`09`/`10` must not change the
   order or the task field lists.
4. Task bodies are inert placeholders that write a `Default` product. **They must
   not move any generation yet.** The app's behaviour after this manifest is
   byte-identical to before it.
5. **Do not wire `BurntRubber` to the schedule.** That is `11`'s job, because it
   requires `app.rs`. Your `RacePreparation` is constructed and tested but not yet
   used by the running app — dead code is expected and correct here.

## Required behavior

- `RacePreparation::new()` yields three empty cells.
- `tasks()` returns exactly three pairs, in push order course → textures → meshes.
- After a `Runtime::prepare` driving them, all three cells are `Some`.
- **The game behaves identically to before this manifest.**

## Error behavior

`tasks()` is infallible. Task bodies cannot fail yet; `08`/`09`/`10` add the real
failure modes.

## Determinism requirements

- Push order is the order; no ids, no order key, no hashing, no counters.
- `Rc<RefCell<Option<T>>>` for every product cell — **never a defaultable bare
  `T`** (README §8): a premature read must be `None`, not a plausible empty
  value.

## Tests

Inline `#[cfg(test)] mod tests` in `preparation/mod.rs`:

- `tasks_are_returned_in_push_order` — course, textures, meshes
- `preparing_fills_every_product_cell`
- `a_fresh_preparation_has_no_products`
- `the_mesh_task_holds_the_course_cell` — the cross-task wiring exists

## Architecture validation

- `apps/` is **outside** the branchless law, the coverage gate and the dylint
  rulebook (`engine_lint_helpers::is_engine_file` requires a `crates`/`modules`
  component). Write ordinary readable Rust. Do **not** contort for branchlessness.
- `apps/burnt-rubber/app.toml` already lists `runtime`; **no manifest change**.
- No junk-drawer name: `preparation` is a real domain word, not `utils`.

## Performance considerations

None — the tasks are inert.

## Documentation changes

Module docs on all four new files. `TESTING.md` belongs to `13`.

## Completion criteria

- [ ] `preparation/` exists with `mod.rs` + three domain files
- [ ] `mod.rs` declares all three submodules and returns all three from `tasks()`
- [ ] `CourseTask` carries `tuning`; `MeshTask` carries the course cell + tuning
- [ ] No order key and no id constants (push order is the order)
- [ ] Every product cell is `Rc<RefCell<Option<T>>>`
- [ ] `src/lib.rs` changed by exactly one line
- [ ] `cargo test -p axiom-burnt-rubber` green
- [ ] **Golden run green with unchanged bytes** — nothing has moved yet
- [ ] `app.rs` and `render/mod.rs` untouched

## Validation commands

```sh
cargo test -p axiom-burnt-rubber
cargo test -p axiom-burnt-rubber --test agent_golden
git diff --name-only
```

## Deliverable to orchestrator

Report: commit hash; the five paths; the **frozen** contract exactly as
implemented (so `08`/`09`/`10` can be dispatched against it); confirmation that
the golden bytes are unchanged; confirmation that `app.rs` and `render/mod.rs`
are untouched; deviations.
