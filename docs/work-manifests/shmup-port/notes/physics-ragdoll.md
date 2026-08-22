# Ragdoll (PBD solver) — port notes

Source: `C:/dev/Claude-of-Duty/src/physics/ragdoll.js:1-763` (763 lines).
Target: `apps/shmup/src/physics/ragdoll.rs` (new module).

## What was ported

Everything except the three `THREE.Skeleton` methods:

- `humanoidSpec(height, scaleMass)` → `humanoid_spec` — the 15-capsule
  7.5-head figure, every literal preserved.
- `class Ragdoll` → `struct Ragdoll` — the constructor (particle merge,
  bone tables, inverse masses, bone lengths, reference up-vectors, AABB,
  self-pair prefilter), `setVelocity`, `applyImpulse`, `wake`, `step`,
  `_solveDistance`, `_solveCones`, `_solveContacts`, `_solveSelf`,
  `_frictionAt`, `_transportUp`, `_updateAabb`, `_initUp`, `_buildSelfPairs`,
  `getBoneCapsule`, `getBoneTransform`, `dispose`.
- The module constants `DEG`, `MAX_PARTICLE_STEP`, `SLEEP_MOTION`,
  `SLEEP_TIME`, and the module-level `_nextRagdollId` counter.
- Two routines the source gets from the engine and Rust does not:
  `hypot3` (V8's `Math.hypot`) and `js_round` (JS `Math.round`). Both are
  discussed below; both are real, load-bearing divergences from the obvious
  Rust spelling.

It composes the already-ported physics rather than re-deriving it:
`bvh::StaticWorld::overlap_capsule` + `bvh::Contacts` for world contact,
`bvh::Aabb` for the bounds record, `math::closest_pt_seg_seg` for
self-collision and the bind-pose prefilter, `surfaces::mask::DEBRIS` for the
default query mask, and `Surface::props().friction` for the per-surface
Coulomb coefficient.

## What was NOT ported, and why

`adoptSkeleton` (`:653-663`), `writeToSkeleton` (`:666-695`) and
`specFromSkeleton` (`:709-763`). All three traffic in a live `THREE.Skeleton`
/ `THREE.Bone` object graph — `bone.parent`, `matrixWorld`,
`updateWorldMatrix(true, false)`, `Matrix4.decompose`, `getBoneByName` — and
`specFromSkeleton` additionally walks `bone.children.find(c => c.isBone)` to
derive a spec from an authored rig. There is no such graph in this port, and
inventing a scene-graph abstraction inside a physics module to host it would
be exactly the wrong place for it. `get_bone_transform` and
`get_bone_capsule` are the read-back a renderer actually consumes, and they
*are* ported and golden-pinned; whichever tier grows a skinned-mesh binding
owns the write-back. Same precedent and same reasoning as `bvh.rs` omitting
`bakeMesh`.

The source's `opts.userData` and `opts.actor` are dropped: opaque
back-pointers to the owning AI actor, read by nothing in this file.

The `_sleepWritten` flag (`:676-677`) belongs to `writeToSkeleton` and goes
with it.

## The traps, checked by name

**`Float32Array` storage width.** This file is the worst case for it. Grepped
first, as instructed. The source mixes widths deliberately:

| source field                                                        | width |
|---------------------------------------------------------------------|-------|
| `boneHead` `boneTail` `boneParent` `selfPairs`                        | `i32` |
| `boneLen` `boneRadius` `boneMass` `boneCone` `boneTwist` `boneUp`     | `f32` |
| `px py pz qx qy qz invMass`                                           | `f64` |
| `aabb` (a plain object, not a typed array)                            | `f64` |

Every one is matched. The non-obvious consequences, all ported:

- `boneLen[i] = Math.hypot(...)` computes in `f64` and stores `f32`; the
  *next line's* `if (boneLen[i] < 1e-4)` reads the rounded value back, and the
  clamp it writes is `f32(1e-4)` = 9.999999747378752e-5, not the `f64` literal.
- `pm[a] += boneMass[i] * 0.5` accumulates a `f64` running mass from a value
  that has already been rounded to `f32`, so `invMass` inherits the rounding.
- `boneRadius[i] + boneRadius[j]` in the self-pair prefilter and in
  `_solveSelf` is a `f64` sum of two `f32`-rounded radii.
- `boneUp` is read, normalised in `f64`, and re-quantised to `f32` every
  single step by `_transportUp`.
- `Contacts` (`bvh::Contacts`) is `f32` too, so `depth`, `s`, `nx/ny/nz` all
  come into `_solveContacts` pre-rounded.

Measured cost of getting this wrong, on the `standing_drop_on_floor` golden
(max absolute particle-position delta vs the unmodified original):

| mutation                       | 1 step  | 60      | 300     |
|--------------------------------|---------|---------|---------|
| `boneLen` held as `f64`        | 8.6e-9  | 3.1e-7  | 2.7e-6  |
| `boneRadius` held as `f64`     | 2.1e-10 | 5.1e-8  | 8.8e-7  |
| `boneUp` held as `f64`         | 0       | 0       | 0       |

`boneUp` moves no particle at all (it feeds only `getBoneTransform`), so a
position tolerance can never catch it. That is why the test *additionally*
asserts every golden `f32` value round-trips through `f32` and that the port's
own `boneUp` does too — a check a tolerance cannot express.

**`sign` is not `signum`.** No `Math.sign` in this file, and no hand-rolled
sign selection. Not applicable.

**Matrix storage order (column-major).** Two sites, both handled explicitly:

- `opts.transform` is a `THREE.Matrix4`; `Vector3.applyMatrix4` indexes
  `elements` column-major (`e[0] e[4] e[8] e[12]` is the first *row*) and
  includes a perspective divide. `RagdollOpts::transform` is documented as
  column-major `[f64; 16]` — the same convention `math::ray_obb` already
  takes — and the capture records the raw `elements` so no re-derivation
  happens on either side.
- `getBoneTransform` calls `Matrix4.set(...)`, which takes its sixteen
  arguments **row-major** and writes them into a **column-major** `elements`.
  The source's `set(xx, dx, zx, 0, xy, dy, zy, 0, xz, dz, zz, 0, 0,0,0,1)`
  therefore produces the basis whose *columns* are `X=(xx,xy,xz)`,
  `Y=(dx,dy,dz)`, `Z=(zx,zy,zz)` — Y down the bone, the THREE convention.
  `setFromRotationMatrix` then names elements `m11..m33` in row,column order
  (`m12 = te[4]`). Transcribing this row-major instead transposes the rotation
  and flips every off-diagonal quaternion sign, which is precisely the
  `a9bf4781` inertia-tensor failure. The mapping is spelled out at the call
  site and pinned by 1,191 golden bone quaternions.

**Euler order.** No Euler angles anywhere in this file. The only orientation
construction is the Rodrigues rotation in `_solveCones`, transcribed
term-for-term, and `setFromRotationMatrix`.

**Float arithmetic is not associative.** Nothing was tidied. Notable places
where the clumsy form is the correct form: `(d - boneLen[i]) / d / w` (two
sequential divides, *not* `/(d*w)`), `ax*ca + cross_x*sa + kx*kdot*(1-ca)`
(left-to-right, no factoring), `w0*w0*wa + w1*w1*wc`, and the Kahan
accumulation inside `hypot3`.

**`Math.hypot` is not `sqrt(x*x + y*y + z*z)`.** Eighteen call sites. V8's
`MathHypot` (a Torque builtin) takes the largest absolute argument, divides
every argument by it, sums the squares with Kahan compensation, then
`sqrt(sum) * max`. It is transcribed as `ragdoll::hypot3`.

Verification, because a remembered transcription is worthless: the capture
script contains the same routine in JavaScript and compares it to the engine's
own `Math.hypot` over **500,000 random triples spanning 12 decades** before it
writes a byte. Result: **0 mismatches** for the transcription, **205,887
mismatches (41.2%)** for the naive `sqrt` form. The capture then records 523
input triples with V8's exact answers, and `hypot3` is asserted **bit-for-bit**
against them in Rust (there is no libm involved — it is `/ * - +` and `sqrt`,
all IEEE-exact — so anything looser would be hiding a bug).

Interestingly, substituting the naive form *in the simulation* moves positions
by at most 3.1e-9 over 600 steps: the trajectory alone cannot police it. The
direct `hypot3` golden is what does.

**`Math.round` is not `f64::round`.** JS rounds half towards `+Infinity`
(`Math.round(-2.5) === -2`); Rust rounds half away from zero
(`(-2.5f64).round() == -3.0`). The constructor's millimetre particle-merge key
is `Math.round(v * 1000)`, and that key decides whether two bone endpoints
*become the same particle*. `js_round` implements the ECMA-262 behaviour, and
deliberately not as `floor(x + 0.5)` — that form is wrong for
`0.49999999999999994`, where the addition itself rounds up to `1.0`.

**Dead computation is still part of the source.** The constructor writes
`boneUp[i*3..+3] = (0, 0, 1)` and then, in the next loop, `_initUp(i)`
overwrites all three components before anything reads them. Ported with a
comment rather than dropped.

**An enum used as a table index.** `SURFACE_PROPS[world.surface[tri]]` — the
existing `Surface`/`SURFACE_PROPS` pairing from the already-ported
`surfaces.rs`, reused rather than re-declared, so no reindexing risk. The
source guards the lookup with `if (sp)`; `Surface` is a closed enum here, so
the lookup is total and the guard has nothing to guard. Noted at the site.

**Your comparator can be the bug.** The tolerances below are measured, not
picked; see "Tolerances".

## Source quirk found: the humanoid rig is five disconnected pieces

`ragdoll.js`'s own header states that "joints are shared particles, so joint
separation is impossible by construction and only the *angular* limits need
constraints". That is true of the chains — but `humanoidSpec`'s coordinates
defeat it at every limb root:

| child           | child head        | parent        | parent endpoint |
|-----------------|-------------------|---------------|-----------------|
| `upperArmL` (5) | `(-0.105h, 0.815h, 0)` | `chest` (2) tail  | `(0, 0.83h, 0)` |
| `upperArmR` (8) | `( 0.105h, 0.815h, 0)` | `chest` (2) tail  | `(0, 0.83h, 0)` |
| `thighL` (11)   | `(-0.055h, 0.53h, 0)`  | `pelvis` (0) head | `(0, 0.53h, 0)` |
| `thighR` (13)   | `( 0.055h, 0.53h, 0)`  | `pelvis` (0) head | `(0, 0.53h, 0)` |

None of those pairs round to the same millimetre, so none share a particle.
The 15-bone humanoid resolves to **20 particles in five disconnected islands**
— spine+head (6), each arm (4), each leg (3) — coupled only by the *cone*
constraint, which constrains direction and translates a limb bodily rather
than pinning it to anything.

The visible consequence: a doll dropped 35 cm onto a flat floor does not
settle as a body. It flattens to ~10 cm tall and spreads to **4.08 metres
wide** before falling asleep at step 219, with the limbs sliding away from the
torso. Even a 5 cm drop reaches 3.8 m within half a second.

This is ported faithfully and pinned by
`source_quirk_the_humanoid_rig_is_five_disconnected_islands`, which
reconstructs the particle graph with union-find and asserts exactly five
components. It is **not** silently fixed: the fix is a change to the rig
coordinates (welding each limb root to its parent endpoint), which would
change every captured trajectory and is a gameplay/art decision, not a
transcription one. Flagged for whoever owns the ragdoll's visual quality.

Two smaller quirks, also pinned:

- `boneParent[i] = s.parent ?? -1` uses null-coalescing, not truthiness, so a
  spec declaring `parent: 0` keeps `0` and does not silently become a root.
  `||` here would detach the chain. (`source_quirk_parent_zero_survives_the_null_coalesce`)
- `boneCone[0]` is `0` for the pelvis — a *zero-width* cone, which would pin
  the bone rigidly to its parent's direction. It never fires only because the
  pelvis is a root and `_solveCones` skips it on `parent < 0`. Asserted.

## Golden capture

`apps/shmup/tests/physics_ragdoll/capture.mjs` → `golden.json` (1.9 MB,
byte-reproducible — re-running produces an identical SHA-256; there is no
capture timestamp, no clock and no unseeded RNG in it).

Node 24, three r180, importing `C:/dev/Claude-of-Duty` read-only by absolute
`file:` URL. The triangle soup of each world is recorded through the same
`StaticWorld.prototype.addTriangles` hook `tests/rigidbody/capture.mjs` uses,
so the Rust test rebuilds a byte-identical BVH.

Sections:

| section      | what it pins |
|--------------|--------------|
| `hypot3`     | 523 input triples + V8's exact `Math.hypot`, plus the 500k-triple self-check counts |
| `trig`       | `cos`/`sin` of all 17 distinct cone/twist limits, and `acos` at 14 arguments |
| `specs`      | `humanoidSpec()` and `humanoidSpec(1.62, 61.5)`, every field |
| `edgeCases`  | a degenerate zero-length bone (length clamp through the `f32` store) and dispose-stops-stepping |
| `scenarios`  | 5 runs: full construction state + a several-hundred-step trajectory each |

Scenarios:

| name | world | steps @ dt | recorded | what it isolates |
|---|---|---|---|---|
| `standing_drop_on_floor` | floor | 420 @ 1/120 | every step | the reference case; falls asleep at 219 |
| `impulse_headshot_floor_wall` | floor+wall | 480 @ 1/120 | every 2nd | `applyImpulse`, two surfaces (concrete 0.92 / metal 0.52), custom gravity+iterations |
| `set_velocity_tumble_ramp` | floor+20° ramp | 480 @ 1/120 | every 2nd | rotated `applyMatrix4`, `setVelocity`, oblique contact normals, 8 iterations, never sleeps |
| `free_fall_no_world` | none | 300 @ 1/90 | every 2nd | Verlet + distance + cone + self-collision with the BVH out of the picture |
| `custom_spec_defaults` | floor | 300 @ 1/120 | every step | every `??` default, `parent: 0`, a 3-bone chain |

Each records: full construction state (all 17 arrays plus the AABB), per-step particle
positions, and at 19 sampled steps the `boneUp` array, AABB, sleep state, age,
all 15 bone transforms (position + quaternion) and all 15 bone capsules.

Total: 1350 recorded trajectory steps, 66,600 pinned particle coordinates and 1,191 bone transforms.

## Tolerances, and where the numbers came from

Every figure was **measured** by running the original JavaScript twice — once
unmodified, once with one `Math.*` wrapped to nudge its result by one ULP —
and taking the largest divergence over the whole trajectory.

| perturbation of the original | max position delta over 600 steps |
|---|---|
| `Math.acos` +1 ULP | **0** |
| `Math.cos` +1 ULP | 3.11e-9 |
| `Math.sin` +1 ULP | (same order) |
| `Math.hypot` → naive `sqrt` | 3.11e-9 |

`acos` contributes **nothing** because `_solveCones` uses its result only to
*gate* the correction (`if (angle <= cone) continue`) — the Rodrigues rotation
that follows turns by `cone`, never by `angle` — and the only other `acos`, in
`_transportUp`, feeds `boneUp`, which no particle ever reads.

So `cos`/`sin` are the only transcendentals on the critical path, and the
chosen tolerances are:

| quantity | tolerance | justification |
|---|---|---|
| particle positions, AABB, capsule endpoints, age, sleepTimer | `1e-7` | ~32x above the measured 1-ULP `cos` effect (3.11e-9), ~9x below the smallest storage-width defect it must catch (8.8e-7). Tolerates roughly a 30-ULP libm disagreement. |
| `boneUp` components | `1e-6` | measured worst case 1.192e-7 — exactly one `f32` quantum at unit magnitude |
| bone quaternion components | `1e-6` | measured worst case 4.686e-8 |
| `hypot3` | **bit-exact** | no libm involved; anything looser hides a transcription bug |
| construction arrays, specs, `selfPairs`, counts, masks, `firstSleepStep` | **exact** | pure `+ - * /` and `hypot3`, all bit-reproducible |
| `cos`/`sin`/`acos` vs V8 | relative `1e-15` | a diagnostic instrument, not a gate — see below |

The first sleeping step is asserted **exactly**. It is not a knife-edge
comparison: it stays at 219 under a **256-ULP** perturbation of `cos` or
`sin`.

The `libm_agrees_with_v8_on_every_angle_the_solver_uses` test exists so that
if a trajectory ever does drift, the next reader can tell in one run whether
Rust's libm disagrees with V8 on the seventeen cone/twist angles or whether the port
is wrong. It also prints how many of them match bit-for-bit — if all of them
do, the trajectories are bit-exact and the tolerance is slack, not budget.

## Dependency on the already-ported BVH

Four of the five scenarios drive `bvh::StaticWorld::overlap_capsule` every
iteration of every step. That arm is already golden-verified by
`tests/physics_port.rs`, but this file's trajectories now depend on it being
faithful to well under 1e-7 — a stronger requirement than any previous test
put on it. If the ragdoll trajectories fail at integration while
`free_fall_no_world` (which uses no world at all) passes, the fault is in the
contact arm, not here. That split is the reason `free_fall_no_world` exists.

One known, pre-existing `bvh.rs` divergence to be aware of: its `build()`
normalises triangle normals with `sqrt(x*x+y*y+z*z)` where the source uses
`Math.hypot`, documented there as absorbed by the subsequent `f32`
truncation. That claim now has 66,600 more coordinates resting on it.

## Wiring the orchestrator must do

`apps/shmup/src/physics/mod.rs`: `pub mod ragdoll;`

No `Cargo.toml`, `lib.rs` or `app.toml` change is needed — `serde_json` (with
`arbitrary_precision`, which this test relies on for exact golden equality) is
already a dev-dependency.
