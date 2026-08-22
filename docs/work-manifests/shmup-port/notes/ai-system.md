# `ai/index.js` — the `AiSystem` facade

Source: `C:/dev/Claude-of-Duty/src/ai/index.js:1-1107` (the whole file).
Port: `apps/shmup/src/ai/system.rs`.
Golden: `apps/shmup/tests/ai_system/{capture.mjs,golden.json}` →
`apps/shmup/tests/ai_system_port.rs`.

This is the AI slice's orchestration tier — the file `ai/mod.rs` deferred. Every
dependency it composes (`nav`, `agent`, `squad`, `grounding`, `soldier`,
`textures`, `animator`, `rig`, `geo`, `parts`, `clips`, `weapon`) had already
landed, which is what made it portable.

---

## 1. `ai/mod.rs` is now out of date

Its doc comment (lines 12-39) says `soldier.js`, `parts.js`, `rig.js`,
`animator.js`, `clips.js`, `geo.js`, `textures.js`, `weapon.js` and
`index.js` are "deliberately not in this slice". **All nine have landed.**
The paragraph beginning "`src/ai/index.js` (`AiSystem`) is also not ported
here" is wholly obsolete, and the sentence about "once the deferred
body/animation slice lands" has come true. The final paragraph — about naming
the narrowest trait per unported collaborator — is still correct and still the
governing principle here.

Concretely, the block that should go is `mod.rs:12-39`, replaced by a row per
new module in the table at the top and a short note that `system` is the
orchestration tier. Not edited here, per the fan-out brief.

---

## 2. What is ported

Everything in `index.js` except the scene graph and the GPU. Method by method:

| `index.js` | port |
|---|---|
| `init` (55-155) | `AiCore::new` + the seam setters + `boot_nav` + `prewarm_materials` |
| `_bootNav` (161-169) | `AiCore::boot_nav` |
| `prewarmMaterials` (199-265) | `AiCore::prewarm_materials` — the deterministic half; see §4 |
| `_dummySkinGeometry` (273-286) | **not ported** — a 3-vertex `BufferGeometry` that exists only to feed `renderer.compileAsync` |
| `_wireEvents` (292-350) | `AiCore::on_weapon_fire` / `on_bullet_impact` / `on_damage_dealt` / `on_explosion` / `on_player_footstep`, plus `AiSystem::wire_events` for the bus half; see §5 |
| `_falloff` (352-359) | `AiCore::falloff` |
| `_distanceToRay` (361-367) | `ai::system::distance_to_ray` |
| `variant` (373-392) | `AiCore::variant` |
| `rigIndex` (395-397) | `AiCore::rig_index` |
| `get phys` (399-401) | `AiCore::set_physics` |
| `_buildNav` (407-430) | `AiCore::build_nav` |
| `probeGround` (433-444) | `AiCore::probe_ground` (+ the animator's `GroundProbe` impl) |
| `groundAt` (446-452) | `AiCore::ground_at` |
| `playerPosition` (455-465) | `AiCore::player_position` |
| `spawn` (471-475) | `AiCore::spawn` |
| `populate` (483-538) | `AiCore::populate` |
| `createSquad` (540-544) | `AiCore::create_squad` |
| `_daylight`/`_flashGain`/`_flashLight` (551-582) | `AiCore::daylight` / `flash_gain` / `flash_light` |
| `onAgentFire` (584-626) | `AiCore::on_agent_fire` |
| `_testPlayerHit` (628-655) | `AiCore::test_player_hit` |
| `emitReload` (657-659) | `AiCore::emit_reload` |
| `_ensureGrenade` (662-670) | **not ported** — `IcosahedronGeometry` + a `MeshStandardMaterial`; its only behavioural trace is `prewarm.materials = mats.length + 1` |
| `throwGrenade` (672-699) | `AiCore::throw_grenade` |
| `_updateGrenades` (701-717) | `AiCore::update_grenades` |
| `update` (723-761) | `AiCore::update` |
| `lateUpdate` (763-773) | `AiCore::late_update` + `shadow_placements` |
| `requestPath` (785-793) | `AiCore::request_path` (+ the internal `BudgetedPath`) |
| `_sunDirection` (796-803) | `AiCore::sun_direction` |
| `_updateRelevance` (825-859) | `AiCore::update_relevance` + `Frustum` + `actor_matrix_world` |
| `_updateStaged` (869-902) | `AiCore::update_staged` |
| `_stageSlot` (911-973) | `AiCore::stage_slot` |
| `debugStage` (980-1054) | `AiCore::debug_stage_firefight` |
| `_stageInspect` (1057-1083) | `AiCore::stage_inspect` |
| `dispose` (1087-1104) | `AiCore::dispose` |

`stats.navMs` is deliberately dropped: it is `performance.now()` arithmetic and
therefore not a value a deterministic port may carry. Every `console.info`
diagnostic is dropped; each number it prints is a field of `AiStats` or of the
variant build.

Three pieces of Three.js had to be transcribed because `_updateRelevance` runs
on them: `Frustum.setFromProjectionMatrix` (WebGL coordinate system),
`Frustum.intersectsSphere`, `Plane.normalize`/`distanceToPoint`,
`Sphere.applyMatrix4` and `Matrix4.getMaxScaleOnAxis`. All were transcribed from
`node_modules/three/build/three.core.js` directly, and all are exercised by the
`relevance` battery.

---

## 3. Determinism — the fork order, and the one non-obvious rule

| source | port |
|---|---|
| `index.js:55` `this.rng = ctx.rng.fork()` | `AiCore::new`'s argument |
| `index.js:61` `new SoldierMaterials(this.rng.fork(), …)` | `AiCore::new` |
| `index.js:377` `buildSoldier(name, { rng: this.rng.fork() })` | `AiCore::variant` |
| `agent.js:97` `this.rng = ai.rng.fork()` | `AiCore::spawn`, **before** `variant()` |
| `agent.js:136` the animator's `this.rng.fork()` | `Agent::new`'s second return value |
| `index.js:541` `new Squad(this.rng.fork())` | `AiCore::create_squad` |

**`spawn`'s order is the trap.** `new Agent(this, …)` runs its body top-down, so
the agent's own fork (`agent.js:97`) is taken *before* `ai.variant(name)`
(`agent.js:99`) can fork for a variant the level has not built yet. Swapping
those two reorders every draw in the garrison. `populate_garrisons_the_level_draw_for_draw`
pins each agent's own four state words *and* its animator's, so a swap fails
immediately rather than as a position drift.

`populate`'s per-member order is `range(0, 2π)`, `range(0.8, 3.2)`,
`signed()` — the yaw's `signed()` draw precedes the agent's fork because
JavaScript evaluates `spawn(…, anchor.yaw + this.rng.signed() * 0.7, …)`'s
arguments before it calls.

---

## 4. Divergences, each deliberate

1. **`prewarmMaterials` stops at the material enumeration.** `renderer.compileAsync`,
   `r.patcher.patch` and the throwaway `SkinnedMesh` need a live WebGL context.
   The half that is deterministic — resolving every `VARIANTS` entry against
   `MATERIAL_SLOTS` in the builder's own order and de-duplicating — is ported and
   returns `PrewarmReport { ok: false, materials, programs: 0 }`. `ok` is `false`
   on *both* sides here, and for the same reason: the source takes
   `if (!renderer) return out` with `ok` still `false`. The golden's
   `materials: 26` is the source's own `mats.length + 1`. De-duplication in the
   source is on the *material object*, which `SoldierMaterials.get` caches by an
   options key; `MaterialRequest` is that key as a value, so `Vec::contains` is
   the same relation.
2. **The scene graph is gone, but the one number it fed back is not.**
   `_updateRelevance` reads `a.mesh.matrixWorld`. `ai.root` is never transformed
   and the mesh sits at the identity inside the actor group, so the world matrix
   *is* `compose(position, Euler('XYZ', 0, yaw, 0), scale)` — `actor_matrix_world`.
   Multiplying an identity into it is exact, so nothing is approximated.
3. **`ctx.peek` becomes setters,** exactly as in `audio/system.rs` and
   `ui/system.rs`: `set_camera`, `set_sky`, `set_player`, `set_world`,
   `set_clock`. `sky.setTimeOfDay(17.9)` is a *write into another subsystem*, so
   `debug_stage_firefight` returns the number instead of making the call.
4. **Emits become an ordered effect journal** (`AiEffect`), the `ui/system.rs`
   precedent. It carries the source's full payloads — including `weapon:fire`'s
   `seed`/`intensity`/`light`/`flashScale` and `weapon:shell`'s `velocity`, which
   no existing bus payload type has a field for. See §5.
5. **`_grenades` holds a `BodyId`, not a `RigidBody`.** `phys.addRigidBody` and
   the per-frame `body.position` read-back are the `GrenadeBodies` seam.
6. **`slopeLimit: 48` is ported as written, bug and all.** `CharacterController`
   reads `slopeLimit` in *radians* (`character.js:40`, whose own default is
   `50 * PI/180`), so `ai/index.js:151` is asking for 48 **radians** and gets
   `cos(48) = -0.748` — i.e. every surface, including a vertical wall, counts as
   ground for an enemy. Pinned by the frame golden (the agents' `grounded`
   behaviour) and commented at the site in `AiCharacters::create_character`.
7. **`_bootNav`'s `try`/`catch` has nothing to catch.** Every condition the
   source's `catch` could see — no physics, no triangles, no level — is an early
   return in the port, and `nav_pending` is left set the same way.
8. **One defect the golden caught, in the port rather than the source.** The
   first draft of `on_damage_dealt` read
   `self.falloff(Some(e.point.unwrap_or(agent.position)))`. The source is
   `e.amount * this._falloff(e.point)` and *then*
   `applyDamage(amount, part, e.point ?? a.position, …)` — two different
   defaults for the same missing field, and folding the second into the first
   applies distance falloff to a hit that should take none. The two `handlers`
   cases that end the battery are built to disagree (the target is moved 110 m
   from the player first, so falloff is a live `0.45x`): the no-`point` arm
   leaves the agent at 40 health, the far-`point` arm at 73. Before the fix
   both read 73.
9. **`phys.staticWorld.dirty` / `rebuildStatic()` / `triangleCount <= 0` are
   gone.** They are the physics facade keeping its own BVH current; the probe
   handed to `AiCore` is already built. A caller with no level passes no probe,
   which is the same "retry next frame" state.

---

## 5. The event-payload vocabulary is forked, and this slice did not widen it

`EventBus` dispatches on `TypeId`, so there must be exactly one payload type per
event name across the whole game. There are already **three** partial sets —
`crate::audio::system`, `crate::ui::system`, `crate::player::system` — and
`ui-system.md` §5.2 and `player-system.md` §2 both flag it as an integration-pass
decision. This slice adds **no fourth set of bus payloads**. Instead:

* `AiCore`'s handler methods take AI-shaped *argument* structs
  (`WeaponFireHeard`, `BulletImpactHeard`, `DamageDealtToAgent`,
  `ExplosionHeard`, `PlayerFootstepHeard`) — the shape `index.js`'s handler
  bodies actually read. These are complete and are what the golden pins.
* `AiSystem::wire_events` subscribes to the **existing** types.

What each existing type could and could not supply:

| event | subscribed to | complete? |
|---|---|---|
| `weapon:fire` | `audio::system::WeaponFire` | **no** — no `dir` field, so the line-of-fire suppression arm (`index.js:303-306`) is unreachable from the bus |
| `bullet:impact` | `audio::system::BulletImpact` | yes |
| `explosion` | `player::system::ExplosionEvent` | yes — the only one of the three `explosion` types carrying `damage` |
| `player:footstep` | `player::system::PlayerFootstepEvent` | yes, and it is the emitter's own type |
| `damage:dealt` | *nothing* | **no** — see below |

**What did not exist, precisely:**

1. `weapon:fire` has no `dir` anywhere. `audio` needs `origin` + `weapon`; the
   AI needs `origin` + `weapon` + `dir` + (as emitter) `seed`, `intensity`,
   `light`, `flashScale`.
2. `damage:dealt` has no *agent identity*. The source's guard is
   `e.target instanceof Agent` and its body reads `amount`, `part`, `point`,
   `incident`, and **writes `e.killed = true` back onto the payload**. No
   existing type carries `part`, `incident` or an actor id, and `&dyn Any`
   payloads are immutable, so `AiCore::on_damage_dealt` returns `killed` as a
   value.
3. `weapon:shell` has no `velocity` (`audio::system::WeaponShell` is
   position-only).
4. `explosion` as *emitted* by the AI also carries `source` (the throwing
   agent); nothing carries it.
5. `weapon:reload` as emitted carries `actor`; `audio::system::WeaponReload`
   does not.
6. `actor:death` as raised by `Agent::die` carries `point`, `impulse` and
   `headshot`; `audio`'s carries `point` + `actor_id`, `ui`'s carries two names.

All six live in `AiEffect` so nothing the source emits is silently dropped while
the fork stands.

---

## 6. `Agent::update` had to be unpacked into its five phases

`Agent::update` (`agent.rs`) returns its events in a batch at the end. That is
one tick too late for two of `ai/index.js`'s callbacks:

* `onAgentFire` reads `agent.animator.ejectWorld` (`index.js:600`) from inside
  `_shoot`, i.e. **before** `_drive` re-poses the skeleton.
* `throwGrenade` calls `agent.animator.fire(0.35)` (`index.js:698`) from inside
  `_think`, so a deferred grenade would start its recoil envelope a frame late
  and every muzzle transform after it would drift.

So `AiCore::step_actor` drives `sense`/`think`/`move_step`/`shoot`/`drive`
individually (`Phase5`), exactly as the source's own `_updateStaged` does, and
processes the events between the phases that raise them.

That needed one value `Agent` keeps private: `_pendingDest`, which
`agent.js:281-284`'s retry reads. It is recovered at the seam —
`BudgetedPath::request_path` records the destination it refused, on exactly the
deferral that sets `path_pending`, so it is the same value taken from the same
call. `Actor::pending_dest` holds it.

**Suggestion for the `ai-agent` slice** (not made here): either make
`pending_dest` `pub`, or give `Agent` a `retry_pending_path(&mut AgentCtx)`
method. Either would let this tier drop `Actor::pending_dest`.

---

## 7. Two borrow-checker shapes worth knowing

Both are trait-object-lifetime invariance, and both are commented at the site.

1. **`CtrlRef`.** `Actor` owns its controller as `Box<dyn AgentController>`,
   whose *object* lifetime is `'static`; `AgentCtx<'a>::controller` is
   `&'a mut (dyn AgentController + 'a)`. Shortening a trait object's lifetime
   behind a `&mut` is invariant and forbidden — but *unsizing a sized type* is
   not, and picks the short lifetime freely. So the box is reborrowed into a
   sized wrapper and the wrapper is unsized instead. (`cover.as_mut().map(|c| c
   as &mut dyn CoverSource)` needs no wrapper because `CoverMap` is sized.)
2. **`Phase5` instead of a closure.** `AgentCtx<'a>` borrows eight things built
   inside `run_phase`. `impl FnOnce(&mut AgentCtx<'_>)` either pins `'a` to
   `'static` (elided) or demands the callback work for *every* `'a`
   (higher-ranked). Naming the phases as data sidesteps both — and it makes the
   source's five-call sequence legible at the call site.
3. `Option<&mut dyn AiBallistics>` is written `Option<&mut (dyn AiBallistics +
   '_)>` throughout so the object lifetime can stay long while the reborrow
   stays short. Without the explicit `+ '_`, every `as_deref_mut()` reborrow is
   an invariance error.

---

## 8. The golden

`tests/ai_system/capture.mjs` stands up the **real** `PhysicsSystem`, a real
`THREE.Scene` of seven boxes registered through the real `addStatic`, and the
real `AiSystem`, and calls the real `await ai.init(ctx)`. So `SoldierMaterials`
bakes for real, `NavGrid`/`CoverMap` build against the real BVH, `buildSoldier`
runs for real, and `populate()` garrisons the level through the real `Agent`
constructor. The RNG draw order captured is the real one, not a reconstruction.

**The static world crosses as triangles, not as a stub.** `tests/ai_nav`'s
capture had to transcribe a slab ray test twice and pin the instrument first.
This one does not need to: `physics/bvh.js` bakes every registered mesh to a
flat world-space `Float32Array`, and `crate::physics::bvh::StaticWorld::add_triangles`
accepts exactly that, so both sides build the same BVH from the same
`f32`-widened numbers. A 40-ray `probes` battery still runs first
(`bvh_agrees_with_the_sources_before_anything_reads_nav`) so a BVH disagreement
names itself.

Sections pinned:

| section | what |
|---|---|
| `probes` | 40 rays through the shared BVH — the instrument, before the measurement |
| `boot` | `ctx.rng` before/after the fork, `ai.rng` after init, the nav grid's `nx/nz/min/top_y/walkable`, the cover point count, all three variants' triangle/vertex/material counts and bounding spheres, `prewarm`, and per agent: id, variant, position, yaw, scale, height, radius, eyeHeight, fireRate, the three constructor draws, collider count, controller presence, **its own four RNG state words and its animator's**, and the patrol route |
| `math` | `_falloff` (7 cases incl. `null`), `_distanceToRay` (16), `_daylight`/`_flashGain`/`_flashLight` over 11 sun altitudes incl. the `?? 0.6` fallback, `_sunDirection`'s three fallbacks, `playerPosition`'s two sources, `groundAt`/`probeGround` (14) |
| `frames` | 300 ticks of `update` + `lateUpdate` with scripted `weapon:fire`/`bullet:impact`/`explosion`/`damage:dealt`/`player:footstep` injections; per-tick per-agent 22-field digest, `stats`, and the ordered emit log |
| `fire_bullets` | every `phys.fireBullet` the AI made, argument by argument, with the impacts it got back — replayed by `ScriptedBallistics`, which asserts the request as it goes |
| `handlers` | 25 cases walking every arm of the five consumed events from a reset agent state, including the `!e`, `ai_rifle`, no-`dir`, out-of-range, dead-target and non-`Agent`-target guards |
| `path_budget` | five `requestPath` calls across the 2-per-frame budget, with the deferral counter |
| `relevance` | six camera/actor pose sets through `_updateRelevance` |
| `grenades` | three ballistic solves (body position/velocity/mass/radius/restitution/friction) and the fuse → `explosion` |
| `stage_slots` / `stage` | the six `_stageSlot` placements and the whole `debugStage('firefight')`, including `ai.rng` before and after |

**Tolerances.** Exact for RNG state words, counts, ids, flags, cell indices and
the material enumeration. `1e-12` for anything through `sin`/`cos`/`sqrt`/`atan2`.
`1e-9` for the frame loop's agent kinematics — values a long `-= dt` / `+= dt*k`
accumulation has walked through, summed in the same order on both sides. That
figure was measured, not chosen: the worst disagreement over the whole 300-tick
run is `1.7e-9` absolute on an agent x of `2.94` at tick 280 (relative
`4.4e-10`), and `1e-12` fails on exactly that one value and nothing else.
Everything the frame test compares that is not an accumulation — ids, states,
clips, ammo, path indices, the effect log's `seed` — is exact.

The contact-shadow matrices are the one place a wider figure is needed:
`InstancedMesh.instanceMatrix.array` is a `Float32Array`, so the golden holds
them at `f32` while the port composes in `f64` (`1e-6`). That is the width of
the storage, not slack in the algorithm — `grounding.rs`'s own golden already
pins `instance_matrix` against the `f64` `Matrix4.elements`.

### Two things the harness deliberately does not do

Both keep this a golden of `ai/index.js` and nothing else, and both are recorded
in the golden rather than hidden:

1. **`phys.init(ctx)` is not called.** It forks `ctx.rng`
   (`physics/index.js:244`), which would put the AI's stream in a state the Rust
   test could not reconstruct from a literal seed; and it subscribes physics'
   own `explosion` / `actor:death` handlers, which are a different slice's
   behaviour.
2. **`phys.emitImpact` is replaced with a recorder.** `fireBullet` emits
   `bullet:impact` from inside physics (`physics/index.js:730`) — and
   `damage:dealt` too, when the round struck an actor collider — which re-enters
   `ai/index.js`'s *own* handlers mid-`update`. That coupling is real in the
   browser, but it is the physics facade emitting, and modelling it would force
   the ballistics seam into a re-entrant shape (the stub would have to call back
   into the core that is calling it). The AI's `bullet:impact` handler is pinned
   directly instead, by the tick-60 injection and by the `handlers` battery, and
   every `fireBullet` the AI makes is logged so the seam itself is pinned. The
   suppressed count is written to the golden as `impacts_suppressed`.

   *This also means AI-on-AI friendly fire is not exercised by the frame golden.*
   When the physics facade is wired for real in an app, it will happen; the
   behaviour it triggers (`on_damage_dealt`) is pinned by the `handlers` battery.

---

## 9. Lines the orchestrator must add

```
apps/shmup/src/ai/mod.rs:  pub mod system;
```

and, in the same pass, the `mod.rs` doc-comment correction described in §1.
