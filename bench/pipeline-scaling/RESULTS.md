# Pipeline scaling: load-test results

Run: `cargo run --release --manifest-path bench/pipeline-scaling/Cargo.toml`
(numbers are machine-dependent; rerun locally — these are one representative run).

Question: as a scene grows to thousands of animated renderables, **how does the
deterministic per-frame CPU cost grow, and where does it live?** The harness
drives the real native pipeline — `scene.advance` (spin animation +
world-transform propagation + snapshot) then `RenderPipelineApi::submit`
(snapshot → render input → command list → recording GPU submission) — and times
the two phases separately across a sweep of renderable counts.

## After the fix (current)

| renderables | advance µs/fr | submit µs/fr | total µs/fr | submit ns/obj | fps @ total |
|---|---|---|---|---|---|
| 100    | 25.3    | 63.6     | 89.0     | 636 | 11239 |
| 500    | 196.2   | 421.5    | 617.7    | 843 | 1619  |
| 1000   | 404.3   | 893.2    | 1297.5   | 893 | 771   |
| 5000   | 2283.6  | 4236.6   | 6520.1   | 847 | 153   |
| 10000  | 4672.6  | 7793.1   | 12465.7  | 779 | 80    |
| 50000  | 29609.0 | 48921.4  | 78530.4  | 978 | 13    |

`submit ns/obj` is now **flat** (~600–980 ns with no upward trend): `submit`
scales linearly. Both phases are now linear in scene size.

## Before the fix (the original finding)

| renderables | submit µs/fr | submit ns/obj |
|---|---|---|
| 100    | 101.1     | 1011  |
| 1000   | 1201.0    | 1201  |
| 10000  | 57434.4   | 5743  |
| 50000  | 1438951.7 | 28779 |

`submit` was **super-linear — ~O(N²)**: the per-object cost climbed ~28× (1011 →
28779 ns) while the scene grew 500×, and a single 50k-renderable frame's
`submit` took ~1.4 **seconds**. At 50k the fix cut `submit` ~29× (1,438,951 →
48,921 µs) and the whole frame ~19×.

## Root cause and fix

`RenderPipelineApi::submit` resolved each renderable's world transform with a
**linear scan of every node** (`snapshot.nodes().iter().find(...)`) — plus a
linear scan for the camera node and per-renderable `.find()`s over the
mesh/material asset lists. With N renderables over ~N nodes that node scan was
O(N²).

Fixed at the source, the lowest correct layer:

- **`axiom-scene`** now offers `SceneSnapshot::node(id) -> Option<&NodeSnapshot>`,
  an `O(log N)` binary search. The snapshot already guarantees nodes are stored
  in ascending id order, so this exploits an existing invariant with no extra
  allocation, and fixes *every* consumer that resolves nodes by id — not just
  this pipeline. That is the structural fix: the inefficiency was that the
  snapshot only offered linear access to a collection it keeps sorted.
- **`axiom-render-pipeline`** uses that lookup for the camera and per-renderable
  node resolution, and resolves mesh/material ids through `O(1)` hash maps
  instead of `.find()` scans (used for lookup only, never iterated, so output
  stays deterministic).

The whole pass is now `O(renderables · log nodes)` instead of
`O(renderables · nodes)`.

## Scope: what this does and does not measure

- **In scope (deterministic, native, reproducible):** the full CPU pipeline up
  to a *recording* GPU submission (`WebGpuApi::new_recording()`, the same
  backend the engine's own tests use). No browser, stable numbers.
- **Out of scope:** the actual on-GPU paint — the wasm `LiveGpuBinding`
  instanced cube drawer. That is non-deterministic and browser-bound; its
  throughput (FPS at increasing instance counts) is measured in a real browser
  via the Playwright controller (see `apps/axiom-stress-cubes-browser`), not in
  this harness.

## Takeaway

"Draw a ton of animated things" is a fair load test — and run as the
deterministic CPU pipeline it caught a quadratic in `submit` that would
otherwise have surfaced only as a mysterious frame-time collapse in a busy
browser scene. Fixing the node lookup at its source flattened `submit ns/obj`
and lifted the high-N frame rate by an order of magnitude.
