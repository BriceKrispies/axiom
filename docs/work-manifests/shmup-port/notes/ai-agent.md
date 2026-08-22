# `ai/agent.js` + `ai/grounding.js` — finishing the half-finished port

Slice: `apps/shmup/src/ai/agent.rs`, `apps/shmup/src/ai/grounding.rs`.
Source: `C:/dev/Claude-of-Duty/src/ai/agent.js` (1009 lines),
`src/ai/grounding.js` (196 lines).

Both files were already on `main`, compiling and wired in, with **no tests at
all** — the hazard `06-parallel-port-plan.md` lists as "indistinguishable from
correct". `agent.rs` was ~745 lines against 1009 (≈74%); `grounding.rs` was
flagged borderline. This slice audits the stated exclusions, ports the
remainder, and pins the lot with a golden captured from the original running
under Node 24.

---

## 1. The audit: which exclusions were real

The old module doc comment listed nine things as "deliberately not carried
over". Judged one at a time:

| claimed exclusion | verdict |
|---|---|
| `THREE.SkinnedMesh`, `group.add`, `mesh.bind(skeleton)`, `updateMatrixWorld` (`agent.js:104-132`) | **Legitimate.** Pure scene-graph construction. The engine's render arm is a separate slice. Nothing here creates a mesh. |
| `animator.js`'s layered blending and four IK solvers | **Legitimate as a subsystem** — it is its own 559-line row in the plan. **But the exclusion was applied far too widely:** `agent.js` *calls into* the animator eleven different ways, and every one of those calls is behaviour. Now ported against `AgentAnimator`. |
| the ragdoll hand-off (`_makeRagdoll`, `agent.js:891-925`) | **Legitimate at the solver call** — `phys.createRagdollFromSkeleton` enters `physics/ragdoll.js`, 763 unported lines of PBD. **Not legitimate for `die` as a whole**, which was dropped wholesale. `die`'s impulse, hit-point fallback, collider/controller teardown and `actor:death` event are ordinary arithmetic and state and are now ported. |
| `InstancedMesh` upload in `grounding.js` | **Half legitimate.** `setMatrixAt` / `instanceMatrix.needsUpdate` / `mesh.count` / `mesh.visible` are the GPU upload. But `_place`'s *quaternion composition and `Matrix4.compose`* were dropped along with them, and that is arithmetic — it was the only non-trivial maths in the file. Now `Placement::instance_matrix`. |
| `_shoot`/`_fireRound` ("firing needs the muzzle transform") | **Unfinished port.** Only `muzzleWorld`/`muzzleDir` need the animator. The aim-target lerp with its wobble, the ammunition/reload branch, the burst timer, the spread draws and the normalisation are pure. Now ported; the muzzle is two trait methods. |
| `applyDamage` / `_sideOf` ("body concerns") | **Unfinished port.** Health, suppression, the last-known-position back-projection, the state promotion, the region selection and the `Math.sign(...) || 1` side are all pure logic. Now ported. |
| `syncHitboxes` ("body concern") | **Unfinished port.** It is the `HITBOXES` table plus two bone reads per row. Now returns `Vec<HitboxSegment>` (there is no collider registry in this slice), the same "return the intended writes as data" shape `squad.rs` already uses. |
| `searchPoint` / `reactionTimer` / `aimActual` "are dead fields, so they are omitted" | **The observation was right, the conclusion was wrong.** The recipe says *dead computation in the source is still part of the source*. Restored as fields with a comment. Same for the `DOLL` table and `DEG`, which the old port dropped without mentioning them at all. |
| `_nextId` → explicit `id` argument | **Legitimate**, and unchanged — but `squad.rs` already links `[super::agent::next_agent_id]`, a function that did not exist. That broken intra-doc link is now a real function. |

Three further gaps the old doc comment did **not** admit to:

* **`update(dt, ctx)` — the whole per-frame tick driver — was missing.** Every
  timer the FSM reads (`stateTime`, `suppression` decay, `fireCooldown`,
  `burstCooldown`, `grenadeCooldown`, `peekTimer`, `repathTimer`,
  `vaultCooldown`, `lastKnownAge`) is advanced there, as is the deferred-path
  retry. Without it the ported FSM could not run at all.
* **`move_step`'s doc comment claimed "`position`/`yaw` are still advanced
  here".** Only `yaw` was. The source's no-controller branch
  (`agent.js:700-701`, `position.x += steer.x * speed * dt`) was absent, and so
  was the entire controller branch, `animator.turn`, the stuck-timer repath and
  `_tryVault`. All now ported.
* **`go_to` silently dropped the `if (!grid)` fast path and could never set
  `path_pending`**, so the field it documented as "kept only so a future budget
  layer has somewhere to write it" was unreachable. `PathSource` now models all
  three of the source's answers (budget spent / no route / a route).

## 2. Bugs found in the existing Rust while auditing

These were live defects in the code on `main`, not gaps:

1. **`multiplyScalar(1/d)` ported as a divide.** Three's `normalize()` is
   `divideScalar(length() || 1)`, which is `multiplyScalar(1/l)`. `a * (1/l)`
   and `a / l` differ in the last bit. Four sites: `_sense`'s `to`, `_move`'s
   `to` and `_steer`, `_combat`'s `away` and `perp`. All now reciprocal
   multiplies.
2. **A zero-length normalise diverged.** The old port guarded with
   `if l > 1e-9 { … } else { … }`; Three divides by `length() || 1`, i.e. it
   leaves a zero vector alone and *does not* special-case a tiny non-zero
   length. Now `1.0 / if l == 0.0 { 1.0 } else { l }`.
3. **`_combat` passed `ai.agents` where the source passes `sq?.members`.**
   `cover.pick`'s bunching penalty scores against the *squad*, not against every
   agent in the level. `AgentCtx::squad_positions` now carries the right list,
   and is `None` when there is no squad (the source passes `undefined`).
4. **`Neighbor` used `id` where the source uses object identity** (`o === this`).
   Kept — the ids are unique — but now documented, along with the fact that
   `this.id % 2 ? 1 : -1` takes the `1` arm for a *negative* odd id too, because
   JS `%` keeps the dividend's sign and any non-zero value is truthy.
5. **`_steer.lengthSq()` was computed over x/z only.** Harmless today (y is
   always 0) but it is not what the source computes; restored to the 3-component
   form, which is exactly equal when y is 0 and honest when it is not.

## 3. Where an unported subsystem forced a seam

Following the precedent `grounding::FootSource` set — *name the narrowest trait
that is exactly the call the source makes*:

| trait | the source expression it names |
|---|---|
| `PathSource` | `ai.requestPath(from, dest, out)` — `None` is `n < 0`, `Some(vec![])` is `n === 0`. `nav::NavGrid` implements it. |
| `CoverSource` | `ai.cover?.pick` / `.peekOffset` / `.release`. `nav::CoverMap` implements it. |
| `SquadPermissions` | `sq.requestPeek` / `canFlank` / `claimFlank` / `requestGrenade`. `SquadSeat` adapts a real `squad::Squad` plus the frame's `MemberSnapshot`s (the squad stores ids; `canFlank` needs live state). |
| `AgentAnimator` | the eleven `this.animator.*` members `agent.js` touches. |
| `AgentController` | the six members of `phys.createCharacter(...)`'s return value that `_move`/`_drive` touch. Mirrors `player::movement::CharacterController`. |
| `GroundHeight` | `ai.groundAt(x, z, fromY)` — the one AI-system query `_tryVault` makes. |
| `nav::WorldProbe` (reused) | `phys.raycast` / `raycastAny`. |

`_sense`'s `phys.lineOfSight` routes through `nav::line_of_sight`, which is
correct rather than convenient: `physics/index.js:616-623` and `nav.js`'s helper
are the same six lines — same `Math.hypot`, same `d - 1e-3` shortfall.

Everything the source pushes *outward* (`ai.emitReload`, `ai.onAgentFire`,
`ai.throwGrenade`, `ctx.events.emit('actor:death')`) is returned as an
`AgentEvent` list instead of being written through a borrowed reference — the
same ownership-forced divergence `squad::SquadUpdate` already makes.

## 4. The golden

`apps/shmup/tests/ai_agent/capture.mjs` → `golden.json` (5.9 MB, byte-
reproducible: two runs `cmp` clean). Read by `apps/shmup/tests/ai_agent_port.rs`.

**The `Agent` under test is the real class, built by the real constructor.**
`RIG.createSkeleton()`, `new THREE.SkinnedMesh(...)` and `new Animator(...)` all
run headless under Node, so the constructor's RNG draw order is the real one:
`ai.rng.fork()`, then the animator's `this.rng.fork()`, then `range(0.4,1.4)`,
`range(0.5,2.5)`, `range(9,22)`. The stub animator is installed *after*
construction precisely so that fork is not lost. `Agent::new` therefore takes
the already-forked stream and **returns the animator's fork** rather than
discarding it — a caller must keep it or the animator slice will land on a
shifted stream.

Only the collaborators are scripted, and every call into one is appended to a
single ordered log with its arguments and result. The Rust harness replays that
log through one shared cursor, so the test asserts three independent things:

1. **Call order and arguments.** The port asking a collaborator something the
   original did not ask, or in a different order, trips the cursor immediately.
   `raycast_any` serves both `_sense`'s line of sight and `_tryVault`'s ledge
   probe, so the harness dispatches on *what the original did next*, never on a
   guess from the arguments.
2. **The RNG state after every tick**, as four exact `u32`s. This is the real
   determinism pin: one extra, missing or reordered draw diverges on the tick it
   happens.
3. **A ~40-field snapshot per tick.**

### Scenarios

| name | ticks | what it reaches |
|---|---|---|
| `bare` | 360 | no physics / A* / cover / squad — the source's degenerate path: line of sight short-circuits to `true`, `_goTo` copies the destination, `_move` integrates position directly. Fires, retreats, dies, and takes a hit *after* death (a no-op). |
| `wired` | 540 | physics + controller + A* + cover + squad + patrol + three neighbours (one dead), empty magazine, ready grenade, an LOD window. Suppressed, vaults, reloads, throws, dies. |
| `wounded` | 420 | wounded from tick 0: `_combat`'s `health < 34` fallback and RETREAT. |
| `gunfight` | 900 | no cover map, so `_combat` always takes the peek-and-shoot branch — the magazine-emptying run. Flanks and claims the flank. |
| `patrol` | 900 | **no player at all** (`_sense`'s `if (!player) return`): IDLE times out into PATROL, walks the route, hits the A* budget deferral (`n < 0`) and the no-route answer (`n === 0`), is promoted to ALERT by a heard shot, and falls back to PATROL when ALERT's 12 s expires. |
| `empty-patrol` | 300 | an empty patrol list: `patrolPoints?.[patrolIndex % 0]` is `undefined`, PATROL drops straight back to IDLE without advancing the index. |

Across the six, every one of the eight states and every one of the thirty
collaborator call kinds is exercised, and all six locomotion clips appear (a
test asserts that, so trimming a scenario cannot silently lose coverage).

Scenario *inputs* that would otherwise take ten thousand ticks to reach (an
empty magazine, a ready grenade, a wounded agent) are set as recorded `init`
overrides, so the Rust side sets exactly the same fields before ticking.

### Pure tables

Driven through real `Agent` instances, never re-implemented in the script:
`_sideOf` over 6 yaws × 5 points; `applyDamage`'s side over 6 yaws × 8
directions **including `0` and `-0`**; region selection over 3 yaws × 4 parts ×
3 points (with the `part === 'leg'` speed penalty); `die`'s impulse and
hit-point fallback over 4 amounts × 3 directions × 2 points.

`HITBOXES` is captured through the colliders the real constructor builds.
`DOLL` is dead in the source — declared at `agent.js:60-86` and never
referenced — so no runtime path reveals it; rather than hand-copy it, the
capture evaluates the literal straight out of the source text.

### grounding.js

`GroundShadows` needs no stubbing: the real class runs headless. The capture
records the sprite textures' full byte arrays (both powers), `_place`'s
`Matrix4.elements` in f64 (by calling `_place` directly and reading `gs._m`),
and the six `addActor` cases' instance matrices as uploaded.

### Tolerances

* **Exact** — booleans, state and clip names, all integers, the four RNG words,
  and every pure `+ - * /` timer (`stateTime`, `health`, `suppression`, the five
  cooldowns, `stuckTimer`, `desiredSpeed`).
* **`1e-12`** — the port-wide figure, for everything `sin`/`cos`/`sqrt`-derived.
* **`1e-9`** — position, yaw, velocity and the vectors built from them. These
  accumulate through `sin`/`cos`/`atan2` across up to 900 ticks *with feedback*
  (this tick's position sets next tick's steering), so a sub-ULP libm difference
  on tick 1 is amplified. The amplification has a hard floor: the FSM's own
  comparisons (`distanceTo(coverPos) < 0.85`, `d < 0.45`) are discrete, so a
  genuine divergence stops being a rounding difference and becomes a state
  mismatch, which is asserted exactly.
* **Exact f32** for `grounding`'s uploaded matrices. `InstancedMesh.instanceMatrix`
  is a `Float32Array` — storage width is part of the algorithm. The port composes
  in f64, as the source's `Matrix4` does, and the test rounds to `f32` before
  comparing. Comparing the f64 directly would fail by ~1e-8 for no reason, and
  widening the tolerance to absorb that would hide a real error.

## 4a. First run of the golden: three failures, triaged

The golden was written before the crate compiled, so its first execution was
also its first review. 16/19 passed; the three failures split two ways, which is
the point of the exercise.

**1. `scenario_bare` / `scenario_empty_patrol` — the PORT was wrong.**
`agent.js:157-172` builds the hitbox capsules inside `if (phys) { … }`, and
`agent.js:146-154` makes the controller `phys ? phys.createCharacter(…) : null`.
A collider only exists by being registered with `phys.addCollider`, so with no
physics subsystem the source's `this.colliders` stays empty. The port built all
seven unconditionally and set `has_controller: true` regardless. `Agent::new`
now takes `has_physics: bool` — the source's
`const phys = this.ctx.peek('physics')` — which gates both fields from the one
check the source makes. (This also retires the wart flagged in §1: a phys-less
agent claiming to own a controller.)

**2. `ground_shadow_placement_matches_the_original` — the GOLDEN was wrong.**
`JSON.stringify(NaN)` is `null`, so the `non-finite-y` case — which exists
purely to pin `addActor`'s `Number.isFinite(p.y)` guard — round-tripped as a
null and blew up the reader. The capture now writes non-finite numbers as their
JS spellings (`"NaN"`, `"Infinity"`, `"-Infinity"`) and the Rust `f()` decodes
them; finite values are byte-unchanged. This also removed two ad-hoc null hacks
(`lastKnownAge`'s `Infinity`, `groundAt`'s `NaN`), which had been papering over
the same gap in two other places.

**3. `scenario_bare` again, after fix 1 — the CAPTURE HARNESS was wrong.**
With the port corrected, `bare` disagreed on `syncHitboxes` segment count: port
0, golden 7. The port was right. The capture's `sync` probe was *fabricating*
colliders — `agent.colliders = HITBOXES.map(…)` — because the collider objects
my `phys.addCollider` stub returned had no `setSegment`. So the harness forced
seven capsules onto an agent the real constructor had given none, and recorded
segments the original would never produce. Fixed at the root: `addCollider` now
returns a collider with a recording `setSegment`, the constructor's own
colliders are left alone, and the probe just swaps a sink in around the call.
`bare` now records 0 segments, `wired` 7, and `wired` after death 0 (the
`!alive` early return).

Textbook instance of "your comparator can be the bug", and worth the emphasis:
of the three failures, **one** was the port. Had I made the other two pass by
adjusting the Rust, I would have written a real defect into the engine and a
lie into the golden.

**Final: 19/19.**

## 4b. Folded into `crate::jsmath`

Done after the suite was green, so a regression could be attributed. `agent.rs`
had a local `js_sign`; both it and the `|| 1` that follows it are now
`jsmath::sign` / `jsmath::or_one`. Three's `normalize()` is
`divideScalar(length() || 1)` — the same JS idiom — so the five reciprocal
normalisations in `agent.rs` use `jsmath::or_one` too. `grounding.rs`'s
`buildTexture` moved to `jsmath::hypot2` (`Math.hypot(u, v)`,
`grounding.js:38`) and `jsmath::round`. The 16384 texture bytes still compare
exactly, so V8 and Rust agreed here — but the port no longer depends on their
agreeing.

Deliberately **not** converted: `agent.rs`'s `distance()` and the vector lengths
in `_sense`/`_move`. Those are `THREE.Vector3.distanceTo`/`length`, which are
`Math.sqrt(x*x + y*y + z*z)` — *not* `Math.hypot`. Swapping them to `hypot3`
would be the trap run backwards.

## 5. Trap checklist

Each trap in `06-parallel-port-plan.md`, checked by name:

* **`Float32Array` storage width** — `grep Float32Array src/ai/agent.js` →
  nothing. `grounding.js` has one, `InstancedMesh.instanceMatrix`; handled above.
* **`sign` is not `signum`** — hit. `applyDamage`'s
  `Math.sign(...) || 1` (`agent.js:829`). JS `sign` is three-valued and both
  zeros are falsy, so a dead-on hit takes the `|| 1` arm; `f64::signum` returns
  `1.0` for `0.0` and `-1.0` for `-0.0`, which compiles and flips the reaction.
  Hand-rolled `js_sign` plus the falsy test; the golden includes `[0,0,0]` and
  `[-0,0,0]`.
* **Euler order** — no Euler composition in either file. `grounding.js` builds
  its quaternion from two axis-angles and one `multiplyQuaternions`, transcribed
  term by term rather than routed through any `from_euler_*`.
* **Matrix storage order** — `Placement::instance_matrix` returns Three's
  **column-major** `elements` layout. Row-major would flip every off-diagonal
  sign and still compile; the golden is the real `elements` array.
* **Float arithmetic is not associative** — the flank vector
  (`agent.js:552-557`) is transcribed in the source's grouping, including the
  `y` term `0 * r + position.y + perp.y * 4` that folds to `position.y`.
* **An enum used as a table index** — none; `HITBOXES`/`DOLL` are iterated, and
  `BodyPart`/`HitRegion`/`Clip` are compared by value and pinned to the source's
  strings via `as_str()`.
* **`Math.hypot`** — `agent.js` uses none (`distanceTo`/`length` are
  `sqrt(x²+y²+z²)`); `grounding.js`'s `buildTexture` uses `Math.hypot(u, v)`,
  and the port uses `f64::hypot`. The texture bytes are compared exactly and
  pass, so the two agree at this precision.
* **A matching count is not proof** — the grounding cases assert counts *and*
  every matrix element; the scenarios assert the whole snapshot, not just the
  state name.
* **Your comparator can be the bug** — the harness reconstructs the
  line-of-sight ray from the recorded endpoints and checks it is unit, parallel
  and the right length, rather than hard-coding `nav`'s `1e-3` shortfall.
* **Dead computation is still part of the source** — `DOLL`, `DEG`,
  `searchPoint`, `reactionTimer`, `aimActual`, and `_think`'s unused
  `const sq = this.squad` (noted in a comment) are all carried.

## 6. Source quirks pinned, not fixed

* **`this.grenadeCooldown < 0 === false`** (`agent.js:547`). JS parses this as
  `(grenadeCooldown < 0) === false` — "the cooldown is not negative" — because
  relational operators bind tighter than equality. It reads like a leftover in a
  gate that is otherwise about flanking, not grenades, and it is almost always
  true. Ported as `!(self.grenade_cooldown < 0.0)` (identical predicate, no
  `clippy::bool_comparison`) and exercised by `gunfight`, which reaches
  `sq.canFlank` 188 times.
* **`this.targetVisible !== false`** (`agent.js:527`). `targetVisible` is only
  ever a boolean, so this is `&& targetVisible`; the `!== false` spelling would
  matter only if the field could be `undefined`, which `_sense` never leaves it
  as.
* **`patrolIndex` is not advanced when the route is empty** (`agent.js:384-388`)
  — `pts[NaN]` is `undefined`, so the `else` arm runs. Pinned by
  `empty-patrol`.
* **`_combat`'s target expression** `hasTarget ? lastKnown : lastKnownAge < 5 ?
  lastKnown : null` — both live arms are the same vector. Transcribed as written
  rather than collapsed, so a future reader sees the source's shape.

## 7. What is still not ported, and what would unblock it

* **The pose evaluation itself** — `animator.js` (559), `rig.js` (265),
  `clips.js` (354). `AgentAnimator` is the seam; an implementation of that trait
  is the whole job.
* **The ragdoll solver** — `physics/ragdoll.js` (763, PBD).
  `AgentEvent::Death` carries the impulse and hit point the solver needs; the
  15 cm lift/unlift around `createRagdollFromSkeleton` (`agent.js:898-911`) is a
  workaround for that solver's contact behaviour and is documented at the site
  rather than faked.
* **`grounding.js`'s two draw calls** — the `InstancedMesh` pair, the
  `MeshBasicMaterial` alpha-over blend and the `DataTexture` upload.
  `GroundShadows::end` hands the rendering slice exactly what it needs.
* **`ai/index.js`** (`AiSystem`) — still the orchestration tier: the per-frame
  A* budget that makes `PathSource` return `None`, the LOD relevance sweep that
  sets `lod_irrelevant`, and the event wiring that turns an `AgentEvent` into a
  tracer or a grenade. Every hook it needs now exists.

## 8. For the orchestrator

No new module needs wiring — `apps/shmup/src/ai/mod.rs` already declares
`pub mod agent;` and `pub mod grounding;`. Two notes for the integration pass:

* `Agent::new` changed shape: it now takes
  `(id, rng, variant_name, scale, rig_eye_height, position, yaw, has_physics)`
  and returns `(Agent, Rng)`. Two things to get right: the second return value
  is the animator's `rng.fork()` (`agent.js:136`) and must be kept, not
  dropped; and `has_physics` is `ctx.peek('physics') != null`, which gates both
  the hitbox colliders and the character controller.
* `go_to`, `fire_round` and `throw_grenade` take `&mut AgentCtx<'_>` rather than
  the bare trait object. Not style: `AgentCtx<'a>` stores
  `&'a mut (dyn Trait + 'a)`, and reborrowing that into a parameter with an
  elided object lifetime has to shorten the trait object's lifetime behind a
  `&mut`, which is invariant — so the borrow of the ctx got pinned to `'a` and
  every later use conflicted. Reborrowing inside the callee is local and needs
  no coercion.
* `Agent::think` / `Agent::go_to` no longer take `&mut NavGrid` / `&mut CoverMap`
  directly; they take the `PathSource` / `CoverSource` traits, which
  `nav::NavGrid` and `nav::CoverMap` implement (the impls live in `agent.rs`, so
  `nav.rs` is untouched). If a sibling slice wired an older signature, that is
  the seam to re-point.
