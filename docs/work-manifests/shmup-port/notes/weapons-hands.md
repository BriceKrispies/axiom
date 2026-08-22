# `weapons/hands.js` → `apps/shmup/src/weapons/hands.rs`

Finishing one of the six half-finished ports listed as a hazard in
`06-parallel-port-plan.md`. The file was 374 Rust lines against 1163 JS lines
(~32%), compiled, was wired into `viewmodel.rs`, and had **no golden test** —
nothing signalled that it was incomplete.

## The audit: what the previous pass had, and what it was missing

The existing 374 lines carried three things and were correct on all three:

- `L_UPPER` / `L_FORE` and the six `HAND_POSES` entries, transcribed exactly.
- `Arm::solve` — the two-bone analytic IK — including the reach clamps, the
  `a`/`h` elbow-circle algebra, the pole projection and its degenerate
  re-seed, and `aimBone` with both nested roll-reference fallbacks.
- `hand_mirror_x()`, the chirality decision.

Everything else was absent. In source order:

| `hands.js` | what it is | lines | status before |
|---|---|---|---|
| 51-69 | `segment` — tapered chamfered finger capsule | 19 | missing |
| 72-76 | `segmentPad` — dorsal glove reinforcement | 5 | missing |
| 91-96 | `segmentSeam` — 1.5 mm stitched panel seam | 6 | missing |
| 102-147 | `buildFinger` — three nested curl groups | 46 | missing |
| 153-274 | `buildGlove` — palm, thenar, heel, knuckles, dorsal caps, seams, cuff, strap | 122 | missing |
| 295-330 | `buildThumb` + the `THUMB` dimension table | 36 | missing |
| 337-459 | `buildSleeve` — shell, joint mass, fold rings, wrinkle ridges, elbow pad, rolled cuff | 123 | missing |
| 514-658 | the constructor's scene graph (all of it, ~40 `Object3D`s) | 145 | scalars only |
| 690-866 | `Arm.fitToCylinder` — the build-time contact solve | 177 | missing |
| 897-919 | `Arm.bakeSurfaceMasks` + the four amplitude profiles | 23 | missing |
| 937-969 | `Arm.bakeContactAO` | 33 | missing |
| 972-983 | `setPose` — the `this.poses` override lookup | 12 | partial (no override map, no joints) |
| 986-993 | `setTrigger` | 8 | partial (no joints; inconsistent field) |
| 1044-1048 | `dispose` | 5 | n/a in Rust |

The previous pass's stated reason for stopping was that the mesh builders
"have no consumer yet (no material binding)". That is true of the *materials*
but not of the *geometry*: `fitToCylinder` is not a mesh feature, it is the
rig's most important behaviour, and it cannot exist without the joint
hierarchy the mesh builders create. The previous `viewmodel.rs` therefore fell
back to the authored `HAND_POSES.clamp` where the source uses a per-weapon
solved fit — a real behavioural divergence, now removed.

Two defects in the existing 374 lines, both fixed:

1. **`trigger_curl` was inconsistent with itself.** The constructor
   initialised it to `[0.55, 0.72, 0.34]` (a *curl*, positive) and
   `set_trigger` wrote `[-(0.55 + t*0.3), …]` (a *rotation*, negative). The
   field is now unambiguously the three joint rotations, is initialised by
   `set_pose` from the constructed pose exactly as the source's `setPose`
   does, and is written alongside the real joints.
2. **`set_pose` ignored `this.poses`.** The source's lookup is
   `this.poses[name] ?? HAND_POSES[name] ?? HAND_POSES.wrap`; the port had
   only the middle term, so a fitted per-weapon pose could never be restored
   after a clip swapped the hand to `open` and back. That is the exact leak
   the source's comment at `hands.js:649-654` exists to prevent.

## What was ported

All of it. `hands.rs` is now 1895 lines (the source's 1163 plus doc comments
carrying the source's own measured-and-rejected-alternative reasoning, which
is most of what makes this file diffable).

Structurally the one real decision was to carry the `THREE.Object3D`
hierarchy as an **arena** (`Arm::nodes`, with the source's child order
preserved) rather than to inline the chain. `fitToCylinder` and
`bakeContactAO` both walk the real transform chain — that is the entire point
of the contact solve, per `hands.js:660-676`: the analytic version was 8-14 mm
out on screen because it ignored the 0.88 Y-scale on the finger capsules, the
-6 mm palmar MCP offset, the fan-out rotation and the four different contact
clock angles. Reproducing "the same matrices the renderer will use" means
having the matrices.

That needed a `Matrix4`, which `rig_math.rs` did not have.

## `rig_math.rs` — TWO changes, both flagged loudly

**I own `rig_math.rs` for this wave and I changed it.**

### 1. Added `M4` (a new type), `V3::apply_matrix4`, `V3::distance_to_squared`

`M4` is `THREE.Matrix4` pared to the four operations the arm rig runs:
`compose` (what `Object3D.updateMatrix` calls), `multiplyMatrices` (what
`updateWorldMatrix` calls), `invert` (for `_fitInv`) and `Vector3.applyMatrix4`.
Each is transcribed element-for-element from `three@0.180`'s `Matrix4.js`
against **Three's column-major element order** (`e[0..4]` is the first column,
`e[12..15]` is the translation) with the source's own index literals intact.
`invert`'s singular case returns the all-zero matrix, as the source does,
rather than `None`.

This is purely additive — no existing item changed shape — so it should not
collide with the concurrent `viewmodel.rs` work.

### 2. FIXED a real defect in `V3::apply_quat` (reported by the `ai/weapon.js` agent)

`rig_math.rs` grouped Three's `applyQuaternion` as

```rust
vx + qw * tx + (qy * tz - qz * ty)
```

`three@0.180`'s `Vector3.js` actually writes

```js
this.x = vx + qw * tx + qy * tz - qz * ty;
```

which JS evaluates strictly left-to-right as `((vx + qw*tx) + qy*tz) - qz*ty`.
The parenthesised form is a **different sequence of roundings** and differs in
the last bits — the port recipe's "float arithmetic is not associative, do not
tidy an expression" trap, and easy to fall into here because the line above it
in the Three source is the comment `// v + q.w * t + cross( q.xyz, t );`,
which *suggests* the parenthesised grouping.

I read `C:/dev/Claude-of-Duty/node_modules/three/src/math/Vector3.js` and
confirmed it. Fixed to the literal left-to-right form on all three components.

It matters here: `hands.js:1038`'s `_up.set(0,1,0).applyQuaternion(targetQuat)`
is the forearm's roll reference, so every hand solve inherits the grouping. The
golden was captured **after** the fix, and `solve_matches_every_captured_configuration`
covers 30 configurations including four with non-identity hand quaternions, so
the fixed grouping is pinned.

## The golden

`tests/weapons_hands/capture.mjs` → `tests/weapons_hands/golden.json`
(3.15 MB, byte-reproducible — re-running `node capture.mjs > golden.json`
produces an identical file), read by `tests/weapons_hands_port.rs`.

Captured by running the **original** `hands.js` under Node 24 against the
source repo's own `three@0.180`. Nothing is hand-transcribed.

Contents:

- `HAND_POSES`, `THUMB`, `L_UPPER`/`L_FORE`.
- Three constructed `Arm`s — the two `viewmodel.js:130-149` really builds
  (right at scale 1, left at 0.97) plus one taking every `opts` default — each
  dumped as its whole scene graph in `root.traverse` order: 82 nodes, local
  position / rotation / quaternion / scale, and for every mesh its material
  slot, render flags, vertex and triangle counts, bounding box and position
  sum. For the right arm the **full** `position`/`normal`/`uv`/`index` buffers
  of all 45 meshes are dumped too — 25,314 vertices, 22,656 triangles.
- `setPose` for all six authored keys **and** for a bogus key, read back off
  the real joints; `setTrigger` at `t = 0, 0.25, 0.5, 0.75, 1`.
- `Arm.solve` over **30** configurations (below).
- `Arm.fitToCylinder` over **5** configurations (below).
- `Arm.bakeContactAO` over four successive passes.

### The solve cases, and why each one is there

Eight ordinary working-range poses (hipfire, handguard, rolled hand with both
an `'XYZ'`- and a `'YXZ'`-composed target quaternion, ADS, high, behind the
shoulder, cross-body), five bone-length variations (equal bones, short upper,
long upper, the 0.97-scaled arm, millimetre-scale bones), then the
degenerates:

| case | branch it forces |
|---|---|
| `beyond-reach`, `beyond-reach-far`, `exactly-maxD` | `d > maxD` clamp |
| `inside-minD`, `inside-minD-equal-bones` | `d < minD` clamp (incl. the `minD = 1e-4` equal-bones case) |
| `target-at-root`, `target-at-root-left` | `d < 1e-5` → `_t.set(0, 0, -minD)` |
| `target-1e-6-away` | the same branch reached from a nonzero but sub-threshold `d` |
| `zero-pole`, `zero-pole-left` | **zero-length pole vector** — `normalize()` divides by `length() \|\| 1`, so the pole stays zero, `_perp.lengthSq() < 1e-8` fires and the solver re-seeds from `(side, -1, 0)` |
| `pole-parallel-to-dir`, `pole-antiparallel-to-dir`, `pole-nearly-parallel` | the same re-seed from a *parallel* pole |
| `aimbone-fallback-1` | `aimBone`'s first `_by.lengthSq() < 1e-9` fallback |
| `aimbone-fallback-2`, `aimbone-fallback-2-offset` | the **nested** `_by.set(1,0,0)` fallback |
| `forearm-up-parallel` | the forearm's own `aimBone` hitting the fallback via the hand's local +Y |

**The `aimBone` fallbacks needed exact arithmetic to be reachable
deterministically.** They fire when the bone direction is parallel to the roll
reference, which happens exactly when `a == 0`, i.e. `l1² - l2² + d² == 0`. If
that only holds to within rounding, the branch selection depends on the last
bits and the two languages could legitimately disagree. So those cases use
`l1 = 0.375`, `l2 = 0.625`, `d = 0.5` — every square exactly representable, so
`a` is exactly `0`, `h` is exactly `0.375`, and the branch is forced in both
languages. `aimbone-fallback-2`'s captured upper-arm quaternion is
`(0.5, 0.5, 0.5, -0.5)`, which is the value the basis
`bx = (0,0,1), by = (1,0,0), bz = (0,1,0)` gives by hand — a useful
sanity check that the case really is on the branch it claims.

### Two guards in `solve` that are dead, and are ported anyway

`if (_hp.lengthSq() > 1e-12)` on each bone. By construction `|hp| == l1` and
`|hp2| == l2` for every input (that is what the elbow-circle solve produces),
so both are only false when a bone length is zero, and a zero bone length makes
`maxD` zero and drives `dir` to `NaN` before either guard is reached. They are
ported as written per the recipe's "dead computation in the source is still
part of the source"; no golden case can exercise them.

### The fit cases

Driven by `models/rifle.js:435-453`'s **real** `gripL` and `handguard` nodes
(`pos [-0.1, 0.0734, -0.2098]`, axis `[0, 0.075, 0]`, `dir [0,0,1]`,
`r = 0.0235 + 0.0036`), with the wrist quaternion produced by a transcription
of `viewmodel.js:89-98`'s `handBasis` — recorded literally in the golden, so
the Rust side consumes the number and never re-derives it (this slice does not
own `handBasis`).

1. `rifle-handguard` — the real thing on the real left arm.
2. `rifle-handguard-refit` — the **same** `poseName`, so `base` now resolves
   out of `this.poses` rather than out of `HAND_POSES`. This is the per-weapon
   override path the previous port was missing entirely.
3. `unknown-pose-key` — falls through both lookups to `HAND_POSES.clamp`.
   Note this fallback is **`clamp`**, where `setPose`'s is **`wrap`**; getting
   those the same way round is a real hazard, and both are asserted.
4. `fat-tilted-cylinder` — a wider, off-axis, tilted cylinder on the right arm
   at scale 1, so the axis projection in `gapAt` is not the trivial z-axis one
   and `axisDir` genuinely needs normalizing.
5. `unreachable-cylinder` — a tube the thumb cannot reach, so every scan parks
   at its limit. That saturated outcome is a real behaviour of the source and
   is worth pinning; the first draft of case 1 was accidentally saturated too,
   which is how I noticed that a plausible-looking fit case can pin almost
   nothing.

### Tolerances

- **Exact**: counts, triangle indices, pose tables, per-mesh material slot,
  and the *set* of vertices the contact-AO bake touches.
- **`1e-12`** on everything the rig computes in `f64` — node transforms,
  solved bone quaternions, fitted joint angles, contact points. The
  established figure on this port; `sin`/`cos`/`sqrt`/`atan2` are not
  bit-guaranteed between V8's libm and Rust's.
- **`1e-5`** on geometry attributes, matching `weapons_parts_barrel_port.rs`:
  each hand mesh is several `lathe_z`/`blob`/`box_geo`/`ring` calls merged and
  welded, so per-vertex error compounds across more independent trig calls
  than a single primitive's own golden does. A real algorithmic divergence
  shows up as a *count* mismatch first, and counts are exact.
- **`1e-7`** on the contact-AO mask, which is `f32`-wide in the source.

### One knife-edge, stated

`fitJoint` picks the argmin of a cost over 49 sampled angles. If two samples
ever tied to the last bit the two languages could pick different ones and the
fitted angle would differ by a whole grid step (0.035 rad) — far above any
tolerance, so the failure would be loud rather than silent. No captured case
is near a tie. The same applies to the thumb base's 21x15 grid.

## The one golden failure, and what it actually was

First run of the suite: **14 pass, 1 fail.**

```
first.glove[13].entry[0]: got 0.04947275295853615, golden 0.0494728684
                          diff 1.154e-7, tol 1e-7
```

The coordinator's read was right and worth recording: the gap is ~2.3e-6
*relative*, about **30x an f32 ULP** at that magnitude, so it was never going
to be a missing `as f32`. Something upstream was computing a different number.

### Measuring it rather than guessing

Glove-subtree node 13 is finger 0's **middle phalanx segment**, a 117-vertex
`lathe_z`. I dumped the same vertex from both sides:

| | JS | Rust (before) |
|---|---|---|
| `matrixWorld` | `-0.4800099562406106, …` | **bit-identical, all 16 elements** |
| contacts (all 5) | `0.02450465628656738, …` | **bit-identical** |
| raw vertex `x` | `3.121820784010601e-18` | `-2.228546458482583e-9` |
| raw vertex `y` | `-0.005872767884284258` | `-0.005872766487300396` |
| raw vertex `z` | `-0.02549160085618496` | `-0.02549159899353981` |
| distance to nearest contact | `9.411781306343694e-3` | `9.41178361741589267e-3` |

So the entire rig — the Euler-`'XYZ'` node tree, the `M4` chain, the fit
solve, the contacts — is **exact**. The only divergence is the vertex
position, by 3.2 nm.

The chain that turns 3.2 nm into 1.15e-7 is the smootherstep:

```
value = peak * s(t),  t = 1 - dist/radius,  s'(t) = 30 t^2 (1-t)^2
|dvalue/d(dist)| = peak * s'(t) / radius
```

At this vertex `t = 0.2157`, so `s' = 0.859` and the gain is
`0.7 * 0.859 / 0.012 = 50.1` per metre. Measured `Δdist = 2.311e-9`;
`50.1 * 2.311e-9 = 1.158e-7`, against the observed `1.154e-7`. The mechanism
is fully accounted for.

### The defect: an `f32` rotation where the source uses `f64`

`raw.x` should be ~0 — it is a lathe vertex on the seam column. JS has
`3.1e-18`; the port had `-2.23e-9`. That is not noise, it is a specific
number:

- The port built `rotateY(PI)` as `Mat4::from_quaternion(Quat::from_axis_angle(UNIT_Y, f32::PI))`
  — the geometry kit's `primitives/xform.rs` pattern. In `f32`,
  `cos(PI/2) = -4.371139e-8`, so the matrix carries an off-diagonal
  `m02 = 2w = -8.742278e-8`.
- Three's `BufferGeometry.rotateY` uses `Matrix4.makeRotationY(theta)`, whose
  off-diagonal is `sin(theta)` computed in **`f64`**: `sin(PI_f64) = 1.2246e-16`.

**Those differ by a factor of 7e8.** `x_new = c*x + s*z` with `x ≈ 0` and
`z = 0.0254916` gives `-8.742278e-8 * 0.0254916 = -2.2285e-9` — the observed
value to five digits.

This is exactly the port recipe's named trap: *"Compute in `f64`, store `f32`.
JS numbers are `f64` and Three computes in `f64` while storing
`Float32Array`."* Every one of this module's `translate`/`scale`/`rotate*`
calls was going through an all-`f32` path: an `f32` matrix, `f32` arithmetic
in `transform_point`, and `f32` literals (`0.88f32` is
`0.87999999523162841796875`, not `0.88`).

**The port was wrong; the golden was right.**

### The fix

`hands.rs` now carries a faithful `BufferGeometry.applyMatrix4`:

- `geo_apply` reads each component out of the `f32` buffer, widens to `f64`,
  transforms against `f64` matrix elements (`rig_math::M4`, including the
  perspective divide Three does), and rounds **once** on store.
- Normals go through `normal_matrix`, a literal transcription of
  `Matrix3.getNormalMatrix` = `setFromMatrix4().invert().transpose()` — the
  three steps, not the algebraically-equal `cofactor/det` shortcut
  `Geo::apply` uses, because they differ in the last bits.
- `m4_translation` / `m4_scale` / `m4_rotation_{x,y,z}` transcribe
  `Matrix4.makeTranslation` / `makeScale` / `makeRotation*`, taking `f64`
  angles and offsets, with `Matrix4.set`'s row-major-in / column-major-out
  index mapping written out.
- Every call site now passes `f64` (`std::f64::consts::PI`, `r * 0.78`, …)
  instead of `dim(...)`-narrowed `f32`. `dim()` survives only where it
  belongs: at the boundary into the geometry kit's `f32` primitive
  constructors.

### What the fix bought, measured

With the transform path corrected, the residual against the golden is:

| quantity | before | after |
|---|---|---|
| the failing AO entry | 1.154e-7 | bit-exact |
| max AO residual, all 4 passes, all entries | (not measured) | **2.53e-7** |
| max vertex-position residual, all dumped meshes | (not measured) | **3.03e-8** = 1.02 f32 ULP |
| max vertex-normal residual | (not measured) | 2.74e-6 |

And, cross-checked independently: I transcribed the *new Rust helpers* back
into JS and ran them against Three's own `BufferGeometry` methods over all
eleven operations `hands.js` performs. **Bit-exact on every one, position and
normal, worst difference 0.**

So what is left is not mine. The give-away is the **UV** channel: `applyMatrix4`
never touches UVs, yet 72% of UV components still differ from Three's. The
remaining error is the geometry kit's own primitives (`lathe_z`, `blob`,
`box_geo`, `dome`, `ring`) landing on a different f32 for ~1 ULP of their
internal `f64` trig — which is exactly what their own goldens already accept
at 1e-6.

### The tolerance was ALSO wrong, and is now derived rather than chosen

`F32_TOL = 1e-7` came from a wrong model: "the mask is stored in a
`Float32Array`, so one f32 ULP covers it." The mask is not an independent
`f32` — it is a smootherstep of a distance derived from `f32` **vertex
positions**, and the smootherstep has gain:

```text
value = peak * s(t),  t = 1 - dist/radius,  s'(t) = 30 t^2 (1-t)^2
|d value / d dist| = peak * |s'|max / radius = 0.9 * 1.875 / 0.012 = 140.6 per metre
```

Glove-subtree meshes reach |coord| ~ 0.1 where one f32 ULP is 7.45e-9, so an
irreducible vertex error of `sqrt(3) * 7.45e-9 = 1.29e-8` admits
`140.6 * 1.29e-8 = 1.8e-6` of mask error. `F32_TOL` is now **2e-6**, derived.

Loosening a tolerance is normally the wrong move, so the important half is
what replaces it. **The defect class that 2e-6 would hide is now caught
tightly and directly**, by a new golden section and two new tests:

- `capture.mjs` captures every `translate`/`scale`/`rotate*` the module
  performs **in isolation**: the exact `Matrix4.elements`, the exact
  `Matrix3.getNormalMatrix`, and the exact buffers the **real** Three method
  produces over two probe geometries (a 45-vertex ring for curved normals, a
  324-vertex chamfered blob for axis-aligned ones).
- `transform_helpers_are_bit_exact_against_three` feeds `geo_apply` the
  golden's OWN input buffers — so the primitive kit's ULP noise is out of the
  picture entirely — and asserts matrices at `1e-15` and buffers at `1e-9`
  (under half an f32 ULP at these magnitudes).
- `rotate_y_by_pi_has_a_1e_16_shear_not_a_1e_7_one` states the specific number:
  `e[8]` must be ~1.22e-16, where the `f32` quaternion path gives -8.74e-8.

That is the right split: **pin the mechanism bit-exactly, and bound the
amplified end-to-end result honestly.** A tight bound on the AO would only be
pinning the primitive kit's f32 quantisation, which is not this slice's
contract.

The transform helpers are `pub` for this reason, documented at the site. That
is not "widening the API so a test can reach an internal" — this layer *is* a
contract (Three's `BufferGeometry.applyMatrix4`), and it is the only place a
sub-nanometre defect is visible before the primitives' own noise buries it.

### The structural half, which is NOT in this slice

`Geo::apply` (`geometry/geo.rs`) has the same defect, and so therefore do
`Assembly::add` and `primitives/xform.rs`: they transform in `f32` where Three
transforms in `f64`, and `xform.rs` builds its rotations from the same `f32`
`Quat::from_axis_angle`. That is the lowest correct layer for this fix.

I did not make it there, for one reason only: `geo.rs` is shared, every other
geometry slice's golden was captured against it mid-wave, and silently moving
all of their numbers while their agents are live is the kind of thing that
turns a wave into a debugging session. **Recommended follow-up: lift
`Geo::apply` to `f64` (read `f32` → compute `f64` → store `f32`), give
`primitives/xform.rs` Three's `makeRotation*` instead of the quaternion, and
then delete `hands.rs`'s local `geo_apply`/`m4_*` helpers in favour of it.**
Those slices' goldens should get *closer*, not further away — the tolerances
are 1e-5/1e-6 and this moves things by ~1e-9 in the right direction.

## Traps checked, by name

- **`Float32Array`.** One occurrence, `hands.js:949`: `bakeContactAO` allocates
  the `color` attribute as a `Float32Array`, so its `Math.max` accumulate reads
  back an **f32-rounded** value on every subsequent pass. `HandMesh::color` is
  `Vec<f32>` and the max is taken in `f64` before the `f32` store, matching the
  source's evaluation order exactly. The golden runs the bake four times
  (0.7, then 0.9, then a *lower* 0.4 that must change nothing, then an empty
  contact list that must early-out) specifically to pin that read-back.
- **Euler order.** `hands.js` never writes an order string — which is precisely
  what makes this dangerous. Every rotation it sets is `Object3D.rotation`, a
  `THREE.Euler` whose **default order is `'XYZ'`**, composed `qx*qy*qz`.
  `axiom_math::Quat::from_euler_xyz` composes `qz*qy*qx`, a different rotation.
  All node rotations go through `rig_math::Q::from_euler_xyz`, a literal
  transcription of Three's `case 'XYZ'` branch. It is live in
  `fitToCylinder`'s two-axis thumb-base scan, where an order mix-up silently
  selects a different `(y, z)` pair. The whole node tree's quaternions are in
  the golden, so any order error fails on node 13 (a finger root with only a
  `y` rotation) or node 73 (the thumb root with all three).
- **Matrix storage order.** `M4` is column-major, transcribed with Three's own
  element indices. See the `rig_math.rs` section.
- **`sign` vs `signum`.** `hands.js` contains no `Math.sign` (grepped). Nothing
  to get wrong; recorded so the next reader does not have to re-check.
- **`Math.hypot`.** Ruled out with evidence, not by eye — it was the
  coordinator's first suspect on the golden failure (five slices have been
  bitten by it), and a wrong answer here would have sent the investigation
  somewhere useless. `hands.js` contains **zero** occurrences, and so do all
  five Three math files the rig touches (`Vector3.js`, `Matrix3.js`,
  `Matrix4.js`, `Quaternion.js`, `Euler.js`). `gapAt` ends in
  `Vector3.length()` = `Math.sqrt(x*x + y*y + z*z)` and `distanceToSquared`
  is `dx*dx + dy*dy + dz*dz`; both are transcribed as such.
  `crate::jsmath::hypot3` is correctly **not** used here.
- **`crate::jsmath::round`.** Also ruled out: no `Math.round` / `toFixed`
  anywhere in `hands.js`. Nothing on this path quantises.
- **Compute in `f64`, store `f32`.** This is what the golden failure was; see
  the section above. Every `translate`/`scale`/`rotate*` in this module now
  builds an `f64` matrix, transforms in `f64`, and rounds once on store, and
  `dim()` survives only at the boundary into the kit's `f32` primitive
  constructors.
- **Float arithmetic is not associative.** This is what the `apply_quat`
  finding above was. Beyond that: `add_scaled(v, -dot)` is used wherever the
  source calls `addScaledVector(v, -dot)` rather than the algebraically equal
  `sub(v.scale(dot))`; `t.scale(1.0 / d)` reproduces
  `divideScalar(d)` = `multiplyScalar(1/d)` (reciprocal formed first); the
  scan loops keep `lo + ((hi - lo) * i) / 48` and `y0 - 1.3 + (2.6 * i) / 20`
  in the source's grouping.
- **Enum as a table index.** No table indexed by an enum here. `HandSurface`
  is matched, never indexed.
- **A matching count is not proof.** Counts are asserted exactly *and* the full
  vertex buffers are compared for the scale-1 arm.
- **Your comparator can be the bug.** The mesh comparison is index-aligned, not
  nearest-neighbour, so there is no pairing heuristic to get wrong.
- **Dead computation is still part of the source.** Three cases, all kept and
  documented: `hands.js:1023`'s `addScaledVector(_dir, 0)` (an exact no-op —
  kept as the comment it degenerates to, since it cannot change a bit); the two
  unreachable `lengthSq() > 1e-12` guards; and `hands.js:641-647`'s render-flag
  sweep, which re-authors values the meshes already carry.

## Source quirks ported as written (not fixed)

- **`buildGlove`'s side seams wear `materials.pad`, not `materials.seam`**
  (`hands.js:243`), while the finger and thumb seams wear `seam`. That changes
  which `bakeSurfaceMasks` amplitude profile they get (PAD's tight 2.2 wear
  exponent instead of SEAM's 2.6). Ported as written and pinned by the tree
  dump's per-mesh surface assertion.
- **`buildGlove`'s wrist strap profile has one unscaled literal**: the second
  profile point's `z` is `0.0022`, not `0.0022 * scale`, while all seven other
  coordinates in the cuff and strap are scaled (`hands.js:262`). At `scale =
  0.97` this shifts one lathe ring by 66 µm. Ported as written; the left arm's
  digest pins it.
- **`segmentSeam`'s chamfer and `buildGlove`'s side-seam chamfer are unscaled**
  (`0.0003`, `0.0004`) — same class, same treatment.
- **`buildFinger`'s `seamSide` is never supplied by any caller**, so
  `(seamSide ?? 0) === 0` is always true and the single-seam arm is dead. Kept.
- **`buildThumb`'s pad and nail chamfers are `0.0012` unscaled** while their
  dimensions are scaled. Kept.

## Deliberate divergences

- **Materials are a `HandSurface` enum, not `THREE.Material` objects.** There
  is no material library in this port yet. The source's
  `materials.seam ?? materials.glove` fallback is unreachable in the real
  caller (`viewmodel.js:114-119` binds a real `glove_seam`) and is not
  modelled.
- **`bakeSurfaceMasks` takes closures** for `materials.bakeMasks` and the mask
  re-shaper, which live in the unported `materials.js`; the source's `rng`
  argument is whatever the caller's closure captures. Its `if (!bake) return`
  guard has no counterpart. The four amplitude profiles (`CLOTH`/`SLEEVE`/
  `PAD`/`SEAM`) and `BAKE_MASK_OPTS` are public constants. **This is the one
  routine with no golden**: it is a pass-through, and everything of this
  module's own in it (the profile constants and the traversal order) is
  already pinned by the constants and the tree dump.
- **`dispose()` has no counterpart** — Rust frees the geometry when the `Arm`
  drops.
- **`Arm::pose_name`** (the `HandPoseName` enum, which `viewmodel.rs:599`
  reads) is kept alongside the new `Arm::pose_key` (the source's `this.pose`
  string, which may be a synthetic `clamp:<weapon>`). `pose_key` is
  authoritative; `pose_name` records the last *authored* pose and is unchanged
  by `set_pose_key` with a synthetic key.
- **The two sleeve pivots' `rotation` Euler goes stale** when `aimBone` writes
  their quaternion directly. Three has the same asymmetry in reverse (it
  back-syncs the Euler); nothing in either language reads it, and inventing the
  back-sync would be adding a value the source never uses.

## A small overlap worth knowing about

`crate::jsmath::or_one` is Three's `length() || 1` idiom, which
`rig_math::V3::normalize` and `Q::normalize` both hand-roll (they predate
`jsmath`). The behaviour is identical and both are golden-pinned, so this is a
tidy-up rather than a defect — but if `jsmath` is to be the single home for
the V8-exact primitives, those two call sites are the ones to fold in.

## Nothing to wire

`weapons/mod.rs` already declares `pub mod hands;` and `pub mod rig_math;`, and
`serde_json` is already a dev-dependency. No `mod.rs` / `lib.rs` / `Cargo.toml`
change is needed for this slice.

## Follow-up for the integration pass

`viewmodel.rs` (owned by a concurrent agent this wave) can now use the real
per-weapon contact fit instead of the authored `HAND_POSES.clamp`: build the
weapon, read its `handguard` node and `gripL`, call
`Arm::fit_to_cylinder(hand_pos, hand_quat, axis, dir, r, 0.001, "clamp:<id>")`,
filter the returned contacts to the handguard's own `z` extent, then
`Arm::bake_contact_ao(&kept, 0.012, 0.7)` — i.e. port `viewmodel.js:460-483`'s
`_fitSupportHand`. That is `viewmodel`'s slice, not this one, so it is left
undone and flagged here rather than reached into.
