# Home Run!

An arcade baseball **batting** game on a toy-tabletop diamond, authored purely in
TypeScript on the `@axiom/game` SDK. A fixed elevated camera behind home plate
frames a compact striped field — brown base paths, white foul lines, a mechanical
pitcher on the mound, blue stadium walls, and nine red toy fielders wandering
their own patrol circles. Ten pitches per round; the whole game is hitting them
as far as possible.

**A / D (or ←/→)** shift the batter inside the batting box. **Holding SPACE**
winds the spring-loaded bat — quick at first, resisting toward maximum load —
and **releasing SPACE** snaps the committed forward swing (the game never swings
on press). **SPACE or ENTER** restarts once the round is over. On touch, the
on-screen pad mirrors the same three controls.

## The batting model

Contact is resolved from the actual spatial relationship of bat and ball — never
a timing-window roll. Each tick of a forward swing runs a swept segment-vs-ball
test (both the bat sweep and the ball path are subsampled, so neither tunnels).
A touch resolves into:

- **exit speed** — bat angular velocity (load-scaled) × the contact radius along
  the barrel (tip beats handle), shaped by the sweet spot (~76% out), squareness
  of timing, and vertical mishit;
- **spray** — the bat's tangential direction at the contact angle: early pulls,
  late pushes, extremes go foul;
- **loft** — the vertical contact offset: undercut lifts (flies, popups), topping
  drives down (grounders).

Outcomes: MISS · FOUL · WEAK HIT (25) · GROUNDER/POP UP (50) · CLEAN HIT
(100 + distance) · **HOME RUN!** (500 + 2×distance, consecutive homers multiply
up to 4×) — a homer means clearing the blue wall line above wall height in fair
territory. Fielders converge on reachable projected landing points (clamped near
their patrol circles) and catch or field what they can reach.

Pitches come from a deterministic seeded sequence over seven profiles (slow ball,
fastball, heater, sinker, riser, inside, outside) with seeded aim/speed jitter;
the first pitches are easy, hard profiles appear only late. Every pitch is
telegraphed by the machine's compression wind-up and muzzle flash.

## Structure

This is a **pure-TypeScript leaf app** over the shared engine (the heat-check
shape). There is no `Cargo.toml` / `app.toml` / `package.json`; everything lives
under `web/`:

- `web/src/{vec,constants,types,pitch,swing,fielders,ball,session}.ts` — the
  **SDK-free** core. All variation derives from the session seed via a pure
  integer hash (no stateful RNG, no wall-clock), so the whole game is
  constructible and replayable under bare `node --test`.
  `web/src/home-run.test.ts` covers it.
- `web/src/scene.ts` — the ONE file that touches `@axiom/game`, building the 3D
  stadium procedurally and mirroring the session's `view()` into scene nodes.
- `web/src/game.ts` — registers the fixed-update loop, folds input into an
  `Intent`, advances the session, exposes `readHud()` for the DOM overlay, and
  plays the synthesized audio hooks (`playTone`).
- `web/src/harness.ts` — the browser boot edge (wasm init + `boot({ present3d })`
  + the DOM HUD + touch pad). URL affordances: `?seed=N`, `?shot=N` (freeze after
  N ticks), `?auto=1`, `?loadAt=N&swingAt=N` (scripted deterministic swing) —
  used by the screenshot/convergence harness.

## Run

```sh
# Tests (no wasm, no DOM):
node --test apps/axiom-home-run/web/src/home-run.test.ts

# Typecheck + compile the app:
npm --prefix packages/axiom-game exec -- tsgo -p apps/axiom-home-run/web/tsconfig.json

# Build + package the self-hosted gallery page, then browse it:
make gallery-home-run
make gallery            # serves dist/ at http://localhost:8000
# open http://localhost:8000/home-run/index.html
```

The 3D present path needs a GPU; in a headless browser append
`?backend=canvas2d` to use the software backend (the same one the visual
convergence screenshots use).
