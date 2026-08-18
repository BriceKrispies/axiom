# Player: movement, camera feel, springs, tuning

Ported `src/player/springs.js`, `src/player/tuning.js`, `src/player/mantle.js`,
`src/player/movement.js`, `src/player/camera.js` into `apps/shmup/src/player/`
(`springs.rs`, `tuning.rs`, `mantle.rs`, `movement.rs`, `camera.rs`, `mod.rs`).

## What was ported

- **`springs.rs`** — the whole file: `clamp`/`clamp01`/`lerp`/`smoothstep`/
  `smootherstep`/`easeOutCubic`/`easeInOutSine`/`approach`/`moveToward`/
  `angleDelta`/`hashNoise`, `Spring`, `RecoilAxis`.
- **`tuning.rs`** — the whole file: `GRAVITY`/`JUMP_APEX`/`JUMP_SPEED`,
  `STANCE` (as `Stance` + `StanceDef`/`STAND`/`CROUCH`/`PRONE`), `MOVE`,
  `CAMERA`, `HEALTH` (not consumed by the four other files ported here — it
  belongs to an un-ported health subsystem — but is part of `tuning.js`),
  `FOOTSTEP`.
- **`mantle.rs`** — the whole file: `LedgeProbe`/`LedgeResult`/`LedgeKind`/
  `ledge_kind_name`, `MantleMotion`.
- **`movement.rs`** — the whole file: the `Movement` state machine, `STATES`,
  `PlayerCommand`.
- **`camera.rs`** — the whole file **except** `applyTo(camera)`
  (`camera.js:346-356`), which writes onto a live `THREE.PerspectiveCamera`.
  No render-layer camera type exists in this crate yet; everything
  `applyTo` would consume (`eye_position`, `rotation`, `fov`) is public on
  `CameraRig` for whenever the viewer arm binds it. `forward` therefore stays
  at its constructed default `[0, 0, -1]` — the source only ever recomputes
  it inside `applyTo`.

## The physics and input seams

Neither `src/physics/`'s collision world nor `src/core/input.js` back a
player controller yet (physics exists as `crates`... no — `apps/shmup/
src/physics/` — a low-level BVH/raycast kernel, landed by a concurrent agent
during this same port pass, but it has no `createCharacter()`-shaped
controller or player-facing adapter over its `u16` masks / `HitRecord` yet;
input.js is explicitly out of the ported-core scope per `lib.rs`'s doc
comment). Following the precedent `audio::spatial::WorldProbe` set, this
module names the exact methods the source calls as narrow traits rather than
inventing a physics facade:

- `mantle::LedgeCharacter` — `position()`/`radius()`/`check_capsule()`, the
  three facts `LedgeProbe.probe` reads off `c`.
- `mantle::WorldProbe` — `raycast`/`capsule_cast`/`check_capsule_segment`,
  the source's `phys.raycast`/`phys.capsuleCast`/`phys.checkCapsule`.
- `movement::CharacterController` — a supertrait of `LedgeCharacter` that
  adds everything else `movement.js` reads/writes on `this.character`
  (`height`/`stepHeight`/`grounded`/`velocity`/`canFit`/`lastMoveBlocked`/
  `touchingCeiling`/`groundNormal`/`groundFriction`/`groundSurfaceName`/
  `landingSpeed`/`move`/`teleport`/`setPosition`/`depenetrate`/
  `probeGround`). One implementation over the physics BVH's `StaticWorld`
  (translating its `u16` masks and `HitRecord` into `ProbeMask`/`RayHit`/
  `CapsuleHit`, and wrapping a character position/velocity/stance around its
  `sweep_capsule`/`overlap_capsule`/`raycast`) satisfies both traits.
- `movement::PlayerInput` — `move_vector`/`action`/`stick_move_y`/`ads`, the
  source's duck-typed `ctx.input`. `InputAction` names the six actions
  `movement.js` queries by string (`jump`/`crouch`/`prone`/`sprint`/
  `leanLeft`/`leanRight`).

`Time` (`crate::engine::Time`) and `Config` (`crate::config::Config`) are
**not** re-seamed — both already carry every field `movement.js`/`camera.js`
read off `ctx.time`/`ctx.config`, so `Movement::step`/`latch_input` and
`CameraRig::update` take them directly.

**What the eventual physics binding needs to supply**, concretely: a
character-controller wrapper around `physics::bvh::StaticWorld` that owns a
capsule's position/velocity/stance and implements `move_by` via
`sweep_capsule` + depenetration, plus a thin `WorldProbe` adapter translating
`ProbeMask::{Character,World}` to the physics module's `u16` collision masks
and `physics::bvh::HitRecord`/`Contacts` into `mantle::RayHit`/`CapsuleHit`.
`physics::surfaces` already carries a `Surface`-keyed response table (the
same `world::palette::Surface` this port's `Surface` type uses), so no third
surface taxonomy is needed anywhere in that binding.

## Surface type reuse

`mantle.rs`/`movement.rs` use `crate::world::palette::Surface`, **not**
`crate::audio::foley::Surface` (a second, independently-ported 12-variant
enum with the same names in a different order). `world::palette::Surface` is
what `physics::bvh::StaticWorld::surface_of` already returns, so this keeps
the eventual physics binding to one surface vocabulary instead of three.

## Divergences (documented at the site too)

- **`GRAVITY` / `STAND.height` / `STAND.eye` / `CROUCH.height` / `CROUCH.eye`
  carry a small `f32` round-trip error.** `config::UNITS` (this crate's
  existing, committed boundary) stores these as `f32` `Meters`; widening to
  `f64` here is not bit-identical to the JavaScript's pure-`f64` `-9.81 *
  2.1` / `1.78` / etc. `tests/player_port.rs` uses `f32::EPSILON` (as an
  `f64`) or a `1e-6` tolerance at these sites instead of exact equality, and
  says why at each assertion. This is a consequence of using `config.rs`
  as-instructed ("use them; do not redefine"), not a new defect.
- **`Spring::step`'s 24-substep guard cap, ported and pinned as a source
  quirk** (recipe rule 7): a `dt` hitch bigger than `24 * (1/360) s ~= 66.7
  ms` is not fully integrated — the remainder is silently dropped rather than
  carried to the next call. Pinned in
  `spring_step_caps_substeps_at_24_and_drops_the_remainder_on_a_big_hitch`,
  which asserts the guard-capped value visibly diverges from a fully
  substepped integration of the same total `dt`.
- **`self.character` is taken out of `Movement`, not borrowed in place.**
  The source holds `this.character` as a field every private method reaches
  into directly; Rust can't hold `&mut self.character` while also calling
  `&mut self` helpers for other fields. Every public entry point that
  touches the controller (`step`, `teleport`, `cancel_mantle`) does a
  `self.character.take()` / thread `c: &mut dyn CharacterController`
  through / `self.character = Some(character)` dance instead. Behaviourally
  identical; purely how Rust has to hold the reference.
- **Dropped dead parameters/fields that the source never reads**:
  `cmd.cronePressed` (an unused near-duplicate of `crouchPressed`), and the
  `travelled`/`h` parameters of `_postMove(h, travelled)` (neither is
  referenced in the method body). `_updateJump(cmd)`'s `cmd` parameter is
  likewise unused and dropped.
- **`CameraRig`'s spring/RecoilAxis fields are `pub`**, matching the source
  (plain JS object properties, never actually hidden) rather than adding a
  test-only accessor to reach them.
- Scratch fields the source preallocates for GC pressure (`_fwd`, `_right`,
  `_wish`, `_p0`, `_p1` on `Movement`; `_fwd`, `_right` on `CameraRig`) are
  plain stack locals/parameters here — Rust has no GC to protect against,
  and the source's own comments say this preallocation exists for exactly
  that reason.
- `Math.hypot(x, y, z)` (3-arg) has no direct Rust equivalent; ported as
  `(x*x + y*y + z*z).sqrt()` (mathematically identical, not bit-identical —
  already `sqrt`-tolerance-eligible either way).

## What is pinned, and how

`tests/player_port.rs`, 31 tests. Everything reachable by `+ - * /` and
comparisons only (springs' non-transcendental helpers, `LedgeProbe::probe`'s
full decision logic — no `sin`/`cos`/`sqrt` appears in that function at all —
and `MantleMotion`'s position outputs) is pinned exactly. Everything a
transcendental touches (`Spring`/`RecoilAxis::step`'s `exp`, `hash_noise`'s
bit-hash-derived `f64`, `MantleMotion`'s camera-garnish `sin` fields,
`CameraRig::on_land`) is pinned at `1e-12`. `tuning::JUMP_SPEED` uses `1e-6`
for the reason above.

`movement.rs`/`camera.rs`'s full per-frame integration (`Movement::step`,
`CameraRig::update`) is exercised **natively**, not JS-pinned: driving the
real `physics.createCharacter`-shaped controller from a Node capture script
would need a JS collision mock at least as large as this test file, for a
port that has no physics binding yet to check the *result* against anyway.
What's tested there is state-machine behaviour: sprint+crouch-press starts a
slide and ends it within its duration cap; a sprint double-tap inside the tap
window enters tactical sprint; a jump transitions Jump -> Fall -> a landing
event; a camera recoil impulse visibly moves the composed pitch and decays.
`LedgeProbe::probe` (the other physics-shaped routine) *is* golden-pinned,
by scripting `WorldProbe`/`LedgeCharacter` mocks with canned raycast/
capsule-cast answers on both the Rust test and a matching Node capture
script — five scenarios (no wall, wall-but-no-walkable-top, wide mantle,
crouch-clearance mantle, thin-rail vault), all pure arithmetic so pinned
exactly.

## What could not be ported

`camera.js`'s `applyTo(camera)` — see above, needs a render-layer camera
type this crate doesn't have yet. Nothing else in the five source files was
skipped.
