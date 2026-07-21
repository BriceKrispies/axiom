---
name: extract-engine
description: Extract the Axiom engine out of a pure-TS @axiom/game app, making it a FULLY SELF-CONTAINED pure-TypeScript app with its own in-app engine (WebGL2 renderer + z-buffered Canvas2D fallback, fixed-step loop, input, WebAudio) — no SDK, no wasm. Invoke with the app name (e.g. "/extract-engine heat-check" or "apps/axiom-heat-check"). Follows the proven three-point playbook and copies its app-agnostic engine template.
---

# extract-engine

Turn a `@axiom/game`-based pure-TypeScript app (`apps/axiom-<name>/web/`) into a
**fully self-contained app**: zero runtime imports from Axiom — no `@axiom/game`
SDK, no `axiom-game-runtime` wasm, no `/vendor/` or `/pkg/` paths. The app ships
its own engine under `web/src/engine/`, written in pure TypeScript over bare
browser APIs (WebGL2 + Canvas2D, requestAnimationFrame, DOM events, WebAudio).

**The template is a living app, not a scaffold:** `apps/axiom-three-point/web/src/engine/`
is a complete, app-agnostic engine (nothing basketball-specific in it) that was
extracted this way and is verified in production. COPY IT — do not regenerate it
from scratch. If the template app has fixes newer than this skill, the copy
inherits them for free.

## Inputs

- **The target app** — a name (`heat-check`, `minimal-3v3`, …) or a path
  (`apps/axiom-heat-check`). The app must be a pure-TS leaf app in the
  heat-check convention: an SDK-free deterministic core (constants / types /
  gameplay / session / vec / meshgen …) under one engine-facing `scene.ts`,
  wired by `game.ts`, booted by `harness.ts`.

## Step 0 — Survey and gate

1. `grep -rn "@axiom/game\|/vendor/\|/pkg/" apps/<app>/web/src/*.ts apps/<app>/web/index.html apps/<app>/web/tsconfig.json`
   and list every touchpoint. In a conventional app the only SDK-facing files
   are `scene.ts` (scene calls), `game.ts` (Sim/input/audio wiring), and
   `harness.ts` (boot + wasm init). Anything else importing the SDK must be
   refactored behind those three first (that is the heat-check convention doing
   its job — restore it before extracting).
2. **Check the present mode in `harness.ts`.** This skill's renderer replaces
   the 3D retained-scene surface (`present3d` + `createMesh`/`spawnRenderable`/
   `setCamera3D` …). A 2D `draw2d` app (`present2d`, e.g. signal-runner) needs a
   different, much simpler renderer (a canvas drawing of the Frame command
   list) that the template does not include — STOP and tell the user instead of
   improvising one silently.
3. Inventory which SDK input surface the game uses. The template's `InputState`
   provides `isDown` / `pressed` / `released` / `look()` / `pointer()` +
   `bindAction`. If the app uses other SDK helpers (`axis(neg, pos)`,
   `swipe()`, `pointerPressed()`), plan tiny equivalents in `game.ts` (e.g.
   `axis` = `(isDown(pos)?1:0) - (isDown(neg)?1:0)`) — do NOT widen the engine
   for one app.
4. Note the fixed Hz, seed usage (`sim.rng` has no engine replacement — if the
   app uses it, port a small deterministic PRNG into its own core), and any
   `sim.world` / ECS / `sim.physics` usage (these have no template equivalent;
   the game must already own that logic app-side, as all extracted apps do).

## Step 1 — Copy the engine template

```sh
cp -r apps/axiom-three-point/web/src/engine apps/<app>/web/src/engine
```

What it contains (all pure TS, only browser APIs):

| File | Role |
|---|---|
| `api.ts` | The contract: `Entity`/`Handle`/`Rgba`/`Transform`/`MeshData`/`MaterialSpec`/`Light`/`Camera3D`/`TickInput`/`ToneSpec` — deliberately vocabulary-compatible with `@axiom/game`, so `scene.ts` barely changes |
| `renderer.ts` | Retained-scene store + backend selection (`initRenderer(canvas, "auto"|"webgl2"|"canvas2d")`, logs the chosen backend) behind the free functions `createMesh`/`createMeshData`/`createMaterial`/`spawnRenderable`/`setNodeTransform`/`setCamera3D`/`addLight`/`clearScene`/`renderScene`/`resizeRenderer` |
| `backend.ts` | Internal store↔backend contract + shared lighting constants |
| `backend-webgl2.ts` | Hardware path: one Lambert forward program, emissive/opacity, 8 dir + 8 point lights, translucent back-to-front pass |
| `backend-canvas2d.ts` | Software fallback: **z-buffered** scanline rasterizer at half internal resolution, near-plane Sutherland–Hodgman clipping, flat Lambert via the exported parity-tested `lambertLight`, low-poly primitives (`meshDetail: "low"`), software alpha blend. Do NOT replace with a painter's algorithm — that was tried and cannot render scenes whose camera stands inside large boxes or whose decals stack millimetres apart |
| `loop.ts` | `FixedStepper` + `startLoop` — REAL-TIME fixed-step accumulator under rAF (100 ms stall clamp, catch-up cap). Note: this is NOT frameLocked; the sim runs at true wall-clock speed on any refresh rate |
| `input.ts` | `InputState` (node-testable core; `beginTick()` snapshot semantics for exact per-tick edges) + `attachDomInput` (keyboard on window, canvas-click pointer lock, mousemove look while locked, canvas pointer events) |
| `audio.ts` | `playTone(ToneSpec)` — lazy AudioContext, no-op headless |
| `mat4.ts`, `meshes.ts` | Column-major matrix math; unit box / sphere / cylinder generators (box = unit cube ⇒ scale = full extents, sphere = unit diameter, cylinder = unit diameter × unit height) |
| `render.test.ts`, `platform.test.ts` | node:test suites for the pure parts (mat4, meshes, lighting parity, stepper, input edges) — they come along and must stay green |

Mesh/material conventions are the SDK's, so the app's `scene.ts` numbers work
unchanged. Materials are plain uniforms — the old "materials upload once at
bind / frameLocked first-frame scene build" gotcha is GONE.

## Step 2 — Rewire the three engine-facing files

- **`scene.ts`** — change only the import: SDK functions →
  `./engine/renderer.ts`, types (`Entity`, `Rgba`, `Transform`) →
  `./engine/api.ts`. Everything else stays.
- **`game.ts`** — replace the `onFixedUpdate(sim => …)` side-effect module with
  two explicit exports (see three-point's `game.ts` as the model):
  - `initGame(input: InputState): void` — bind actions
    (`input.bindAction(...)`), `buildScene()`, fresh session, reset any
    module-level gesture state.
  - `updateGame(input: TickInput, tick: number): void` — the old fixed-update
    body: fold input → `Intent` (replace `sim.input.X()` with `input.X()`,
    `sim.tick` with the `tick` parameter), advance the session, drain audio
    cues → `playTone` from `./engine/audio.ts`, `applyFrame`.
  - Keep `readHud()` / `configureViewport()` exports as-is.
- **`harness.ts`** — rewrite the boot while KEEPING the DOM-HUD code and the
  two dev-server packager anchors VERBATIM (`` import(`/dist/game.js?v=${version}`) ``
  and `new EventSource("/events")`); the wasm anchor (`await initWasm();`) is
  deleted. New boot shape (three-point's `harness.ts` is the model):
  1. `?backend=` query param → `initRenderer(canvas, choice)`.
  2. `new InputState()` + `attachDomInput(input, canvas)` (per hot-reload
     `load()`, detaching the old one).
  3. `mod.configureViewport(canvas.clientWidth || canvas.width, …)` once and on
     window resize.
  4. `mod.initGame(input)` then
     `startLoop({ fixedHz, maxCatchUpSteps: 8, update: t => { input.beginTick(); mod.updateGame(input, t); }, render: () => { renderScene(); updateHud(mod.readHud()); } })`,
     keeping the stop function for hot-reload teardown.
  5. Delete the wasm imports, `createGame`/`boot`/`onRender`, and any
     console.log filter for wasm backend spam.

## Step 3 — De-Axiom the shell

- **`web/index.html`** — remove the `@axiom/game` import map block.
- **`web/tsconfig.json`** — remove the `paths` mapping (every import is now
  relative); update the header note.
- **Packaging** — nothing to write. `scripts/package_gallery.py` compiles,
  bundles, and lays out every registered app; there are no per-app packagers or
  Makefile targets any more. Confirm the app's `web/index.html` has a
  `<script type="module" src="/dist/<entry>.js">` — that is what the packager
  bundles from.
- **Registration** — `cargo run -p axiom-serve -- init <app>` writes
  `apps/<app>/app.json`; edit its `title`, `blurb`, `description`, and `tags`.
  That single file is the gallery card.
- Sweep stale comments: `grep -rn "@axiom" apps/<app>` should end up matching
  nothing but honest history notes; update the README and the app's `app.json`
  description to say the app is fully self-contained.

  NOTE: a fully self-contained app that keeps its own in-app engine does NOT
  share the gallery's `@axiom/web-engine` build. Extraction and engine-sharing
  pull in opposite directions — extract only when an app genuinely needs to
  diverge from the shared engine, not by default.

## Step 4 — Verify (all of it, not just typecheck)

```sh
node --test apps/<app>/web/src/*.test.ts apps/<app>/web/src/engine/*.test.ts
npm --prefix packages/axiom-game exec -- tsgo -p apps/<app>/web/tsconfig.json
node apps/<app>/web/src/agent.ts        # if the app has a headless driver
make gallery-<app>
```

Then drive the real page with `uv run scripts/playwright_controller.py`
(`goto` → `console` → `eval` → `screenshot`) on BOTH backends (default and
`?backend=canvas2d`), and play real game moments end-to-end, watching for
rendering artifacts while moving the camera/scene — not just the first frame.
Browser-driving notes that WILL bite otherwise:

- The loop is now REAL-TIME: time synthetic input by **milliseconds**
  (N ticks = N·1000/fixedHz ms), not by counting rAF frames (that was the
  frameLocked-era rule).
- Pointer lock cannot be acquired synthetically: shim
  `Object.defineProperty(Document.prototype, 'pointerLockElement', { get: () => canvas })`
  then dispatch `MouseEvent('mousemove', { movementX })`; keyboard via
  `window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }))`
  (listeners are on window, no isTrusted filter).
- A Chrome tab with `document.visibilityState === "hidden"` has rAF frozen —
  the game looks dead. Use the Playwright controller instead.
- Long controller `eval`s (≳40 s) can kill its daemon — split long drives.

Fix everything found; do not claim completion on compilation.

## Step 5 — Commit

Commit ONLY the target app's files + its packager + its Makefile/gallery hunks,
per the repo's direct-to-main workflow. If `Makefile`/`gallery.js` carry
unrelated uncommitted hunks, use the split-staging trick: back up the working
copies, `git show HEAD:<file> > <file>`, re-apply only this app's hunks, `git
add`, restore the backups (the index keeps the clean version).

## Divergence from the template

If the target app needs something the template engine lacks, improve the
template IN `apps/axiom-three-point/web/src/engine/` first (with tests), verify
three-point still works, then copy — the template is the single source of
truth, and every extracted app should carry an identical engine. Never fork the
engine per app for a shareable capability; per-app forks are only for genuinely
app-specific rendering.
