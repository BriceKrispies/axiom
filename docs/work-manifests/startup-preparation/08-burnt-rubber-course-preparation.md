# 08 — Burnt Rubber Course Preparation

## Mission

Move the course compile — the single largest piece of startup work in the game —
into its preparation task, and in doing so remove **three of the four** redundant
compiles per construction+restart cycle. Also delete two gratuitous 371 KB deep
copies of the sample table that sit on the same path.

## Architectural owner

- **Package:** `apps/burnt-rubber`
- **Classification:** App
- **Why here:** `CoursePlan` is a racing concept end to end. The generic
  lifecycle layer must never learn what a course is.

## Depends on

**`07-burnt-rubber-preparation-scaffold.md`** — `preparation/mod.rs` is frozen;
you own only `preparation/course.rs`.

## Parallel safety

**Fully concurrent with `09` and `10`.** Disjoint file sets, guaranteed by `07`
having pre-declared the module structure.

## Files owned

| Path | Action |
|---|---|
| `apps/burnt-rubber/src/preparation/course.rs` | modify (stub → real) |
| `apps/burnt-rubber/src/sim/mod.rs` | modify (2208 lines) |
| `apps/burnt-rubber/src/course/compiler/mod.rs` | modify (695 lines) |

## Files allowed to modify

Only the three above.

## Files forbidden to modify

- **`apps/burnt-rubber/src/preparation/mod.rs`** — FROZEN by `07`
- **`apps/burnt-rubber/src/app.rs`** — reserved for `11`. It holds
  `restart_ghost` (`:317`) and `start_race` (`:284`), which also want the
  prepared plan. **You do not wire them; `11` does.**
- `apps/burnt-rubber/src/render/**` — `09`, `10`, `11`
- `apps/burnt-rubber/src/preparation/{textures,meshes}.rs` — `09`, `10`
- `apps/burnt-rubber/src/course/traffic/**`, `src/sim/traffic.rs` — `12`
- `apps/burnt-rubber/tests/golden/**`, `slice.toml`, `tests/agent_golden.rs` —
  **read-only for every manifest**

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `apps/burnt-rubber/src/sim/mod.rs:187-224` | `RaceSim::with_profile` — compiles at `:191`, then the discarded clone at `:207` |
| `apps/burnt-rubber/src/sim/mod.rs:202-206` | **`RaceSim::from_plan`** — already takes `(Arc<CoursePlan>, Tuning, PlayProfile)`. This is re-plumbing, not a rewrite |
| `apps/burnt-rubber/src/course/procedural.rs:391` | `plan_for(seed, &tuning)` — the entry point |
| `apps/burnt-rubber/src/course/compiler/mod.rs:184-200` | `compile`, and the `geometry.samples.clone()` at `:197` |
| `apps/burnt-rubber/src/course/runtime/mod.rs:57, :92-232` | `CoursePlan::assemble`; the type has **no `&mut self` method** and no interior mutability |

## Contract consumed

From `07`, frozen:

```rust
pub struct CourseTask {
    pub seed: u64,
    pub tuning: Tuning,          // plan_for(seed, &tuning) needs it
    pub out: Rc<RefCell<Option<PreparedCourse>>>,
}
```

## Contract produced

```rust
// apps/burnt-rubber/src/preparation/course.rs
#[derive(Debug, Clone)]
pub struct PreparedCourse {
    plan: Arc<CoursePlan>,
}

impl PreparedCourse {
    /// The compiled course, shared. Cloning is an Arc bump, never a recompile.
    pub fn plan(&self) -> Arc<CoursePlan>;
}
```

`11` will consume this to build both the player's `RaceSim` and the ghost's from
**the same `Arc`**.

## Implementation instructions

1. **`preparation/course.rs`** — replace the stub. `CourseTask::prepare` calls
   `crate::course::procedural::plan_for(self.seed, &self.tuning)` and stores the
   resulting `Arc<CoursePlan>` in `PreparedCourse`.

   > **Load-bearing and easy to get wrong.** `plan_for` reads `tuning.course`,
   > `tuning.race` and `tuning.vehicle` (`compiler/mod.rs:192, :292, :413-414`)
   > and does **not** read `tuning.camera` — which is the only field
   > `BurntRubber::with_profile` rewrites (`app.rs:180`, `framed_for_aspect`).
   > That is precisely why a plan prepared **before the window is sized** is
   > bit-identical to today's. Preserve this. If a future change moves an
   > aspect-derived value into `tuning.course/race/vehicle`, preparation breaks
   > silently — say so in a comment at the call site.

2. **`sim/mod.rs`** — leave `RaceSim::with_profile` **signature-compatible**
   (`11` and 861 existing tests depend on it), but make `from_plan` the path a
   prepared plan flows through. Do not delete `with_profile`.

3. **Remove the two gratuitous deep copies** (`TrackSample` ≈ 80 B × 4 635 ≈
   **371 KB each**):
   - `sim/mod.rs:207` — `let track = plan.track().clone();` is used only at
     `:209`, `:211`, `:219` and then **discarded**; `RaceSim` has no `track`
     field (it reads `self.plan.track()` at `:264`). NLL allows
     `let track = plan.track();`.
   - `course/compiler/mod.rs:197` — `geometry.samples.clone()` into
     `Track::from_samples`; `geometry.samples` is never read again, so this can
     be a move.

4. **Do not** touch `app.rs`. The ghost and restart paths are `11`'s.

## Required behavior

- `CourseTask::prepare` produces a `PreparedCourse` whose `plan()` equals what
  `plan_for(DEFAULT_SEED, &Tuning::DEFAULT)` produces today.
- `RaceSim::from_plan` builds a sim indistinguishable from
  `RaceSim::with_profile`'s.
- `with_profile` keeps its signature and its behaviour.
- The compiled course is **byte-identical** to today's.

## Error behavior

`plan_for` returns a `Result`. On failure `CourseTask::prepare` must return
`Err(RuntimeError::new(RuntimeErrorCode::PreparationFailed, "burnt-rubber/course"))`
— **never `panic!` and never `.expect`**. Today `sim/mod.rs:192` panics
(`"the shipping course must compile"`); inside a preparation task that panic
would unwind through `Runtime::prepare` and abort on `wasm32`, bypassing the
failure protocol entirely (README §8).

## Determinism requirements

- Same seed + same tuning ⇒ byte-identical `CoursePlan`.
- Sharing one `Arc` between player and ghost changes nothing observable:
  `CoursePlan` exposes no `&mut self` method and has no interior mutability, and
  the ghost owns a separate `RaceSim` (`ghost.rs:43-49`).
- Removing the two clones must not change any value — only the copy count.

## Tests

Inline `#[cfg(test)] mod tests` in `preparation/course.rs`:

- `preparing_produces_the_shipping_course` — length, sample count, seed match
- `two_preparations_from_the_same_seed_are_identical`
- `a_prepared_plan_builds_the_same_sim_as_with_profile`
- `an_invalid_course_spec_fails_the_task_rather_than_panicking`
- `the_prepared_plan_is_shared_not_copied` — `Arc::ptr_eq` across two clones

## Architecture validation

`apps/` is outside the branchless, coverage and dylint gates. Write natural Rust.
No `app.toml` change.

## Performance considerations

This is the largest single win available. Today the course compiles **four
times** per construction+restart cycle (`app.rs:184`, `:208`, `:285`, `:319`).
After `11` wires it, that becomes once. This manifest makes it *possible*; `11`
realises it.

Do **not** cite the ghost-validation sim as savings: `course/validation/ghost.rs:105`
has **zero non-test callers** and is never paid at launch.

## Documentation changes

Module doc on `preparation/course.rs` covering what is prepared, the
`tuning.camera` independence, and the failure mode.

## Completion criteria

- [ ] `CourseTask` compiles the real shipping course into `PreparedCourse`
- [ ] `PreparedCourse::plan()` returns `Arc<CoursePlan>`
- [ ] Both 371 KB clones removed
- [ ] `RaceSim::with_profile` signature unchanged
- [ ] Failure returns `Err`, never panics
- [ ] `cargo test -p axiom-burnt-rubber` green — all 861 lib tests
- [ ] **Golden run green with unchanged bytes**
- [ ] `app.rs`, `render/**` and `preparation/mod.rs` untouched

## Validation commands

```sh
cargo test -p axiom-burnt-rubber
cargo test -p axiom-burnt-rubber --test agent_golden
cargo test -p axiom-burnt-rubber --test course_pipeline
git diff --name-only        # exactly three paths
```

## Deliverable to orchestrator

Report: commit hash; three paths; the `PreparedCourse` contract as implemented;
confirmation the golden bytes are unchanged and `AXIOM_REGOLD` was never set;
confirmation both clones are gone; deviations.
