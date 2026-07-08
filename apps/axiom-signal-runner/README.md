# Axiom — Signal Runner (authored purely in TypeScript)

A **third-person downhill traversal game**: a small red hooded courier rides a
hovering gyro-sled down a winding mountain-ruin path, collecting blue signal
shards, tripping yellow pressure plates, and restoring a final beacon before a
purple storm wall overruns the relay. It is a **pure-TypeScript `@axiom/game` leaf
app** — a sibling of `apps/axiom-soccer-penalty-kick` — with **no Rust in this app
at all**: the whole game is the TypeScript under `web/src/`, driving the *real*
engine (the shared `axiom-game-runtime` wasm) through the SDK's **2D draw2d**
surface, deterministic fixed-step loop, and input projection. The engine
rasterizes the authored frame through its live WebGPU → WebGL2 → Canvas2D cascade,
so the flat-shaded, procedural illustrated look reads identically on a Canvas2D
fallback.

It matches the game's reference art: a clean low-poly snowy/stone world (faceted
mountains, triangular pines, grey rocks, ruin columns, a cream path winding to the
horizon, cyan shards, a jagged purple storm), and the full HUD — objective panel +
checklist, centre timer, storm status, a live route minimap, controls legend,
speed/charge readout, and four ability cards — all drawn procedurally in-canvas.

## Run it

It is a **self-hosted gallery demo**. Regenerate its committed single-file page and
browse it from the gallery:

```sh
make gallery-signal-runner   # build SDK + runtime wasm, compile the app, inline into
                             # apps/axiom-gallery/web/signal-runner/index.html
make gallery                 # assemble + serve the gallery at http://localhost:8000
# then open the "Signal Runner" card, or go straight to
#   http://localhost:8000/signal-runner/
```

The produced page is fully self-contained (SDK + wasm + app inlined, wasm gzip+base64
with a `DecompressionStream` boot), so it also runs straight from `file://`. Append
`?backend=canvas2d` (or `webgl2` / `webgpu`) to pin a backend.

**Controls:** **A/D** or **←/→** steer · **SHIFT** brake (tightens turns) · mouse/touch
**drag** to steer toward the cursor · **Space/1** boost · **2** shield · **3** pulse ·
**4** helper drone · **Enter** activate the relay (in range, objectives met) / restart.

## Objectives & loop

Collect **20** signal shards, activate **3** pressure plates, then reach the final
beacon and restore it with **Enter** before the **2:30** storm timer expires. You
lose if the storm wall catches you, the timer hits zero, you crash three times, or
you fall off the path. Win/lose screens restart with **Enter**.

## Layout

| File | Role |
|------|------|
| `web/index.html` | The page: a 1200×800 canvas + an import map pointing `@axiom/game` at the SDK build; loads `dist/harness.js`. |
| `web/src/harness.ts` | Host edge: the one-call `bootHotApp({ present: "2d" })` wiring (+ the single static-build seam the packager rewrites). |
| `web/src/app.ts` | The `defineApp` manifest — the ONLY place the live SDK meets the game: a fixed-update system (`input → Intent → game.step`) and a render system (`renderGame(frame, state)`). |
| `web/src/game.ts` | `SignalRunnerGame`: owns the state, steps it deterministically, exposes the pure `hud()`, folds win/lose confirm into restart. |
| `web/src/types.ts` `constants.ts` | The whole state vocabulary and every tuning number. |
| `web/src/rng.ts` `level.ts` | Seeded PRNG + deterministic route generation (20 shards, 3 plates, obstacles/drones with a guaranteed fair lane, decor, mountains, beacon). |
| `web/src/state.ts` `sim.ts` | Initial-state factory + the fixed-step simulation (movement, collection, plates, storm, all four abilities, crashes, win/lose). |
| `web/src/hud.ts` | The pure UI model (objective counts, timer text, speed, charge segments, ability cards, minimap nodes). |
| `web/src/projection.ts` | The fake-perspective chase camera (world path-space → screen). |
| `web/src/render.ts` `render-world.ts` `render-player.ts` `render-ui.ts` `draw.ts` `palette.ts` | The renderer: world (backdrop → mountains → storm → path → props/entities), the courier + sled, all HUD panels, the draw2d helpers, and the palette. |
| `web/src/signal-runner.test.ts` | 15 deterministic game-logic tests (`node --test`, no wasm/DOM). |

## Determinism & structure

The whole gameplay core (`rng`/`level`/`state`/`sim`/`game`/`hud`) imports nothing
from `@axiom/game`, so it is constructible and replayable in bare Node — every
outcome is a pure function of `(seed, intent sequence)`. `app.ts` is the sole seam
to the live SDK. There is no `Cargo.toml`/`app.toml`: this is a pure-TypeScript app
over the shared engine, which is the whole point — no game concepts leak into any
engine layer.

## Tests

`node --test apps/axiom-signal-runner/web/src/signal-runner.test.ts` — 15 pass:
seeded generation determinism, exactly 20 shards / 3 plates, a final beacon, shard
collection (+charge), plate activation, the beacon requirement gate (before/after),
the storm timer game-over, boost, shield-absorbs-a-crash, pulse-disables-drones, the
helper drone collecting a shard, restart-equals-fresh, the HUD model, and
collision/off-path replay equality.
