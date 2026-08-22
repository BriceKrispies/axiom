# `player/index.js` + `health.js` + `lowhealth.js` — the player facade

Slice: the three unported files in the player subsystem, ported into
`apps/shmup/src/player/`. The rest of `src/player/` (`springs`, `tuning`,
`movement`, `camera`, `mantle`) was already ported and pinned by
`tests/player_port.rs`; this slice is the integration layer that turns those
into a running player.

| source | target | lines |
|---|---|---|
| `src/player/index.js:1-752` | `apps/shmup/src/player/system.rs` | 752 |
| `src/player/health.js:1-239` | `apps/shmup/src/player/health.rs` | 239 |
| `src/player/lowhealth.js:1-172` | `apps/shmup/src/player/lowhealth.rs` | 172 |

Artifacts:

- `apps/shmup/tests/player_system_port.rs`
- `apps/shmup/tests/player_system/capture.mjs` → `golden.json` (1.78 MB,
  byte-reproducible; two consecutive runs hash identically)

## What is pinned, and at what tolerance

The golden is a **1600-frame scripted trajectory** driven through the *original*
`PlayerSystem` under Node 24, at 60 fps with two 120 Hz substeps per frame:

idle → walk → mouse look → sprint → gamepad stick look → strafe →
*teleport* → walk into a 0.9 m ledge with jump held → **mantle** →
*teleport* → sprint → double-tap → **tactical sprint** → crouch → **slide** →
*teleport 12 m up* → fall → **land at 22.3 m/s with fall damage** →
crouch → prone → crouch → **lean** → **ADS** → *teleport* →
**explosion** (−41.8) → three bullet hits (−14 each) → **critical at 16.16 hp**
→ two near misses → **regenerate to 100**.

Recorded per frame (43 fields): interpolated + simulation position, velocity,
yaw/pitch/yaw-rate, the composed camera rotation, eye position, forward vector,
FOV, quaternion, the rig's eye height / crouch blend / slide blend / trauma /
bob phase, the view-kick channel, the state name and every state flag, every
health field (value, low, critical, dead, regenerating, suppression, hit flash,
effect, pulse, beat phase), the low-health pass's enabled flag and uniform
state, and the eight hitbox numbers. Plus, sampled every 10 frames, the four
damage indicators and the whole HUD adapter. Plus **the complete ordered list of
the 100 events the facade emitted**, payload field by payload field.

Tolerances:

- **Exact** — every discrete fact (state, stance, booleans, event order, event
  kinds, indicator activity, HUD booleans).
- **`1e-12`** — everything continuous. The run passes through `sin`, `cos`,
  `exp`, `sqrt` and `powf` thousands of times.

Separately, five unit tables from the same capture:

- `LowHealthPass`'s fullscreen-triangle vertex data, UVs, bounding-sphere
  radius and the 1×1 unit-exposure texel — all compared **as `f32`**, because
  all four are `Float32Array` in the source.
- `resize`'s aspect rule over five viewport shapes, including the two
  degenerate ones (`0×600`, `640×0`) that exercise `Math.max(1, …)`.
- `sync`'s enable gate over six `(effect, hitFlash, pulse, critical)` rows,
  straddling the `0.004` threshold on both channels.
- **80 samples of the fragment shader** (5 UVs × 4 states × 4 exposures).
- `Health.damage`'s view-space angle over 20 yaw/offset pairs, and the
  indicator-reuse source quirk (below).

## The one source defect found, and how it is pinned

`health.js:154-156`:

```js
slot.active = true;
slot.angle = angle;
slot.amount = Math.max(slot.active ? slot.amount * 0.5 : 0, amount);
```

`slot.active` is assigned `true` on the line *above* the ternary that tests it,
so the `: 0` arm is unreachable and the halving branch fires even for a slot
that was **inactive** and still carries the previous occupant's `amount`. The
intent was obviously "halve it only if it was already active". Ported as-is
(`Health::push_indicator`), and pinned by
`indicator_reuse_halves_a_stale_amount_source_quirk`, whose expected values
(`40 → expired → still 40 → 20`, where a fresh slot should read `5`) come from
running the original `Health` in the capture script, not from reasoning.

## Traps checked by name

- **Euler order is a convention, not a spelling.** This is the one that
  mattered. `camera.js:85` builds `new THREE.Euler(0, 0, 0, 'YXZ')` and
  `core/engine.js:30` sets `this.camera.rotation.order = 'YXZ'`, so
  `applyTo`'s `camera.quaternion` — and therefore `rig.forward`, which
  `_onBulletImpact` uses to decide friend-or-foe — comes out of three's
  **`YXZ`** table (`qY * qX * qZ`), not `XYZ`. `axiom_math::Quat::from_euler_xyz`
  composes the other way and would produce exactly the "camera that banked on
  its own" this port has already seen once. `system::quat_from_euler_yxz` is
  transcribed from three r180 `three.core.js:3730-3735` and
  `apply_quaternion` from `three.core.js:4798-4817`, both with the source's
  grouping. The golden pins `camQuat` on all 1600 frames, and
  `the_camera_quaternion_uses_threes_yxz_order_not_xyz` states the failure mode
  in one place (and asserts `YXZ ≠ XYZ` so the test cannot be vacuous).
- **`Float32Array`.** `grep` of the three source files finds it only in
  `lowhealth.js` — three times: the 1×1 exposure fallback (`[1,1,1,1]`), the
  fullscreen-triangle positions and its UVs. All three are `[f32; N]` in
  `lowhealth.rs` and compared as `f32` in the test. Nothing in `index.js` or
  `health.js` uses it; every number there is a JS `f64`.
- **`Math.hypot`.** `index.js` uses none. `health.js` uses none.
  `_onExplosion` uses `Vector3.distanceTo`, which is
  `sqrt(dx²+dy²+dz²)` — ported as `sqrt`, not `hypot`. `_onBulletImpact`
  computes `Math.sqrt(d2)` explicitly. The GLSL `length(vec2)` in
  `lowhealth.js` is likewise `sqrt` of the dot product, **not** `hypot` (noted
  at that line in `lowhealth.rs`).
- **`sign` is not `signum`.** None of the three files calls `Math.sign` or
  GLSL `sign`. Nothing to hand-roll.
- **Float arithmetic is not associative.** Every expression is transcribed with
  the source's grouping and left-to-right order, including
  `(1.1 + severity) * DEG * s * 0.7` and the shader's
  `c += vec3(0.115, 0.008, 0.005) * k * invExp`. Nothing was folded or hoisted.
- **`rng.fork()` and literal seeds.** `index.js:147` takes exactly one
  `ctx.rng.fork()` and **never reads the result**. It is preserved
  (`PlayerCore::rng`) because the fork consumes one `u32` from the root stream
  and dropping it would shift every value every later subsystem draws. The
  golden records the root's next `u32` after the fork
  (`init.rootAfterFork = 2216195662`) and the test asserts it, which is what
  makes "exactly one draw" checkable rather than assumed.
- **Dead computation is still part of the source.** Beyond the fork:
  `opts.type` is set by all three of `index.js`'s damage call sites and read by
  nobody in `health.js` — carried as `health::DamageKind` with a comment.
  `opts.suppress`, named in `health.js`'s JSDoc but never passed and never
  read, is **not** carried (it does not exist in any code path).
  `movement.js`'s `_postMove(h, travelled)` ignores `travelled`; the existing
  `movement.rs` already noticed.
- **JavaScript truthiness.** Two places where `||` is not a boolean or:
  `Math.sqrt(d2) || 1e-4` in `_onBulletImpact` (zero *and NaN* fall through to
  the epsilon — both handled explicitly), and
  `if (stick.lookX || stick.lookY)` in `_consumeLook` (nonzero, not "is true").
- **An enum used as a table index.** No table indexing in this slice.
- **A matching count is not proof / your comparator can be the bug.** The
  trajectory test therefore also asserts that the run *visits* every state it
  claims to (`the_trajectory_visits_every_state_it_is_supposed_to`): stand,
  crouch, prone, tacsprint, slide, jump, fall, mantle, lean, plus "went
  critical", "regenerated", "did not die", "emitted a mantle", "emitted a
  heartbeat". Without that, two idle players agreeing would look like a pass.

## Why the golden's world is a stub

The capture harness drives the original facade against a **hand-written
analytic world** — an infinite ground plane at `y = 0` plus one axis-aligned
box (`x ∈ [-2,2]`, `y ∈ [0,0.9]`, `z ∈ [-6,-4]`) — instead of the real
`src/physics/{bvh,character}.js`. `tests/player_system_port.rs` transcribes the
same stub line for line, so the two are bit-identical by construction.

The reason is scope. The slice under test is the *facade*: the integration
order between movement, camera, mantle, health and the low-health pass. Driving
the real physics would fold two other slices' numeric behaviour into every
assertion — and those ports already carry a known last-bit divergence (below).
A facade golden that drifts because of somebody else's `hypot` proves nothing
about the facade.

Two stub choices, mirrored exactly on both sides: directions normalise with
`sqrt(x²+y²+z²)` rather than `Math.hypot` (there is no one-line three-argument
`hypot` in Rust, and using it would reintroduce the divergence the stub exists
to avoid), and the swept-capsule query is a Minkowski-expanded slab test with
no corner rounding — exact for the face-on approach the trajectory takes, which
is the only approach it takes.

**Follow-up worth doing once the physics facade lands:** a second,
looser-tolerance golden over the *real* `StaticWorld` + `Character`, which
would pin the player↔physics seam end to end. It is deliberately not this
slice's job.

## Triage: the four failures on the first integration run

`8 pass / 4 fail` on the first run; `12 pass / 0 fail` after. Which side was
wrong, per failure:

| failure | wrong side | cause |
|---|---|---|
| `frame 1 pos[1]` 0.028569375 vs 0.028569375011656 | **port** | `GRAVITY` narrowed through `f32` |
| `land velocity` 1.03005 vs 1.0300499916 | **port** | same |
| `hud position y` ...695079941 vs ...679366493 | **port** | same |
| `sync` `state[0]` 0.00422118448 vs 0.0 | **golden** | capture harness reused a live pass |

### Failures 1-3 were one bug: the storage-width trap in `config.rs`

The expected values were *exact decimals* and the actual ones were the nearest
`f32` - the tell. `apps/shmup/src/config.rs` typed `config.js`'s `UNITS` block
as `f32` / `Meters` (which is `f32`-backed):

```text
-9.81 * 2.1  in f64  = -20.601000000000003
             via f32 = -20.60099983215332     (2e-8 low)
```

`core/config.js`'s `UNITS` are plain JavaScript numbers and the simulation
*integrates* them 120 times a second, so the rounding happened at the source of
truth rather than at a carrier. One fixed step: `0.03 + g/14400` is
`0.028569374999999998` in `f64` and `0.028569375011656017` via `f32` - the
observed value to every digit. Six steps of the same gravity gives
`1.030049991607666` against the golden's `1.0300500000000001` - the observed
land velocity to every digit.

**Fixed at the root**, not at the call site: `Units` carries `f64` now, with
two opt-in `*_meters()` accessors for a boundary that genuinely stores `f32`.
`player/tuning.rs` lost every `as f64` cast (`GRAVITY`, `STAND.height`/`eye`,
`CROUCH.height`/`eye` - the last two would have failed next, at 2.6e-8).

### The same defect one layer up, found by fixing the first

With gravity fixed the trajectory reached `frame 680 fov` - the first
aim-down-sights frame - and missed by 1.25e-7. `Config::ads_fov_scale` was a
`Ratio` (`f32`), so `0.72` was stored as `0.7200000286102295` and the camera
read it *inside* a per-frame `lerp`. Predicted the magnitude from the algorithm
before touching anything: `80 x (1 - e^(-dt/tau_fov)) x ads x dScale`
= `80 x 0.2743 x 0.1993 x 2.86e-8` = `1.25e-7`, matching the observed residual
exactly - which is what made it safe to widen the *type* rather than the
tolerance.

Fixed the **class**, not the instance: all five numeric `Config` settings
(`fov`, `ads_fov_scale`, `sensitivity`, `ads_sens_scale`, `exposure`) are `f64`
now - the width `config.js` authors them in. `sensitivity` (`0.0022`, not
representable in `f32`) was the identical latent bug in `input.rs`, unmeasured
only because this slice's stub supplies `look` directly. The one legitimate
narrowing is `PauseMenu::set_fov`'s `h.set_camera_fov(fov as f32)`: a render
camera genuinely stores `f32`, so it narrows *there*.

Blast radius, each file checked clean of other agents' edits first:
`config.rs`, `player/tuning.rs`, `player/camera.rs` (one line),
`player/system.rs`, `scene/game.rs`, `input.rs`, `ui/menu.rs`,
`tests/core_port.rs`. `MenuHost::set_camera_fov(f32)` was deliberately **not**
changed, so `ui/system.rs` (another agent's in-flight file) is untouched.

### Two tolerances that existed only to hide this, now tightened

`tests/player_port.rs` carried widened bounds *because of* the `f32` round
trip. Leaving them would let the bug return silently, so:

- `JUMP_SPEED`: `1e-6` -> `1e-12` (only `sqrt` is left), and the test renamed
  off `..._within_a_wider_tolerance`.
- the stance table: `f32::EPSILON` -> **exact equality** on all four values.

Both pass. A tolerance whose stated reason has been removed is not a tolerance,
it is a hole.

### Failure 4 was the golden, not the port

`sync` returns early without writing `uState` when it disables itself. The
capture reused `player.lowHealthPass` - live from the 1600-frame run - for the
unit table, so the first (disabled) row reported the last frame's uniform,
`0.004221184480411328`. The Rust test starts from a constructed pass and quite
correctly produced `0.0`. `capture.mjs` builds a fresh pass for that table now,
and a seventh row (disabled, immediately after a fully-on row) pins the
retain-on-disable behaviour on purpose. The 1600 frame records and the 100
event records re-captured **byte-identically** (`frames` sha1 `f68e201b...`,
`events` sha1 `e854a4d4...`, before and after), so the simulation half of the
golden is unchanged by the fix.

### Final state

`12 passed, 0 failed` on `player_system_port`. Whole-crate re-run after the
shared-file edits: **1261 passed, 0 failed** (`--lib` plus every integration
target except `ai_weapon_port`, which is another agent's in-flight file with a
pre-existing `cannot find value TOL` compile error, untouched by this work).
`TRAJ_TOL` is still `1e-12`.

## Findings for other slices

0. **The storage-width trap was live in `config.rs`, and is now fixed** - see
   the triage section above. Everything in the port that reads
   `crate::config::UNITS` or `Config`'s numeric settings gets the correct
   `f64` today.
1. **`physics/character.rs` drops five `Math.hypot` calls.** `character.js`
   calls `Math.hypot` five times; `apps/shmup/src/physics/character.rs` has
   zero. `movement.js` calls it nineteen times; `movement.rs` has seventeen,
   and writes `speed` (`movement.rs:1279`) as `sqrt(x*x+y*y+z*z)` where the
   source uses a three-argument `Math.hypot`. `Math.hypot` scales by the
   largest magnitude first and rounds differently — ~1 ULP per call. Not fixed
   here (not this slice), but it is the **first thing to suspect** if a
   continuous field in the trajectory misses at `1e-12`; the test carries that
   note and a `TRAJ_TOL` constant that exists so widening it is a one-line,
   documented change that cannot loosen the unit tables.
   `physics/probe.rs::raycast` has the same `sqrt`-for-`hypot` substitution.
   **Recorded by the coordinator as a separate defect owned elsewhere, and
   deliberately not touched in this pass so it could not confound the triage
   above.** In the event it was never reached: the trajectory passes at
   `1e-12` with `TRAJ_TOL` untouched, so whatever `hypot` drift exists is below
   that bound for this slice's inputs. (`crate::jsmath` now provides V8-exact
   `hypot2/3/4` for whoever fixes it.)
2. **Event payload types will collide at integration.** `crate::audio::system`
   already defines listener-side `PlayerFootstep`, `PlayerLand`, `PlayerState`,
   `DamageTaken`, `BulletImpact` and `ExplosionEvent` carrying only the fields
   audio needs. The bus dispatches on `TypeId`, so the player emitting
   `player::system::PlayerLandEvent` will simply **not reach** the audio
   handler downcasting to `audio::system::PlayerLand` — silently, with no
   error. The emitter owns the canonical payload (that is the Module Law's
   data-contract rule), so the reconciliation belongs at integration: either
   every listener downcasts to the emitter's type, or the app re-emits a
   translated payload. Flagged rather than unilaterally resolved, because it
   touches `audio`, `fx` and `ui`.
3. **`physics.lineOfSight` is duplicated.** `PlayerPhysics::line_of_sight`'s
   body for `PhysicsWorld` is a six-line transcription of
   `physics/index.js:616-623`. When the physics facade slice lands it should
   move there and this impl should delegate.

## Divergences from the source, and why

1. **`CameraRig.applyTo` is ported here, not in `camera.rs`.**
   `camera.rs`'s module doc says `applyTo` (`camera.js:346-356`) is not ported
   because there was no camera type to write onto — and, consequently, that
   `rig.forward` never updates. That hole is load-bearing for this slice:
   `_onBulletImpact` reads `rig.forward`, and `Health.damage` /
   `_onExplosion` read `ctx.camera.position`. So the facade owns
   `system::PlayerCamera` (position, `YXZ` rotation, quaternion, fov) and
   `PlayerCamera::apply_rig` is that missing method. **It belongs in
   `camera.rs` the moment a render camera lands**; it is here because the
   facade is the first caller that genuinely needs it, and `camera.rs` was off
   limits for this slice.
2. **`this.ctx` becomes three cached fields.** The source reaches
   `ctx.time.elapsed`, `ctx.config` and `ctx.events` from inside event
   handlers, `teleport` and `debugState`. `EventBus` handlers are `Fn` with no
   `ctx`, so `PlayerCore` holds `time`, `config` and the bus, refreshed by
   `PlayerCore::set_ctx` at the top of `fixed_update`/`update`. `ctx.time` in
   JavaScript is a *live object*, so a frame driver that advances the clock and
   then calls an external API (`teleport`) or emits into the bus **must call
   `set_ctx` first** — otherwise `_lookFrame` and `lastDamageTime` land one
   frame late. The test does exactly that, and the method's doc comment says
   so. This was a real bug caught while writing the test, not a hypothetical.
3. **`Health`'s `ctx`/`rig` are parameters, not fields.** Rust cannot hold a
   `&mut CameraRig` inside a struct that `PlayerCore` also owns. Call order is
   unchanged. The source's `if (this.rig)` guard is dead at both construction
   sites, so the parameter is not an `Option`.
4. **`_payload` / `_beat` are constructed per emit; `_statePayload` is a
   field.** The first two are preallocated only to avoid a per-frame
   allocation and every field is written before every emit. `_statePayload` is
   different — `_emitState` reads its *previous* `low` to compute
   `changedLowState` — so it stays a field.
5. **Five seams named as traits**, following the audio port's precedent:
   `PlayerLook` (the two `ctx.input` reads `movement::PlayerInput` does not
   already cover), `SpawnSource` (`world.spawn(i)`), `PlayerPhysics`
   (`groundHeight` / `lineOfSight` / `createCharacter`, on top of the
   `WorldProbe` the ledge and lean probes already needed), plus the existing
   `movement::CharacterController` and `mantle::WorldProbe`.
   `PhysicsWorld` implements `PlayerPhysics` directly, so the real collision
   world binds with no adapter.
6. **`PlayerHitbox` is a value, not a registered collider.** No collider
   registry is ported (`physics/index.js` is its own slice), so the facade owns
   the capsule and keeps it on the interpolated position exactly as
   `_syncHitbox` does. Registering it is the physics facade's job.
7. **`console.info` at `index.js:192-196` is dropped.** `PlayerCore::spawn`
   carries the same facts as a value.
8. **`LowHealthPass`'s GPU half is not ported.** `render(renderer,
   inputTexture, target, r)` and the `Material`/`Geometry`/`Mesh`/`Scene`/
   `Camera` objects have no counterpart yet. `VERT`/`FRAG` are carried
   verbatim as `&'static str` so the eventual binding transcribes from the GLSL
   rather than from prose, and `LowHealthPass::shade` is a CPU `f64`
   transcription of `FRAG`'s body.
   **This transcription is itself the risk the recipe names**: GLSL in a
   JavaScript string has no oracle to call. The mitigation is that
   `capture.mjs`'s `shadeFrag` was transcribed from the same GLSL
   *independently*, and the test compares the two over 80 samples. A shared
   misreading would defeat both, which is why each was written line by line
   against the shader rather than against the other.
9. **`debugState`'s `default: break;` arm is dropped.** An unrecognised name is
   not expressible as an enum variant, and the only caller is a dev overlay
   picking from the list.
10. **`PlayerStateName` is a new type.** `_publishState` can publish `'lean'`,
    which is *not* one of `movement.js`'s ten `STATES` — the movement machine
    never enters a lean state; the facade synthesises the name. So the payload
    needs its own eleven-variant enum rather than `MovementState`.
11. **`Rc<RefCell<PlayerCore>>` re-entrancy.** As with `crate::registry`, a
    third-party handler that reaches back into the player *while the player is
    emitting* would hit a `RefCell` double-borrow panic where JavaScript would
    silently re-enter. The player emits only events it does not itself listen
    to, so no self-re-entrancy exists today.

## What the orchestrator must wire

`mod.rs` / `lib.rs` / `Cargo.toml` were not touched, per the fan-out brief.
Required additions, plus five small accessors in files this slice was not
allowed to edit:

```rust
// apps/shmup/src/player/mod.rs
pub mod health;
pub mod lowhealth;
pub mod system;
```

```rust
// apps/shmup/src/player/camera.rs — inside `impl CameraRig`
/// `get bobPhase()`. `player/index.js:581-583` reads `this.rig.bobPhase`.
pub fn bob_phase(&self) -> f64 {
    self.bob_phase
}
```

```rust
// apps/shmup/src/player/movement.rs — inside `impl Movement`

/// `this.movement._footHold = FOOTSTEP.landHold` (`player/index.js:338`).
pub fn set_foot_hold(&mut self, v: f64) {
    self.foot_hold = v;
}

/// `this.movement._cmdFrame = -1` (`player/index.js:630`) — a frame number no
/// real frame equals, so the next latch always runs.
pub fn invalidate_cmd_frame(&mut self) {
    self.cmd_frame = None;
}

/// `this.movement.latchInput(-2)` (`player/index.js:621`), whose only caller
/// has already set `controlEnabled = false`, so it takes the flush branch.
pub fn flush_latched_input(&mut self) {
    self.cmd = PlayerCommand::default();
    self.prev_held = PrevHeld::default();
    self.cmd_frame = None;
}

/// `m._beginSlide(m.cmd, m._wish.set(...), 1, MOVE.sprintSpeed)`
/// (`player/index.js:692`) — `debugState('slide')` reaches into the private
/// slide entry, so the facade needs a door.
pub fn debug_begin_slide(&mut self, wish: Vec3, wish_len: f64, current_speed: f64) {
    let Some(mut character) = self.character.take() else {
        return;
    };
    let cmd = self.cmd;
    self.begin_slide(character.as_mut(), cmd, wish, wish_len, current_speed);
    self.character = Some(character);
}
```

Nothing else outside the assigned paths needs to change. `Cargo.toml` already
has the `serde_json` dev-dependency the test needs.

**One thing to expect at integration:** `PlayerSystem::deps()` declares
`["physics", "world", "render"]`, faithfully. None of those three is a
registered subsystem in this crate yet, so `Registry::resolve()` will reject a
registry that contains `PlayerSystem` and not them. That is the source's own
contract (its registry throws identically), and it is informative rather than
wrong — but an app wiring the player has to register all three, or the
orchestrator has to trim the list deliberately.

## Not covered by the trajectory

- `respawn(index)`, `setControlEnabled`, `setAdsProgress`, `debugState`,
  `addKick`, `addCameraShake`, `heal`, `dispose` — ported, but the pinned
  trajectory does not call them (the source's own harness does not either).
  `teleport` **is** exercised, five times.
- `LEDGE_VAULT`. The stub world's single box is 0.9 m tall and 2 m deep, which
  the probe resolves as a mantle. A vault needs a thin obstacle; the two paths
  differ only in `kind` and the landing point, both of which `mantle.rs` (a
  different slice) already pins.
- `PlayerSystem`'s `Subsystem` impl. The test drives `PlayerCore` directly and
  wires the three incoming-damage subscriptions by hand, in the same order
  `wire_events` does, because building a real `Ctx` needs an `Engine` and a
  `Registry` — and the registry would reject the `deps()` above. What the
  wrapper adds over that is `id`/`deps`/`phases` and two forwarding calls.
