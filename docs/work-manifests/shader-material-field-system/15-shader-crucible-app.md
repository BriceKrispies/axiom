# 15 — The shader crucible app

## Objective

Build `apps/shader-crucible`: one app that demonstrates the procedural appearance
system **in its entirety**, on screen, on both backends — and that closes the
three requirements manifest 13 could not reach.

This is a demonstration, not a game. Its job is to make every capability the
previous fourteen manifests landed *visible and checkable by a human*, and to be
honest in the same frame about the four things that do not yet work.

## Architectural placement

**App** — `apps/shader-crucible/`, with an `app.toml`. A leaf composition root:
nothing may depend on it. Apps are exempt from the Branchless Law and the 100%
coverage gate, and are still expected to ship the tests their behaviour warrants.

```toml
[app]
name = "shader-crucible"
crate_name = "axiom-shader-crucible"
allowed_layers = ["kernel", "math", "frame", "host", "runtime", "field", "surface", "recipe", "proc-texture", "mesh", "mesh-ops"]
allowed_modules = ["engine", "input", "windowing", "debug-overlay"]
```

Trim that list to what is genuinely used — an app manifest listing a layer it
never names is the same ceremonial-dependency failure the Layer Law bans, and
`check-architecture` reports `AppAllowedLayerUnknown` for a name that is not real.

## Why this app exists — the gap it closes

Manifest 13 proved requirements 1, 5, 6, 7, 8 and 9 of the vertical slice, but
**not 2, 3 and 4**: it bakes asphalt to a texture through `TextureOp::Field`, so
the graph never becomes a `Surface`, never gets a `surface_program`, and never
reaches the WGSL emitter. That path is proven by manifests 06–09's own tests on
real GPU captures, but by **no shipping app**.

That was the right call for burnt-rubber — asphalt is the largest surface in
frame and making it live per-pixel changes the fill-rate story. It is the wrong
call here. **The crucible authors live `Surface`s and takes the generated-shader
path**, because that is the half of the system nothing yet demonstrates.

## What it must demonstrate

Each station is a distinct, labelled subject in one scene. A viewer should be
able to point at one and say what capability it proves.

| # | Station | Proves | Landed by |
|---|---|---|---|
| 1 | **Layered material** — metal base + paint + scratch + dirt, each masked | Mask-driven layering flattening to `Mix` composition; the case the whole `Surface` design exists to serve | 04 |
| 2 | **Live procedural surface** — a field-authored base colour and roughness, evaluated per pixel | The graph → `Surface` → `surface_program` → WGSL path **13 could not prove** | 06, 08, 09 |
| 3 | **Baked texture** — the same graph baked through `TextureOp::Field` | One graph, two realisations; bake and live agree | 05 |
| 4 | **Parameter retune** — a slider/keypress that retunes a parameter live, with the digest displayed on screen and **not changing** | Retuning is a uniform write; the program is never recompiled | 01, 09 |
| 5 | **Time-varying displacement** — wind and ripple as authored graphs | Vertex-stage fields, deterministic engine time | 10 |
| 6 | **Three lighting models** — the same surface under `Unlit`, `Lambert`, `LambertSpecular` | The closed lighting discriminant; zero extra pipelines | 11 |
| 7 | **Implicit surface** — a metaball-ish body from `ScalarField::sample` → `implicit_surface_mesh` | The gap `mesh-ops` documented in the negative and could not fill | 05 |
| 8 | **Transcendental patterns** — marble veining and wood grain via `Sin`/`Pow` | The 27-operator vocabulary; effects as authored graphs, not Rust | 14 |
| 9 | **Both backends** — the identical scene on GPU and Canvas2D | Per-pixel vs per-triangle-centroid as a *reported substitute*, not a drop | 07 |
| 10 | **Introspection** — `explain()` / `digest()` / `diff()` for the selected station, on screen or dumped | The graph is machine-readable data, not opaque source | 12 |

## What it must NOT hide

A demo that quietly avoids the broken cases is worse than no demo. Each of these
is **stated on screen or in the app's README**, next to the station it affects:

1. **A displaced vertex casts an undisplaced shadow.** The shadow pass runs no
   displacement program (`SHADOW_WGSL` is a separate module). Station 5 must be
   lit so this is visible, and labelled.
2. **Skinned geometry always gets the default program.** `SkinnedGpuDraw` carries
   no `surface_program` lane. If the crucible shows a skinned body at all, it is
   labelled as unsurfaced.
3. **Canvas2D shades per triangle.** A fine scratch mask may vanish entirely at
   that granularity — station 1 on the software arm is the honest illustration of
   what "substitute, not drop" costs. Do not tessellate the mesh to hide it.
4. **`metallic` changes no pixel.** It is carried and inert by design (SPEC-11's
   "resist PBR scope creep"). Label it, do not quietly omit it.

## Node budget — a real risk, measure it early

`MAX_NODES = 256`, and a layered surface **flattens into one graph per channel**.
Station 1 is four layers × several channels. **Build station 1 first and print
its flattened node count per channel**; if it does not fit, the finding is that
`MAX_LAYERS = 4` and `MAX_NODES = 256` are in tension — report it, do not raise
either cap. That is a genuine design signal about the primitive, and it is worth
more than a prettier demo.

## Determinism

The crucible is a deterministic app: fixed seed, `EvalContext::time` from the
engine's frame clock, never a wall clock. Tick N replayed twice is identical;
tick N and N+60 differ only where a station is time-varying. `axiom-shot` capture
at a fixed tick is the regression artifact.

## Testing requirements

Apps are outside the 100% gate, but this one earns specific tests:

* Every station's graph `validate()`s and its digest is pinned.
* Station 4: retuning the parameter leaves `Surface::digest()` **identical** —
  the load-bearing assertion of the whole design.
* Station 2 vs 3: the live surface and the baked texture agree within a stated
  tolerance at sampled points.
* `supported_by` reports the truth for both profiles before rendering.
* A grep test: **the app authors no WGSL** (same shape as 13's).
* Node-count assertions per station, so a future edit that blows the budget fails
  a test rather than a frame.

## Verification — a green build is not the deliverable

```sh
cargo run -p axiom-shot --features offscreen -- --app shader-crucible --backend gpu      --tick 0 --out screenshots/crucible-gpu.png
cargo run -p axiom-shot --features offscreen -- --app shader-crucible --backend canvas2d --tick 0 --out screenshots/crucible-c2d.png

uv run scripts/localhost_servers.py start-app shader-crucible --port 8086
uv run scripts/localhost_servers.py logs shader-crucible -n 20
uv run scripts/playwright_controller.py goto http://localhost:8086/
uv run scripts/playwright_controller.py wait 2500
uv run scripts/playwright_controller.py console          # must be error-free
uv run scripts/playwright_controller.py screenshot crucible
```

**Read every screenshot and say what is actually in it, station by station.** The
build compiling and the page painting are different facts, and this repo's own
instructions say so.

## Explicitly excluded

* No new engine capability. If a station cannot be built with what exists, **that
  is the finding** — report it rather than adding an operator, a channel, or a
  capability bit. `FieldOp` is closed at 27 after manifest 14.
* No raw WGSL in the app.
* No new layer, module, or feature module.
* No gameplay, no scoring, no physics.
* Do not fix the four "must not hide" limitations — demonstrate them.

## Completion criteria

1. `apps/shader-crucible` exists, classifies as an App, and `check-architecture`
   passes.
2. All ten stations render on the GPU arm; the scene renders on Canvas2D with the
   documented substitutions reported.
3. Station 4 proves the digest does not move under parameter retune.
4. The four limitations are visible and labelled.
5. Screenshots captured **and read**, described station by station.
6. `cargo test --workspace` adds no new failures; `check-slices` and
   `check-slice-placement` pass.

## Parallel safety

**Sequential, after 14** — not for file overlap (it owns only `apps/shader-crucible/**`
plus one root `Cargo.toml` members line), but because 14 renames every operator
constant and the crucible names them throughout.
