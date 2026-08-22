# `weapons/viewmodel.js` — finishing the half-finished port

Source: `C:/dev/Claude-of-Duty/src/weapons/viewmodel.js` (1088 lines).
Target: `apps/shmup/src/weapons/viewmodel.rs`.
Golden: `apps/shmup/tests/weapons_viewmodel/{capture.mjs,golden.json}`.
Test: `apps/shmup/tests/weapons_viewmodel_port.rs`.

`06-parallel-port-plan.md` lists this file as ~56% ported, compiling, wired in
and **untested** — the hazard being that nothing signalled it was unfinished.
It is now finished for the rig, and pinned by an 840-frame golden captured from
the original running under Node 24.

## The audit: what was missing

The pre-existing 604 lines carried `update`'s additive layer stack (base pose,
sway, bob, lag, recoil/settle, jump/land, the clip *offset*), `ads_pose`,
`add_recoil` and `solve_hands`. Diffing against the source, the gaps were:

| missing | source | status |
|---|---|---|
| **clip event dispatch** (`onClipEvent`) | `:803-805` | ported ([`FiredClipEvent`] + `clip_events()`) |
| **`_updateParts`** — bolt/slide/charging/trigger/selector/magazine drive | `:856-913` | ported as [`PartsState`] |
| **`_magFromHand`** | `:915-927` | ported |
| **`_updateReticle`** — the collimator solve | `:972-1034` | ported as [`ReticleState`] (minus `lookAt`, below) |
| **viewmodel FOV** | `:846-851`, incl. the `> 1e-3` dead zone | ported |
| **`addWeapon`'s node half** (the `entry` object) | `:405-434` | ported as [`WeaponRig`] + `WeaponRig::from_rifle` |
| **`setActive`'s full state reset** and its already-active no-op | `:517-534` | ported |
| **`play`'s duration return, `clipPlaying`, `clipName`** | `:540-563` | ported |
| **`boltHold` / `magInHand` / `stepT`** dead-ish state | `:244`, `:275-276` | carried, documented |
| the anchor orientation read-back the reticle needs | `:641` | ported (`anchor_quat`) |

Also fixed while in here: `set_active` previously reset the recoil springs even
when re-selecting the weapon already in hand, because the source's
`if (w === this.active) return` early-out was not carried. That would have
cleared a burst's recoil mid-burst.

## Deliberately not carried, with the reason

These are *not* "left for later"; each has a structural reason that is written
at the site in the module doc.

* **`addWeapon`'s mesh construction**, `shapeMasks`, `_fitSupportHand`,
  `_bakeContactAOOnWeapon`, the four reticle sprite geometries, `dispose`.
  Geometry + vertex-colour baking against a renderer this port does not have,
  all gated on `materials.js`'s `bakeMasks`, which is not ported.
* **`_updateReticle`'s `lookAt`** (`:1006`). It orients a sprite this port does
  not build. Reproducing it faithfully needs the anchor's world *position*
  (which cancels out of every other value in the file) plus `Matrix4.lookAt`
  and `Matrix4.extractRotation` — i.e. widening the `ViewCamera` trait purely
  to orient absent geometry. Everything `_updateReticle` *decides* — visibility,
  the on-axis dot position, angular size, all four opacities — is ported and
  pinned.
* **`muzzleWorld` / `ejectWorld` / `ejectVelocity` / `boreDir`** (`:1041-1071`).
  Each reads `w.group.matrixWorld`, and `Object3D.updateMatrixWorld` composes
  that against the **anchor's** world matrix, which the source refreshes from
  the *renderer's* scene-graph walk, not from `update`. Their value is a
  function of render-loop ordering that does not exist here yet; porting them
  now would pin an ordering this port does not have. They belong with the
  renderer slice.
* **`trackCamera` / `rigOverride` / `debugFrozen`** — hooks for
  `weapons/preview.js` and `weapons/index.js`'s debug freeze, not the runtime rig.

## The golden

`capture.mjs` imports the ORIGINAL `viewmodel.js` (and `three` by absolute path
inside the source repo, so Node resolves the *same* module instance the
source's own bare `import 'three'` resolves to — otherwise there would be two
THREE copies and every internal `instanceof` would fail). It stubs only `mats`
(throwaway `MeshBasicMaterial`s) and drives the real class.

**Why a trajectory and not a frame.** The rig is seven spring integrators, three
exponential `damp` blends and a phase accumulator over one base pose. Every one
is an accumulator: a wrong denominator in `Spring::step`, a transposed lag
target, a `damp` rate on the wrong operand — all invisible on frame one. So the
golden is **1200 fixed 1/120 s frames** through:

```
  0- 59  idle, camera still
 60-179  walk (speed 3.2), camera spinning at 4 rad/s  -> crosses +-PI, exercises wrapPi
180-299  sprint (speed 6.4) -> sprint pose blend + the 1.05 stride stretch
300      camera TELEPORTS +2 rad in one step (240 rad/s) -> drives clamp(dy, -9, 9) into its rail
300-419  ADS in; from 360, automatic fire every 9 frames (7 shots, first=true on shot 0)
420-449  ADS out
425-475  low ready
450      jump();  450-469 airborne (bob x0.25);  470 land(4.2)
480-731  play('reloadTac') — clip channel, moving parts, magazine-in-hand, 6 clip events
         480-619 ADS button HELD: a non-'draw' clip forces wantAds to 0, so adsT stays 0
         560-619 SPRINT button HELD: `sprintTarget = s.sprint && !this.clip` keeps it suppressed
740-813  play('draw') with ADS held — the one clip that does NOT suppress ADS; adsT climbs to 1
840-1188 play('reloadEmpty') — the ONLY clip whose parts track is non-zero for
         `bolt`, `slide` and `charge`, and the only one that fires the
         `charge` and `boltrelease` beats
```

Plus a 7-frame `dtEdge` run over dt = -1, 0, 0.5, 1e-6, 1/120, 0.1, 0.25, for
`update`'s `dt > 0 ? (dt < 0.1 ? dt : 0.1) : 0` guard and the `dt > 1e-5`
angular-velocity gate.

Per frame the golden records the rig position/quaternion, the anchor quaternion
(the exact `ViewCamera` input), the FOV, all seven blend scalars, all seven
spring states, both hand targets, the selected support-hand pose, every
moving-part transform, the whole reticle state, the clip sample result, the
clip name/time, and the events dispatched that frame.

`node capture.mjs` writes `golden.json` (2.15 MB) with `writeFileSync`, not a
shell redirect — PowerShell's `>` writes UTF-16 and would corrupt it. Verified
byte-identical across re-runs (`md5 4ec19b2a...`).

### Tolerances

* **Exact** — booleans (`reticle.visible`, `magVisible`, `res.active`), the clip
  name, the support-hand pose selection, and the clip-event dispatch (name,
  owning clip, order, and the exact frame each fires on). A missed or duplicated
  beat is a state-machine bug, never rounding.
* **`1e-12` absolute** — every float, the figure `tests/core_port.rs`
  established. The stack runs `sin`/`cos`/`exp`/`sqrt`/`atan2`/`asin`, none
  bit-guaranteed across libm. This is *not* a loose bar for an accumulator: the
  springs and `damp` are contractions, so a 1-ULP `exp` difference decays rather
  than compounding.
* **Exact as `f32`** — the six `Noise1` sway tables (`Float32Array` in the
  source), compared at `1e-12`, five orders below the `f32` grid.

### Independent check of the newly-derived maths

Because the fan-out forbids compiling, the algebra the new Rust encodes was
re-derived in a throwaway Node script *from each frame's recorded inputs* and
compared to the recorded outputs, over all 840 frames of the first draft:

| quantity | worst abs delta |
|---|---|
| viewmodel FOV (incl. the 1e-3 dead zone, 73 steps) | 0 |
| bolt / charging positions, trigger + selector rotation | 0 |
| magazine position (164 in-hand frames) | 5.6e-17 |
| magazine quaternion (slerp) | 0 |
| reticle visibility (all frames) | exact |
| reticle position / scale / four opacities | <= 6.2e-15 |

## Traps checked by name

* **`Float32Array`** — grepped. The only one reachable from this file is
  `Noise1`'s table, already handled in `mathx.rs` (`Vec<f32>`, narrowing at both
  write sites). `viewmodel.js` itself declares none. The golden pins all six
  tables anyway.
* **Euler order** — grepped every `setFromEuler` / `.order` / `'YXZ'` /
  `'XYZ'` in the file. Seven sites. Six build a quaternion in `'XYZ'`: the
  hip, sprint, lowReady and adsCant pose eulers, the composed additive
  `_e.set(rx, ry, rz, 'XYZ')`, and `magSeatQuat`'s
  `new Euler().fromArray(rot)` — THREE's *default* order, which is `'XYZ'`.
  All six go through `Q::from_euler_xyz`, which is transcribed from THREE's
  `case 'XYZ'` (`qx*qy*qz`) and is **not** `axiom_math::Quat::from_euler_xyz`,
  which composes `qz*qy*qx`. The seventh is
  `_e.setFromQuaternion(anchor.quaternion, 'YXZ')` for the lag layer, which
  goes through `Q::to_euler_yxz`.
  `mag_seat_quaternion_comes_from_an_xyz_euler` pins the default-order one
  directly, because a bare `new Euler()` is the easiest to get wrong.
  Also checked, because it is exactly the shape that bites: `_e` is a single
  *shared mutable scratch* euler whose `.order` is mutated by the `'YXZ'`
  decomposition at `:649`. Every later use re-specifies `'XYZ'` explicitly
  (`:685`, `:693`, `:702`, `:711`, `:824`), so the order never leaks across
  sites in the source and there is no hidden ordering to reproduce.
* **Float arithmetic is not associative** — no spring/damper expression was
  tidied. `js_hypot2`'s Kahan loop, `Spring::step`'s denominator, the sway sums
  and the `px/py/pz`/`rx/ry/rz` accumulation order are transcribed
  left-to-right as written, including the deliberate axis swap at `:786-787`
  (`rx += recRot.x + settle.y`, `ry += recRot.y + settle.x`).
* **`sign` is not `signum`** — `viewmodel.js` calls neither `Math.sign` nor any
  three-valued sign. Nothing to do.
* **`Math.hypot` is not `sqrt(x*x + y*y)`** — this one bit. `_updateReticle`'s
  vignette offset uses `Math.hypot`. Measured against Node 24 over 200 000 pairs
  in the reticle's own magnitude band: V8's scaled Kahan algorithm matches
  bit-for-bit (0 mismatches); the naive form differs on **38%** of inputs. The
  Rust port transcribes V8's algorithm (`js_hypot2`), and `capture.mjs` re-runs
  the measurement every time the golden is regenerated so the claim cannot rot.
* **Dead computation is still part of the source** — `stepT`, `magInHand`,
  `boltHold`, `ironSight`, `muzzle`/`eject`/`ejectDir`, `_updateParts`'s unused
  `s` parameter, and the `selectorLive` lerp are all carried with a comment
  rather than dropped.
* **An enum as a table index** — no enum-indexed table here.
* **Matrix storage order** — no matrix is materialised in the ported scope
  (the one place THREE builds one, `Matrix4.makeBasis` inside `handBasis`, is
  consumed directly by `Q::from_basis`, which transcribes
  `setFromRotationMatrix`'s trace method against `makeBasis`'s column layout).

## Source defects found, ported as written, pinned by name

1. **`boltHold` is never raised.** Declared "1 = locked back (empty)"
   (`:275`), written only by the constructor and `setActive`, both to `0`.
   Nothing in the whole source tree sets it to 1. So
   `boltOff = Math.max(stroke, boltHold, clipBolt * boltHold)` (`:868`) reduces
   to `stroke` for the entire game, and the `bolt`/`slide` tracks the reload
   clips author — `reloadEmpty` holds `bolt: 1` through most of its timeline —
   are multiplied away and never reach a mesh. The bolt only ever moves from
   *firing*. Pinned by
   `bolt_hold_is_never_raised_so_the_clip_bolt_track_is_multiplied_away`, which
   finds real frames where the clip asks for bolt travel and the bolt is still
   at rest.
2. **`selectorLive` does not exist.** `p.selector.rotation.x = lerp(-0.95, 0,
   clamp01(this.selectorLive ?? 1))` (`:897`). The property is declared
   nowhere, so `??` always yields 1 and the expression is a constant `0`: the
   fire-selector lever never moves in any state. Pinned by
   `selector_never_moves_because_selector_live_is_undefined`.

Both are ported as the source evaluates them, not folded to literals.

## Documented divergences

1. **The fitted support-hand pose.** `_fitSupportHand` solves each fingertip
   against the handguard cylinder at build time and registers the result on the
   arm as a per-weapon pose named `clamp:<id>`, which then replaces the authored
   `clamp` everywhere. This crate does not port `_fitSupportHand`, so
   `WeaponRig::lhand_pose` carries the authored `clamp`. The golden records
   `clamp:rifle` and the test normalises on the `:` — the divergence is asserted
   by name (`fitted_support_hand_pose_diverges_by_name`) rather than hidden.
   `Arm::fit_to_cylinder` itself **is** ported and pinned (it landed with the
   `hands.rs` slice), so closing this gap is now just porting the call site:
   run the fit at `WeaponRig` build time, keep the returned contacts, and give
   the arm the fitted pose. Bounded follow-up, listed below.
2. **Determinism caveat around `bakeMasks`.** In the running game
   `mats.lib.bakeMasks` exists, so `addWeapon` and `Arm.bakeSurfaceMasks` draw
   from the viewmodel's forked RNG stream at construction (`:158-162`, `:344`),
   *before* any shot is fired — which shifts `addRecoil`'s jitter. `bakeMasks`
   is mesh vertex-colour work and is not ported, so neither the port nor the
   capture makes those draws and the two agree exactly. Wiring `bakeMasks` up
   later must be done together with re-capturing this golden. Recorded in
   `Viewmodel::new`'s doc as well as here.
3. **`onClipEvent` is a queue, not a callback.** A `&mut self` callback field
   would infect every method signature; `update` queues the frame's beats into
   `clip_events` (cleared at the top of each `update`) and the caller drains
   them. Same beats, same order, same frame — pinned per frame.
4. **The `f32`/`f64` seam.** `weapons::models` stores nodes as `f32`; the rig
   integrates in `f64`. The trajectory test builds its `WeaponRig` from the
   golden's `f64` node values on purpose, so this slice's error is not blended
   with the models slice's storage width. `WeaponRig::from_rifle` (the
   `f32` -> `f64` converter) gets its own, correctly-loose `1e-7` check.

## Known coverage gaps in the golden

Small, and stated rather than papered over:

* `_updateReticle`'s `s <= 0.02` early return (the dot at/behind the eye) is
  never reached: with the shipped weapons `s` sits around 0.3. It is unreachable
  with real data rather than untested by omission.
* The `!optic` early return is likewise unreachable — all three shipped models
  supply an `opticGlass` node.
* `clips::Pose::Clamp` is not exercised by the trajectory (only `inspect` uses
  it, and at 3.2 s it would have added another 384 frames to an already 2.15 MB
  golden). The other three poses are.
* Only the rifle is driven. The smg shares its shape; the pistol differs
  (`slideRest`/`slideTravel` instead of `boltRest`/`chargeRest`, `cup` support
  pose), so its `if (p.slide)` arm is ported but not golden-pinned. That wants a
  `WeaponRig::from_pistol` plus a second capture when a pistol consumer exists.

## The three failures on the first real test run, and what each one was

Written up because two of the three were **my** error, in two different
directions, and both are the errors the recipe warns about by name.

### 1. `frame 0 dotCore.opacity: got 0.95, want 1.0` — *the port was wrong, and so was the golden*

`ReticleState`'s pre-first-write opacities are observable: `_updateReticle` has
three early returns that set only `visible = false` and leave the material
opacities untouched, so every frame before the reticle is first visible reports
the **authored material** value. I filled those in by reading the call site —
`mats.reticle(0xff1206, 0.95)`, `mats.reticle(0xff2a0c, 0.34)`,
`mats.reticle(0xff1206, 0.95 * 0.5)`, `mats.reticleOutline(0.85)` — and
assumed the second argument was an opacity.

It is not. `materials.js:1154-1163`: `reticle(color, intensity)` multiplies the
**colour** by `intensity` and sets `opacity: 1` flat. Only
`reticleOutline(opacity)` (`materials.js:1135-1146`) takes an opacity. So the
true values are core 1, halo 1, ring 1, rim 0.85 — three of my four were wrong.

The golden was *also* wrong, and hid it: my capture stubbed `mats` with bare
`MeshBasicMaterial`s, whose default opacity is 1, so it reported 1 for the rim
too. Two wrong sides that happened to disagree — which is the only reason it
surfaced.

Fixed on both sides, and fixed structurally rather than by patching numbers:
the capture now instantiates the **real `WeaponMaterials`**, which runs headless
as long as `ctx.peek('materials')` yields nothing — and that same condition
leaves `mats.lib` undefined, which is exactly the no-`bakeMasks` state the RNG
contract already depended on. There is no hand-written material stub left, so
this class of error cannot recur.

*Recipe rule broken:* "never assert a value you reasoned out yourself". I read a
call site instead of the factory. The 0.05 delta was not drift; it was a guess.

### 2. `the trajectory must actually reach a frame where the clip asks for bolt travel` — *the test was right, the trajectory was too thin*

The `bolt_hold` defect test guards itself against becoming vacuous, and the
guard fired: `reloadTac`'s parts track is all zeroes for `bolt`, `slide` **and**
`charge`, so the trajectory never reached a frame where the defect was
observable. I had reasoned about `reloadEmpty`'s track (which holds `bolt: 1`
through most of its timeline) while driving `reloadTac`.

That also meant the charging-handle drive (`charge_pos`) and the
`charge`/`boltrelease` beats were never exercised — three whole paths silently
uncovered. Fixed by extending the trajectory from 840 to 1200 frames and playing
a full `reloadEmpty` at frame 840. The defect test now has 315 qualifying frames,
`charge` is non-zero on 16, and the golden carries 14 clip events instead of 7.

### 3. `the_dt_guard_and_the_angular_velocity_gate_match` — *the same cause as (1)*

Same `assert_frame` helper, same stale-opacity comparison, on a run whose
reticle is never visible. Fell out with (1); no separate fix.

### Not a failure, but relevant: `V3::apply_quat`'s grouping

The `hands.rs` agent corrected `rig_math`'s `apply_quat` to Three r180's real
grouping (`vx + qw*tx + qy*tz - qz*ty`, not `vx + qw*tx + (qy*tz - qz*ty)`) after
this slice was drafted. The full 1200-frame trajectory passes at `1e-12` with
that fix in place, against a golden captured from the JavaScript — which is the
correct direction of confirmation.

## Follow-ups this slice leaves behind

* **`_fitSupportHand`** (`viewmodel.js:460-485`). Its dependency
  (`Arm::fit_to_cylinder`) is now ported and pinned, so this is a bounded piece
  of work: run the fit at `WeaponRig` build time, keep the returned contacts, and
  give the arm the fitted pose. The contact-AO bake half stays out (mesh vertex
  colours). Until then `fitted_support_hand_pose_diverges_by_name` holds the gap.
* **A pistol capture.** `WeaponRig::from_pistol` plus a second trajectory, to
  cover the `if (p.slide)` arm and the `cup` support pose.
* **`bakeMasks`.** When it lands, `addWeapon` and `Arm.bakeSurfaceMasks` start
  drawing from the viewmodel's forked RNG at construction and shift
  `add_recoil`'s jitter. Re-capture this golden in the same change.

## Orchestrator: wiring

No `mod.rs` / `lib.rs` / `Cargo.toml` change is needed — `weapons::viewmodel` is
already declared and `serde_json` is already a dev-dependency.
