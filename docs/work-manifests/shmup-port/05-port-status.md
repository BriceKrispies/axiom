# Port status — the living record

Supersedes the "Landed / In flight / Remaining" tables in `04-remaining-work.md`
(its per-slice algorithmic notes remain current and are still worth reading before
starting any slice). Update this file as slices land.

Branch `port/shmup`, worktree `.claude/worktrees/shmup`, cut from
`origin/main` @ `9b43ae5e`.

## Engine changes (crates/ + modules/)

| commit | what |
|---|---|
| `843577af` | `RenderCapability::HdrTargets` + `HostAttachmentFormat` (RGBA16F/RG16F/R32F/RGBA32F/Depth32F), declared Canvas2D degradation, honest adapter-driven grant |
| `3eb2bd67` | `axiom-math`: `Segment`, `Capsule`, `Triangle`, `Obb`, `Hit`, closest-point solves, Möller–Trumbore, capsule/OBB raycast, swept capsule↔triangle / capsule↔capsule / sphere↔triangle. 100% covered, zero branches |
| `b6b5758f` | `axiom-windowing` carries authored surfaces end to end; `App::surfaces`; surface programs compile at the preparation barrier |
| `75dbc8ff` | `axiom-physics`: capsule↔sphere/capsule/plane/box + box↔box contacts, capsule overlap, capsule cast, `PhysicsHit` with point/normal/distance/front_face, `raycast_all`. Fixed a live out-of-bounds panic by sizing every dispatch table from `PhysicsShapeKind::COUNT` |
| `d2de35f5` | `App::install(FnOnce(&mut RunningApp))` in `modules/axiom` — author geometry on the normal app path |

## Game (apps/shmup)

| area | source | commits | state |
|---|---|---|---|
| core | `core/{rng,registry,engine,config}.js` | `16fbf5d4` | done — RNG bit-exact vs the original |
| input | `core/input.js` | `f9c66d7f` | done |
| audio | all of `audio/` (4,241 lines) | `5fc83e9d` | done — 47 voice graphs at **zero ULP** |
| weapons: math | `mathx.js` | `78f80aa8` | done |
| weapons: data | `defs.js`, `ballistics.js` | `cafbdebf` | done |
| weapons: clips | `clips.js` | `5fc83e9d` | done |
| weapons: geometry | `geometry.js` + Three internals | `5c504d5b`, `e91a5eda`, `6af2a9c3` | done — 14 primitives, RoundedBox/Lathe/Sphere/Torus/Extrude-with-bevel-and-holes, earcut |
| weapons: parts | all 27 builders in `parts.js` | `1bd9fbab`, `861e3cdb`, `f074284a`, `34fa8d5e`, `4853125f`, `5d3c789d`, `7acca324` | done |
| weapons: models | `models/{rifle,smg,pistol}.js` | `7fb1fde5` | done — exact triangle counts vs the JS build |
| world: data | `palette.js`, `layout.js` | `3e863295`, `29f9dd8c` | done — 46 palette entries, 20 buildings |
| world: noise/masks | `util.js` (partial) | `25dcf85c` | done |
| world: assembler | `builder.js`, `ground.js`, rest of `util.js` | `3240e1bd` | done |
| world: kit | `kit.js` | `7b7c1067` | done |
| world: props | `props.js` (~60 prototypes) | `dd7f651d` | done |
| world: buildings | `buildings.js` | `5a0b602c` | done — found the setback-shifted weathering-seed bug |
| materials: data | `library.js` (19 surfaces) | `e271b214` | done |
| materials: noise | `glsl/noise.js` | `20efdfd4` | done — periodicity proven, not assumed |
| materials: bake | `generator.js`, `masks.js` | `c2f3fbb5` | done — CPU bake; a GPU path is noted as future work |
| materials: surfaces | `glsl/surfaces-{arch,ground,metal,organic}.js` | `ef7633c7`, `2856a5f7`, `78e06ea3`, `b893880d` | done — all 19 generators |
| physics | `math.js`, `surfaces.js`, `bvh.js` | `eab2182f` | done — binned-SAH BVH, capsule sweeps |
| physics: character | `character.js` | `f9c66d7f` | done |
| player | `springs.js`, `tuning.js`, `mantle.js`, `movement.js`, `camera.js` | `765bf1fc` | done |
| ui | `style.js` + 11 components | `7f8779bf` | done — `minimap.js` deferred |
| sky | `atmosphere.js`, `luts.js`, `noise.js`, `celestial.js` | `f70406c5` | partial |
| fx | `particles`, `atlas`, `decals`, `impacts`, `muzzle`, `shells`, `tracers`, `explosions`, `haze`, `lights`, `noise`, `util` | `48e42856` | partial |
| scene | — | `f9c66d7f`, `7f8e3689` | walkable street, buildings, collision, <150 draws |
| test tooling | — | `c5eeb3cc`, `80c9e946` | shared triangle-soup comparator (weld-invariant) |

## Remaining, in priority order

### 1. Runtime material shader — the biggest visual gap
`materials/shader.js` (40k). All 19 generators are ported and produce real
albedo/roughness/metalness/normal data, and **nothing samples it**. Needs
hand-written WGSL in `modules/axiom-gpu-backend`: POM (a bounded loop with a linear
refine), triplanar (nine fetches), de-tiling with explicit derivatives, the detail
and macro layers, the weathering stack, curvature wear. Per `01-engine-gaps.md` this
cannot live in the field algebra — no loops, no derivatives, no sampling, 256-node
budget. Plus `materials/index.js` (13k) for the bake/material caches.

### 2. Render frame graph — the other half of the visual gap
The 18 passes in `00-manifest.md`. Engine-side, in `crates/axiom-host` (pass
vocabulary, attachment formats, EV100) and `modules/axiom-gpu-backend`
(realization). Branchless, 100% covered. The dependency order is in
`01-engine-gaps.md`. **This is the last genuinely hard piece of engine work.**

### 3. Weapons: the held weapon
`viewmodel.js` (45k) — the additive layer stack, the solved ADS translation, the
spring lag. `hands.js` (50k) — two-bone IK solved from the hand, pole vector in rig
space. `materials.js` (56k) — hand-held-scale re-parameterisation, mostly data.
`index.js` (31k) — the firing facade. Until these land the rifle lies on the ground.

### 4. World: making it a place
`dressing.js` (85k) — market stalls, wrecks, palms, laundry, rubble, tyre stacks,
and the scatter passes with their exponential wall falloff and camera-clearance
guard. `interiors.js` (25k) — room plans in normalised coordinates. `index.js` (18k)
— orchestration; **delete the light-ballast helpers**, they are Three
shader-permutation workarounds with no analogue here.

### 5. AI
`nav.js`, `agent.js`, `soldier.js`, `parts.js`, `rig.js`, `animator.js`, `clips.js`,
`geo.js`, `textures.js`, `grounding.js`, `squad.js`, `weapon.js`, `index.js`.

### 6. Physics: the rest
`rigidbody.js` (25k), `ragdoll.js` (29k, PBD), `penetration.js` (9k, multi-layer),
`index.js` (39k).

### 7. Sky and FX: the rest
Sky: `dome.js`, `clouds.js`, `volumetrics.js`, `stars.js`, `fullscreen.js`,
`index.js`. FX: `index.js`, and `ambience.js` on the audio side.

### 8. Deferred, blocked on the render work
`ui/minimap.js` — needs an orthographic depth bake read back once, then a Sobel
pass for roof outlines.

### Not being ported, deliberately
- `core/prewarm.js` — the engine compiles surface programs at a preparation
  barrier, so the entire problem is solved structurally.
- `world/index.js`'s light-ballast helpers — pure Three shader-permutation
  workarounds.

## Known residuals (measured, documented, not swept under a tolerance)

- `picatinny_normal` 1.013e-6, `mlok_slot_normal` 1.34e-6, `wall_panel_arch_hole`
  2.68e-6 — genuine one-ULP Rust-vs-V8 `sin`/`cos` differences amplified at
  `round_rect`'s arc-to-tangent corners.
- `carbine_stock.polymer` — position and normal exact; a uv axis-tie in `extrude`'s
  projection, up to 0.084.
- `slide.steel` — normal deviation of exactly 1.0 on 8 of 2604 triangles, traced to
  **degenerate zero-normal triangles in the original JS mesh**.
- `crate_a` uv — `chamfer_box`'s bevel-edge axis pick sits on an exact float tie;
  inherent to its `f32` parameter contract. Widening that contract to `f64` would
  close it.
- `wall_panel` / `facade_wall` — triangle count only, because the ported `extrude()`
  welds vertices and the raw JS does not.
- A two-arch-hole-plus-jag earcut corner case in the shared `wall_panel`/`extrude`
  path.

## Environment gotchas that cost real time

- **The coverage gate cannot pass in this repo.** Three stacked causes: the default
  gnu toolchain has no `profiler_builtins`; MSVC full-workspace linking OOMs
  (`link.exe 0xc0000142`); and two app test suites abort the run before it measures
  — `axiom-end-zone --test attempt_loop` fails 4/9 **on `origin/main`, untouched by
  this branch**, and `axiom-burnt-rubber --test agent_golden` passes uninstrumented
  but fails under instrumentation (a 61-second test, several times slower when
  instrumented). `apps/` is outside the coverage *report* yet its tests still gate
  it.
- Long **backgrounded** commands get killed unpredictably here; foreground runs
  survive but the tool ceiling is 600 s.
- The shell cwd resets between calls — `cd` explicitly in every command that touches
  this worktree.
- Piping a gate to `tail` discards its exit status. Redirect to a file.
