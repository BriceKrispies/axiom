# Home Run!

An arcade baseball **batting** game on a toy-tabletop diamond, authored purely in
TypeScript on the `@axiom/game` SDK. A fixed elevated camera behind home plate
frames a compact striped field — brown base paths, white foul lines, a mechanical
pitcher on the mound, blue stadium walls, and nine red toy fielders wandering
their own patrol circles. Ten pitches per round; the whole game is hitting them
as far as possible.

**A / D (or ←/→)** shift the batter inside the batting box. The batter idles
**wound at full power**: **pressing SPACE** fires the max-power swing instantly,
then the bat follows through and **re-winds on its own** — that re-wind is the
swing cooldown, shown by a small ready meter that fades once he's armed again.
**SPACE or ENTER** restarts once the round is over. On touch, the on-screen pad
mirrors the same controls (tap SWING).

## The batting model

Contact is resolved from the actual spatial relationship of bat and ball — never
a timing-window roll. Each tick of a forward swing runs a swept segment-vs-ball
test (both the bat sweep and the ball path are subsampled, so neither tunnels).
A touch resolves into:

- **exit speed** — bat angular velocity (always full power) × the contact radius
  along the barrel (tip beats handle), shaped by the sweet spot (~76% out),
  squareness of timing, and vertical mishit;
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

## Base running

A fair ball that ISN'T caught on the fly puts the batter on base and advances
everyone already aboard. Only a ball caught in the air is an out (leave the
batter, re-bat as normal). Bases earned scale with how far the ball got: a homer
clears the yard (4), a deep drive is a **triple**, a gapper a **double**, and any
other fair ball that drops or is fielded off a bounce is a **single** — the runner
beats it out. Runners are **persistent** across pitches within a round, all
advancing by the same base count on each hit (so nobody passes anyone), and a
runner who reaches home scores a **run**. The HUD tracks **BASES** (cumulative
advance) and **RUNS HOME** (runs driven in); the round-over card totals both.

The runners — and the batter and fielders — are the **rigged player figure**
ported from the sibling `apps/end-zone` (`axiom-figure` skeleton): a 17-box jointed
body driven by a `JointPose`. Runners use the end-zone **distance-driven,
planted-foot running gait** (two-bone leg IK, world-locked stance feet — no
skating). Because a base runner travels a KNOWN deterministic path, that gait is
ported **stateless** here (the anti-skate lock is reconstructed closed-form from
distance travelled), so `view.ts` stays a pure function. The batter uses a batting
stance (the bat still swings from the hands); fielders an idle athletic stance.

## Structure

This is a **pure-TypeScript leaf app** over the shared engine (the heat-check
shape). There is no `Cargo.toml` / `app.toml` / `package.json`; everything lives
under `web/`:

- `web/src/{vec,constants,types,pitch,swing,fielders,ball,bases,session}.ts` — the
  **SDK-free** core. All variation derives from the session seed via a pure
  integer hash (no stateful RNG, no wall-clock), so the whole game is
  constructible and replayable under bare `node --test`. `bases.ts` is the
  base-running model. `web/src/{home-run,base-running,figure}.test.ts` cover it.
- `web/src/{figure-math,figure,figure-pose}.ts` — the rigged player figure ported
  from `apps/end-zone`: quat/TRS math, the 17-box skeleton + rig + two-bone leg IK,
  and the pose builders (stateless running gait, batting stance, fielder idle).
- `web/src/view.ts` — the ONE pure presentation file, building the 3D stadium and
  the rigged batter / fielders / runners from the session's `view()` snapshot.
- `web/src/game.ts` — registers the fixed-update loop, folds input into an
  `Intent`, advances the session, exposes `readHud()` for the DOM overlay, and
  plays the synthesized audio hooks (`playTone`).
- `web/src/harness.ts` — the browser boot edge (wasm init + `boot({ present3d })`
  + the DOM HUD + touch pad). URL affordances: `?seed=N`, `?shot=N` (freeze after
  N ticks), `?auto=1`, `?swingAt=N` (scripted deterministic full-power swing) —
  used by the screenshot/convergence harness.

## Run

```sh
# Tests (no wasm, no DOM):
node --test apps/axiom-home-run/web/src/home-run.test.ts

# Typecheck + compile the app:
npm --prefix packages/axiom-game exec -- tsgo -p apps/axiom-home-run/web/tsconfig.json

# Package the gallery and browse it (this app registers via its app.json):
make gallery            # builds every registered app, serves dist/ at http://localhost:8000
# open http://localhost:8000/home-run/index.html
```

The packaged page is bundled against the SHARED `@axiom/web-engine` build at
`dist/engine/web-engine/<version>/` rather than carrying its own copy, so there is
no per-app packaging step and nothing generated to commit.

The 3D present path needs a GPU; in a headless browser append
`?backend=canvas2d` to use the software backend (the same one the visual
convergence screenshots use).
