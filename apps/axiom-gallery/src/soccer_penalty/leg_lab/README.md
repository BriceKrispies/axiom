# Leg Lab — one procedural leg, authored in TypeScript

The smallest deterministic scene that proves a single procedural leg can move
smoothly, so the soccer game's static kicker can eventually be replaced with a leg
that is actually animated. **There is no Rust in this folder** — the whole lab is
TypeScript authored on the [`@axiom/game`](../../../../../packages/axiom-game) SDK,
driving the shared `axiom-game-runtime` wasm engine, which renders the retained 3D
scene through its live WebGPU → WebGL2 → Canvas2D backend.

It was ported 1-to-1 from the previous Rust implementation (see the git history of
this folder); the pure sim is identical maths, and the rendering moved from the
Rust engine prelude to the SDK's `createMesh` / `spawnRenderable` / `setNodeTransform`
verbs.

## Run it

```sh
python apps/axiom-gallery/src/soccer_penalty/leg_lab/serve.py
```

Then open **http://localhost:8010/**. Add `?backend=canvas2d` to force the software
renderer if your browser has no WebGPU/WebGL2. The script compiles the TypeScript
with tsgo and (on first run) builds the SDK + the shared wasm engine if they are
missing.

The leg loops on its own — no input. **Cyan** marker = hip/root, **magenta** =
the foot target; bones are cylinders, joints are spheres.

### Hot reload

`serve.py` watches `src/*.ts` and, on save, recompiles and pushes a `reload` event
over Server-Sent Events (`/events`); the browser tears down the loop, re-imports
the game module, and re-runs deterministically from tick 0 with your change
applied — no manual refresh. Edit a tunable in `src/leg-rig.ts` (stride, step
height, hip height) or the gait in `src/gait.ts` and just save. Because the lab is
split across several modules, `serve.py` version-stamps every relative `/dist`
import so a change to **any** module takes effect (not just `game.ts`). Editing
`index.html` or `src/harness.ts` needs a normal page refresh.

## What it proves

- A repeating, tick-derived **gait cycle** (`gait.ts`): a planted phase (foot
  locked to a world contact point while the hip advances) and a swing phase (foot
  lifted along a smooth arc to the next contact).
- A **2-bone IK** solve (`leg-ik.ts`) hip → knee → foot whose knee bends
  consistently forward and can never flip.
- A critically-damped **hip-bob spring** (`hip-spring.ts`) so the root reads as
  intentionally animated, not snapped.
- Full **determinism**: `LegLabSim.atTick(params, n)` reconstructs any tick's pose
  from scratch, identically every time.

## Layout

| File | Role |
|------|------|
| `src/vec3.ts` | The minimal 3-vector the sim runs on (plain TS f64). |
| `src/leg-ik.ts` | The two-bone IK solver + its no-flip invariant. |
| `src/gait.ts` | The gait cycle and its tunable `GaitParams`. |
| `src/hip-spring.ts` | The critically-damped hip-bob smoothing spring. |
| `src/leg-rig.ts` | The kicker leg's dimensions (see the provenance note in the file). |
| `src/leg-lab-sim.ts` | Folds gait + spring + IK into one debug-carrying frame. |
| `src/scene.ts` | Builds the 3D scene once and moves the leg each frame via the SDK. |
| `src/game.ts` | Wires the sim to the engine loop; exposes the HUD read-out. |
| `src/harness.ts` | Browser boot: `createGame` + `boot({ present3d })` + the DOM HUD. |
| `index.html` | The page (canvas + import map pointing `@axiom/game` at its `dist/`). |
| `serve.py` | Compiles the TS, serves this folder, and hot-reloads on save (SSE + tsgo watch). |

## Gaps / findings vs the Rust lab

1. **Character import.** The Rust lab loaded `assets/soccer/kicker.figure` to read
   the real leg segment lengths and box sizes. The `@axiom/game` SDK has no
   figure/skeleton surface, so `src/leg-rig.ts` mirrors those four numbers as
   constants (with a provenance note pointing at the figure's authoring source).
   Making it live again would mean adding a small `loadFigure` seam to the
   SDK/runtime.

2. **The engine's 3D present used to upload the mesh set once, at bind, and never
   re-upload — now FIXED.** The SDK binds the surface lazily on the first frame and
   snapshotted `mesh_set()` then; if the first frame advanced zero ticks (real-time
   pacing), the scene wasn't built yet and only the engine's demo cube uploaded — so
   every non-cube mesh a game registered was an "unknown mesh" and silently skipped
   (`draws(proj/skip)=2/7`). The root fix landed alongside this lab: the 3D present
   now carries a **mesh-set generation** and re-uploads the set to the live backend
   when it changes, exactly mirroring the 2D `textures_generation` path. See
   `GameBridge::mesh_generation` (runtime), `WindowingApi::update_present_meshes` +
   `LivePresenter::load_meshes` (windowing), and `*BackendApi::load_meshes` (both
   backends). So this lab boots on normal real-time pacing (no `frameLocked`
   needed), and any 3D TS game's own meshes reach the GPU regardless of when they
   are registered.
