# The rest of the port — work breakdown and tier assignment

Every remaining source file, what it needs, and which model tier fits.

Tiering rule, from measured cost this session (Haiku ~85–110k tokens/task,
Sonnet ~170–330k, Opus ~220–550k):

- **Haiku** — pure data transcription with an obvious target shape.
- **Sonnet** — pure functions and algorithms verifiable by golden capture. This is
  most of the game.
- **Opus** — only where judgment is load-bearing: branchless spine code under the
  Coverage Law, architectural placement, and the shader/frame-graph work.

## Landed

| slice | source | commits |
|---|---|---|
| deterministic core | `core/{rng,registry,engine,config}.js` | `16fbf5d4` |
| audio (all) | `audio/*.js` — 4,241 lines | `5fc83e9d` |
| weapon spring/noise math | `weapons/mathx.js` | `78f80aa8` |
| ballistics + weapon tables | `weapons/{ballistics,defs}.js` | `cafbdebf` |
| animation clips | `weapons/clips.js` | `5fc83e9d` |
| geometry kit + primitives | `weapons/geometry.js` + Three internals | `5c504d5b`, `e91a5eda`, `6af2a9c3` |
| all 27 part builders | `weapons/parts.js` | `1bd9fbab`, `861e3cdb`, `f074284a`, `34fa8d5e`, `4853125f`, `5d3c789d`, `7acca324` |
| world palette / layout | `world/{palette,layout}.js` | `3e863295`, `29f9dd8c` |
| positional noise + masks | `world/util.js` (partial) | `25dcf85c` |
| 19-surface library data | `materials/library.js` | `e271b214` |
| periodic noise basis | `materials/glsl/noise.js` | `20efdfd4` |
| geometry viewer | — | `d2de35f5` |

## In flight

`weapons/models/*.js` · `player/*` · `ui/*` · `physics/{math,surfaces,bvh}.js` ·
`materials/{generator,masks}.js`

## Remaining

### weapons — Sonnet (materials.js is Haiku-viable)

- **`hands.js`** (50k) — two-bone analytic IK solved *from the hand*, so hands
  cannot slide off the grip. Bone lengths deliberately cheated 10% long (330/300 mm
  vs anatomical 300/272) because at the 300 mm weapon distance a real arm locks
  straight. The pole vector lives in **rig space, not hand space** — hand space
  swings the elbow through the near plane. Chirality by mirroring the right arm.
- **`viewmodel.js`** (45k) — the additive layer stack over one base pose: sway (6
  fbm fields at incommensurate rates + two-sine breathing), bob, **lag** (the gun
  trails the camera on a spring — the detail that makes a viewmodel feel real),
  recoil, clip. **ADS is solved, not authored**: the rig computes the translation
  putting the sight node on the camera axis at the weapon's eye relief, so the optic
  is pixel-centred for any weapon. The ADS blend is a linear rate shaped by
  smootherstep, explicitly *not* a spring.
- **`materials.js`** (56k) — re-parameterises the shared library for hand-held
  scale: texel density from a 2.5 m architectural tile down to 0.10–0.15 m,
  triplanar + local space so texture is nailed to the mesh and nothing swims,
  world-space weathering disabled (it keys off world Y, meaningless for a
  camera-parented object), cavity grime kept. Mostly data.
- **`index.js`** (31k) — subsystem facade: fire-rate gate, spread growth/decay,
  recoil split between camera (the learnable part) and viewmodel (the feel), shell
  ejection timing.

### world — Sonnet (largest remaining area)

- **`builder.js`** (17k) — **the Assembler**, the central abstraction. Five verbs:
  `add` (merge into a per-palette static batch), `proto`/`place` (instancing),
  `box`/`collideGeo` (authored collision proxies, *not* derived from visuals),
  `light`. `finalize` builds one merged mesh per palette key and one instanced mesh
  per prototype per 64 m chunk, with `[wear, grime, ao]` packed per instance. The
  level→world transform is baked into every vertex rather than applied to a parent.
- **`kit.js`** (40k) — the modular building kit, in panel space. `wallPanel` cuts
  real holes via extrude-with-holes (already ported in the primitive kit).
  `windowState` picks boarded/open/shuttered/ajar/curtain/lit/glazed — uniform
  windows are named in-source as the loudest tell of procedural architecture.
- **`buildings.js`** (30k) — facade programme: bays of `round(len/3.05)`, a kit
  element rolled per bay per floor from a probability table, with hand-authored
  overrides where a sightline matters.
- **`props.js`** (38k) — ~60 prototypes, each chamfered boxes merged into one geometry.
- **`dressing.js`** (85k) — the pass that makes it a place. Scatter with exponential
  falloff from walls, occupancy tests, and a guard keeping mid-ground masses out of
  the canonical camera positions.
- **`interiors.js`** (25k) — room plans in normalised 0..1 room coordinates so a plan
  survives a footprint change.
- **`ground.js`** (12k) — road camber, wheel ruts, and a seam function interlocking
  every material boundary with patches of both plus pebbles.
- **`index.js`** (18k) — orchestration. **Delete the light-ballast helpers** — they
  are pure Three shader-permutation workarounds.
- **`util.js`** (rest) — the geometry builders not yet ported.

### materials — Sonnet

- **`glsl/surfaces-arch.js`** (26k) — concrete, brick, plaster, tile.
- **`glsl/surfaces-ground.js`** (17k) — asphalt, sand, dirt, gravel. Note the
  explicit **Nyquist budget**: a term at frequency K lays 8K cells across an N-texel
  bake, and under ~5 texels per cell it bakes as white noise and mips to flat grey,
  so K is capped near 20–24 at 1024².
- **`glsl/surfaces-metal.js`** (14k) — rust, painted, brushed, corrugated. **The rule
  driving every metal: bare metal is 1, and every oxide, paint, dust or grime layer
  on it forces metalness to 0.**
- **`glsl/surfaces-organic.js`** (17k) — wood, fabric, burlap, foliage, rubber, glass.
- **`shader.js`** (40k) — the *runtime* material shader: POM (a bounded loop with a
  linear refine), triplanar (nine fetches), de-tiling with explicit derivatives,
  detail and macro layers, weathering, curvature wear. Per `01-engine-gaps.md` this
  belongs in **hand-written WGSL in `gpu-backend`**, not the field algebra — which
  has no loops, no derivatives, no sampling, and a 256-node budget.
- **`index.js`** (13k) — bake cache keyed on name/size/seed/tints/param.

### physics — Sonnet

`character.js` (18k — swept-capsule controller, 5-plane crease stack, step-up and
stair snap, ground probe, crouch clearance) · `rigidbody.js` (25k — impulse solver,
CCD, sleep) · `ragdoll.js` (29k — PBD, 15-capsule chain, cone and twist limits) ·
`penetration.js` (9k — multi-layer, budget in reference-material metres, backface
thickness probe, yaw deflection) · `index.js` (39k).

### ai — Sonnet

`nav.js` (18k — walkability by ray-sampling, A* over an 8-connected grid with slope
and step penalties, string pulling, cover points) · `agent.js` (38k — 100° perception
cone, LOS, angle/distance-scaled reaction delay, behaviour FSM) · `soldier.js` (29k) ·
`parts.js` (39k) · `rig.js` (10k — procedural 25-bone humanoid in a non-T-pose bind) ·
`animator.js` (20k — layered blend tree, additive layer, one-shots, speed-driven
phase so feet do not skate, IK suite) · `clips.js` · `geo.js` (28k) · `textures.js`
(41k) · `grounding.js` · `squad.js` · `weapon.js` · `index.js` (45k).

### fx — Sonnet

`atlas.js` (42k) · `impacts.js` (37k) · `muzzle.js` (22k) · `index.js` (52k) ·
`decals.js` · `particles.js` · `explosions.js` · `shells.js` · `tracers.js` ·
`lights.js` · `haze.js` · `noise.js` · `util.js`.

### sky — Sonnet

`atmosphere.js` (14k — Bruneton scattering through three LUTs, Hillaire units in
megametres, Cornette–Shanks phase) · `luts.js` (11k — transmittance 256×64 float,
multiscatter 32², sky-view 384×192 with azimuth measured *relative to the sun* and
altitude square-distributed about the horizon) · `dome.js` (17k) · `clouds.js` (19k —
two decks against the planet shell, cumulus parallax for fake vertical extent, cirrus
silhouette kept isotropic so it cannot streak) · `volumetrics.js` (20k) · `stars.js`
(8k) · `celestial.js` · `noise.js` · `fullscreen.js` · `index.js` (41k).

### core — Sonnet

`input.js` (8k) — action map, per-frame snapshot with edge queries valid only within
the frame, pointer-lock mouse look, gamepad with a 0.16 dead zone and a cubic
response curve, movement clamped to the unit disc so diagonals are not faster.

**`prewarm.js` is deleted, not ported** — the engine compiles surface programs at a
preparation barrier, so the whole problem is solved structurally.

### ui — Sonnet

`minimap.js` (21k) — deferred: needs an orthographic depth bake to a render target,
read back once, then a Sobel pass for roof outlines. Blocked on the render work.

### render — Opus / engine

The 18-pass frame graph. Not app work: it lands in `crates/axiom-host` (pass
vocabulary, attachment formats, EV100) and `modules/axiom-gpu-backend`
(realization), per the placement table in `00-manifest.md`, in the dependency order
in `01-engine-gaps.md`.

## Standing rules for every slice

- Golden-capture from the original under Node. Exact equality for integer and
  `+ - * /` results; a **stated, measured** tolerance only where transcendentals are
  involved.
- **A differing vertex or triangle count is a different algorithm, never rounding.**
  That bound does not move. But a *matching* count is not proof of a matching
  algorithm — a weld can trade one merge for another (measured in `7acca324`).
- Preserve every `rng.fork()` and every literal seed, in order. Draw order is part of
  the contract.
- Port source defects faithfully and pin them; do not silently fix. Five found so far.
- Apps are outside the Branchless and Coverage laws — write normal idiomatic Rust.
