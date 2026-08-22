# `ai/{rig,clips,animator}.js` — the soldier's skeleton and animation stack

| source | lines | target |
|---|---|---|
| `src/ai/rig.js` | 265 | `apps/shmup/src/ai/rig.rs` |
| `src/ai/clips.js` | 354 | `apps/shmup/src/ai/clips.rs` |
| `src/ai/animator.js` | 559 | `apps/shmup/src/ai/animator.rs` |

Golden: `apps/shmup/tests/ai_animation/{capture.mjs,golden.json}` (939 KB,
byte-reproducible). Test: `apps/shmup/tests/ai_animation_port.rs`.

Ported in full. Nothing from these three files is deferred.

## Wiring the orchestrator must apply

```
apps/shmup/src/ai/mod.rs: pub mod animator;
apps/shmup/src/ai/mod.rs: pub mod clips;
apps/shmup/src/ai/mod.rs: pub mod rig;
```

No `Cargo.toml` change. `ai/mod.rs`'s module doc currently lists `rig.js`,
`animator.js` and `clips.js` under "What is deliberately not in this slice"
(lines 12-18) — that paragraph is now false for those three and needs
narrowing to `soldier.js`/`textures.js`/etc. Likewise `ai/agent.rs`'s header
(lines 6-9) and `ai/grounding.rs`'s "The animator seam" section (lines 24-31),
which says `animator.js` "is not ported in this slice" and that a real
`FootSource` "can bind to the animator once it lands". It has landed:
`impl FootSource for Animator` is in `animator.rs`.

## What each file became

**`rig.rs`** is `class Rig` plus the module-level bone table, the two-bone
author-time elbow solve, and the bore/grip derivation — `rig.js:22-241`. It is
the *shared, immutable* half: one `RIG` for every soldier, exactly as the source
has it.

`createSkeleton()` (`rig.js:243-261`) is **not** here. It builds a fresh
`THREE.Bone` hierarchy and a `THREE.Skeleton` per actor; the hierarchy is
ported, as `animator::Skeleton`, because the animator owns and mutates it every
frame and there is no second owner to hand it to in Rust. `THREE.Skeleton`
itself (`boneInverses`, the bone-matrix `Float32Array` texture) is *skinning*
state that only the unported `SkinnedMesh` reads, so it has no counterpart.

**`clips.rs`** is `clips.js` one-for-one. Clip functions take `&mut Poser`
where the source takes `P`; `Poser` is defined in `animator.rs` because that is
where the source defines it, and an intra-crate module cycle is fine in Rust.
`CLIPS` becomes `ClipId` (six variants, source order) with an `eval` that folds
in the source's `?? C.idle` fallback — an unknown clip name is unrepresentable,
so every variant resolves.

**`animator.rs`** is the whole runtime: `Poser`, the layered blend, the four IK
solvers, the two-bone analytic solve, the muzzle outputs — plus the pieces of
`THREE` the file leans on and that nothing else in the port had yet:
`Matrix4` (`compose`, `multiplyMatrices`, `determinant`, `decompose`),
`Vector3.applyMatrix4`, `Quaternion.setFromAxisAngle`/`setFromUnitVectors`, and
the `Object3D` transform graph (`updateMatrix`, `updateMatrixWorld`,
`updateWorldMatrix(true,false)`, `getWorldQuaternion`). All transcribed from
`node_modules/three/src/math/*.js` and `core/Object3D.js` at r180 rather than
re-derived. `V3`/`Q` are reused from `weapons::rig_math` rather than duplicated;
`Mat4` is new because that module deliberately has none (the viewmodel rig never
materialises one).

## The traps, each checked by name

**`Float32Array` — the one that matters here.** Grepped all three files:
exactly one hit, `animator.js:36`, `this.d3 = new Float32Array(rig.count * 3)`.
That is the pose accumulator every clip writes through, so **every layer's
`+=` is rounded to `f32` on store and read back rounded**, and the eight
layers of a busy frame accumulate through that rounding. `Poser::d3` is
`Vec<f32>` and `Poser::d` does `(d3[i] as f64 + x * w) as f32`. `hipOff` is a
`THREE.Vector3` — plain JS numbers — and stays `f64`. Pinned directly by
`the_layered_pose_accumulates_through_f32`, which stacks eight weighted layers
and additionally asserts every stored value is exactly `f32`-representable.

**Euler order.** Grepped for `setFromEuler`, `.order`, `'XYZ'`, `'YXZ'` across
all three files. Exactly **one** site: `animator.js:311`, and it passes an
**explicit `'XYZ'`**. `weapons::rig_math::Q::from_euler_xyz` is a line-for-line
transcription of three's `case 'XYZ'` branch (verified against
`math/Quaternion.js` in this session) and is what the port uses.
`axiom_math::Quat::from_euler_xyz` composes the opposite way and is not used.

`'YXZ'` **does** appear in `src/ai/` — `parts.js:44` and `weapon.js:28` — but
in neither of my files. The orchestrator relayed a warning that "Three r180's
Euler default for this character code is `'YXZ'`, not `from_euler_xyz`"; that
is true of those two files and false of these three. See "Cross-slice" below.

The only other Euler in the stack is the actor group's `group.rotation.y = yaw`
(`agent.js:927`). `Euler.DEFAULT_ORDER` is `'XYZ'` (`math/Euler.js:446`,
checked), and a single-axis rotation is order-independent anyway.

**Matrix storage order.** `Mat4::e` is THREE's `elements` verbatim:
column-major, translation at `e[12..14]`, which is what `_wp`
(`animator.js:328-331`) reads. `makeBasis(x,y,z)` puts the basis vectors in
*columns*, which is exactly the role `Q::from_basis` takes them in.
`the_matrix_is_column_major_like_three` pins the convention directly (a +90 deg
yaw's `-1` lands in `e[2]`, not `e[8]`), on top of the golden's 25 real bind
matrices.

**`Math.hypot`.** One occurrence in the slice, `rig.js:239` in
`distanceToBone`. Uses `crate::jsmath::hypot3` rather than a seventh local copy.
Measured against the golden's own captured values: V8's Kahan form reproduces
every one **bit for bit** (delta 0.0 across all 150 bone/probe pairs), where
`sqrt(x*x+y*y+z*z)` is off by up to 4.4e-16.

**`Math.sign` / `Math.round`.** Neither appears in any of the three files.
Checked, not assumed.

**Float arithmetic is not associative.** Nothing is folded or reordered,
including inside the THREE bodies. `Matrix4.determinant`'s term order in
particular is transcribed as written even though it reads badly.

**Enum ordering.** `BONES` is the source's row order exactly — every bone is
addressed by index (the pose accumulator, the hitbox capsules, the ragdoll
hand-off). `ClipId` and `HitRegion` follow the source's object-literal and
`switch`-arm order respectively; neither is index-addressed in the source, so
this is discipline rather than necessity, and it is stated as such at both
sites.

## Verification beyond the golden

Two extra passes, because none of this compiles until integration:

1. **Algorithm mirror.** I transcribed `rig.rs` *back* into JavaScript (from the
   Rust, not from `rig.js`) and diffed the result against the golden: worst
   absolute delta **5.6e-17**, one ULP, on `localPos[19].y`. That exercises the
   whole bind-pose construction — bone table, branch-point tail selection, basis
   construction, quaternion conversion, parent-space transforms.
2. **Numeric-literal stream diff.** Every numeric literal in each `.rs`, in
   order, against its `.js`, with comments and strings stripped:
   - `clips.rs`: **zero** substitutions, **zero** Rust-only literals, five
     JS-only literals — all of them default-parameter values Rust has no
     equivalent for (`lobe`'s `k = 1.4`, `aimAdd`'s `w = 1`, `recoilAdd`'s
     `strength = 1`, `hitAdd`'s `dirSide = 0, strength = 1`).
   - `rig.rs`: every value constant present (`0.075`, `-1`, `-2`, `1e-10`,
     `0.985`, `1.665`, `1e-12`); the JS-only remainder is loop counters,
     `spec[2]`/`[3]`/`[4]` index literals, and `createSkeleton`'s body.
   - `animator.rs`: multiset comparison (the file has 765 literals to
     `animator.js`'s 322, from the THREE transcription). Exactly two
     `animator.js` literals appear less often in the Rust: `2.4` and `0.85` —
     the `reload`/`vault` default-parameter values, whose *initial field* values
     are present in both.

## What the golden pins

`capture.mjs` runs the original under Node 24 and writes:

- **the whole rig** — names, parents, children, `bindPos`, `bindQuat`,
  `localPos`, `localQuat`, `tail`, `length`, `count`, `eyeHeight`,
  `BORE_ORIGIN`/`BORE_DIR`/`GRIP_R`/`GRIP_L`, the raw `BONES` rows, and
  `distanceToBone` for **every bone** at six probe points;
- **every clip** at ten phases including `0`, `0.99999` and `1` (the wrap
  boundary), plus `aimAdd` × 5 weights, `turnStep` × 12, `vault` × 9,
  `recoilAdd` × 16, `hitAdd` × **126** (7 regions × 3 `dirSide` × 6 `t`,
  crossing both guards), `suppressAdd` × 5, `reloadAdd` × 7;
- **the f32 layering case** — eight weighted layers into one `d3`;
- **the animator's construction constants** — the five HandR-local anchors with
  and without a weapon, and the freshly-built skeleton's 25 world matrices;
- **six multi-frame trajectories, 220 frames**, each recording `d3`, the hip
  offset, `phase`, `blend`, all five one-shot timers, `reloading`/`vaulting`,
  the three muzzle outputs, both foot world positions, and (every third frame
  plus the last) all 25 bone `matrixWorld`s:

  | scenario | frames | what it exercises |
  |---|---|---|
  | `locomotion-no-ik` | 12 | the pose write alone: no probe, no aim, no look |
  | `crossfade-idle-walk-run` | 24 | two clip transitions with `blend < 1` live, + aim/look/support-hand IK |
  | `all-layers-tilted-floor` | 42 | crossfade + aim + suppress + recoil + hit + reload + turn, foot IK on a tilted plane, actor scale 1.08, moving actor and moving aim target |
  | `vault-override` | 64 | the override layer that switches off foot IK, aim IK and the support hand, then hands back |
  | `reload-hand-path-miss-probe` | 66 | all five segments of the reload hand path, and the probe-misses arm of `_footIk` |
  | `phase-wrap` | 12 | `dt = 0.25` at stride 2.93 Hz, so `phase % 1` wraps every frame |

  `all-layers-tilted-floor` is the one the brief asks for: a single-frame pin
  cannot catch a blend-order bug, and this one has up to seven layers live at
  once across 42 frames with two clip transitions inside them.

- **the disabled case** — `enabled = false` returns before anything.

Tolerances are tabulated at the top of `ai_animation_port.rs`. Timers, `phase`
and `blend` are asserted **exactly** (pure `+ - * /` and `%`); the bind pose at
`1e-12`; `d3` at `1e-6` relative (under one `f32` ULP at these magnitudes);
world matrices and muzzle outputs at `1e-9`.

## The two synthetic inputs

The capture authors two things the source would otherwise drag in whole
subsystems for. Both are **inputs**, emitted into the golden so the Rust side
reads rather than re-derives them:

1. **The weapon anchors.** `Animator` reads four bind-space points off
   `def.weapon` (`ai/weapon.js:284-289`). Building a real one means running the
   whole weapon geometry pipeline, a different slice. Four fixed arrays stand
   in.
2. **The ground probe.** `probeGround` (`ai/index.js:433`) is a physics raycast.
   A tilted plane `y = 0.06x - 0.04z` stands in, so foot IK does real work
   (pelvis drop *and* sole roll) with no transcendental of its own. Its
   coefficients **and its already-normalised normal** are in the golden, so the
   only thing either side transcribes independently is one multiply-add. A
   second scenario uses an always-miss probe to reach `_footIk`'s `!ok` arm.

This is the "GLSL held in a JS string" situation the recipe warns about, kept as
small as it can be: one line, and its constants come from the capture.

## Source quirks, ported not fixed

- **`hitAdd`'s guard disagrees with its own doc comment.** The comment says the
  reaction is "0.45 s long"; the guard is `if (t > 0.5) return;`. Carried, and
  pinned by `the_one_shot_guards_cut_off_where_the_source_says` with samples at
  `t = 0.45`, `0.5` and `0.6`.
- **`solveElbow`'s degenerate-pole fallback is written `-axis.y * -1`**
  (`rig.js:44`). The double negation is correct — it *is* the perpendicular
  projection of `(0,-1,0)` — but it is transcribed as written, not simplified.
- **`Rig`'s branch-point averaging arm is unreachable.** `rig.js:174-178` falls
  back to the mean of a bone's children when every child is a `Clavicle` or an
  `UpLeg`. The two branch points are `Hips` (whose first non-matching child is
  `Spine`) and `Spine2` (`Neck`), so it never runs. Ported anyway, with a
  comment, per "dead computation in the source is still part of the source."
- **Four dead declarations, all kept.** `rig.js`'s `H = 1.8` and `HAND = 0.095`
  are never read. `animator.js`'s `this.time` is written by `update` and read
  nowhere; `this._aimApplied` is written once and read nowhere; `this.armR` is
  built and read nowhere (the right arm is posed by clips, never solved);
  `state.hurt` is settable and read nowhere; `opts.rng` is stored and never
  drawn from. Each is present with a comment saying so.
- **Mixed exits in `_lookAt`.** A degenerate `want` `return`s out of the whole
  chain (so the head never solves that frame), while a small angle or a
  degenerate axis only `continue`s. Carried exactly.
- **`_supportHandIk` reads `hand.matrixWorld` before the vault guard**
  (`animator.js:423-425`), so the read is wasted on a vaulting frame. Kept in
  the source's order.
- **`_aimIk` reads `_wq` before `hand.matrixWorld`** (`:373` then `:374`), and
  `getWorldQuaternion` refreshes `matrixWorld` as a side effect. My first draft
  had these swapped; corrected, with a comment at the site. `_updateMuzzle`
  (`:547-551`) has the opposite order, also preserved.

## Divergences, and why

- **`Animator::new` builds its own skeleton** instead of taking a `bones` array.
  The source's `Animator` borrows the array `agent.js` built one line earlier;
  the IK mutates it every frame, so in Rust it has to be owned, and the animator
  is the only owner that makes sense.
- **`Animator::set_actor(position, yaw)`** is new — it is `agent.js:925-927`'s
  three lines (`group.position`, `group.rotation.y`, `updateMatrixWorld(true)`).
  The animator owns the group node, so the agent's placement has to enter
  through it.
- **The preallocated scratch becomes locals.** `V3`/`Q`/`Mat4` are `Copy`, so
  THREE's allocation-avoidance trick has no purpose. Checked slot by slot that
  none carries state across a call. The one place where aliasing *is*
  load-bearing — `_applyWorld` inverting `_qa` **after** `cur` was built from it
  (`:344-348`) — is reproduced by sequencing.
- **`setState` takes `StateUpdate`, with `Option<Option<V3>>` for the two
  targets.** `s.aimTarget !== undefined` and `s.aimTarget === null` are
  different things in the source: the second clears the target.
- **`ProbeOut`, `_footY`, `_footN` are locals**, not fields — nothing reads them
  across frames.
- **Default parameters** (`fire(strength=1)`, `hit(region='torso', side=1,
  strength=1)`, `reload(duration=2.4)`, `vault(duration=0.85)`,
  `aimAdd(w=1)`, `recoilAdd(strength=1)`, `hitAdd(dirSide=0, strength=1)`,
  `lobe(k=1.4)`) become required arguments. Every default is named in the
  doc comment at its site.
- **`rig.index` panics** where the source throws, with the same message. Every
  call site passes a table constant.

## Cross-slice

**To the orchestrator, re: the `soldier.rs` assumptions.**

- `RIG` is `LazyLock<Rig>`, not a `const`. `RIG.index(...)`, `RIG.count`,
  `RIG.names[i]` all work through `Deref`; `&RIG` coerces to `&Rig`.
- `Rig::index(name: &str) -> usize` — as assumed. ✓
- `Rig::bind_pos` is a **field** (`Vec<V3>`), matching the source's
  `this.bindPos` array. I added `Rig::bind_pos_of(name) -> [f64; 3]` and
  `Rig::bind_pos_at(i) -> [f64; 3]` for exactly the shape `soldier.rs` wants.
- **`GRIP_R`, `GRIP_L` and `BORE_DIR` are `LazyLock<[f64; 3]>`, not `const`** —
  all three are derived through `f64::sqrt` (`normalize`, plus a two-bone solve
  for the grips), and `sqrt` is not a stable `const fn`. Writing them as literal
  arrays would mean hand-transcribing a computed constant, which is the exact
  transcription step the recipe bans; the repo precedent is
  `player::tuning::JUMP_SPEED`. They live in `ai::rig` with element type
  `[f64; 3]` as the two other agents assumed, and `GRIP_R[0]` works unchanged
  through `Deref` — **only a by-value use needs `*GRIP_R`**. `BORE_ORIGIN` *is*
  a real `const [f64; 3]`.
- **The `'YXZ'` warning does not apply to this slice.** `parts.js:44` and
  `weapon.js:28` use `'YXZ'`; `rig.js`/`clips.js`/`animator.js` contain exactly
  one `setFromEuler`, with an explicit `'XYZ'`. Do not share a Euler helper
  across that boundary — they are different rotations for the same three
  angles. (`weapons::rig_math` has both: `from_euler_xyz` and `to_euler_yxz`.
  The `'YXZ'` *forward* conversion does not exist there yet and those two
  slices will need it.)
- `GRIP_L` is used in `rig.js` (it is `HandL`'s bind position), so it is not
  dead here — only its *import* into `soldier.js`/`weapon.js` is.

**Exact integration fixups this slice creates in files I did not touch.** I read
`soldier.rs`, `weapon.rs` and `geo.rs` as they stand and listed every site:

| site | now reads | must read |
|---|---|---|
| `ai/soldier.rs:311` | `RIG.bind_pos[RIG.index(name)]` | `RIG.bind_pos_of(name)` — `bind_pos` is `Vec<V3>`, matching `this.bindPos` |
| `ai/soldier.rs:466` | `occ(GRIP_R, [...])` | `occ(*GRIP_R, [...])` |
| `ai/soldier.rs:1362` | `normalize3(BORE_DIR)` | `normalize3(*BORE_DIR)` |
| `ai/weapon.rs:351` | `normalize(BORE_DIR)` | `normalize(*BORE_DIR)` |

Indexed uses (`GRIP_R[0]`, `BORE_DIR[1]`, …) and `&RIG` / `RIG.count` /
`RIG.index(…)` all work unchanged through `Deref`; only **by-value** uses of the
three `LazyLock` statics need the `*`. That is four characters total, and it is
the price of not hand-transcribing a `sqrt`-derived constant.

One more, and it is not cosmetic: **`geo.rs:982` declares
`trait CharacterRig { fn index(&self, name: &str) -> u16; fn
distance_to_bone(&self, i: u16, …) -> f64; }`, and `soldier.rs:648` passes
`&RIG` into `CharacterBuilder::new`.** My `Rig` uses `usize` for both (the
source indexes a JS array; `usize` is what every other call site in this slice
wants, and `u16` would need a cast at each). Nothing implements the trait yet.
The bridge is four lines and belongs next to the trait or at the composition
point, not in `rig.rs` — putting it there would make my file depend on a
concurrently-edited one:

```rust
impl CharacterRig for Rig {
    fn index(&self, name: &str) -> u16 { Rig::index(self, name) as u16 }
    fn distance_to_bone(&self, i: u16, x: f64, y: f64, z: f64) -> f64 {
        Rig::distance_to_bone(self, i as usize, x, y, z)
    }
}
```

`geo.rs`'s `seg_dist` already routes through `jsmath::hypot3`, so it and
`Rig::distance_to_bone` agree bit for bit — checked, not assumed.

**Consumed seams.** `impl FootSource for Animator` satisfies
`grounding::FootSource` directly: `Foot::Right`/`Left` map to `bone_pos("FootR")`
/`("FootL")`, and the `None` arm is the source's `Number.isFinite` guard.
`grounding.rs` was not edited.

**Not consumed.** `ai/agent.rs` was not edited; the animator seam there
(`this.animator.turn`, `.vault`, `.reload`, `.reloading`, `.vaulting`, `.hit`,
`.muzzleWorld`, `.bonePos`, `.enabled`, `.setState`, `.update`) is all public on
`Animator` under the same names, so wiring it is mechanical.

**`crate::jsmath`.** `rig.rs` uses `jsmath::hypot3` (no seventh local copy).
`animator.rs` needs none — `animator.js` has no `Math.hypot`; its distances are
`Vector3.distanceTo`, which is `Math.sqrt(distanceToSquared)`, a genuinely
different function. The wiring queue lists `ai/animator.rs` as owing a jsmath
migration; it does not.
