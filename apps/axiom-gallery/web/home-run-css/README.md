# Home Run! — pure CSS 3D edition

An experiment: the same arcade batting game as the engine app one directory up,
rebuilt as a **CSS 3D transform "paper theater"** — DOM elements + CSS for the
entire scene and all motion, with the bare minimum of JavaScript for the things
CSS genuinely cannot do. Open `index.html` directly (file://); no build, no
dependencies, three files.

## The mapping (reference → CSS)

Every piece maps from the engine app (`../web/src`):

| Reference | CSS edition |
|---|---|
| `constants.ts` numbers | `:root` custom properties + px literals (1 world unit = 20px, ticks → ms) |
| `Camera3D` (fovY 0.98, pose) | `#viewport { perspective: 525px }` + `#scene` translate / `#floor rotateX` |
| `scene.ts buildGround` decals | absolutely-positioned children of `#floor` (u = (40−x)·20, v = (50−z)·20 — world +X is screen-LEFT, exactly like the engine) |
| `buildStadium` walls/seats | bars anchored at the field corners, rotated to the diagonals, folded upright with `rotateX(-90deg)` (seats lean back at −125°) |
| `buildMachine` / figures | cardboard-cutout "standees" (`rotateX(-72deg)` off the floor) |
| the oversized stepped bat | `#bat-pivot` — swing.ts as ONE floor-plane `rotate()` |
| swing.ts state machine | `@keyframes bat-strike` (190ms snap) → `.rewinding` transition (950ms) — the always-armed press-to-swing model |
| pitch flight kinematics | a linear `transform` transition over `--pitch-ms` = (9.7−(−2.2))/speed, height on an animated `@property --bh` |
| hit flight + bounces | `@keyframes hit-arc` (parabola + bounces), landing point/duration as custom properties |
| fielders.ts wander | per-fielder infinite `alternate` animations with seeded durations/phases |
| fielder interception | `.f-chase` transition toward the landing point, clamped to the patrol radius |
| session.ts round loop | ~100 lines of event-driven JS (class/var flips on setTimeout/animation boundaries) |
| `pitch.ts` hash01/selectPitch/isStrike | **verbatim JS ports** — the same seed deals the same ten pitches |
| `ball.ts scoreFor` | verbatim port (25/50/100+dist/500+2·dist, consecutive-homer ×streak capped at 4) |
| swing.ts swept contact | a timing/position mapping: error `e` = (press+130ms) − plate arrival ↔ θ vs sweet angle; `p` = |batterX − (targetX + 0.88)| ↔ contact radius vs sweet spot; the ms windows are the engine's tick windows |
| HUD / overlays / confetti | the engine `index.html` markup + CSS, near-verbatim |

## What JavaScript does (and nothing else)

1. Input events (keyboard + pointer) — CSS cannot observe input.
2. The seeded pitch sequence + the contact/outcome decision — data + arithmetic.
3. Flipping classes and custom properties at discrete moments (fire, hit, result)
   and writing HUD text — CSS cannot set text or schedule game states.
4. Cloning ten fielder nodes from the spot table — CSS cannot loop over data.

Everything you *see move* — the bat swing and self re-wind, the pitch, the hit
arc and bounces, the machine wind-up squash and muzzle flash, the camera dolly
and shake, the fielder wander and chases, the ready meter, the confetti — is a
CSS animation or transition.

## Controls & dev affordances

Identical to the engine app: A/D step, SPACE swings (always full power),
SPACE/ENTER restarts after pitch 10. URL: `?seed=N` (round seed), `?auto=1`
(self-start), `?swingAfter=MS` (one scripted swing after the first pitch fires),
`?static=1` (pause all animation for deterministic screenshots).

## Known deltas from the engine

- Timing rides the wall clock (CSS animations), not a fixed-step sim — the
  pitch SEQUENCE is deterministic per seed, but replay is not tick-exact.
- The contact model is the documented mapping above, not the swept segment test;
  vertical offset (loft from pitch height) is folded into the outcome tiers.
- No audio (WebAudio would be more JS, not less).
