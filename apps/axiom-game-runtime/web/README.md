# @axiom/game hot-reload dev harness (spike)

A working spike of the developer experience for authoring an Axiom game in
TypeScript: **edit `src/game.ts`, save, and watch the browser update live** — no
page reload, no WASM rebuild, the engine never restarts.

## What this proves

The architecture is already Phaser-shaped for hot module reloading:

- **The WASM engine is loaded once and stays alive.** `apps/axiom-game-runtime`
  is the deterministic runtime (fixed-step accumulator + seeded RNG). The
  harness boots it a single time per session.
- **The game author's code is plain JS on top.** `src/game.ts` registers
  `onFixedUpdate` callbacks through the real `@axiom/game` SDK and exports a
  `draw`. On each save it is hot-swapped via a fresh dynamic `import()`; the
  engine — and its still-advancing tick — is untouched.
- **The swap uses the real SDK seam:** `defaultRegistry.reset()` then re-running
  the author module's registrations.

This is "Mode A" (state-preserving hot patch) from the design. The deterministic
"Mode B" re-run/replay loop is a later phase.

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
top) and save. The page updates within a few hundred milliseconds.

## Layout

| File | Role |
|------|------|
| `index.html` | The page. Import map points `@axiom/game` at the SDK's `dist/` build; loads `dist/harness.js`. |
| `src/game.ts` | **The author's file** — the only thing a game dev edits. |
| `src/harness.ts` | Boot + frame loop + canvas painter + the SSE hot-reload client. The platform edge (an app `web/` file, outside the engine gates). |
| `tsconfig.json` | Compiles `src/*.ts` → `dist/*.js` with tsgo. |
| `scripts/axiom_dev_server.mjs` | Repo tooling: serves files, watches `src/`, recompiles with tsgo, pushes a `reload` event over Server-Sent Events. |

## Honest limitations (deliberately out of scope for the spike)

- **The WASM bridge is RNG-complete only.** `WasmGame` exposes `advance`,
  `snapshot`, and the full SPEC-01 RNG seam today. World / input / physics /
  timer / tween are not wired through wasm yet, so the harness bridge throws if
  an author reaches for them. Completing that seam is game-side work this spike
  intentionally skips.
- **The canvas2d painter is a stand-in** for the not-yet-wired 2D render
  surface. The hot-reload machinery is identical regardless of what the author
  eventually draws into.
- **The dev server is a spike**, not a hardened tool (no source maps wired, a
  `shell:true` spawn warning, no port-in-use fallback). If this graduates it
  should become `tools/axiom-dev-server`.
