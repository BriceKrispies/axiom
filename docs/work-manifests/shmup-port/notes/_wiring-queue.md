# Wiring queue — orchestrator only

Agents do not touch `mod.rs`/`lib.rs`/`Cargo.toml`; they report the lines and the
orchestrator applies them. This is the running list. Delete a row once it is
applied AND `cargo check -p axiom-shmup` is green with it in.

Wiring is applied per **coherent group**, not per slice: adding one `pub mod`
whose siblings are still unwritten just produces cascading unresolved-import
noise that says nothing about the slice that landed.

## apps/shmup/src/ai/mod.rs

| line | slice | reported |
|---|---|---|
| `pub mod weapon;` | `ai/weapon.js` | yes — complete |
| `pub mod parts;` | `ai/parts.js` | pending |
| `pub mod soldier;` | `ai/soldier.js` | yes — complete |
| `pub mod geo;` | `ai/geo.js` | pending |
| `pub mod rig;` `pub mod clips;` `pub mod animator;` | `ai/{rig,clips,animator}.js` | pending |
| `pub mod textures;` | `ai/textures.js` | pending |

## apps/shmup/src/physics/mod.rs

| line | slice | reported |
|---|---|---|
| `pub mod debug;` | `physics/debug.js` | pending |
| `pub mod ragdoll;` | `physics/ragdoll.js` | pending |

## apps/shmup/src/world/mod.rs

| line | slice | reported |
|---|---|---|
| `pub mod dressing;` | `world/dressing.js` | pending |

## apps/shmup/src/{ui,player,materials,weapons}/mod.rs

| line | slice | reported |
|---|---|---|
| `ui: pub mod system;` | `ui/index.js` | pending |
| `player: pub mod system; pub mod health; pub mod lowhealth;` | `player/{index,health,lowhealth}.js` | pending |
| `materials: pub mod system;` | `materials/index.js` | pending |
| `weapons: pub mod materials;` | `weapons/materials.js` | pending |

## Cross-slice findings raised by one agent about another's file

- `weapons/rig_math.rs:110-120` — `V3::apply_quat` groups Three's
  `applyQuaternion` as `vx + qw*tx + (qy*tz - qz*ty)`; Three r180 evaluates
  left-to-right. Non-associativity trap, sits under the two-bone IK. Raised by
  the `ai/weapon.js` agent, handed to the `weapons/hands.rs` agent (owns the
  file this wave).

## Corrections to the brief, found by agents

- `ai/weapon.js` is **not** an FX/event facade — no events, no ballistics, no
  ammo, no `fx/`, no `weapons/`, no `EventBus`. It is one pure geometry builder,
  `buildWeapon(nz, style, rng)`, importing only `./geo.js` plus two constants
  from `./rig.js`. My brief told the agent to compose `fx/` and `weapons/`; it
  correctly refused, on the grounds that doing so would invent a seam the source
  does not have. Do not re-introduce that instruction.

## Slices complete (golden captured, awaiting wiring + check)

| slice | golden | note |
|---|---|---|
| `ai/weapon.js` | 2.66 MB, byte-reproducible | pure geometry builder, not a facade |
| `ai/soldier.js` | 303 KB, byte-reproducible | recipe fingerprint + RNG state after each build |
| `sky/volumetrics.js` | 48 KB, byte-reproducible | no wiring needed; `sky/mod.rs` already declares it |

## Doc corrections owed (orchestrator applies after the owning agent finishes)

- `apps/shmup/src/sky/mod.rs` lines 28-34 now state a falsehood — they claim
  `skRayFor` and `skSunVisibility` are unported. Both are ported. Replacement
  paragraph is in §6 of `notes/sky-volumetrics.md`. Held until the
  `sky/dome`+`clouds` agent finishes, since it may also revise that doc.

## Findings that change how the remaining slices must be verified

- **Dead transcription is real, and it shared the port's bugs.**
  `tests/sky/capture.mjs` hand-transcribes every volumetrics shader body and
  then calls none of them: no volumetrics key in `golden.json`, no volumetrics
  assertion in `sky_port.rs`. That slice read as covered and had zero coverage.
  Worse, the dead transcription contained *the same two bugs* as the Rust it was
  meant to check (`scale(1.0/sigma_e)` where the GLSL divides, inside a 56-step
  accumulation; `expr * ratio * mono` folded into one multiply) — because the
  same reading produced both sides.

  So the "second independent transcription" method is weaker than
  `sky_port.rs`'s doc comment implies whenever one agent writes both sides.
  Mitigation, now briefed to every GLSL slice: transcribe from the shader source
  text alone, never from the existing Rust, and look specifically for divisions
  turned into reciprocal-multiplies and re-associated multiply chains.

- **"Deliberately not ported" was mostly unfinished work wearing a
  justification.** Of volumetrics' four such claims, one was real
  (render-target object lifetime) and three were plain maths with a single GPU
  input each. Two more functions were missing and not mentioned at all. Apply
  that prior to every remaining audit.

## Orchestrator work done during the fan-out

### `crate::jsmath` — the JS builtin primitives, consolidated (DONE, wired)

`apps/shmup/src/jsmath.rs` + `tests/jsmath_port.rs` + `tests/jsmath/{capture.mjs,golden.json}`.
`lib.rs` now declares `pub mod jsmath;`.

Trigger: the `physics/debug.js` agent measured V8's `Math.hypot` and found it
Kahan-compensated. Investigating that turned up **six** independent `hypot3`
implementations in the crate using **three different algorithms**, and **nine**
independent three-valued `sign`s:

- correct, derived independently three times: `ai/nav.rs`, `ai/parts.rs`, `physics/debug.rs`
- uncompensated max-scaled (wrong): `physics/rigidbody.rs`
- plain root (wrong): `audio/spatial.rs`, and `ai/geo.rs` — **which cited
  `audio::spatial`'s "within a couple of ULP" comment as its justification.**

A wrong implementation propagated by citation, because the excuse read as
authority. Re-measured independently in `tests/jsmath/capture.mjs`: over 4,096
sampled metre-scale triples the plain root is bit-wrong on **1,538 (37.5%)** and
the uncompensated form on **191 (4.7%)**.

Migrated so far (the files no agent owned): `physics/rigidbody.rs`,
`physics/debug.rs`, `audio/spatial.rs`. `rigidbody.js:618` renormalises its
quaternion every step and feeds the world inertia tensor, so that one was
compounding from first contact onward.

**Still to migrate at integration** (agent-owned while the fan-out runs):
`ai/nav.rs`, `ai/parts.rs`, `ai/geo.rs`, `ai/agent.rs`, `ai/animator.rs`,
`ai/soldier.rs`, `audio/dsp.rs`, `input.rs`, `world/dressing/occupancy.rs`,
`world/dressing/street_floor.rs`, `world/props/mesh.rs`, `world/props/cover.rs`,
`materials/surfaces/metal.rs`, `sky/atmosphere.rs`,
`weapons/geometry/primitives/{earcut,rounded_box}.rs`. Each keeps its local name
as an alias so call-site transcription stays diffable against the source.

Note `weapons/geometry/primitives/earcut.rs`'s `sign` returns `i32`, not `f64` —
check it against the source before folding it in; it may be a genuinely
different function.

### Known build break, to fix at integration

`ai/agent.rs` does not compile — 10 errors, all one root cause. `AgentCtx<'a>`
holds `path: Option<&'a mut dyn PathSource>`; `w.path.as_deref_mut()` reborrows
at the trait object's own `'a`, and `&mut` is invariant over its parameter, so
every later `&mut w` in `update`/`think` conflicts. Sites: `agent.rs:861, 865,
866, 867, 868, 1003, 1113, 1144, 1249, 1431`.

Deliberately **not** relayed to the agent that owns the file: it is mid-flight
on the golden capture, which is the part only it has the JavaScript context to
do. Borrow-checker plumbing needs no such context and is exactly what the
integration pass is for. Fix by decoupling the trait object's lifetime from the
outer borrow so the reborrow can be shorter than `'a`.

**Consequence:** `cargo test -p axiom-shmup` cannot run at all until this is
fixed, so `jsmath_port.rs` is written but unverified. It is the first thing to
run once the crate builds.


---

# Running tally (orchestrator, live)

## Complete

| slice | golden | wiring needed |
|---|---|---|
| `ai/weapon.js` | 2.66 MB | `ai/mod.rs: pub mod weapon;` |
| `ai/soldier.js` | 303 KB | `ai/mod.rs: pub mod soldier;` |
| `ai/textures.js` | 485 KB | `ai/mod.rs: pub mod textures;` |
| `ai/nav.js` + `squad.js` | 37 KB | none (folded its hypot into `jsmath` itself) |
| `physics/debug.js` | 367 KB | `physics/mod.rs: pub mod debug;` |
| `physics/ragdoll.js` | 1.86 MB | `physics/mod.rs: pub mod ragdoll;` |
| `sky/volumetrics.js` | 48 KB | none |
| `sky/dome.js` + `clouds.js` | additive to `tests/sky/` | none |
| `weapons/viewmodel.js` | 1.44 MB | none |

## The `Math.hypot` defect, measured five times independently

Five agents measured V8-vs-naive disagreement on their own data, without
coordination: **37.5%** (jsmath, metre-scale triples), **36%** (physics/debug,
2M triples), **25%** (ai/textures, Sobel-shaped triples), **41%**
(physics/ragdoll, 500k triples), **38%** (weapons/viewmodel, 200k reticle
pairs). The spread is just input distribution. It is not marginal anywhere.

`ai/nav.rs` found its own instance and folded it into `crate::jsmath`
unprompted, which is the consolidation working as intended.

## `Math.round` — the same shape, six copies

Rediscovered independently by `ai/geo`, `audio/foley`, `materials/masks`,
`materials/system`, `physics/ragdoll`, `sky/volumetrics`. Ties break toward
`+Infinity` in JS and away from zero in Rust, so they differ on every negative
half-integer, and `Math.round(-0.5)` is `-0`. Now `jsmath::round`, pinned over
614 inputs with the halves enumerated exhaustively rather than sampled.

It decides real structure, not just presentation: in `physics/ragdoll.js` the
rounded value decides whether two bone endpoints merge into one particle, so the
tie rule changes the doll's *topology*.

**Not yet in the trap list in `02-port-recipe.md` / `06-parallel-port-plan.md`.**
It should be. Add it when the fan-out settles.

## Source defects found and pinned (not fixed)

- `physics/ragdoll.js` — `humanoidSpec`'s coordinates contradict the file's own
  header. Each arm root and thigh root is offset from its parent's endpoint, so
  they never share a particle: the 15-bone humanoid is **20 particles in five
  disconnected islands**, coupled only by a direction-only cone constraint. A
  doll dropped 35 cm settles 10 cm tall and **4.08 m wide**. Pinned with a
  union-find test. Fixing it is an art decision and would invalidate every
  captured trajectory.
- `weapons/viewmodel.js` — `boltHold` is never raised, so `boltOff` reduces to
  `stroke` and the reload clips' authored bolt/slide tracks are multiplied away
  and never reach a mesh. `selectorLive` is declared nowhere, so the selector
  lever is pinned at 0 forever.
- `ai/textures.js` — urban camo's stated budget is 0.083 but the bake lands at
  0.0956, via `apply_budget`'s bottom clamp.
- `ai/nav.js` — `build()`'s crouch-only (`flags = 2`) and blocked-ceiling arms
  are unreachable in any world whose bounds contain its geometry. A test
  scenario deliberately under-covers its bounds to reach them, and names them so
  nobody deletes them as dead code. Separately, a grid-aligned wall generates
  **no cover at all** (0.8 m from the nearest cell centre vs a 0.42 m shoulder
  probe) — relevant when the real level generator is wired in.

## Deferred re-captures

- `weapons/viewmodel` — once `materials.bakeMasks` is wired, `addWeapon` will
  draw from the viewmodel's forked RNG at construction and shift `addRecoil`'s
  jitter. That golden must be re-captured at that point.

## Integration risk flagged by an agent

`physics/ragdoll`'s four world-driven scenarios call `bvh::overlap_capsule`
every iteration at a `1e-7` tolerance — a stronger demand than any prior test
placed on the BVH port. `free_fall_no_world` uses no world at all, so **if that
one passes while the others fail, the fault is the contact arm, not ragdoll.**


---

# Integration log (orchestrator)

## Verified green so far

| suite | result |
|---|---|
| `jsmath_port` | 7/7 |
| `ai_nav_port` | 17/17 |
| `ai_agent_port` | 19/19 (after agent triage) |
| `sky_port` | 37/37 |
| `sky_volumetrics_port` | 26/26 |
| `weapons_viewmodel_port` | 12/12 (after agent triage) |

The whole sky subsystem had **zero** real coverage before this wave.

## `jsmath` consolidation — round 2

`materials/system.rs` reported a third naive `floor(x + 0.5)` in
`materials/masks.rs`. That is exactly the double-rounding bug the jsmath golden
caught in *my own* `round` on its first run: for `x = 0.49999999999999994`
adding `0.5` rounds to exactly `1.0`, so `floor` gives `1` where `Math.round`
gives `+0`. Audited all six copies:

| file | form | verdict |
|---|---|---|
| `audio/foley.rs` | `(x+0.5).floor()` | **had the bug** |
| `materials/masks.rs` | `(x+0.5).floor()` | **had the bug** |
| `materials/system.rs` | spec-correct | independently converged, still a duplicate |
| `physics/ragdoll.rs` | `floor`+`>= 0.5` | correct, but loses `-0` |
| `ai/geo.rs`, `sky/volumetrics.rs` | no local `fn js_round` | n/a |

All four collapsed onto `crate::jsmath::round` behind a `use … as js_round`
alias, so call-site transcription stays diffable against the source.

Two of those files are *already-landed slices with passing goldens*. Their
goldens pass either because they never hit the pathological input or because
they hit it and agreed by luck — which is the argument for the primitive living
in one place rather than in every subsystem that happens to need it.

**Not yet re-verified**: `weapons/hands.rs` and `ui/system.rs` are both red with
live agents in them, so the crate does not build and these four migrations are
unconfirmed. Re-run `audio_port`, `materials_*`, `physics_ragdoll_port` the
moment it is green.

## Triage quality worth recording

Two agents found that **the test/harness, not the port, was the wrong side** —
the case the recipe warns about and the reason it forbids hand-written
assertions:

- `ai/agent` — three failures, and only one was the port. `JSON.stringify(NaN)`
  is `null`, so a non-finite guard case round-tripped as a null (golden wrong);
  and the capture's `addCollider` stub was **fabricating seven colliders** onto
  an agent the real constructor gives none (harness wrong). Textbook "your
  comparator can be the bug". Had it made those pass by adjusting the Rust it
  would have written a real defect into the engine.
- `weapons/viewmodel` — the frame-0 opacity mismatch had **both sides wrong**.
  The port assumed `mats.reticle(colour, 0.95)`'s second argument was opacity;
  it is an *intensity* that multiplies the colour, with `opacity: 1` flat. The
  golden's stub returned bare materials defaulting to opacity 1, so it was wrong
  too — "two wrong sides that happened to disagree is the only reason it
  surfaced." Fixed structurally by instantiating the real `WeaponMaterials` in
  the capture, so no hand-written material stub remains.

Also: `ai/agent` deliberately did **not** convert `Vector3.distanceTo`/`length`
to `jsmath::hypot3` — those are `sqrt(x²+y²+z²)`, genuinely a different
function. Converting them would be the trap run backwards.

## Outstanding integration work

1. **`ai/parts.rs` contains a second ~600-line private copy of `ai/geo`** (an
   inline `mod geo` with `Noise`/`Mesh`/`loft`/`box_round`), written because the
   plan claimed geo was already ported. Collapse to `use crate::ai::geo::{…}`;
   note it names the types `Mat4`/`Quat` where `geo`/`weapon` use `M4`/`Q`.
2. **Forked event-payload vocabulary** — 7 event names with two incompatible
   payload structs across `audio::system` and `ui::system`; `EventBus`
   downcasts to one type, so only one subsystem sees any given emit. Needs one
   canonical home (a crate-level `event_payloads` module) before more facades
   fork it further. Table in `notes/ui-system.md` §5.2.
3. **`ui/mod.rs` holds a second, buggy port of `ui/index.js`** (`Hud`), superseded
   by `UiCore`. Not a delete: `scene/game.rs:57` drives the live browser scene
   from `Hud`/`HudFrame`/`CameraBasis`/`FramePull`/`PlayerPull`. Migrate as its
   own step, with the app served and screenshotted — it changes what renders.
4. **`materials/surfaces/metal.rs::hex_to_linear_tint`** uses the GLSL `ow_srgb`
   where its real call site (`index.js:145`) is `new THREE.Color(...)`, i.e.
   three's `SRGBToLinear`. Algebraically equal, **254/256 byte values differ**.
5. Seam fixes named by agents: `soldier.rs:647` `Noise::new(rng.fork())` needs
   `&mut rng.fork()`; `GRIP_R`/`GRIP_L`/`BORE_DIR` are `LazyLock`, so four
   by-value sites need `*`; `impl CharacterRig for Rig`; `impl AgentAnimator for
   Animator`; `impl FootSource for Animator`; `Agent::new` now returns
   `(Agent, Rng)` and takes a trailing `has_physics: bool`.
6. Stale doc paragraphs now that the animator landed: `ai/mod.rs`, `agent.rs`,
   `grounding.rs`.
7. `materials/mod.rs` gaps blocking a faithful facade: `ThreeOptions` has no
   `transparent`; `BakeParams::param` is `[f32;4]` not `Option`, so "declared
   all-zero" and "not declared" are indistinguishable; `MatParams`/`BakeParams`
   store `f32` where the source is `f64`.

---

# Material shader — orchestrator log

## Foundation landed (mine)

| stage | what | state |
|---|---|---|
| 1 | `SurfaceIn` gains `world_pos`/`world_normal`/`view_dir` | done, 251/251 incl. GPU parity |
| 2 | Group 0 gains bindings 4/5/6: ORM+height, detail, macro | done |
| 3 | `MaterialParams` → the 32-slot block, slot map pinned | done, 10/10 |

Stage 1 cost no new vertex outputs — the fragment stage already interpolates
`world_pos` for the fog and specular terms and already has `lights.camera`.
Three construction sites needed updating (`scene_wgsl`, `emit_vertex`'s
`DISPLACE_ENTRY`, and the parity harness's own shader); the vertex stage passes
zero for all three world lanes, which is honest rather than a placeholder — a
displacement program has no fragment, no camera ray and no interpolated normal.

Stage 2's neutral 1x1 defaults are chosen so each term is an *identity*, not a
zero: occlusion 1, metalness 0, roughness 1 (the unscaled value the parameter
remap then applies), height 0, detail flat-normal + zero height, and **macro
mid-grey** — macro is a variation *around a midpoint*, so zero would darken every
surface by the full macro amplitude. All three upload as `Rgba8Unorm`: an ORM
triple, a tangent-space normal and a noise field are measurements, not colours,
and binding them sRGB is exactly G16 in `01-engine-gaps.md`.

## Cross-cutting items raised by the layer agents

1. **Extract a shared GPU parity harness.** `surface_program::parity`'s is
   `pub(super)`, so every layer is writing its own ~200-line
   adapter/render/readback. Two agents (`frames`, `uv_mode`) independently asked
   for `material_shader/parity_gpu.rs`. With twelve layers that is ~2,000
   duplicated lines. **Do this at composition**, not now — interrupting nine
   in-flight agents to re-plumb their tests costs more than the dedup saves, and
   tests are exempt from the Branchless Law so the duplication is ugly rather
   than unsound.
2. **`dead_code` warnings until composition.** Every layer's `pub(crate)` items
   are unused until `axiom_surface` calls them. No agent added `#[allow]`, which
   is correct — the warnings disappear when composition lands, and silencing them
   first would hide a layer that never got wired.
3. **`aoStrength` belongs at the LIGHTING stage, not in `axiom_surface`.**
   `shader.js:678` applies it at the `aomap_fragment` chunk as
   `(owORM.r - 1.0) * owAoAmt + 1.0` — a lerp toward 1, not a multiply. The
   `masks` agent ported it correctly and flagged that the composition must call
   it where the engine applies AO to indirect diffuse.
4. **`frames` does not duplicate the dominant-axis selection** — that is
   `uv_mode`'s. It must be passed 0/1/2; anything else silently takes the Z arm,
   as in the source.
5. **`uv_mode` asks that `frames` call `axiom_uv_axis_sign`** rather than restate
   the `mix`/`step`, so the basis and the uv share one `s` by construction.
   Reconcile at composition.

## Findings from the layers so far

- **`axis_sign` is `step`, not `sign`.** A back-facing axis-aligned normal is
  `[-0.0, -0.0, -1.0]` and `step(0.0, -0.0) == 1.0`, which `signum` gets wrong.
  The `uv_mode` agent's first test asserted `signum`'s answer — the reference was
  right and the test was wrong.
- **The planar dominant-axis chain and the triplanar detail-plane chain are
  different comparisons and genuinely disagree** at `|n| = (0.5, 0.5, 0.1)`
  (planar picks Y, detail picks X). Both transcribed as written; the divergence
  is asserted rather than reconciled.
- **Cavity is not a derivative.** `cav = 1.0 - owHeightS`, the plain complement
  of the height field. No `dpdx`/`dpdy` anywhere in that section, so the
  `fwidth`-shaped parameter the brief anticipated was not needed. The real
  curvature input is the per-vertex bake.
- **`owTangentFrame`'s handedness is not a fixed sign** — it reproduces the
  mesh's uv winding. Correct, but it will read as a bug in a debug view, so a
  test states it outright.
- **`wear[3]` is dead in the source** (`shader.js:91` calls it "w curvature";
  nothing reads it). Ported as the whole `vec4` with the lane named and unread,
  pinned by a test that moves it and asserts bit-equality.
- Tolerances are being *derived*: `masks` tightened its own from 1e-6 to 4e-7
  after measuring one f32 ULP; `frames` landed a two-part
  `1.0e-7 + 1.2e-7·|v|` budget at ~1.5x what the hardware needs; `uv_mode`
  traced its 7.63e-6 to the adapter contracting `uv*tile.xy + tile.zw` into an
  `fma`.

---

# Final wave — wiring queue

## MaterialTexture contract (landed, needs wiring)

**Add:**
- `crates/axiom-host/src/lib.rs:194` — `pub use material_texture::MapPixels;`
  (already present in `tests/architecture.rs`'s curated list)

**Delete the `normals` parameter/argument at every site** (the slice collapses
the parallel slice into the carrier):
- `modules/axiom-gpu-backend/src/gpu_backend_api/mod.rs:553` (param), `:591` (arg)
- `modules/axiom-gpu-backend/src/gbuffer.rs:1631` — `&[],`
- `modules/axiom-gpu-backend/src/surface_program/bound_image.rs:122`, `:512` — `&[],`
- `tools/axiom-shot/src/capture.rs:88` — `&[],`
- `tools/axiom-shot/tests/translucency_parity.rs:196` — `&[],`
- `tools/axiom-shot/tests/capability_parity.rs:150,168,193,207` — the param on
  `gpu_look`/`gpu`, its forwardings, and every call site
- `apps/axiom-growth/src/bin/agent.rs:291` — `&[],`
- `apps/axiom-growth/src/bin/visual_target.rs:305` — `&rd.normals,`

**Migrate the one real caller.** `apps/axiom-growth/src/visual_target/build.rs:92,268`
(`bark_normal_material()`, `ground_normal_material()`) move onto the carrier via
`.with_normal(Some(MapPixels::new(w, h, px)))`. **This is the only caller in the
repo passing real normal maps**; every other passes `&[]` — which is exactly why
the live browser arm had no normal-map lane at all.

## The three lines that MUST land together

The slice's byte-identity test fails unless all three do, which is what it is
for:

1. `material_shader/compose.rs:319` — `dn` from `.xy`, with
   `z = sqrt(max(0, 1 - dot(xy, xy)))`.
2. `material_shader/compose.rs:347` — micro albedo reads **`.b`**, not `.r`;
   `.r` is now the normal's x.
3. `scene_renderer.rs` — `neutral_detail` `[128,128,255,0]` → **`[128,128,128,0]`**.

Item 3 is the subtle one and the notes had missed it: with the repacked layout
`.b` is the micro-albedo lane, so a `255` there decodes to
`(1.0 - 0.5) * 1.25 = 0.625` and **brightens every material that supplies no
detail map**. A neutral that is not an identity is the same class of bug as
"macro mid-grey, not zero" from the bindings slice.

`apps/shmup/src/materials/upload.rs` repacks in the same change.

## Binding-5 decision, recorded

Pack `(normal.x, normal.y, micro_albedo, height)` — **no binding 7**. Lossless:
both `dn` consumers (`axiom_detail_blend_normal`,
`axiom_detile_fold_detail_normal`) are UDN and never read `dn.z`, so four
channels carry the four scalars actually consumed. A seventh binding would cost
a fifth carrier slot, a `scene_wgsl.rs` change and another WebGL2 binding.

## Flagged, may predate this work

`tools/axiom-shot/tests/capability_parity.rs` passes
`&[(u64, u32, u32, Vec<u8>)]` where `render_offscreen_rgba` wants
`&[MaterialTexture]`. That mismatch is older than this slice and may mean the
file already does not compile. Check at integration rather than assuming the
wave broke it.

## Cross-slice defect: the CSM `mix` proved itself

Found by the `materialpatch` slice while reading a neighbour, and **verified**:

```
cascade/shading.rs:27       fn mix(a, b, t)     -> a + (b - a) * t
cascade/adapter_proof.rs:85 fn ow_mix(a, b, t)  -> a + (b - a) * t
gtao/reference.rs:53                            -> x * (1.0 - a) + y * a
gtao/wgsl.rs:78                                 -> x * (1.0 - a) + y * a
indirect_lighting/tests.rs:29                   -> x * (1.0 - a) + y * a
```

GLSL's spec for `mix` is `x*(1-a) + y*a`. The two forms are algebraically equal
and **numerically different** — float arithmetic is not associative.

The damage is not the ULP. It is that **the CSM slice wrote the same misreading
on both its Rust and its WGSL side, so its "bit-exact, worst delta 0.0"
real-adapter proof compared a wrong implementation to itself.** A proof that
cannot fail is not a proof.

This is precisely the failure mode this port documented after `sky/` — where ten
defects survived because one author wrote both transcriptions — and it recurred
in a slice that had been told about it. The brief's rule ("transcribe from the
source text, never from your own Rust") is necessary and evidently not
sufficient: what would have caught it is a *third* reference, or writing the two
sides from the spec independently.

**Fix at integration:** both `cascade` sites to the spec form, then re-run the
CSM adapter proof and record the delta it reports — it will no longer be 0.0,
and whatever it is will be the first honest measurement of that pass.

Also from the same slice, same file: `cascade/adapter_proof.rs:22-24` drops the
`dot(lightDirView, owSunDirView) < 0.999` sun test. Axiom's loop runs 16 lights
and shadows *every* directional (`scene_wgsl.rs:745-758`), so that test is needed
**more** here than in the source, not less. `indirect_lighting` kept it.

## `indirect_lighting` — verdict-first result worth keeping

`render/materialpatch.js` is **not** `onBeforeCompile` plumbing, despite the
name; that is `prepass.js`, already ported as `gbuffer.rs`. It is one lighting
decision: AO on *indirect* only (+0.35 on direct as micro-shadow), a contact ray
on *the sun only*, SSR *replacing* IBL specular, and an undocumented two-band
hemispheric fill gated by normal and interior volume.

~90 lines dropped as already-solved-by-construction (the `onBeforeCompile`
string surgery, `PATCH_VERSION`/`customProgramCacheKey`, the `_patched` WeakSet —
all answered by the content-addressed splice), and the Three-only duck-typing
dropped by name. That is the second time this port has retired work rather than
ported it, after `core/prewarm.js`; both were argued from the code, not assumed.

Wiring: `modules/axiom-gpu-backend/src/lib.rs: mod indirect_lighting;`

## fx/ambience — wiring + a defect a natural port would have hidden

Wiring: `apps/shmup/src/fx/mod.rs: pub mod ambience;` and drop that module doc's
"every file except `ambience.js`" caveat — it is now false.

Recommended in `fx/system.rs` (not touched by the slice): an `ambience` field
built after `ShellSystem::new`, `add_smoke_column` forwarding via
`ColumnOpts::from`, and `update` calling `ambience.update`.

**The defect worth remembering.** `resetSpawn()` returns the one module-level
`SP`, so `_puff`'s ember block re-zeroes the puff it just built: `t.x = s.x`
reads a zero, and **every ember spark spawns at world (0, 0)**. A literal Rust
transcription would have silently *fixed* it, because our `reset_spawn` returns a
fresh value — so the port writes the zeros explicitly to keep the bug.

That is a new category for this port: not a defect the port might miss, but one
the port would **accidentally repair**, changing behaviour while looking
faithful. Rust's value semantics quietly remove a whole class of JS
shared-mutable-scratch bugs. Worth grepping for wherever the source returns a
module-level scratch object — the slice audited all of `src/fx/*.js` and found
this was the only site there.

Second defect, pinned: `_warm` saturates at 2 while the delay ternary tests
`<= 2`, so the `-rng.float()*dt` arm is dead and all motes spread across
`-life*0.95`.

**Retires a false caveat**: `Ambience`'s constructor spends **zero** RNG
(`rngBefore == rngAfter`, pinned), so `fx/system.rs`'s note about "the one place
this port's stream can diverge" is wrong and should be deleted.

## probe/env — a brief error, and an OVERLAP to resolve at integration

**My brief was wrong.** `render/probe.js` is not a light probe: it is
`RenderProbeScene`, a procedural blockout *validation scene* ("Nothing here is
shipped content"), driven end to end by the app's xoshiro128** `Rng` — so a
module legally cannot hold it. It is **still unported** and belongs at app tier
(`apps/shmup/src/render/probe.rs`). Re-list it.

The indirect gate the boot log names (`[render] indirect gate: 2 interior
volumes`) is `materialpatch.js` + `index.js::_updateRooms`, not `probe.js`.

**⚠ OVERLAP — two slices ported the same thing.** `modules/axiom-gpu-backend/`
now has BOTH:
- `indirect_lighting.rs` (from the `materialpatch` slice), and
- `probe.rs` (from this slice, 2010 lines) — *also* the interior gate,
  `_updateRooms`, `sun_bounce` and the bounce fill.

They were briefed separately and converged on the same source. **Reconcile at
integration**: read both, keep one, and check whether they disagree anywhere —
if they do, that disagreement is free information about which reading is right.
Note `probe.rs` also carries a misleading name now, since it is not
`render/probe.js`.

**The `2 volumes` figure is derived, and the obvious derivation is wrong.**
3 enterable buildings (`W2`, `E1`, `E3`) − 1 `ruin` = 2. It is **not** the 15
interior light anchors — those are hanging bulbs inside those shells. My brief
suggested the anchors; a port built from them would have matched the boot log
and still been wrong.

## The cheapest visual win found so far

`FrameAmbient`'s `mix(ground, sky, up)` **is** the source's
`owSkyFill`/`owGroundFill` — except the source gates the two bands
**independently**. A vertical wall takes 0.5416 sky **plus** 0.0127 ground: sum
0.554, not 1.0. A `mix` normalises that to 1 and hands ~46% of the wall to the
warm ground band, which the source explicitly calls out as the bug it is
avoiding.

Fixing it needs **no new frame data** — same carrier, different blend. Then one
`FrameIndirect` lane (`fill_dir`, `fill_gain`, `indirect`, level transform, ≤10
volumes) for the gate and the anti-sun wrap.

## Another `f16` trap

`THREE.DataUtils.toHalfFloat` **truncates**; this crate's
`half_storage::to_half_bits` **rounds to nearest**. Different functions,
measured to disagree on about half of all inputs. Anywhere a port hands f16 data
to or from Three, the rounding mode is part of the algorithm.

Wiring: `modules/axiom-gpu-backend/src/lib.rs: mod probe; mod env;`

## minimap — the deferral was wrong twice, and never checked

`ui/minimap.js` was recorded for months as "blocked on the render work — needs an
orthographic depth bake read back once, then a Sobel pass". Both halves were
false:

- **There is no Sobel pass.** `minimap.js:10-23`'s *comment* describes one; the
  code uses a blurred-coverage rim (`4w(1-w)`, `:415`). The deferral was written
  from the prose, not the code.
- **The depth bake is the FALLBACK** (`:74-76`). The primary map is
  `_buildVectorMap`, pure CPU, needing only `world.{buildings, levelToWorld,
  isOpen}` — all three already public on `WorldSystem`.

So it ported in full with **no engine capability added and nothing invented**.
The reference screenshot confirms the vector path is what ships: the footprints
are rotated by the level yaw, which only the `levelToWorld` affine produces.

This is the fifth deferral this port has found to be a defect — but the first
that was **never true**, rather than true-then-expired. Different failure, same
lesson: a deferral is a claim, and a claim needs checking against the code, not
against the comment above it.

**Source defect found and pinned:** the street network **west of level x = 0
never draws**. `run = -1` (`:229-241`) is both the "no run open" sentinel and a
legal `lx ∈ [-44, 44]`, so a negative run start walks forward one cell per
iteration and never closes. Measured on the real level: 7,202 open cells produce
213 rects, **none starting negative**. That is why the reference minimap shows no
street layer at all.

**Second defect avoided in transcription:** `_buildBitmap`'s occupancy test reads
`h` (f64), not the `hgt[i]` it had just narrowed to f32 (`:317-319`).

Wiring:
- `apps/shmup/src/ui/mod.rs: pub mod minimap;`
- `apps/shmup/src/ui/system.rs`: add
  `pub fn set_minimap_bake_done(&mut self, done: bool) { self.minimap_bake_done = done; }`
  — the field is private, so the bake gate currently re-fires forever.
- Stale docs to fix: `ui/mod.rs` says "minimap is not ported" in 3 places,
  `ui/system.rs` in 4 plus a comment in `resize`.

## dof / lut

Wiring: `lib.rs: mod dof;` and `mod lut;`, plus four composite edits for the LUT
(splice on the **HDR arm only**, renumber the placeholder `@group(1)`, add
`AXIOM_LUT_SIZE`/`AXIOM_LUT_STRENGTH` to `tone_constants`, insert the
`axiom_lut_apply` line). Detail in `notes/dof-lut.md`.

**LUT placement, settled by reading the source rather than guessing.**
`lut.js` is display-referred and runs *inside* the composite at
`composite.js:144` — immediately after `owAgX` → `clamp(0,1)` →
`owLinearToSrgb`. So in Axiom it goes inside `post_chain`'s `graded()`, as the
**first statement after `srgb_encode(linear)` and before the `FramePostProcess`
grade terms**. Before, because the source feeds the LUT raw AgX output with
nothing between, `FramePostProcess` has no counterpart in the source chain, and
every preset constant (`pivot = 0.50`, `saturation = 1.20`) is calibrated to
where AgX puts 18% grey. It is **not** scene-referred and **not** part of the
bloom chain — bloom is added in linear light eleven lines earlier.

**Depth is a point fetch.** `prepass.js` sets `NearestFilter` on the R32F
channel, so DOF must use `textureLoad` on `GBufferChannel::Depth` with no
sampler — which is also the only legal fetch of a non-filterable format.

Other findings: no tile prepass exists (the prefilter's alpha max plus the
gather's running max substitute for one); blur targets are `Rgba16Float` so the
CoC rounds to f16 **twice**; two settings tables in the source disagree
(`maxCoc` 5.0 vs 3.3) and both are ported; `lut.js`'s `srgbToLinear`/
`linearToSrgb` are unused exports, deliberately not ported because
`surface_encode` owns that curve (verified `0.41666667` and `1/2.4` are the same
`f32`), with an expiry note.

Half-res targets must be `max(1, w >> 1)` — **a shift, not a rounded divide**.
Confirm WebGL2 3D-texture support before enabling the LUT's browser arm.

## gtao

Wiring: `lib.rs: mod gtao;` — nothing else. (`frame_graph/schedule.rs` already
names `"crate::gtao"` for `FramePass::Gtao`; the paths agree, which is a good
sign for the two slices meeting.)

**Four stale or dead things in the source, all pinned rather than assumed:**
- the header says "two slices"; `OW_SLICES` is **3**;
- the constructor's radius/intensity (0.9 / 1.25) are **dead**, overwritten by
  `aoRadius: 1.35` / `aoIntensity: 1.1`;
- there is **no thickness term** — `uParams.y` and `.w` are never read, and what
  looks like a thickness heuristic is the *squared* `clamp(len²/r², 0, 1)`
  falloff inside `mix(c, cosH, fall)`;
- `glsl.js`'s "velocity can be added" comment contradicts both shaders, which
  subtract.

That is four separate places where the comment and the code disagree in one
324-line file — the same trap that made the minimap deferral wrong for months.

**Three WebGPU `v`-axis corrections**, each a named constant rather than a
silent sign flip: `NDC_UV_V_SIGN`, `SCREEN_STEP_V_SIGN`, and
`resolution.y - position.y` for `gl_FragCoord`'s bottom-left origin. The middle
one matters most — `sliceDir` is view-space, so `+dir2.y` *decreases* v, and
getting it wrong swaps the two horizons, which is exactly the arc collapse the
source warns about.

**Two duplications to lift at integration** (both flagged by the slice, neither
fixed by it):
- `bloom_pyramid::half_storage` now has a second consumer (GTAO's RG16Float
  quantisation) and has earned the lift its own header asks for.
- `owIGN` now has **two** CPU references: `cascade::shading::ig_noise` is private
  to `cascade`, so `gtao::reference::ign` duplicates it. Given the `mix` defect
  already found in `cascade`, two copies of a noise function is exactly the shape
  worth collapsing before it diverges.

## frame_graph — and a primitive that must move to the kernel

Wiring: `lib.rs: mod frame_graph;` (unconditional, after `mod cascade;`).

**Sibling entry points are carried as DATA, not `use`s** —
`FramePass::module_path()` / `module_exists_today()`, enumerated by one test. So
the crate compiles without any of the seven passes, and fixing a wrong guess is a
one-string edit. That is the right shape for a wave where every dependency was
written concurrently, and worth copying next time.

**⚠ `render/composite.js` (353 lines) may be unowned.** The exposure/AgX slice
took its *tonemap* half into `agx.rs`, but the frame graph says composite also
owns plan steps 14/17/18/debug — 4 of its 20 slots — and that `post_chain.rs` is
not a substitute. **Verify at integration**; if the composite proper is missing,
that is a real hole in "the remaining 4,983 lines".

### `jsmath` belongs in the kernel

`frame_graph/rooms.rs` needed V8's Kahan-compensated `Math.hypot` and could not
reach `crate::jsmath`, **because that lives in `apps/shmup` and a module may not
depend on an app**. So it wrote a **seventh** copy.

The Layer Law already answers this, in its own words: *"a broadly-shared
primitive (one many layers need but no single adjacent layer owns — e.g.
dimensioned scalar quantities) belongs in the kernel, the shared root every layer
may depend on."* `Meters`/`Radians`/`Ratio` are the cited precedent.

That is exactly what `jsmath` is. It exists because **the source's arithmetic is
JavaScript arithmetic**, which is a property of the whole port and not of the
app: `hypot`, `sign`, `round`, `or`. The count so far — six `hypot3`s across
three algorithms (two wrong, one wrong *by citation*), six `js_round`s (two with
a real double-rounding bug), nine three-valued `sign`s, and now a seventh hypot
in a module — is the strongest possible argument that its current home is too
high.

**Move `apps/shmup/src/jsmath.rs` to `crates/axiom-kernel`** at integration,
keeping the V8 goldens, and have both the app and `gpu-backend` depend on it.
Note the kernel is branchless and 100%-covered, so the app-tier `if`s in `round`
and `or` need the table/blend form the spine already uses.

Also: `frame_graph/rooms.rs`'s `MAX_ROOMS = 10` duplicates the same constant in
the `materialpatch`/`probe` slices — collapse onto one when resolving that
overlap.

**Source defects pinned:** `viewKeyMax: 2.6` is unreachable (the ratio clamp
bounds the key at 2.53); motion blur and TAA are the two velocity consumers gated
*without* `needsPrepass` (harmless in JS, not on a G-buffer-less arm);
`pass.js`'s `uv` attribute is dead; `addLight`'s `priority` is never read.

**Tier detail worth keeping:** `ultra`'s `shadowMapSize: 4096` is clamped to 2048
by the CSM constructor — which is *why* the boot log reads `4x2048`. A port that
took the preset at face value would have allocated four 4096 maps.

## taa / motionblur

Wiring: `lib.rs: mod taa;` and `mod motionblur;` — no cfg gate.

**Two shader modules for motion blur, not one:** both MB passes claim
`@group(0) @binding(0)`, so the shared vertex stage lives in a binding-free
`MOTION_BLUR_WGSL_COMMON`. A single module would not have bound.

**Jitter:** Halton(2,3) over `1..=16`, minus 0.5, indexed `frame % 16 + 1`,
evaluated in `f64` and narrowed once — `f32` rounds twice and moves the sample
position. Deliberately not v-flipped, because the resolve never learns the
jitter.

`VELOCITY_TEXTURE_V_SIGN` lands in exactly **four** places, applied *after* MB's
length-based tile-vs-own select so that select stays bit-identical to the source
— and the count is pinned by a test, which is the right way to hold a sign
convention that is easy to apply one place too many.

**Coverage rejection uses the source's own curve** and is asserted against both
`COVERAGE_DYNAMIC` and `COVERAGE_STATIC` from `gbuffer.rs` — exactly 1.0 at one
and 0.0 at the other. Two slices written weeks apart agreeing on a constant is
worth noting.

**Dead code carried and pinned:** TAA's `uParams.w` ("motionScale") and MB's
`uTexel` are declared and never read; the `MotionBlur` constructor's
`shutter = 0.5` is overwritten every frame; the 8×8 tile dilation spans 15 of its
16 texels.

**One accepted divergence, with the fix written down:** `owIGN` reads
`gl_FragCoord.xy`, and WGSL's `@builtin(position)` is y-down, so the dither is
mirrored versus WebGL. Harmless — IGN has no preferred origin and feeds a ±0.5
offset. One-line exact-parity fix if ever wanted:
`vec2(position.x, resolution.y - position.y)` in `MOTION_BLUR_BLUR_WGSL`.

**`half_storage` is now requested by two separate slices** (gtao, taa) — that is
the condition its own doc sets for lifting it out of `bloom_pyramid` into
`hdr_target`. Neither could do it without touching a shared file. Do it at
integration.

## GPU bake — and a correction to `01-engine-gaps.md`

**`01-engine-gaps.md` is wrong about where bake-time generation belongs.** It
states the load-bearing decision as: *"the 19 procedural surface generators are
straight-line noise math with no sampling and no derivatives. They belong in the
field/proc-texture path, and the node budgets need raising to hold them."*

The premise is false, and the slice measured it:

- `owWorley` is a 3×3 **loop** with an F1/F2 comparison chain carrying a `vec2`
  payload; `owVoronoiEdge` is two-pass 3×3 + 5×5 centred on a **runtime-found**
  cell; `FOLIAGE` nests `if (cover > 0.01) { if (depth > bestDepth) }` over five
  accumulators; all three fbms **divide** (`s / max(n, 1e-4)`); every lattice
  access needs `floor`/`fract`/GLSL-`mod`.
- `FieldOp` permanently excludes `Div`/`Step`/`Compare`/`Select`, never
  contemplated `Floor`/`Fract`/`Mod`, and its `Fbm` is non-periodic 3D noise with
  no `per`.
- **Measured node counts** (fully inlined, fully unrolled, CSE'd — a lower
  bound): PLASTER 43,403 · CONCRETE 35,555 · ASPHALT 28,558 · BRICK 25,468 ·
  DIRT 15,710 · GLASS 4,337. Against `MAX_SURFACE_NODES = 256` **per whole
  surface** — about **170× over at the median**.

So raising the budget (G15) would not have helped; the vocabulary is the
blocker, not the size. **Amend `01-engine-gaps.md`** rather than leaving a plan
that a future agent would try to follow.

**Transcription method worth copying.** Four independent agents, one per
`surfaces-*.js`, **each forbidden from reading the existing Rust port**. So the
CPU port is a genuinely second reading and the parity test is where the two
meet. That is a direct answer to the failure this port has hit repeatedly — most
recently `cascade`'s `mix`, where one author wrote both sides and the proof
compared a mistake to itself.

**Another expired deferral, found by the slice that inherited it.**
`notes/materials-upload.md` says four of five maps are "produced but
unbindable". That contract **landed this wave**
(`MaterialTexture::with_{normal,orm_height,detail,macro_field}`). Every map is
bindable today with no engine change — yet `scene/app.rs` still calls
`upload::bake_albedo_maps` (albedo only, 64², CPU), which **is** the streaking in
`axiom-street-agx.png`. Grep `bake_albedo_maps` at integration.

**An attribution corrected:** `RUNTIME_BAKE_SIZE`'s doc blames
`fract(sin(dot()))` in `ow_hash22`. The library is sin-free Dave-Hoskins; the
transcendentals are in `owGrad2` (32 per 4-octave fbm). The timings and the
conclusion stand — only the cause was misnamed.

**G16 is structurally unreachable through this path**: the sRGB encode and the
binding format are chosen by the same `linear_albedo` flag, so a baked texture
cannot be written linear and bound sRGB.

Wiring:
- `crates/axiom-host/src/lib.rs`: `mod procedural_bake;` +
  `pub use procedural_bake::{BakeOutput, ProceduralBakeMaps, ProceduralBakeRequest};`
  and `layer.toml` `introduced_capabilities += "ProceduralBakeRequest"`
  (plus the curated-export test).
- `modules/axiom-gpu-backend/src/lib.rs`: `mod texture_bake;` (unconditional).
- `gpu_backend_api/mod.rs`: the `bake_procedural_texture` method (body in notes).
- `apps/shmup/src/materials/mod.rs`: `pub mod wgsl;` + `pub mod gpu_bake;`.
- `apps/shmup/Cargo.toml`: `axiom-host` dep, `axiom-gpu-backend` dev-dep with
  `features = ["offscreen"]`.
- `apps/shmup/app.toml`: `allowed_layers += "host"`,
  `allowed_modules += "gpu-backend"`.

**Still unwired, with an expiry check:** nothing carries a bake request to the
live browser device (`modules/axiom/src/app/` → `axiom-windowing` →
`live_gpu_binding.rs`). Until that lands this slice is itself a deferral — which
is precisely the shape it just caught in someone else's notes.
