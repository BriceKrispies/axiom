# `physics/debug.js` → `apps/shmup/src/physics/debug.rs`

Source: `C:/dev/Claude-of-Duty/src/physics/debug.js:1-342` (the whole file).

Files written:

| path | what |
|---|---|
| `apps/shmup/src/physics/debug.rs` | the port |
| `apps/shmup/tests/physics_debug_port.rs` | the golden test |
| `apps/shmup/tests/physics_debug/capture.mjs` | the Node capture |
| `apps/shmup/tests/physics_debug/golden.json` | 581,271 bytes, byte-reproducible, all floats as IEEE hex |

Nothing else was touched by this agent. (During integration the orchestrator
added `pub mod debug;` to `physics/mod.rs` and rewired two helpers onto the
new `crate::jsmath` — see the last two sections.)

## The judgement call: where the port stops

`debug.js` is a rendering system — one `THREE.LineSegments` with a
`BufferGeometry`, two `BufferAttribute`s and a `LineBasicMaterial` — wrapped
around about 180 lines of pure vertex arithmetic. The house rule established
by `ai/grounding.rs` is to port the **placement and vertex maths as data** and
stop where Three constructs a `Mesh`/`Material`/`Scene` object. That is
exactly what I did, and I drew the line precisely here:

**Ported — every function, none dropped:**

- `MAX_VERTS`, `COL` (all ten entries), `BOX_EDGES`.
- the constructor's *buffers*: `positions`, `colors`, `_corners`, `rays`,
  `_v`, `rayHead`, `rayCount`, and the six behaviour flags
  (`enabled`, `showTriangles`, `showNodes`, `showRays`, `radius`).
- `setEnabled` (the flag half), `logRay`, `begin`.
- all five drawing primitives: `line`, `triangle`, `box`, `obb`, `capsule`.
- `rebuild` in full — all six passes (triangles, BVH leaf nodes, characters,
  rigid bodies, ragdolls, colliders) plus the ray-ring decay.

**Not ported, and why:** each of these *is* the Three object, not the data
that feeds it. They carry zero arithmetic. Every one of them is enumerated
with its exact values in the module doc comment so the future rendering slice
can reproduce the render state without going back to the JS:

- `BufferGeometry` + the two `DynamicDrawUsage` attributes and their
  `needsUpdate` flags. The port exposes `positions()` / `colors()` /
  `draw_count()` — the two attribute arrays and the `setDrawRange(0, _v)`
  count — which is the entire payload those objects carry.
- `geometry.boundingSphere = Sphere(origin, 1e6)`.
- `LineBasicMaterial { vertexColors, transparent, opacity: 0.85, depthWrite:
  false, toneMapped: false, fog: false }`.
- the `LineSegments` object's `name`/`frustumCulled`/`renderOrder: 9000`/
  `visible`/`matrixAutoUpdate` and its three `userData` flags
  (`owNoPrepass`, `owProbe`, `noCollision`).
- `attach()` (`scene.add`) and `dispose()` (`parent.remove` + two
  `dispose()`s) — scene-graph lifecycle only. `setEnabled`'s two extra
  statements (`object.visible = …`, `attach()`) fall in the same bucket.

I deliberately did **not** manufacture `const`s for the material parameters.
They are prose in the module doc instead: greppable, non-silent, and not dead
code pretending to be an API.

## Traps, checked by name

- **`Float32Array`** — grepped first, as instructed. Four of them, and every
  one is load-bearing:
  - `positions` / `colors` (`MAX_VERTS * 3` each): every vertex and every
    colour component is computed in `f64` and **rounded on store**. The golden
    shows it plainly: `COL.tri`'s `0.32` reads back as `0.3199999928474426`.
    That is why `col::*` is declared as `[f64; 3]` here and narrowed inside
    `line`, not declared as `f32` literals — declaring `0.32_f32` rounds the
    decimal once where the source rounds it twice, and those are not always
    the same value.
  - `_corners` (24 floats): the OBB corner scratch. Each corner is rounded to
    `f32` *before* `line` sees it. An all-`f64` corner buffer would feed
    `line` different numbers. Kept as `[f32; 24]`, a struct field, as in the
    source.
  - `rays` (256 × 7): the TTL column decays with `r[o + 6] -= dt`, which is a
    read-widen-subtract-**round**-store. Frame 2 of the golden is there
    specifically to pin the accumulated rounding.
- **`sign` is not `signum`** — `debug.js` contains no `Math.sign`. The only
  sign-like expression is `const s = e === 0 ? -1 : 1` in the end-cap loop,
  which is a plain ternary and is ported as one.
- **Matrix storage order is column-major** — two sites. `obb` transforms a
  point as `e[0]*x + e[4]*y + e[8]*z + e[12]`, which is only correct for
  Three's column-major `Matrix4.elements`; the port keeps `[f64; 16]` in that
  same order, matching what `physics::math::ray_obb` already takes. And
  `rebuild` calls `Matrix4.compose` for box bodies, which the port has to
  reimplement — transcribed element by element from three r180
  (`three.core.js:12302-12336`) and pinned exactly by four golden cases,
  including an unnormalised quaternion (`compose` does not normalise).
- **Float arithmetic is not associative** — nothing was tidied. Notably
  `x = cxp + px * co + qx * s` is left as three terms in source order, and
  `Math.sin(t) * r * s` is left as `(sin(t) * r) * s`.
- **`Math.hypot` is not `sqrt(x*x + y*y + z*z)`** — see below; this turned out
  to be the most interesting thing in the file.
- **Dead computation is still part of the source** — `this.rayCount` is
  written by `logRay` and never read by anything in `debug.js` (`rebuild`
  walks all 256 slots and tests the TTL instead). Carried as a public field
  with a comment, not dropped, and pinned by the ring test.
- **A matching count is not proof** — the vertex counts agree *and* every
  vertex is compared positionally. The odd-segment case below is a concrete
  example of a divergence the count alone would miss.

## `Math.hypot` — measured, not assumed

`capsule` calls `Math.hypot(x, y, z)` twice, and the first one divides the
axis direction that every ring vertex is built from. I tested three candidate
implementations against Node 24's `Math.hypot` over 2,000,000 random
metre-scale triples:

| candidate | mismatches (1 ULP) |
|---|---|
| `sqrt(x*x + y*y + z*z)` | 721,728 / 2,000,000 (36%) |
| max-scaled, uncompensated: `m * sqrt((x/m)² + (y/m)² + (z/m)²)` | 93,216 / 2,000,000 (4.7%) |
| max-scaled **with Kahan compensation** (V8's algorithm) | **0** |

So `debug.rs` carries `pub fn js_hypot3` implementing the Kahan form, and the
golden has its own `hypot` section asserting bit-exactness on a fixed grid
(including subnormal-ish and `1e150` triples, where the max-scaling is the
whole point) so a hypot regression localises instead of surfacing as "the
capsule rings moved".

**Finding for the orchestrator, outside my slice:** the port already contains
two *other* `hypot3` helpers, and neither is this one.

- `apps/shmup/src/physics/rigidbody.rs:947` is the max-scaled uncompensated
  form (4.7% of inputs off by 1 ULP). Its own doc comment explains it feeds
  the quaternion normalisation and the world inertia tensor every step and
  "compounds from first contact onward" — so that 1 ULP is exactly where it
  matters most.
- `apps/shmup/src/audio/spatial.rs:501` is the plain root (36% off by 1 ULP),
  with a comment saying the difference is "a couple of ULP" and harmless.
  That may well be true for an audio distance attenuation; it is stated as an
  assumption rather than a measurement.

I did not touch either file (not my slice, and `rigidbody.rs` is adjacent to
another live agent). But `js_hypot3` is `pub` precisely so a follow-up can
consolidate on the measured-correct one.

## The two unported seams, and how they are named

`rebuild` reads four actor lists off `phys`. Two of those types are ported and
are used directly — `physics::character::Character` and
`physics::rigidbody::RigidBody` (including its `Shape` enum, whose `Box` arm
takes the source's `else`). Two are not:

- `physics/ragdoll.js` is a concurrent slice → `trait RagdollBones`, naming
  exactly the four reads `rebuild` performs (`boneCount`, `boneHead`,
  `boneTail`, `boneRadius`, and the `px/py/pz` particle triple). Its doc notes
  that `ragdoll.js` stores `px/py/pz` as `Float64Array` but `boneRadius` as a
  `Float32Array`, so an implementer must widen the radius from `f32` storage.
- the `Collider` registry in `physics/index.js:111-166` is a separate slice →
  `trait DebugCollider`, naming the six reads (`enabled`, `shape === 'box'`,
  `matrix`, `hx/hy/hz`, `ax..bz`, `radius`).

This follows the precedent of `fx::world::FxWorld`,
`ai::grounding::FootSource` and `weapons::ballistics::RaycastWorld`: name the
one capability, do not invent a type another slice owns. `DebugScene<'a>`
bundles the four lists plus the static world and the camera position, which is
the whole `rebuild(phys, camera, dt)` argument surface.

## Divergences forced by Rust

1. **No default arguments.** `line`'s `c = COL.tri`, `obb`'s `c = COL.proxy`,
   `capsule`'s `c = COL.proxy, segments = 12`, and `logRay`'s `ttl = 1.5` are
   all explicit parameters. The call sites inside `rebuild` pass the source's
   defaults literally, and the capture script reaches the defaults through
   JavaScript's own default-argument path (which is the only honest way to
   read the module-private `COL` out of the original — `COL` is not exported).
2. **`box` is a reserved word.** The method is `r#box`, a raw identifier, so
   the two files still diff cleanly.
3. **`_corners` is copied out before drawing.** `obb` takes `self.corners` by
   value (it is `Copy`) to sidesteps the simultaneous `&self.corners` /
   `&mut self` borrow `line` would otherwise need. Identical values.
4. **`begin()` does not clear.** Preserved: the buffers keep whatever the
   previous frame wrote past `draw_count()`, exactly as in the source.

## Source quirks pinned by name

1. **`segments / 2` is a float division.** The end-cap loop runs
   `for (let i = 0; i <= segments / 2; i++)` with `t = (i / (segments / 2)) *
   PI`. Every call site passes an even count (12, 10, 8), so an integer
   halving would look correct — and it would *also produce the same vertex
   count* for an odd count, because `i <= 2` and `i <= 2.5` both run three
   samples. Only the cap angles differ. The port keeps `half` as an `f64` and
   the loop as a `while (i as f64) <= half`, and `odd_segment_count_uses_float_half_bounds`
   pins the positions for `segments = 5` against the original.
2. **`Math.hypot(...) || 1` fires on zero *and* NaN.** JavaScript's `||`
   treats both as falsy. A zero-length capsule axis therefore takes `l = 1`,
   the basis cross product comes out zero, and `|| 1` fires a second time —
   leaving every ring vertex sitting exactly on the capsule centre rather than
   producing NaN. A Rust `.max(1.0)` would give the same answer for zero but
   the wrong one for NaN. Ported as an explicit `or_one` and pinned by
   `degenerate_zero_length_axis_collapses_to_the_centre`, which asserts the
   whole buffer is finite and equals the centre.
3. **`rayCount` is dead.** See the trap list above.

## The golden

`node apps/shmup/tests/physics_debug/capture.mjs` → `golden.json`
(581,271 bytes; SHA-256 verified stable across two runs; every float an
IEEE-754 big-endian bit pattern in hex — see "The integration failure").

The oracle is the **real `PhysicsDebugView`**, instantiated for real:
`debug.js` only touches Three for objects that construct fine under Node with
no WebGL context, so nothing in the capture is a re-transcription of the
class. (The one thing the capture *does* recompute independently is
`Math.hypot` and `Matrix4.compose`, and both are compared against Three /
V8 themselves, not against my reading of them.) `three` is imported by
absolute path to `node_modules/three/build/three.module.js` because the script
lives outside the source repo — that is the same file the source's bare
`import … from 'three'` resolves to.

Sections captured:

| section | contents |
|---|---|
| `defaults` | the six constructor flags + `MAX_VERTS` + `logRay`'s default TTL |
| `primitives` | `line` ×3 (incl. `1/3`, `0.1+0.2`, `1e-7`, `-1e7`), `triangle`, `box` ×2 (one degenerate zero-size) |
| `compose` | 4 cases: identity, two real rotations (one with non-unit scale), one unnormalised quaternion |
| `obb` | the same 4 matrices × half-extents, full vertex buffers |
| `capsule` | 7 cases: default-12 upright, along-Z (the `|dz| > 0.9` arm), diagonal seg-10, tiny seg-8, zero-axis degenerate, a near-boundary axis that does *not* trip `> 0.9`, and odd seg-5 |
| `hypot` | 10 fixed triples incl. `1e150` and `1e-160` |
| `overflow` | 60,001 `line` calls against a 120,000-vertex budget |
| `rayInputs` / `rayRing` | 261 logged rays (so the 256-slot ring wraps), plus the two no-op guards (disabled, `showRays` off) |
| `rebuild` | a real `StaticWorld` (floor + wall + a 12-triangle crate = 16 triangles, 5 BVH leaves), 2 characters (one grounded on a tilted normal, one airborne), 3 rigid bodies (box awake / sphere asleep / capsule asleep), a 3-bone ragdoll, 3 colliders (box, capsule, one disabled), 4 logged rays — run for **two consecutive frames**, plus a disabled run, a no-camera run, and a static-passes-off run |

The `rebuild` section also records the exact `queryAabb` candidate list, so a
BVH-side divergence is distinguishable from a debug-view divergence: the test
asserts the candidate list first, then the vertex buffer.

Between them, the `actorsOnly` and full-`rebuild` buffers exercise all ten
`COL` entries (`tri`, `node`, `charGrounded`, `charAir`, `contact`, `body`,
`bodySleep`, `ragdoll`, `proxy`, `ray`) and every arm of `rebuild`.

## The integration failure, and what was actually wrong

First integration run: **14 pass, 3 fail**, all three `rebuild` tests panicking
in the golden reader on `.as_f64()` — "number". The golden held **288 `null`s**.

**The golden was wrong; the port was right.** And it was *not* the `|| 1`
NaN quirk I had pinned, which was the obvious suspect. The cause:

> `THREE.Matrix4.compose` reads the **private** `quaternion._x/_y/_z/_w`
> fields, not `quaternion.x`. `x` is only a getter.

My capture mocked `phys.bodies.bodies[i].quaternion` as a plain
`{x, y, z, w}` object literal. `compose` read `undefined` from every
component and produced a matrix of NaN, so the box rigid body's eight OBB
corners were NaN — 72 position values per buffer, across the four buffers
that draw a box body (`frame1`, `frame2`, `noCamera`, `actorsOnly`) = exactly
288. `JSON.stringify(NaN)` is `null`, so the Rust reader hit a type error
instead of a value mismatch.

`rigidbody.js:42-43` really does hold `new THREE.Vector3()` /
`new THREE.Quaternion()`, so the fix is the faithful one: the mocks now hold
the same THREE types the source does (characters' `position`/`groundNormal`
and the camera's `position` too). Nothing in `debug.rs` changed.

This is the port recipe's "when a golden disagrees with the port, work out
what the value should be from the algorithm before changing either side",
and the answer was in the *original's variable declarations* — exactly where
the plan says the last one was hiding.

### The structural fix, not just the instance

Patching the mock alone would leave the same trap armed for the next
non-finite value. So every float in the golden is now written as its exact
**IEEE-754 big-endian bit pattern in 16 hex digits**, following the
`tests/jsmath/capture.mjs` precedent. JSON cannot represent `NaN`,
`±Infinity` or `-0`; hex carries all four, and it removes the decimal
round-trip (a second transcription step with its own rounding). The capture
also counts non-finite values and prints `nonFinite=0 nullsInFile=0` on every
run, so a future regression is loud at capture time rather than silent until
the reader panics.

Two knock-on improvements: `assert_exact` is back to true `to_bits()`
comparison (the decimal golden had forced a `==` fallback because JSON drops
the sign of `-0`), and the slice no longer depends on `serde_json`'s
`arbitrary_precision` decimal parsing being correctly rounded.

Golden after the fix: 581,271 bytes, byte-reproducible, zero nulls, same
vertex counts as before (794 / 690 / 570 — the counts were never wrong, only
the box body's coordinates).

**Final: 17 passed, 0 failed.**

## Tolerances

- **Exact (bit-for-bit, via `to_bits()`)** for everything built only from
  `+ - * /`, comparisons and stores: `line`/`triangle`/`box`, `compose`,
  `obb`, the budget guard, and the whole recent-query ring *including* the
  `f32` TTL decay across two frames.
- **Exact** for `Math.hypot`. IEEE `/` and `sqrt` are both correctly rounded,
  so the Kahan max-scaled algorithm reproduces bit for bit; if that assertion
  ever fails it is a real divergence, not libm noise.
- **Exact** for every colour buffer, every vertex count, and the BVH candidate
  list — even in the cases whose positions get a tolerance.
- **`1e-6`, scaled as `1e-6 * (1 + |expected|)`**, for any position buffer
  `sin`/`cos` reaches: all seven `capsule` cases and therefore the whole-scene
  `rebuild` buffers. `sin`/`cos` are not bit-guaranteed across libm
  implementations. 1e-6 is a shade above the `f32` ULP at the ~20 m scale
  these buffers hold, which is the width the source stores them at anyway.

  **Measured, so the slack is known rather than assumed:** with the
  comparator instrumented to log every value that is not *also* bit-equal,
  the count over every capsule case and every `rebuild` frame is **0** on
  `x86_64-pc-windows-msvc`. The `f32` store absorbs the whole libm
  difference. The tolerance is therefore buying portability to another libm,
  not covering a real gap here — which is the right trade, but it should not
  be mistaken for the test being loose today.

## Wiring (done by the orchestrator during integration)

```
apps/shmup/src/physics/mod.rs:    pub mod debug;
```

Nothing else: no new Cargo dependency (the test uses the existing
`serde_json` dev-dependency), no `lib.rs` change, no `app.toml` change.

## `jsmath` supersedes two helpers in this file

During integration the orchestrator replaced this module's private
`js_hypot3` and `or_one` with

```rust
pub use crate::jsmath::{hypot3 as js_hypot3, or_one};
```

which is strictly better and leaves every call site here reading as the
source does. The 2M-triple measurement above is what caused `jsmath` to
exist; sweeping the port for the same shape turned up **six** `hypot3`
implementations across three algorithms (two wrong, one wrong *by citation*
of another's comment) and **nine** hand-rolled three-valued `sign`s, plus
four `Math.round` copies of which two carried a real double-rounding bug at
`0.49999999999999994`. `jsmath` is now pinned bit-for-bit against V8 with no
tolerance. The `Math.hypot` note earlier in this file is kept as the record
of how the divergence was found and measured.

## Status

**17 passed, 0 failed** —
`cargo test -p axiom-shmup --test physics_debug_port`.
Golden re-captured and verified byte-reproducible across two runs.
