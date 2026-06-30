# Axiom — DOOM, authored purely in TypeScript

A DOOM-style first-person shooter that is the **TypeScript-only twin of
`apps/axiom-doom-browser`**: the same cube-walled two-room level, first-person
camera, tank movement with wall collision, four chasing enemy cubes, hitscan
shooting, and a health / ammo / score loop — but with **no Rust in this app at
all**. The whole game is `web/src/game.ts`, ~250 lines authored against the
`@axiom/game` SDK; it drives the *real* engine (the shared `axiom-game-runtime`
wasm) through the SDK's scene-authoring, query, input, and world surfaces, and the
engine renders the retained 3D scene to the canvas via its live
WebGPU → WebGL2 → Canvas2D backend.

This is the answer to "can a game be authored on Axiom in TypeScript instead of
Rust?" — yes, including a 3D FPS.

## How it differs from `axiom-doom-browser`

| | `axiom-doom-browser` | `axiom-doom-ts-browser` (this app) |
|---|---|---|
| Game logic | Rust (`src/lib.rs` `DoomGame`) | TypeScript (`web/src/game.ts`) |
| Engine binary | its own wasm crate | the shared `axiom-game-runtime` wasm |
| Authoring API | the `axiom` engine prelude | the `@axiom/game` SDK |
| Rendering | `axiom-windowing` live loop (engine owns the loop) | the SDK loop drives `axiom-windowing`'s new bind-once / present-each-frame seam |
| Hot reload | level `.axiom` over SSE | **the whole game** (`game.ts`) over SSE — edit, save, re-run |

The gameplay is intentionally as close as the SDK allows. The one parity gap is
relative mouse-look (pointer-lock look): the SDK turns with the keyboard (`←`/`→`)
today; mouse-look needs a native look-delta accumulator and is tracked as a
follow-up.

### Pacing: frame-locked, for parity

The Rust DOOM steps its sim **exactly once per displayed frame** (`web.rs`: *"DOOM
steps the sim exactly once per frame"*) — a frame-locked loop, so its speed scales
with the frame rate. The `@axiom/game` SDK defaults to the opposite, frame-rate-
independent model (a real-time accumulator at `fixedHz`, so the game runs at the
same wall-clock speed at 30, 60, or 144 fps). Those two models only agree at
exactly 60 fps; below it the real-time game outruns the frame-locked one (at 30 fps
it is literally 2× faster). For byte-for-byte parity this app boots with
`frameLocked: true`, so it ticks once per frame exactly like the Rust original.
(A normal game should prefer the real-time default; frame-locked is the deliberate
parity choice here.)

## What had to be added to make this possible

A TypeScript author could already author a *2D* game (see the
`axiom-game-runtime/web` hot-reload harness). Presenting an authored *3D* scene
needed new seams, all landed alongside this app:

- **Engine (`axiom`):** `RunningApp::render(tick)` — the present half of a frame,
  split out of `tick` so a host that owns its own fixed-step loop can render the
  *current* scene after its per-frame mutations; `RunningApp::update_world_transforms`
  and `Handle::from_raw`.
- **Windowing (`axiom-windowing`):** a `LivePresenter` extracted out of the rAF
  run-loop closure into a persistent **bind-once / present-each-frame** path
  (`WindowingApi::bind_present_surface` / `present_frame`), reused by both the
  engine's own run loops and a caller-owned loop.
- **Runtime wasm (`axiom-game-runtime`):** `WasmGame.bindSurface` / `renderScene`,
  plus 3D scene-node authoring — `spawnRenderable` / `setNodeTransform` /
  `setNodeBounds` / `clearScene`.
- **SDK (`@axiom/game`):** the matching `spawnRenderable` / `setNodeTransform` /
  `setNodeBounds` / `clearScene` authoring verbs and a `present3d` boot option.

### Input + camera go through the engine, not TS

- **Mouse-look** flows through the same input pipeline keyboard keys use: a native
  relative-look accumulator (`inputLook` → `sim.input.look()`), with pointer-lock
  capture + `movementX/Y` in the SDK's input edge, and the left button reported as
  `Mouse0`. Same path, same `0.0025` rad/px sensitivity as the Rust DOOM.
- **The camera is the engine's first-person Controller.** `createController` spawns
  the camera as a `Controller` node and `controlFirstPerson` feeds it one
  `FirstPersonInput` per frame; the engine yaws, pitches (clamped), and moves the
  node itself, applied **immediately** (zero lag) via a new `RunningApp::control` /
  `Scene::control_now` path that reuses the exact per-tick `ControllerSystem` logic.
  The game never authors a camera transform — it owns only the intent + collision,
  exactly like the Rust DOOM, which drives the same controller.

## Run it

From the repo root:

```sh
# one-time prerequisites
( cd packages/axiom-game && npm install && npm run build )            # build the SDK to dist/
cargo build -p axiom-game-runtime --target wasm32-unknown-unknown --release
wasm-bindgen --target web \
  --out-dir apps/axiom-game-runtime/web/pkg \
  target/wasm32-unknown-unknown/release/axiom_game_runtime.wasm       # the shared wasm

# the dev loop (serves THIS app; reuses the wasm above)
AXIOM_DEV_APP=axiom-doom-ts-browser node scripts/axiom_dev_server.mjs  # http://localhost:8080
```

Open `http://localhost:8080`. **W/↑** forward · **S/↓** back · **A/D** strafe ·
**←/→** turn · **Space** fire. Edit `web/src/game.ts` (try the tunables at the top)
and save — the browser re-runs live within a few hundred milliseconds.

## Layout

| File | Role |
|------|------|
| `web/index.html` | The page: a 960×600 canvas + an import map pointing `@axiom/game` at the SDK's `dist/` build; loads `dist/harness.js`. |
| `web/src/game.ts` | **The whole game.** Level, movement, enemies, hitscan, HUD — authored on the SDK. The only file a game dev edits. |
| `web/src/harness.ts` | Host edge: `createGame` + `boot({ present3d })` wiring, the hot-reload (SSE) client, and the DOM HUD updater. |
| `web/tsconfig.json` | Compiles `web/src/*.ts` → `web/dist/*.js` with tsgo. |

There is no `Cargo.toml`, no `app.toml`, and no `src/*.rs` — this app is not a Rust
crate. It is pure TypeScript over the shared engine, which is the whole point.
