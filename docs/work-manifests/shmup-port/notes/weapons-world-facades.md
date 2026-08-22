# `weapons/index.js` and `world/index.js` — the two facades

Two files, one slice: `src/weapons/index.js` (843 lines, the firing facade) and
`src/world/index.js` (445 lines, level orchestration).

| source | target | golden | test |
|---|---|---|---|
| `src/weapons/index.js:1-843` | `apps/shmup/src/weapons/system.rs` | `tests/weapons_system/{capture.mjs,golden.json}` (2.5 MB) | `tests/weapons_system_port.rs` |
| `src/world/index.js:1-446` | `apps/shmup/src/world/system.rs` | `tests/world_system/{capture.mjs,golden.json}` (1.8 MB) | `tests/world_system_port.rs` |

Both goldens are captured by running the **original, unmodified** JavaScript
under Node 24 with the real `three@0.180`. Both capture scripts write
`golden.json` with `writeFileSync` (a PowerShell `>` redirect writes UTF-16 and
corrupts the file) and both are byte-reproducible.

---

## weapons — status: 12 tests, all green

### What is pinned

* **`init`** — `def.cycleTime` per weapon, the whole recoil pattern compared
  **exactly** (both sides narrow through `f32` at the same point, so there is
  no tolerance to state), the starting magazine/chamber/reserve/mode,
  `stats.tris` per weapon and in total, and the clip `play('draw')` started.
* **960 scripted frames at 1/60**, driving: a long held automatic burst that
  runs the rifle dry, dry-trigger clicks, an auto-reload on a dry pull, ADS
  through a whole empty reload, all six stance/movement branches of
  `_restSpread`, the fire-mode cycle through `auto → burst → semi` with three
  burst pulls and three semi pulls, an explicit tactical reload, inspect, and
  four weapon switches (`Digit2`, `Tab`, wheel, `Digit1`). Per frame, after
  `late_update`: the whole firing state machine, the ammo view, the four
  booleans, all ten HUD fields, `_state`'s eight fields, the eight shell-queue
  timers, the dropped-magazine pool, `stats`, and the four world-space rig
  queries.
* **The ordered effect journal** — 1157 entries: every `weapon:fire`,
  `weapon:shell`, `weapon:reload`, `bullet:tracer`, every
  `player.addRecoil`/`setAdsProgress`, and every
  `physics.fireBullet`/`spawnDebris`/`removeRigidBody`, interleaved, with the
  frame each landed on. Payloads are deep-copied at emit time, because the
  source mutates one preallocated object per event type and a shallow
  reference would read back as the last emit.
* **`debugPose('idle'|'ads'|'fire')`** on a fresh system, including the sixteen
  scripted fire frames.

### Tolerances

| what | tolerance | why |
|---|---|---|
| counts, flags, strings, the recoil pattern | exact | `f32` narrowing happens at the same point on both sides |
| the firing state machine, HUD, `boreDir`, `rig_quat` | `1e-12` relative, abs floor 1 | the established figure |
| `rig_pos` (`SPRING`) | `5e-8` | seven spring integrators + three `exp`-based `damp` blends over 960 frames; **measured worst 4.65e-9** |
| anything reading a rig node (`NODE`) | `5e-7` | `weapons::models` authors every node in `f32`; **measured worst 2.59e-8** |

`the_spring_residual_stays_within_the_stated_bounds` pins those two
measurements so a real regression cannot hide inside the headroom. It also
asserts `rig_quat`'s residual is *smaller* than `rig_pos`'s — the quaternion is
bit-exact (2.22e-16, one ULP) while the integrated position is not, which is
the shape a spring accumulation makes and not the shape a transcription error
makes.

### Two orderings that were nearly ports of the wrong thing

**The one-frame anchor lag is real and is preserved.** `muzzleWorld`,
`ejectWorld`, `ejectVelocity` and `boreDir` read `w.group.matrixWorld`, which
is `anchor.matrixWorld * rig.matrix`. `viewmodel.update` writes
`anchor.position`/`anchor.quaternion` but never composes them into
`anchor.matrixWorld` — the **renderer** does that, in `scene.updateMatrixWorld()`,
after every `lateUpdate`. So a shot fired in `update` on frame N reads frame
N-1's anchor and frame N-1's rig, while the `weapon:fire` payload assembled in
`lateUpdate` reads frame N-1's anchor and frame N's rig.
`WeaponCore::sync_anchor` is that render walk spelled out; the capture script
calls `viewScene.updateMatrixWorld(true)` at exactly the point the renderer
would. `viewmodel.rs` declined to port these four functions for precisely this
reason ("their value is a function of render-loop ordering that does not exist
here"); it exists now, so they live on the first caller that needs them and
should move down to `viewmodel.rs` the moment a camera lands there.

**A clip beat sees the pose from *before* `viewmodel.update`.** The source
dispatches beats between the clip sample and the pose compose. `viewmodel.rs`
queues them and the caller drains them after `update` returns. That is
invisible for `start`/`magout`/`magin`/`end`/`boltrelease` — except for
`magdrop`, where `_dropMagazine`'s `mag.updateMatrixWorld()` reads
`parts.magazine`'s local transform from the previous frame (`_updateParts` has
not run yet) composed against `group.matrixWorld` from the previous render walk
(`rig.updateMatrixWorld(true)` has not run yet either). During a reload the
magazine is travelling fast enough for that to be **1.2 cm**. `late_update`
therefore snapshots `PreStepPose { rig_pos, rig_quat, parts }` before stepping
the rig and hands it to the beat.

### The one divergence that could not be removed

`weapon_switch_composes_one_frame_late`. A `holster` clip's `end` beat swaps the
active weapon, and in the source that happens *before* the pose compose, so the
frame the swap lands on composes the **incoming** weapon's hip pose; this port
drains the beat after `update`, so it composes the **outgoing** one. One frame,
four times in the 960-frame scenario, self-correcting on the next. The test
names those frames (derived from the golden, not hard-coded), skips the rig
comparison on exactly them, and asserts (a) the weapon id still swaps on the
same frame and (b) the pose agrees again on the next. Removing it properly
means giving `Viewmodel::update` a beat callback, which `viewmodel.rs`
deliberately does not have — a change to that file, not this one.

### Seams

| source | seam |
|---|---|
| `ctx.camera` | `FireCamera: ViewCamera` — `position()` plus `aim_orientation()`. **Two quaternions, deliberately:** `tryFire` reads `cam.quaternion` while the anchor takes `setFromRotationMatrix(cam.matrixWorld)`, and the two are not bit-identical. Conflating them compiles and is wrong in the last bits of every shot. The capture writes both into the script table, so the test never re-derives a Euler. |
| `ctx.peek('player')` | `WeaponPlayer` — the ten members `index.js` reads |
| `ctx.peek('physics')` | `WeaponPhysics: RaycastWorld` — `spawnDebris` + `removeRigidBody` on top of the pair `ballistics.rs` already declared |
| `ctx.input` | `WeaponInput` — six members; `crate::input::Input` implements it |
| `ctx.scene` + `THREE.Object3D` mag proxies | `MagProxy` as data |
| `ctx.viewScene.environmentIntensity` | `WeaponCore::env_intensity()` |

### The event-payload fork

`EventBus` dispatches a `&dyn Any` that each handler downcasts, so **two structs
for one event name means only one subsystem sees an emit**. `audio::system` and
`ui::system` already declare two differently-shaped `WeaponFire`/`WeaponReload`
pairs, and `ui/system.rs` says so in a comment ("only one of the two subsystems
will see any given emit… converging the two is a whole-game decision and
belongs in the integration pass"). This facade therefore emits the **audio**
vocabulary — the richer of the two — rather than inventing a third fork.

What the shared vocabulary cannot carry, exactly:

| source payload field | status |
|---|---|
| `weapon:fire.dir` | **missing** — `WeaponCore::fire_dir()` / `fire_payload()` |
| `weapon:fire.seed` | **missing** — `WeaponCore::fire_seed()` |
| `weapon:shell.velocity` | **missing** — `WeaponCore::shell_payload()` |
| `weapon:shell.caseLen` / `.caseRadius` / `.spin` | **missing** — same accessor |
| `ui::system::WeaponFire.recoil` | the HUD's own fork; not fed by this emit |

Every one of those is still *computed*, in the source's order and with the
source's RNG draws, and pinned by
`the_payload_fields_the_shared_vocabulary_drops_are_still_right`. Nothing is
dropped from the simulation — only from the bus. **Converging
`audio::system` + `ui::system` + this facade onto one payload set per event
name is the integration pass's job.** `fx` is not affected: it exposes
`on_weapon_fire`/`spawn_shell` as methods and never subscribes.

### Not ported, and why

* `console.info` (`index.js:181-184`) — a build banner; its facts are
  `stats.tris`.
* `resize()` (`index.js:830`) — empty in the source.
* Three lines of `debugPose` (`index.js:749-751`): `vm._angVel.yaw = 0`,
  `vm._angVel.pitch = 0`, `vm._hasPrev = false`. They reach `Viewmodel`'s
  private working state, which `viewmodel.rs` keeps private following the
  source's own `_` convention. On a freshly-constructed system — the only path
  the harness uses, and the path the golden captures — all three are already at
  those values, so the omission is a no-op there. `vm.debugFrozen = true`
  (`index.js:753`) is likewise absent and is never read anywhere in
  `viewmodel.js`.
* `_fitSupportHand` is `viewmodel.rs`'s gap, not this one. The golden records
  what it costs and `only_the_rifle_gets_a_fitted_support_hand_pose` pins it:
  the source refines `clamp` → `clamp:rifle` for the rifle only; the smg keeps
  a bare `clamp` and the pistol a `cup`, so the gap is one weapon wide.

### Ported dead state, kept

`this._semiLatch` and `this._reloadPhase` are assigned in the constructor and
never read anywhere in `index.js`. Both are carried (`WeaponCore::dead_state()`
reads them back) — dead computation in the source is still part of the source.
`_runTrigger`'s unread `dt` parameter is dropped from the Rust signature, with
a comment.

### Rig converters that belong elsewhere

`rig_from_smg` and `rig_from_pistol` are `addWeapon`'s node half
(`viewmodel.js:405-434`) for the two weapons `WeaponRig::from_rifle` did not
cover. Its doc says "the smg and pistol get their own converters when a
consumer needs them" — this facade is that consumer. **They belong next to
`from_rifle` in `viewmodel.rs`** and are here only because this slice may not
edit that file.

### One real defect found in the harness, worth recording

`Time::default()` derives `scale: 0.0`, while the engine's own start state
(`Time::start`) is `scale: 1.0`. `time.elapsed` only advances by `dt * scale`,
so a test that builds its clock with `Time::default()` silently freezes
`elapsed` at zero — and the only thing that reads `ctx.time.elapsed` in this
facade is the dropped-magazine expiry, which then never retires anything. It
surfaced as `until: 22` against `27.883`.

---

## world — status: 16 tests green, 3 `#[ignore]`d on two upstream geometry defects

### The expired deferral, and what landing it cost

`crate::world::buildings::build_interior` did not call
`crate::world::interiors::furnish_room`. `buildings.rs` said "deferred until
`interiors.rs` lands" — and `interiors.rs` had landed. The deferral was honest
when written and became a defect the moment its blocker cleared.

**What it cost, measured before it was fixed.** The shared stream agreed bit
for bit through `register_props`, `register_dressing_props`, `build_ground`
and the two non-`enterable` buildings, then parted company at `W2`, the first
`enterable` one — the source drawing 5636 values furnishing rooms this port
drew none for. Downstream every draw was offset: 175 instanced batches against
166, and **0 interior light anchors against 15** (`interiors::hanging_bulb` is
the only thing anywhere that fills `Assembler::interior_lights`, so
`world::system`'s `_addLights` bulb loop ran zero times and the world had no
interior lighting at all).

**Landing the call site.** `buildings.js:723-739`, transcribed: inside the `f`
loop, **after** this floor's stairs and **before** the next floor's
partitions, resolving each `RoomFurnish` from normalised 0..1 room coordinates
into a level-space `RoomRect`. Position in the sequence is the contract —
`furnish_room` draws from the same shared `rng`, so anywhere else and every
subsequent placement in the level shifts.

### What that exposed: `interiors.rs` had never been compiled

`world/mod.rs` did not declare the module. 765 lines, no `mod` entry, so no
`cargo build` had ever touched it, its own `#[cfg(test)]` unit tests had never
run, and it has **no golden and no notes file** — unlike every other slice in
this port. It did not even compile: adding `pub mod interiors;` produced 14
borrow errors.

Three classes of defect were in it, all of which a single compile or a single
golden would have caught:

1. **Nine argument-evaluation-order errors** (the 14 borrow errors). JS
   evaluates arguments strictly left to right, so `patchGeometry(rng,
   rng.range(0.4, 1.1), …)` draws the radius *before* the call. Each site now
   hoists its draws into `let`s in the source's order — the same idiom
   `dressing/mod.rs` documents.
2. **Nine `for (let i = 0; i < rng.int(a, b); i++)` loops ported as
   `for _ in 0..rng.int(a, b)`.** A JS `for` re-evaluates its condition every
   pass, so `rng.int` is drawn once per test *including the final failing one*;
   the Rust form draws once. This changes both the iteration count and the
   number of values consumed — it is the single easiest way to desynchronise a
   whole file, and `crate::world::dressing::int_loop_continues` exists
   precisely for it. All nine now use it. (A tenth site,
   `dressWalls`' "objects standing against the skirting", is a *hoisted*
   `const nBase = rng.int(2, 5)` in the source and correctly stays a plain
   range loop — the two forms are one line apart in the JS and mean different
   things.)
3. **One invented branch.** `furnishShop`'s counter loop had a
   `if rng.float() < 0.8 { … }` guard around the `produce` put with **no
   counterpart in the source** — a hallucinated draw, and the last thing
   between the two streams. Removing it took the shop rect from 726 draws to
   760, matching exactly.

Verified per kind, against the original `furnishRoom` driven under Node over
nine rects (two each of `shop`/`living`/`storage`/`ruin` plus the
sub-1.2 m early-out): draw count, final rng state and interior-light count
match **exactly** on all nine. Then end to end: all twenty-nine
`WorldSystem::init` pass checkpoints match, and 15 interior anchors against
the source's 15.

### One real bug this slice owned, found by the same run

`Assembler::light` **mutates** the light it is handed —
`if (!this._identity) light.position.applyMatrix4(this.xform)`
(`builder.js:311`) — so `this.bulbs` holds **world**-space positions even
though `A.interiorLights` is authored in **level** space. `world/system.rs`
was storing the level-space anchor, putting every interior bulb ~2 m from
where the source puts it. Fixed; `WorldSystem` now carries both
(`interior_anchors`/`lamp_anchors` in level space, `bulbs`/`lamps` in world
space) because they are two different facts and only one is what a renderer
wants.

### And one where the comparator was the bug

`every_collision_batch_matches_in_order` failed on `collide[5] wood sum.z`,
`3.5487` against `3.5479` — while the batch's vertex and triangle counts were
*identical*. The digest compares a sum of 3264 signed coordinates spanning
tens of metres that very nearly cancels; a **relative** tolerance on that
result asks the port to reproduce a near-total cancellation to twelve digits.
The rounding error of a sum of `n` terms bounded by `e` grows like
`n · e · eps`, so `assert_digest` now scales the `sum`/`chk` bounds that way
and keeps the plain relative tolerance for `min`/`max`, which are not
accumulations. Recipe rule, applied: before widening a tolerance, check the
instrument.

### What is still blocked, and where

With the draw stream identical end to end, every remaining difference is a
pure geometry-value difference that no RNG draw decides. Two are left, both
upstream of this slice, both now isolated to one function each:

1. **`world::ground::build_ground` scatters a different number of pebbles** —
   476 `rock_a` / 992 `rock_b` against the source's 468 / 903. Every later pass
   then adds *identical* deltas on both sides (measured pass by pass), and
   `rock_a`/`rock_b` are the **only** prototypes in the whole level whose
   instance count differs. `world_port.rs`'s golden covers `buildGround`'s road
   profile but not its prop scatter, which is why it passes.
2. **`facade_wall` builds fewer vertices** (`world::buildings` / `world::kit`).
   Fourteen of thirty-six merged static batches differ in vertex count and the
   large ones are all wall keys: `plaster_cream` 42104/52036, `brick_fine`
   17886/32520, `plaster_sand` 60515/72544, `plaster_blue` 34794/43608,
   `plaster_pink` 40342/51064. Every palette key `interiors.rs` *exclusively*
   owns (`emissive_warm`, `metal_dark`, `wood_prop_dark`, `plywood`,
   `corrugated`) matches exactly — which is the evidence for attributing this
   upstream rather than to the furnishing that just landed.

Three comparisons stay `#[ignore]`d on those, with the blocker in the reason
string: `every_static_batch_matches_in_order`, `the_level_stats_match`,
`every_instance_placement_matches_in_order`. Everything they can still prove
is asserted unconditionally instead: `the_batch_list_matches_key_for_key_in_order`
(all 36 + 175 batches, in order, with key/prototype/surface/shadow flags),
`every_collision_batch_matches_in_order` (all eight, exactly),
`only_the_ground_pebbles_place_a_different_number_of_instances` (the defect,
bounded, so a second placement bug cannot hide behind the first) and
`the_instance_matrix_residual_stays_within_the_stated_bound` (measured worst
4.62e-6 relative over ~99 400 matrix elements — the `f32` compose-then-multiply
chain in `Assembler::put`/`finalize` against `three`'s `f64` compose).

### Also worth fixing upstream, not touched here

`docs/work-manifests/shmup-port/notes/buildings.md` still describes the
furnishing as "a concurrent, not-yet-ported slice" and points at the marker
that is now a real call. It is another slice's notes file, so it is reported
rather than edited.

### What the golden covers

The full level is captured regardless, so the un-ignoring is free: every merged
static batch (36) and collision batch (8) with vertex/triangle counts, bounding
box and an **index-weighted** positional checksum (so a reordering shows, not
only a value change); every instanced batch (175) with **all 8031 instance
matrices** and their per-instance `[wear, grime, ao]`; `A.stats`;
`A.interiorLights`; `A.lampAnchors`; the registered light positions; the bulbs
and lamps `_addLights` builds; the spawn table; the bounds; `spawn(i)` across
`-5..12`; `levelToWorld`/`worldToLevel`; a `groundHeight`/`isOpen` grid; and the
`update` dusk sweep.

The **per-pass rng checkpoints** are captured by re-running the same pass
sequence (`index.js:105-134`, transcribed line for line) against the same
original pass functions. That transcription is the one thing in the capture
that is not the source itself, so it is cross-validated: the capture **throws**
unless its final checkpoint equals the state the real `WorldSystem.init` left.
On the Rust side they come from `WorldSystem::init_observed` — the real `init`
with an observer — so a wrong order here cannot hide behind a test that
re-lists the order itself.

### Deliberately deleted: the light ballast and the pre-warm

Per the port status ("Not being ported, deliberately"), and stated here so the
omission is visible rather than silent. Dropped **in full**:

* `_addBallast` (`index.js:229-251`) — parks `LIGHT_SLOTS + 4` = 24
  zero-intensity **black** point lights under the map.
* `_stabiliseLightCount` (`index.js:266-308`) — tops the visible count up every
  `lateUpdate` by mirroring the renderer's own distance cull, so
  `numPointLights` (a Three shader-permutation cache key) never changes.
* `prewarmMaterials` (`index.js:356-394`) and `_compile` (`index.js:396-406`) —
  a boot-time `renderer.compileAsync` sweep over the forward pass plus the CSM
  depth and gbuffer override materials.
* the `LIGHT_SLOTS = 20` constant they share.
* `lateUpdate` itself (`index.js:333-335`), which called nothing else, and the
  `_ballast` / `_pointLights` / `_pointLightsFrame` / `_lightTarget` /
  `_lightRanges` / `_camPos` / `_collectPointLight` / `_render` state.

Nothing else in the file reads any of them, and Axiom solves the second problem
structurally (surface programs compile at a preparation barrier).

### Other things not carried, each with its reason

* `A.updateLod(ctx.camera)` (`index.js:313`) — needs a live camera and a
  per-frame bounding-sphere test; `assembler.rs` already records that it
  carries each prototype's `max_dist` as data and stops there.
* `A.mat('lamp_lens')` and the `lampLens.emissiveIntensity = 9 * mix` write —
  `assembler.rs` deliberately does not port material resolution. The value is
  computed and exposed as `WorldSystem::lamp_lens_emissive()`, and the dusk
  test asserts it against `9 * mix`.
* `materials.setGroundLevel(0)` — same reason; the constant is `GROUND_LEVEL`.
* `this.root` / `ctx.scene.add` / `physics.addStatic` / `rebuildStatic` — the
  renderer and physics bridges; `finalize` hands the data back instead.
* `console.info` (`index.js:156-160`).

### One fork, not two

The source takes exactly one fork (`this.rng = ctx.rng.fork()`), and
`new Assembler({rng})` stores it as `this.rng` at `builder.js:44` and **never
reads it** (verified: `this.rng` appears exactly once in `builder.js`; the
placement jitter comes from `jitterRig()`'s own fixed-seed stream
`0x9e3779b1`). Rust will not let a pass borrow `&mut Rng` while it also holds
the `&mut Assembler` that owns it, so `Assembler::new` takes an `Rng` of its
own — and this facade hands it `Rng::new(0)`, **not** a second fork, because a
second `fork()` draws a `u32` from the root stream the source never draws and
shifts every subsystem initialised after `world`.

`crate::scene::level::build_level` — an earlier, partial transcription of this
same `init`, written before `dressing.js` landed — takes two forks and says so.
It is now superseded by this facade.

### `LEVEL_YAW`/`TX`/`TZ` are `f64`, and that is load-bearing

`Assembler::set_transform` takes `f32` (the whole geometry pass computes in
`f32`), so the constants were `f32` at first. That pushes the narrowing into
`levelToWorld`/`worldToLevel`/`bounds`, which the source evaluates at full
double precision: `0.9` narrowed to `f32` and widened back is
`0.899999976158142`, a **2.4e-8 error on every query**, and it broke
`ground_height` at 1e-12. The constants are `f64` and the one narrowing happens
at the `set_transform` call site, where it is visible.

`level_xform()` builds the `f64` matrix as `Matrix4.compose((tx, 0, tz),
quaternionFromEuler(0, ry, 0), (1,1,1))`. A yaw-only rotation composes
identically under every Euler order, so the port recipe's "Euler order is a
convention" trap does not bite here — but only because it is yaw-only, which is
why it is spelled out in the code.

`transform_box` transforms **all eight corners** and re-bounds, which is what
`Box3.applyMatrix4` does and is not the same as transforming `min` and `max`.

---

## Files written

```
apps/shmup/src/weapons/system.rs
apps/shmup/src/world/system.rs
apps/shmup/tests/weapons_system/capture.mjs
apps/shmup/tests/weapons_system/golden.json
apps/shmup/tests/weapons_system_port.rs
apps/shmup/tests/world_system/capture.mjs
apps/shmup/tests/world_system/golden.json
apps/shmup/tests/world_system_port.rs
docs/work-manifests/shmup-port/notes/weapons-world-facades.md
```

Plus, outside the original slice, the changes the furnishing fix required:

```
apps/shmup/src/world/buildings.rs   the furnish_room call site + module doc
apps/shmup/src/world/interiors.rs   19 fixes (9 argument-order, 9 loop-idiom,
                                    1 invented branch) so it compiles and
                                    matches the source
apps/shmup/src/world/mod.rs         pub mod interiors;   <-- never declared
apps/shmup/src/weapons/mod.rs       pub mod system;
apps/shmup/src/world/mod.rs         pub mod system;
```
