# 11 — Burnt Rubber Wiring

## Mission

Make the three domain streams live. `08`, `09` and `10` each landed code that is
correct but **not yet called**, because they were locked out of the two files
everything funnels through. This manifest owns those two files —
`apps/burnt-rubber/src/render/mod.rs` and `apps/burnt-rubber/src/app.rs` — routes
`BurntRubber` through `App::prepare_with` + `Runtime::prepare`, and makes the app
genuinely gate on the preparation barrier for the first time.

## Architectural owner

- **Package:** `apps/burnt-rubber`
- **Classification:** App — leaf composition root
- **Why here:** `BurntRubber::with_profile` and `RaceScene::install` are the
  app's own composition points. Translating between prepared products and the
  scene is precisely an app's job.

## Depends on

**`08`, `09` and `10` — all three must be merged.** You consume all three
contracts and repair the compile errors they deliberately left.

## Parallel safety

**Nothing runs concurrently.** This manifest exists so that `09` and `10` could
run concurrently; it holds the files they contended for.

## Files owned

| Path | Action |
|---|---|
| `apps/burnt-rubber/src/render/mod.rs` | modify (1872 lines) |
| `apps/burnt-rubber/src/app.rs` | modify (1060 lines) |
| `apps/burnt-rubber/src/web.rs` | modify (1059 lines) |

## Files allowed to modify

Only the three above.

## Files forbidden to modify

- `apps/burnt-rubber/src/preparation/**` — `07`, `08`, `09`, `10`. If a
  prepared-product API is wrong or missing, **stop and report** — do not patch it
  from here.
- `apps/burnt-rubber/src/render/{palette,chunks,scenery_pool,prop_meshes}.rs`,
  `src/sim/mod.rs`, `src/debug_view.rs` — owned by `08`/`09`/`10`
- `apps/burnt-rubber/src/course/traffic/**`, `src/sim/traffic.rs` — `12`
- `apps/burnt-rubber/tests/**`, `slice.toml` — **read-only**; `13` owns new tests
- `modules/axiom/**`, `crates/**`

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `apps/burnt-rubber/src/render/mod.rs:85-86` | `RaceScene::install`; `ScenePalette::install(app)` at `:86` — **`09`'s signature change lands here** |
| `apps/burnt-rubber/src/render/mod.rs:376-389` | The **fixed install order**: road, scenery, traffic, pickups, player car, ghost car, effects, finish arch, lights. **This order is load-bearing** |
| `apps/burnt-rubber/src/app.rs:158-223` | `BurntRubber::with_profile` — must **keep its signature** |
| `apps/burnt-rubber/src/app.rs:180` | `framed_for_aspect` rewrites `tuning.camera` only — which is why a plan prepared before sizing is bit-identical |
| `apps/burnt-rubber/src/app.rs:284, :317-323` | `start_race` and `restart_ghost` — the 3rd and 4th course compiles |
| `apps/burnt-rubber/src/app.rs:199-200` | `RaceScene::install` and `DebugView::install` call sites |
| `apps/burnt-rubber/src/web.rs:81, :99, :127-128, :328` | Surface measured, app built, resources read, loop started |

## Contract consumed

- `07`: `RacePreparation::{new, tasks}` — `tasks(seed, &tuning)` returns three
  `(name, Box<dyn PreparationTask>)` pairs in push order
- `08`: `PreparedCourse::plan() -> Arc<CoursePlan>`
- `09`: `PreparedTextures::{asphalt, verge, foliage}` + the new signatures of
  `road_materials` and `ScenePalette::install`
- `10`: `PreparedMeshes::{draw_chunk, paint_chunk, cone, palm_crown, shrub}` +
  the new signatures of every `install` it changed
- `06`: `App::prepare_with(name, task)` — no order key; `realize` always pushes
  the engine's `AuthorTask` first

## Contract produced

`BurntRubber::with_profile(seed, tuning, width, height, profile)` — **unchanged
signature**, now internally preparation-driven. Every one of the 861 lib tests,
every capture slice and `tools/axiom-shot/src/registry.rs` continue to compile
untouched. **This is a hard requirement, not a nicety.**

## Implementation instructions

1. **Build and run the schedule inside `BurntRubber::with_profile`.** Construct a
   `RacePreparation`, register it into the schedule the `App` builder carries via
   `prepare_with`, and let `RunningApp::realize` drive `Runtime::prepare`. The
   prepared products are then read out of the `RacePreparation` cells and threaded
   into `RaceScene::install`.

2. **Repair `render/mod.rs`.** Thread `&PreparedTextures` into
   `ScenePalette::install` (`:86`) and `&PreparedMeshes` into `RoadChunks::install`
   and `SceneryField::install` (`:376-377`). **Do not reorder anything in
   `:376-389`.** Materials must still be registered before the meshes that cite
   them: `ScenePalette::install` runs first at `:86` and its handles flow into
   `RoadChunks::install` at `:376`.

3. **Collapse the redundant course compiles.** `start_race` (`:284`) and
   `restart_ghost` (`:317`) must reuse the prepared `Arc<CoursePlan>` via
   `RaceSim::from_plan`, not call `plan_for` again. Player and ghost share **one
   `Arc`** — safe because `CoursePlan` has no `&mut self` method and no interior
   mutability, and the ghost owns a separate `RaceSim`.

4. **`web.rs`** — no structural change is required: `with_profile` keeps its
   signature. Add wall-clock timing **around** the construction call
   (`Instant`/`performance.now()` at the host boundary), because the runtime has
   no clock and must not gain one. Report it through the existing telemetry path.

5. **Preserve `BurntRubber::with_profile`'s signature.** If preparation appears
   to require changing it, stop and report — that would break 861 tests, every
   capture slice and the golden fixture's builders.

6. **Own the two test seams `13` needs.** `13` owns only
   `apps/burnt-rubber/tests/preparation.rs` and cannot add instrumentation to
   `src/`. Provide, in your owned files:
   - a `#[cfg(test)]` **course-compile counter** (an atomic bumped on the course
     preparation path) that `13`'s `the_course_is_compiled_exactly_once_per_launch`
     can read;
   - a `#[cfg(test)]` **failure-injection seam** — a constructor or flag that
     makes course preparation fail — so `13`'s
     `preparation_failure_is_surfaced_not_swallowed` is writable at all.

   Without these two, two of `13`'s five tests cannot be written and the
   programme cannot prove its own headline claim.

7. **Switch to the `_prepared` variants, do not change signatures.** `09` and
   `10` left every original working precisely so this step is a mechanical
   call-site switch. `13` deletes the dead originals afterwards.

## Required behavior

- The app cannot present a frame until preparation completes: `RunningApp::tick`
  is `step` + `render`, and `step` drives `Runtime::step`, which requires
  `Running`, which requires `Prepared`.
- Course compiled **once** per launch (was four times per
  construction + restart cycle).
- Player and ghost share one `Arc<CoursePlan>` (`Arc::ptr_eq`).
- A restart does not recompile.
- Mesh, material and texture ids are **unchanged** — same registration order,
  same count, same fallback arms.
- The game plays identically.

## Error behavior

A preparation failure must leave the runtime `Failed` and **no frame may ever be
presented**. Do not catch a `prepare` failure and continue with a default course
— that is exactly the "start anyway and hope" the barrier exists to prevent.

## Determinism requirements

- **Install order in `render/mod.rs:376-389` is frozen.** Ids are
  registration-order indices and are encoded in the committed goldens.
- Sharing the plan `Arc` changes nothing observable.
- Timing added in `web.rs` must be **outside** any deterministic path: it may not
  influence a seed, a step count or anything the simulation reads.

## Tests

Extend existing inline test modules in your owned files (do **not** create
`tests/preparation.rs` — that is `13`'s):

- `with_profile_still_has_its_original_signature` — a compile-level guard
- `the_course_is_compiled_once_per_launch` — a counting wrapper; **would have
  failed before this manifest**
- `the_ghost_shares_the_prepared_course` — `Arc::ptr_eq`
- `a_restart_does_not_recompile_the_course`

## Architecture validation

`apps/` is outside the branchless, coverage and dylint gates. No `app.toml`
change: `runtime` and every module used are already listed.

## Performance considerations

This is where the win lands: four course compiles become one, and two 371 KB
`Track` copies disappear (removed by `08`). Record, at the host boundary:
total preparation duration, and the before/after **median** gameplay frame time
at the `canyon` checkpoint (`src/telemetry.rs` already keeps a 240-frame rolling
median).

Never A/B across processes — `tools/axiom-shot/src/main.rs:126-134` records two
runs of the *same* slice at 3.29 ms and 13.52 ms from GPU clock drift. Use
`--profile-compare`, which interleaves in one process.

## Documentation changes

Comments at the wiring sites explaining that authoring and generation are
preparation, and that the install order is frozen because ids are
registration-order indices.

## Completion criteria

- [ ] `BurntRubber` routes through `App::prepare_with` + `Runtime::prepare`
- [ ] `with_profile` signature unchanged
- [ ] `render/mod.rs` install order unchanged
- [ ] Course compiled once; player and ghost share one `Arc`
- [ ] Restart does not recompile
- [ ] `cargo test -p axiom-burnt-rubber` green — all 861 lib tests
- [ ] `cargo build --workspace` green
- [ ] **All 15 golden files byte-unchanged; `AXIOM_REGOLD` never set**
- [ ] `tools/axiom-shot` still builds (registry untouched)

## Validation commands

```sh
cargo build --workspace
cargo test -p axiom-burnt-rubber
cargo test -p axiom-burnt-rubber --test agent_golden
cargo test -p axiom-burnt-rubber --test course_pipeline
cargo build --release -p axiom-shot --features offscreen
```

## Deliverable to orchestrator

Report: commit hash; three paths; full test output; **explicit per-file
confirmation that all 15 golden artifacts are byte-unchanged**; the measured
course-compile count before and after; total preparation duration; any prepared
API that was wrong or missing (reported, not patched); deviations.
