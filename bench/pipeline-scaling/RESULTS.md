# Pipeline scaling: load-test results

Run: `cargo run --release --manifest-path bench/pipeline-scaling/Cargo.toml`
(numbers are machine-dependent; rerun locally — these are one representative run).

Question: as a scene grows to thousands of animated renderables, **how does the
deterministic per-frame CPU cost grow, and where does it live?** The harness
drives the real native pipeline — `scene.advance` (spin animation +
world-transform propagation + snapshot) then `RenderPipelineApi::submit`
(snapshot → render input → command list → recording GPU submission) — and times
the two phases separately across a sweep of renderable counts.

| renderables | advance µs/fr | submit µs/fr | total µs/fr | submit ns/obj | fps @ total |
|---|---|---|---|---|---|
| 100    | 25.5     | 101.1     | 126.6     | 1011  | 7896 |
| 500    | 196.6    | 520.2     | 716.8     | 1041  | 1395 |
| 1000   | 390.0    | 1201.0    | 1591.1    | 1201  | 629  |
| 5000   | 2223.6   | 13570.4   | 15794.1   | 2714  | 63   |
| 10000  | 4596.4   | 57434.4   | 62030.8   | 5743  | 16   |
| 50000  | 28605.6  | 1438951.7 | 1467557.3 | 28779 | 1    |

## What it means

- **`submit` is super-linear in scene size — roughly O(N²).** The per-object
  cost (`submit ns/obj`) is not flat: it climbs ~28× (1011 → 28779 ns) while the
  scene grows 500×. A linear pass would hold that column constant. At 50k
  renderables a single frame's `submit` takes ~1.4 **seconds**.
- **`advance` is effectively linear.** Per-object propagation cost rises only
  ~2.2× across the same 500× growth (mild, consistent with allocation/cache
  effects in snapshotting) — the animation + transform-propagation path scales
  fine.
- **So the seam is entirely in `submit`, not in the scene step.** Timing the
  phases separately is what localizes it.

## Root cause

`RenderPipelineApi::submit`
(`modules/axiom-render-pipeline/src/render_pipeline_api.rs:208`) resolves each
renderable's world transform with a **linear scan of every node**:

```rust
for renderable in snapshot.renderables() {
    let world = snapshot.nodes().iter().find(|n| n.id() == renderable.node()) // O(nodes)
    ...
}
```

With N renderables over ~N nodes that is O(N²). (The per-renderable
`mesh_index` / `material_index` `.find()` scans at `:216`/`:221` are also linear,
but bounded by the small, constant asset count here, so they don't drive the
curve.) The fix belongs at the source — an id→node lookup (map or pre-sorted
index) so resolution is O(1)/O(log N) per renderable — not worked around in an
app or the bench. That is a structural change in the owning module, exactly the
kind the No-Shortcuts rule says to make where the defect lives.

## Scope: what this does and does not measure

- **In scope (deterministic, native, reproducible):** the full CPU pipeline up
  to a *recording* GPU submission (`WebGpuApi::new_recording()`, the same
  backend the engine's own tests use). No browser, stable numbers.
- **Out of scope:** the actual on-GPU paint — the wasm `LiveGpuBinding`
  instanced cube drawer. That is non-deterministic and browser-bound; its
  throughput (FPS at increasing instance counts) must be measured in a real
  browser via the Playwright controller, not in this harness.

## Takeaway

"Draw a ton of animated things" is a fair load test — but the useful, repeatable
half of it is the deterministic CPU pipeline, and run that way it immediately
exposes a quadratic in `submit` that would otherwise only show up as
mysterious frame-time collapse in a busy browser scene. Fix the node lookup at
its source and rerun: `submit ns/obj` should flatten.
