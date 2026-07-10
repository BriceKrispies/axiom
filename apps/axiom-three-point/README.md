# Three-Point Shootout

A first-person 3D basketball three-point contest in the spirit of Wii Sports rack
shooting — a **fully self-contained, pure-TypeScript app**. It ships its own
engine under `web/src/engine/` (a WebGL2 forward renderer, a deterministic
fixed-step loop, keyboard/pointer-lock/touch input, and a WebAudio tone synth)
over bare browser APIs: no `@axiom/game` SDK, no wasm, no external packages.
Fifteen shots from three fixed spots around the arc — left wing, top of the key,
right wing — five balls per rack, the fifth one golden.

## The shot is one continuous motion — and the loop never waits

There is no static aim-and-charge, and no downtime between balls: **the moment
you release a shot, the next ball is dealt into your hands** (its pickup
animation plays through the follow-through, the camera glancing toward its
actual rack slot) while every launched ball keeps flying — several can be in the
air at once, each scored independently and applied in shot order.

1. **Auto-pickup** — on release, the next ball lifts off its rack slot into a
   chest hold (each slot's geometry shapes the pose a little); Space is ignored
   until it arrives.
2. **Hold SPACE** — a short chest settle, then the **shot rise**: a normalized
   progress `p` sweeps 0 → 1. The launch's forward speed, vertical speed, and
   release pitch follow keyframed early→ideal→late curves; the ball climbs
   toward the release point; the **reticle is the live launch calculation** (the
   point where this instant's launch would descend through rim height), rising
   toward the hoop.
3. **Release** — letting go of SPACE launches at that exact motion state.
   Early is low and short; the ideal window is aligned and strong; late is hard
   and flat off the glass. Holding past the top auto-releases a poor shot.
4. **Follow-through** — a brief beat while the next ball is already on its way
   to your hands.

**The camera is exclusively yours.** The game never rotates, nudges, eases, or
drifts the view — no pickup glance, no rise tilt, no follow-through motion, no
aim reset. Only your position moves (the glide between the three fixed shooting
spots); orientation is pure mouse, inside soft edge bounds that block rather
than snap. Skill = your horizontal aim (each station's hoop line is different)
plus release timing. No randomness anywhere — identical aim + release progress
always produce the identical shot.

## Controls

Desktop:

- **Click the court** — grab the pointer (Escape releases it).
- **Mouse** — horizontal aim (vertical look is camera-only).
- **Hold SPACE** — rise; **release** to shoot.
- **R** — restart the run.

Touch (the swipe-basketball gesture model — smoothed `PointerHistory` flicks):

- **Drag** anywhere — look (camera stays exclusively yours).
- **Swipe up from the held ball** (lower-center zone) — the whole shot: flick
  strength maps through a deadzone→full band to the release progress (a soft
  flick is short, a full flick overshoots), and the sideways component is a
  bounded launch-yaw steer. Raw screen-Y is never copied into the launch, and a
  shot gesture never moves the camera.

Scoring: a make awards `3 + 3·streak-before` and then increments the streak; a
miss resets it. The golden fifth ball follows the same formula — it is visually
special only. After ball 15 the buzzer shows score, makes/15, best streak, and a
performance label.

## Structure

Two layers, both pure TypeScript:

- **The engine** (`web/src/engine/`) — `api.ts` (the contract), `renderer.ts`
  (WebGL2 forward renderer: box/sphere/cylinder + custom-mesh primitives,
  Lambert materials with emissive/opacity, directional + point lights, look-at
  camera), `loop.ts` (fixed-step 60 Hz accumulator under requestAnimationFrame),
  `input.ts` (keyboard actions, pointer-lock mouse look, canvas pointer
  sampling), `audio.ts` (WebAudio tone synth), `mat4.ts`, `meshes.ts`.
- **The game** — a deterministic, DOM-free core (`constants.ts` — everything
  shot-feel lives in `SHOT_TUNING` — `vec.ts`, `types.ts`, `physics.ts`,
  `gameplay.ts`, `session.ts`, `meshgen.ts`, `pointer.ts`) under one
  renderer-facing `scene.ts`, wired by `game.ts`, booted by `harness.ts`.

The ball is genuinely simulated (semi-implicit Euler, 4 substeps,
sphere-vs-sphere rim ring + sphere-vs-AABB glass/floor with restitution); the
rim colliders are built from the SAME constants as the visible torus. Baskets
are detected by a two-plane downward-crossing detector (upper entry + lower
confirmation, upward crossings disarm, score latches).

Not a cargo workspace member; `cargo xtask check-architecture` does not classify
it. App TS is typecheck-gated (tsgo); tests run separately.

## Run

```sh
node --test apps/axiom-three-point/web/src/three-point.test.ts       # game-core tests
node --test apps/axiom-three-point/web/src/engine/platform.test.ts   # loop + input tests
node --test apps/axiom-three-point/web/src/engine/render.test.ts     # mat4 + mesh tests
node apps/axiom-three-point/web/src/agent.ts                         # headless full-game driver
make gallery-three-point                                             # rebuild the self-hosted gallery page
```

The gallery page (`apps/axiom-gallery/web/three-point/index.html`) is one small
self-contained HTML file (the esbuild-inlined app — no SDK, no wasm) and runs
from `file://`. A `DEBUG_TRAJECTORY` constant in `constants.ts` renders the real
predicted trajectory (same integrator) while holding a shot, for tuning.
