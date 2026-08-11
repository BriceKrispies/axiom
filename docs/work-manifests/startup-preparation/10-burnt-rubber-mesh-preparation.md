# 10 — Burnt Rubber Mesh Preparation

## Mission

Move the CPU geometry construction for the road, the scenery props and the debug
markers into the mesh preparation task, while leaving every `add_mesh_data` /
`add_mesh` / `spawn` call exactly where it is. This is the highest-risk manifest
in the programme: mesh ids are registration-order indices and are encoded in the
committed goldens, so the split must move *what generates the vertices* without
moving *when they are registered*.

## Architectural owner

- **Package:** `apps/burnt-rubber`
- **Classification:** App
- **Why here:** road strips, palm crowns and shrubs are game art.


## THE ADDITIVE RULE — read before anything else

**You must not change the signature of any existing function.** Add a *prepared
variant* alongside it and leave the original in place, working.

This is not style. `RoadChunks::install`, `SceneryField::install` and the three
`install_*` prop wrappers are called from `render/mod.rs` (`11`'s file) and from
`#[cfg(test)]` fixtures. Separately, `chunks.rs:493` calls `road_materials`,
whose prepared variant manifest `09` is adding **concurrently** — if either of
you changed an existing arity, the other's branch would stop compiling.

```rust
// keep, unchanged
pub fn install(app: &mut RunningApp, track: &Track /* … */) -> RoadChunks;

// add
pub fn install_prepared(app: &mut RunningApp, prepared: &PreparedMeshes /* … */) -> RoadChunks;
```

Consequences, all good: the crate compiles at every commit, you can run
`cargo test -p axiom-burnt-rubber` **and the golden run** yourself, `11` becomes
a pure call-site switch, and `13` deletes the now-dead inline paths as its
documented "remove dead compatibility paths" step.

Note also that `cargo test --lib` **builds the lib target first** — so if the
crate did not compile, *zero* of your tests would run and your own completion
criteria would be unverifiable.

## Depends on

**`07-burnt-rubber-preparation-scaffold.md`**.

## Parallel safety

**Fully concurrent with `08` and `09`.**

## Files owned

| Path | Action |
|---|---|
| `apps/burnt-rubber/src/preparation/meshes.rs` | modify (stub → real) |
| `apps/burnt-rubber/src/render/chunks.rs` | modify (784 lines) |
| `apps/burnt-rubber/src/render/scenery_pool.rs` | modify (521 lines) |
| `apps/burnt-rubber/src/render/prop_meshes.rs` | modify (397 lines) |
| `apps/burnt-rubber/src/render/scenery.rs` | modify — `distant_hills` (`:633`), part of P4 |

## Files allowed to modify

Only the five above.

## Files forbidden to modify

- **`apps/burnt-rubber/src/render/mod.rs`** — reserved for `11`. It holds
  `RaceScene::install` (`:85`) and the fixed install order (`:375-389`); `09`
  needs `:86`. Both of you are locked out because `11` must switch every call
  site in one coherent pass.
- **`apps/burnt-rubber/src/preparation/mod.rs`** — FROZEN by `07`
- `apps/burnt-rubber/src/app.rs` — `11`
- `apps/burnt-rubber/src/render/palette.rs` — `09`
- `apps/burnt-rubber/src/render/{car_model,pickups,effects}.rs` — **not in scope**
  (see "What is deliberately left alone")
- `apps/burnt-rubber/tests/golden/**`, `slice.toml`, `tests/agent_golden.rs` —
  **read-only**

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `apps/burnt-rubber/src/render/chunks.rs:168-213` | `RoadChunks::install` — the big one. `build_draw_mesh(track, index, tuning)` at `:179`, `build_paint_chunk` at `:201` |
| `apps/burnt-rubber/src/render/chunks.rs:464-477` | `spawn_retired` — `add_mesh_data(data)` at `:472` **with `unwrap_or_else(\|_\| app.add_mesh(Mesh::cube()))` at `:473`** |
| `apps/burnt-rubber/src/render/prop_meshes.rs:96, :166` | `palm_crown_surface()` and `shrub_surface()` — **already pure**, no `app` argument. The cleanest separation model |
| `apps/burnt-rubber/src/render/prop_meshes.rs:36-48` | `install_cone` — builds **inline**; needs the same split |
| `apps/burnt-rubber/src/render/road_mesh.rs:126` | `span_sample_range` — adjacent chunks **share** their boundary sample so seams are bit-identical. Preserve exactly |
| `modules/axiom/src/app/authoring.rs:73-79` | `add_mesh_data` — `let id = self.meshes.len() as u64 + 1` |

## Counts — do not conflate them

Over the 9 270 m course: `CHUNK_LENGTH = 100 m` → **93** scenery cells;
`DRAW_SPAN = 400 m` → **24** `build_draw_mesh` calls producing **96** entities
(4 material parts each); `PAINT_CHUNK_LENGTH = 10 m` → **927** fine paint meshes.
A fourth, unrelated 92 is `Effects::install`'s entity count.

## Contract consumed

From `07`, frozen:

```rust
pub struct MeshTask {
    pub course: Rc<RefCell<Option<course::PreparedCourse>>>,   // READ cell
    pub tuning: CourseTuning,
    pub out: Rc<RefCell<Option<PreparedMeshes>>>,
}
```
Push order is fixed by `07`'s `tasks()`; there is no order key and no id.

**Read the course cell inside `prepare()`** — you cannot take a `Track` at
construction, because the schedule is assembled before `Runtime::prepare` runs
and the course does not exist until `CourseTask` executes within that same call.
Use the mandated protocol:

```rust
let prepared = self.course.borrow();
let plan = prepared.as_ref().ok_or_else(|| RuntimeError::new(
    RuntimeErrorCode::PreparationFailed, "burnt-rubber/meshes needs the course"))?;
```
(expressed without `?` — `apps/` is exempt from the branchless law, so ordinary
control flow is fine here.) **Never `.expect()`** — README §8.

## Contract produced

```rust
// apps/burnt-rubber/src/preparation/meshes.rs
#[derive(Debug, Clone)]
pub struct PreparedMeshes {
    draw_chunks: Vec<ChunkMeshes>,   // 24 entries, index-aligned to draw span
    paint_chunks: Vec<MeshData>,     // 927 entries, index-aligned
    cone: MeshData,
    palm_crown: MeshData,
    shrub: MeshData,
}

impl PreparedMeshes {
    pub fn draw_chunk(&self, index: usize) -> &ChunkMeshes;
    pub fn paint_chunk(&self, index: usize) -> &MeshData;
    pub fn cone(&self) -> &MeshData;
    pub fn palm_crown(&self) -> &MeshData;
    pub fn shrub(&self) -> &MeshData;
}
```

Each of `RoadChunks::install`, `SceneryField::install`, `install_cone`,
`install_palm_crown` and `install_shrub` gains a **`_prepared` sibling** taking
`&PreparedMeshes`. The originals stay, working. **`11` switches the
`render/mod.rs` call sites to the siblings.**

**`DebugView::install` is NOT in scope.** An earlier draft told you to give it a
`&PreparedMeshes` parameter — but it builds from `Mesh::cube()`
(`debug_view.rs:105`) and would consume no field of `PreparedMeshes`. It falls
squarely under "What is deliberately left alone". Do not touch `debug_view.rs`.

## Implementation instructions

1. **`preparation/meshes.rs`** — `MeshTask::prepare` builds every chunk mesh by
   calling the **existing** `build_draw_mesh` / `build_paint_chunk` for every
   index, in ascending index order, plus the three prop surfaces. Store them
   index-aligned.

2. **Consume, do not re-derive.** In `chunks.rs`, `spawn_retired` takes the
   already-built `MeshData` instead of calling `build_draw_mesh` inline. The
   `add_mesh_data(...)` call stays exactly where it is.

3. **THE CRITICAL CONSTRAINT — the fallback also mints an id.**
   Every `add_mesh_data` site is followed by
   `.unwrap_or_else(|_| app.add_mesh(Mesh::cube()))` (`chunks.rs:473`,
   `prop_meshes.rs:47`, `:91`, `:161`). **Both arms mint an id.** So:
   - You must preserve **which arm runs** for every mesh. If a mesh that
     currently succeeds were to fail (or vice versa), ids shift and every
     downstream golden moves.
   - Keep the `add_mesh_data` + fallback pair intact and in place. Move only the
     *construction of the `MeshData` argument* earlier.

4. **Preserve registration order absolutely.** `spawn_retired` is called 4× per
   draw chunk (`chunks.rs:189-194`: surface, paint, rail, verge) across 24
   chunks, then once per fine-paint chunk (`:199-204`) across 927. Same order,
   same count, same nesting.

5. **`install_cone`** (`prop_meshes.rs:36`) builds inline; split it the way
   `palm_crown_surface`/`shrub_surface` are already split — a pure builder
   function plus a thin registering wrapper.

6. **Preserve the seam guarantee.** `span_sample_range` (`road_mesh.rs:126`)
   makes adjacent chunks share their boundary sample so seams are bit-identical.
   Building all chunks up front must not change which samples each chunk sees.

7. **Do not touch `render/mod.rs`.** Its existing calls keep working against the
   originals; `11` switches them to the prepared siblings.

8. **`distant_hills` (`render/scenery.rs:633`)** is part of P4 and was
   unassigned in an earlier draft. It is a pure function of `(seed, track)`; add
   its output to `PreparedMeshes` and consume it from the prepared scenery
   sibling.

## What is deliberately left alone

`PlayerCar::install`, `TrafficVisuals::install`, `PickupVisuals::install`,
`Effects::install`, `install_finish_arch` and `install_lights` all use engine
primitives (`Mesh::cube()`, `Mesh::cylinder()`) rather than generated geometry.
There is nothing expensive to prepare. **Do not touch them** — moving them would
be churn with golden risk and no benefit.

## Required behavior

- Every prepared mesh is byte-identical to what the inline call produces today.
- Registration order, count and the success/fallback arm are unchanged for every
  mesh.
- Chunk seams remain bit-identical.

## Error behavior

`build_draw_mesh` and the prop surface builders are infallible. If a `MeshData`
is malformed, `add_mesh_data` already returns `Err` and the existing fallback
handles it — **preserve that path exactly; do not turn it into a preparation
failure**, because that would change which arm mints the id.

A consumer that finds the cell `None` must return
`Err(...PreparationFailed...)`, never `.expect` (README §8).

## Determinism requirements

- Geometry is a pure function of `(track, index, tuning)`. Same track ⇒ same
  bytes.
- Build in ascending index order.
- No parallelism, no `HashMap` iteration.

## Tests

Inline `#[cfg(test)] mod tests` in `preparation/meshes.rs`:

- `preparing_produces_every_draw_chunk` — expect **24**
- `preparing_produces_every_paint_chunk` — expect **927**
- `a_prepared_chunk_matches_the_inline_builder` — equality against a direct
  `build_draw_mesh` call. **The most important test here**
- `adjacent_prepared_chunks_share_their_boundary_sample`
- `two_preparations_produce_identical_geometry`
- `preparing_produces_the_three_prop_surfaces`

## Architecture validation

`apps/` is outside the branchless, coverage and dylint gates. No `app.toml`
change.

## Performance considerations

The same geometry is built; only its phase moves. Peak memory rises slightly —
all 24 + 927 meshes are resident between preparation and registration rather than
being built and handed over one at a time. That is bounded and small relative to
the two 371 KB `Track` copies `08` removes.

## Documentation changes

Module doc on `preparation/meshes.rs` stating the three counts, the
registration-order constraint and the fallback-arm hazard.

## Completion criteria

- [ ] `MeshTask` builds 24 draw chunks, 927 paint chunks and 3 prop surfaces
- [ ] Every `install` in your owned files consumes prepared data
- [ ] `add_mesh_data` + fallback pairs unchanged in place, order and count
- [ ] `install_cone` split like its two siblings
- [ ] `car_model.rs`, `pickups.rs`, `effects.rs`, `debug_view.rs` untouched
- [ ] `render/mod.rs`, `app.rs`, `preparation/mod.rs` untouched
- [ ] Your own tests pass, including the inline-builder equality test

## Validation commands

```sh
cargo test -p axiom-burnt-rubber
cargo test -p axiom-burnt-rubber --test agent_golden
git diff --name-only
```

The crate compiles throughout, so you run the **full** suite and the golden run
yourself. That matters more here than anywhere else in the programme:
registration-order regressions live in exactly these files, and this is the only
point at which they can be caught in isolation. Golden bytes must be unchanged —
your prepared siblings are not called yet, so a diff means you altered the
originals.

## Deliverable to orchestrator

Report: commit hash; five paths; the `PreparedMeshes` contract as implemented;
**the exact new signatures of every `install` you changed** (so `11` can wire
them without guessing); the compile errors `11` must repair; explicit
confirmation that registration order, count and the fallback arms are unchanged;
deviations.
