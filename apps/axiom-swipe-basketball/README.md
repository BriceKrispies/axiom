# Axiom — Swipe Basketball

A pure-TypeScript `@axiom/game` leaf app — a sibling of
`apps/axiom-soccer-penalty-kick` and `apps/axiom-signal-runner`. There is no
`Cargo.toml`/`app.toml`/`package.json`: this is a pure-TypeScript app over the
shared engine (the `@axiom/game` SDK + the `axiom-game-runtime` wasm core), so it
is not a cargo workspace member and `cargo xtask check-architecture` does not
classify it.

## The game

An arcade basketball-machine game: you stand in front of a procedurally-built
cabinet, **drag a ball with the pointer/touch, swipe up, and release** to toss it
into the hoop. Every visible thing — cabinet, sloped return ramp, side rails,
backboard, a real **torus** rim, the net, the basketballs (orange spheres with
dark seam rings), and a seven-segment scoreboard — is generated procedurally; no
external assets. Press **R** to reset.

## Architecture

The gameplay core imports **nothing** from `@axiom/game`, so the whole game is
constructible in a bare `node --test` process:

- `constants.ts` — every tuning number (gravity, throw/lift/forward scales,
  restitution, damping, cabinet/hoop/trigger dimensions).
- `vec.ts` / `projection.ts` — pure vector + camera math (the SDK's `mat4`
  forwards to the native host, so it can't be used in tests).
- `colliders.ts` / `physics.ts` — the deterministic fixed-step ball simulator
  (sphere vs. static boxes/planes, restitution, friction, damping). The ball is
  genuinely simulated after release, not animated.
- `pointer.ts` — a bounded ring buffer of pointer samples + swipe velocity.
- `throw.ts` — the swipe → 3D launch mapping.
- `selection.ts` — pick the ball under the pointer (screen-space projection).
- `scoring.ts` — the one-way "downward through the hoop" rule.
- `session.ts` — the whole game as a deterministic `advance(Intent)` state
  machine.
- `hud.ts` — the pure HUD model.

Only three files touch the SDK: `scene.ts` (all procedural geometry + camera),
`game.ts` (the `onFixedUpdate` wiring + input), and `harness.ts` (the browser
boot / DOM HUD). `meshgen.ts` builds the torus + seam meshes as plain vertex data
without importing the SDK.

## Run

- **Tests** (no wasm, no DOM):
  `node --test apps/axiom-swipe-basketball/web/src/swipe-basketball.test.ts`
- **Gallery page**: `make gallery-swipe-basketball` builds the SDK + runtime
  wasm, type-checks the app with `tsgo`, and packages a single self-contained
  `apps/axiom-gallery/web/swipe-basketball/index.html`. Browse it with
  `make gallery` at `http://localhost:8000/swipe-basketball/index.html`.
