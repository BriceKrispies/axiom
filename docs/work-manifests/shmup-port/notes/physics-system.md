# `physics/index.js` → `apps/shmup/src/physics/system.rs`

The physics facade: the world registry, the broadphase, the step order, the
query dispatch and the events it emits. Source: `src/physics/index.js:1-1059`,
the whole file.

Files written:

- `apps/shmup/src/physics/system.rs`
- `apps/shmup/tests/physics_system/capture.mjs` → `golden.json` (1.04 MB,
  byte-reproducible; verified by capturing twice and diffing)
- `apps/shmup/tests/physics_system_port.rs` (11 tests)

Wiring the orchestrator must add: `apps/shmup/src/physics/mod.rs: pub mod system;`

## What was ported

Everything the sibling slices make expressible:

`Collider` (+ `setSegment`/`setSphere`/`setFromObject`/`setMatrix`), the whole
query surface (`raycast` and its three dispatch arms, `raycastAny`,
`lineOfSight`, `sphereCast`, `capsuleCast`, `overlapCapsule`, `checkCapsule`,
`overlapSphere`, `groundHeight`), `createCharacter`/`removeCharacter`,
`fireBullet`/`emitImpact`/`explode`, `addRigidBody`/`removeRigidBody`/
`spawnDebris`, `createRagdoll`/`removeRagdoll` (including the `maxRagdolls`
eviction), `addCollider`/`removeCollider`, `fixedUpdate`/`update`/`lateUpdate`,
`_syncStats`, `setDebugDraw`/`toggleDebugDraw`/`debugState`/`_spawnDemo`,
`dispose`, `_addFallbackGround` and the free `segmentHitsAabb`.

Also transcribed here, because nothing in the port had them and both collider
and rigid-body raycasting need them: `THREE.Matrix4.invert`,
`Vector3.applyMatrix4` / `transformDirection` / `lerpVectors`,
`Quaternion.slerp`, `Vector3.applyQuaternion`. All from `three@0.180` source
text, grouping preserved.

## What was not ported, and why

| source | why not |
|---|---|
| `addStatic` / `addStaticGroup` / `removeStatic` / `rebuildStatic` | they flatten a live `THREE.Mesh`, and `bvh.js`'s `addMesh`/`bakeMesh` are themselves not ported. The *other* half of the reason is the ownership seam below. |
| `_ensureStatics`' scene auto-scan | same: it traverses a `THREE.Scene`. The branch it falls through to when the scene holds no meshes — the fallback ground — **is** ported, and is the branch a Node harness with an empty `THREE.Scene()` actually takes, so the golden exercises the real code path. |
| `createRagdollFromSkeleton`, `_handleDeath`, `rd.writeToSkeleton()` | `specFromSkeleton`/`adoptSkeleton` are not in `physics/ragdoll.rs`. `ignore_death_events` is carried so the flag's contract survives. |
| `?physdebug=1` / `?physdemo=1` | `location.search`. `_spawnDemo` itself is ported (it is the tightest statement of the rng draw-order contract in the file). |
| `stats.stepMs` / `stats.buildMs`, the `console.info` in `_syncStats` | wall clock, and console output the Module Law bans. |

## The five seams

1. **The static world is built once and shared immutably.** Every ported
   sibling (`Character`, `RigidBodyWorld`, `Ragdoll`, `Ballistics`,
   `probe::PhysicsWorld`) takes `Rc<StaticWorld>` and holds it for life. The
   source's `staticWorld` is *shared mutable*. So `PhysicsCore::new` takes an
   owned `StaticRegistry`, runs the fallback-ground arm, builds, and only then
   publishes the `Rc`. **Streaming geometry in later needs
   `Rc<RefCell<StaticWorld>>` (or interior mutability inside `StaticWorld`)
   across six files.** That is a `bvh.rs` change and is the single biggest
   structural item this slice surfaced.

   A consequence that bites a golden capture: the source's `init` registers the
   fallback ground but does **not** build the BVH — `fixedUpdate` does, on the
   first step. Between `init` and step 1 every static query silently misses
   (`nodeCount === 0` early-returns in `bvh.js`). The capture closes that window
   with the public `rebuildStatic()`.

2. **`addMesh` absent** — see the table above.

3. **The hit/impact ring pools are dropped.** `HIT_POOL`/`IMPACT_POOL` exist to
   avoid a per-query allocation, and their cost is the hazard documented at the
   top of `index.js`: *"read or copy now, never stash."* Rust returns `Hit` and
   `Impact` by value, removing the hazard rather than reproducing it. The
   constants are kept for the record. This is not free of consequence — see the
   `capsuleCast` quirk below.

4. **`object3D` lives on the facade, not on `RigidBody`.** `rigidbody.rs` is a
   pure solver with no render handle, so the facade keeps a body-id → object map
   and `update()` returns `Vec<InterpolatedPose>` instead of writing into a scene
   graph. The interpolation itself (`lerpVectors` + `slerp` under `time.alpha`)
   is transcribed and pinned.

5. **`stats.objects` needs an accessor `StaticWorld` does not have.** The source
   counts live batches with `for (const o of objects) if (o && o.alive) n++`;
   `bvh.rs` exposes no object accessor. `StaticRegistry` tracks the count
   instead. **`StaticWorld::object_count()` is the right fix** and belongs in
   `bvh.rs`.

## Events — the fork, and what physics needed from it

`EventBus` dispatches on `TypeId`, and three subsystems have already each named
their own payload struct for the same event name. This module adds **no fourth
fork**; it reuses:

| source | reused type |
|---|---|
| consumes `explosion` | `player::system::ExplosionEvent` (the only fork carrying `damage`) |
| emits `bullet:impact` | `audio::system::BulletImpact` (the richest existing fork) |
| emits `damage:dealt` | `ui::system::DamageDealt` (the only fork carrying `amount`) |

**Three fields have nowhere to go**, and this is the concrete cost of the fork:

- `explosion.impulse` — no fork carries it. Reachable only through the direct
  `PhysicsCore::explode(Explosion { impulse, .. })` call.
- `bullet:impact`'s `normal`, `incident` and `surface_index` — `audio`'s fork
  carries only `point`/`surface`/`damage`/`exit`. They **are** ported and
  golden-pinned; they live on the `Impact` record `emit_impact` builds and
  `fire_bullet` returns. They just cannot cross the bus.
- `damage:dealt`'s `target` identity — every fork replaces the JS object
  reference with a boolean the emitter decides.

A single canonical payload set (one `crate::events` vocabulary, consumers
reading the fields they care about) is the fix. It touches `audio`, `player`,
`ui` and `events`, so it is not this slice's to make.

`actor:death` is deliberately **not wired**: `_handleDeath`'s entire body is
`createRagdollFromSkeleton`, which does not exist. A handler that does nothing
is worse than no handler.

## Defects found in files I do not own

1. **`physics/penetration.rs` raycasts the static world, not the facade.**
   `Ballistics.fire` in the source calls `phys.raycast` — colliders, rigid
   bodies and ragdolls included — and calls `phys.emitImpact` from *inside* its
   loop. The Rust port calls `world.raycast` and emits nothing. Consequences:
   a bullet can never hit an actor hitbox, so `damage:dealt` can never fire; and
   `hit.collider.onHit` / `hit.body.applyImpulse` / `hit.ragdoll.applyImpulse`
   (`penetration.js:99-112`) never run. `fire_bullet` here emits the impacts
   after the trace instead of during it; the *sequence* is identical, the
   dynamic-body arm is genuinely missing. **Fix: widen `Ballistics` to take the
   facade.** The golden works around it by firing at static geometry only.
2. **`penetration.rs` uses `sqrt` where the source uses `Math.hypot`.**
   `travelled` (`penetration.js:80`) is a 3-arg `Math.hypot`; the port writes
   `(dx².powi(2)+…).sqrt()`. Feeds `range01` → `rangeMul` → damage. ~1 ULP, but
   it is the exact trap the recipe names.
3. **`rigidbody.rs:apply_radial_impulse` clamps where the source substitutes.**
   Source: `const d = Math.sqrt(d2) || 1e-4` — JS falsiness, so only an *exactly
   zero* (or NaN) distance takes the substitute. Port: `d2.sqrt().max(1e-4)`,
   which clamps every distance below 1e-4. Not triggered by this golden (all
   distances are ~1 m); a body sitting inside a grenade would diverge.
4. **`rigidbody.rs` has a local `hypot3`.** `jsmath`'s own module doc records it
   as the uncompensated max-scaled form, disagreeing with V8 on ~4.7% of inputs.
   It feeds `boundRadius` → CCD substep sizing, and the quaternion is
   renormalised every step. This is the most likely source of the one soft
   tolerance in this golden (below).

None of these were fixed here — they are other slices' files.

## Source quirks ported faithfully, each pinned by a named test

- **`setSphere` silently changes the shape.** `index.js:147` assigns
  `this.shape = 'sphere'`, so an owner positioning a capsule hitbox that way
  gets a sphere. `setMatrix` (`:161-165`) does not change the shape;
  `setFromObject` (`:152-159`) does. Test:
  `set_sphere_silently_changes_a_capsule_into_a_sphere`.
- **A degenerate `capsuleCast` leaks the hit pool.** `index.js:632-633` returns
  `out` before writing `point`/`normal`/`distance`, so the caller gets whatever
  the query 64 casts ago left there. That is the "never stash" hazard firing on
  the *producer* side; the returned point is genuinely unspecified. This port
  returns a fresh record (seam 3), so the golden pins only `.hit` on that path.
  Test: `a_degenerate_capsule_cast_leaks_the_hit_pool`. **This is the one place
  where dropping the pool changes an observable value, and it changes it from
  garbage to a default.**
- **A zero-length `raycast` returns the origin, not `maxDist`**, and leaves
  `fraction` at its `makePublicHit` default of 1 — it returns before the
  `fraction = distance / maxDist` line. Test:
  `a_zero_length_ray_reports_the_origin_and_no_hit`.
- **`lineOfSight` does not count towards `stats.raycasts`** — it calls
  `staticWorld.raycastAny` directly, skipping `raycastAny`'s `_rayCount++`.
  Test: `line_of_sight_does_not_count_towards_stats_raycasts`.
- **`MASK.WORLD` cannot see debris.** `_raycastBodies` early-returns unless
  `mask & LAYER.DEBRIS` (`index.js:518`), so the camera/cover mask is blind to
  rigid bodies directly in front of it. Test:
  `a_debris_layer_mask_gates_rigid_bodies_out_of_a_raycast`.
- **`while (ragdolls.length >= maxRagdolls) shift()?.dispose()` spins forever
  when `maxRagdolls === 0`** — `shift()` is `undefined`, `?.` swallows it, the
  condition never changes. The port adds an `is_empty` guard. Deliberate,
  documented divergence: an infinite loop is not behaviour to port, and
  `maxRagdolls` is 8.

## Traps checked by name

- **`Float32Array`.** Grepped first, as instructed. One site in this file:
  `_addFallbackGround` (`index.js:380`). `S = 300` and `0` are both exact in
  `f32`, so the width does not bite here — the port writes the cast out anyway
  and the golden asserts the round trip, because "it happens not to bite" is a
  fact about these two constants, not about the code.
- **`Math.hypot` vs the plain root.** Five `Math.hypot` calls in the file
  (`:427`, `:606`, `:618`, `:632`, `:758`), all `jsmath::hypot3`. The inverse
  trap was checked too: `Vector3.length()`/`lengthSq()`/`normalize()` in
  `_colliderNormal`, `_raycastBodies` and `_raycastRagdolls` really are the
  plain root and are written as such.
- **`sign` is not `signum`.** `_colliderNormal`'s `Math.sign(v) || 1`
  (`index.js:501-503`) — `Math.sign(0)` is `0` and `Math.sign(-0)` is `-0`, both
  falsy, so the `|| 1` yields `+1`; `f64::signum` would yield `-1.0` for `-0.0`
  and flip the normal on a dead-centre face hit. `jsmath::sign` plus an explicit
  falsiness check. `colliderRays` includes three rays that strike a box face
  exactly through its axis, so this is exercised.
- **`Math.round`.** Not called in this file.
- **Column-major matrix storage.** Every `[f64; 16]` here is THREE's
  `elements`; `mat4_invert` is transcribed index-for-index.
- **Float arithmetic is not associative.** The cofactor expansion in
  `mat4_invert`, the slerp ratios and `explode`'s
  `dy * inv * f + f * 0.4` are all transcribed in the source's grouping.
- **`rng.fork()` and draw order.** One fork (`index.js:244`), shared with
  `Ballistics`. `spawn_debris` draws its three `signed()` *after* the body is
  added; `_spawnDemo` draws nine per chunk in strict left-to-right argument
  order. `spawn_demo_draws_the_rng_in_the_source_order` asserts the full
  `xoshiro128**` state before and after, which is the sharpest possible test of
  draw count and order.
- **`JSON.stringify(NaN)` is `null`.** `lifetime: Infinity` and
  `groundHeight → -Infinity` are written as tagged strings and decoded back.
- **JS state words are signed int32.** `s1 ^= s2` in `rng.js` leaves a *negative*
  number in the state, which JSON records as such. The test reinterprets rather
  than clamping — the first version clamped and read `0`.

## Golden — what is pinned, and the measured tolerances

Six sections; the big one is a 400-step 120 Hz simulation with three rigid
bodies, six rng-spawned debris chunks, a ragdoll and three colliders over a
wall, a ramp, a platform and the fallback ground, with an `explosion` at step 90
and a bullet at step 200 and full query tables at steps 0/37/120/240/400.

Every tolerance was **measured**, by running with the bars lifted and recording
the largest disagreement per field per hundred-step window:

| field | @0-99 | @100-199 | @200-299 | @300-400 | bar |
|---|---|---|---|---|---|
| body position | 8.7e-12 | 8.7e-12 | 3.4e-9 | 7.9e-9 | 1e-7 |
| body quaternion | 6.8e-11 | 6.8e-11 | 1.9e-8 | 2.6e-8 | 1e-6 |
| body linear velocity | 3.7e-10 | 3.7e-10 | 2.0e-8 | 7.5e-8 | 2e-6 |
| body angular velocity | 3.3e-11 | 5.1e-9 | 3.5e-7 | 4.3e-7 | 2e-6 |
| interpolated render pose | 1.4e-12 | 1.4e-12 | 1.4e-12 | 2.5e-9 | 1e-7 / 1e-6 |
| ragdoll particles + AABB | < 1e-12 everywhere | | | | 1e-7 |
| every query (rays, sweeps, contacts, ground heights) | < 1e-12 | | | | 1e-7 / 1e-6 |
| bullet impacts | < 1e-12 everywhere | | | | 1e-7 |
| collider raycasts | < 1e-12 | | | | 1e-12 |
| fallback ground, `segmentHitsAabb`, eviction, stats, rng state | exact | | | | — |

Three things that table says:

1. **The facade is exact.** Every query, every contact, the ragdoll arm and the
   bullet trace agree below 1e-12 for the whole run. The only softness is the
   rigid-body solver's own state.
2. **It plateaus.** Angular velocity climbs from 3e-11 to ~3e-7 across the
   explosion and the bullet and then stops. A systematic algorithmic difference
   grows; bounded fp noise amplified by threshold-based contact resolution does
   exactly this.
3. **The residue belongs to `rigidbody.rs`** — items 3 and 4 in the defect list
   above are the two places to look.

The 1e-7 position bar matches `tests/physics_ragdoll_port.rs`, which measured it
for the same PBD solver over the same step count; nothing here is a looser bar
than the slice it composes. The ragdoll golden's demand that
`bvh::overlap_capsule` be faithful below 1e-7 held with three orders of
magnitude to spare: every contact in this golden agrees below 1e-12.

## Verification

`cargo test -p axiom-shmup` could **not** be run: `apps/shmup/src/weapons/system.rs`
and `apps/shmup/src/fx/system.rs` are sibling agents' in-flight files and do not
currently compile (22 and 7 errors respectively), so the crate's lib does not
build in the shared checkout.

What was verified instead, both against
`CARGO_TARGET_DIR=…/shmup-agent-targets/physsys`:

1. **`cargo check -p axiom-shmup --lib` in the real workspace**, with
   `pub mod system;` temporarily present in `physics/mod.rs` (removed again
   afterwards; `mod.rs` is byte-identical to how I found it). Zero diagnostics
   for `physics/system.rs`. That the file was really being checked was confirmed
   by injecting a deliberate type error and watching it get reported.
2. **The golden test run** against a pruned standalone copy of the crate in the
   scratchpad — the same `src/` and the same `tests/physics_system_port.rs`, with
   the two non-compiling sibling files and the modules only they need removed.
   **11 tests, all passing.**

Both runs must be repeated by the orchestrator once the tree compiles.
