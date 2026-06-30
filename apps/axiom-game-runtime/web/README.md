# @axiom/game hot-reload dev harness

A working developer loop for authoring an Axiom game in TypeScript: **edit
`src/game.ts`, save, and the browser re-runs live** — no page reload, no WASM
rebuild.

Unlike the original spike, this harness drives the **real engine path** end to
end — there is no stand-in bridge and no stand-in render surface:

- **Real lifecycle + loop:** `createGame()` mints the per-game registry the author
  registers into; the SDK's own `boot()` aggregator installs the real host channel
  (`hostFromWasm`), builds the real `GameLoop` over the real `NativeBridge`
  (`bridgeFromWasm`), wires DOM input, and drives `requestAnimationFrame`.
- **Real 2D surface:** the author draws through `frame.circle/rect/line/…`, which
  records into the native `draw2d` builder. `frame.finish()` returns the geometry
  command stream, which the harness's `present()` rasterizes to a canvas2d context.

## Hot reload model — deterministic re-run (Mode B)

On each save the dev server recompiles with `tsgo` and pushes a `reload` event over
Server-Sent Events. The harness tears down the loop, mints a **fresh `WasmGame`**
(same seed) + `createGame`, re-imports the author module, and re-runs from tick 0
with the new logic. The wasm *module* stays loaded; only a new game instance is
made — so a deterministic engine re-derives the same run under the edited rules.
(Input-replay fast-forward, so the re-run lands back at the exact tick you were on,
is a natural follow-up.)

## Run it

From the repo root:

```sh
# one-time prerequisites
( cd packages/axiom-game && npm install && npm run build )          # build the SDK to dist/
cargo build -p axiom-game-runtime --target wasm32-unknown-unknown --release
wasm-bindgen --target web \
  --out-dir apps/axiom-game-runtime/web/pkg \
  target/wasm32-unknown-unknown/release/axiom_game_runtime.wasm

# the dev loop
node scripts/axiom_dev_server.mjs            # serves http://localhost:8080
```

Open `http://localhost:8080`, then edit `src/game.ts` (try the constants at the
top) and save. The browser re-runs within a few hundred milliseconds.

## Layout

| File | Role |
|------|------|
| `index.html` | The page: a fixed 960×540 canvas + an import map pointing `@axiom/game` at the SDK's `dist/` build; loads `dist/harness.js`. |
| `src/game.ts` | **The author's file** — the only thing a game dev edits. Draws through the real `frame.*` 2D surface. |
| `src/harness.ts` | Host edge: `createGame` + `boot` wiring, the hot-reload (SSE) client, and the canvas2d presenter for the draw2d command stream. An app `web/` file, outside the engine gates. |
| `tsconfig.json` | Compiles `src/*.ts` → `dist/*.js` with tsgo. |
| `scripts/axiom_dev_server.mjs` | Repo tooling: serves files, watches `src/`, recompiles with tsgo, pushes a `reload` event over SSE. |

## What this reconciliation also fixed / added on the engine side

- **`draw2dFinish` now carries geometry.** Previously the wasm boundary returned
  only `[kind, layer, submission]` per command (a determinism/ordering proof, not
  renderable). It now returns a self-describing `[kind, layer, submission, len,
  …geometry]` stream so a 2D presenter can rasterize it — see
  `apps/axiom-game-runtime/src/draw2d.rs`.
- **`bridgeFromWasm` now binds its forwards.** It assigned bare wasm method
  references (`rngUnit: game.rngUnit`) that lose their receiver when called through
  the bridge (trapping as "null pointer passed to rust"). This harness was the
  first code to drive the real wasm bridge, surfacing the bug; the fix binds the
  receiver in `packages/axiom-game/src/wasm-bridge.ts`.

## Packaging into a droppable bundle

`make package APP=game-runtime` (scripts/package_app.py) bakes this dev loop into a
static bundle: it builds the `@axiom/game` SDK, compiles `web/src` with tsgo, copies
the vendored SDK + author module in, and drops the capability-detecting loader in AT
`/pkg/axiom_game_runtime.js` — the exact glue path the harness imports — so a browser
WITH WebAssembly runs the wasm fast-path. The bundle uses absolute `/pkg`, `/vendor`,
`/dist` URLs, so serve it from a domain root. The live SSE hot-reload (`/events`) is a
dev-server feature absent from the bundle; the harness closes that stream when it
fails to open, so a static bundle does not retry the missing endpoint.

**Known fallback gap (a design signal, not a wiring bug).** The wasm2js fallback (for
a browser with *no* WebAssembly at all) is emitted, detected, and loaded — it prints
the one `console.warn` — but cannot yet *execute* for this app: its wasm-bindgen glue
crosses `u64` values (`fixed_step_nanos`, ticks, `seed`) on the JS boundary using the
**BigInt i64 ABI**, whereas Binaryen's `wasm2js` legalizes i64 to split-`i32` pairs.
The two ABIs are incompatible (`new WasmGame(fixedStepNanos)` throws "Cannot mix
BigInt and other types" inside a wasm2js legalstub). game-runtime is the first
packaged app to expose `u64` on the boundary, so it is the first to surface this. The
correct fix is structural and spine-wide: drop `BigInt`/i64 from the `@axiom/game`
wasm boundary (nanos and tick counts fit in an `f64`'s 2^53), which touches the SDK's
`wasm-bridge.ts`/`wasm-host.ts`/… and the Rust wasm signatures — out of scope for a
single app-packaging change. The wasm fast-path (the common case) is fully working.

## Known limitations (out of scope for this harness)

- The presenter handles the flattened kinds (rect / circle / ellipse / line /
  particle); path / sprite / text cross with `len = 0` (ordering kept, geometry
  not yet flattened) and are skipped.
- The presenter assumes an identity 2D camera/transform (the author draws in
  surface pixels and sets no `camera2D`); a non-identity baked transform is not
  applied.
