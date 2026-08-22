# `ai/parts.js` → `apps/shmup/src/ai/parts.rs`

Ported from Claude-of-Duty `src/ai/parts.js:1-1073` — the soldier's body and
clothing part builders.

| path | what |
|---|---|
| `apps/shmup/src/ai/parts.rs` | the port, 1,650 lines |
| `apps/shmup/tests/ai_parts_port.rs` | the golden test |
| `apps/shmup/tests/ai_parts/capture.mjs` | the Node capture, run against the ORIGINAL JS |
| `apps/shmup/tests/ai_parts/golden.json` | 2.5 MB, byte-reproducible |

**Wiring:** `apps/shmup/src/ai/mod.rs: pub mod parts;` — and `pub mod geo;`,
which parts depends on. Nothing else: no new Cargo dependency, no `lib.rs`
change, no `app.toml` change.

## Two corrections to the plan

1. `06-parallel-port-plan.md`'s hazard table lists `ai/parts.js` as "~21%
   ported, 225 lines, on `main`, no goldens". **There was no
   `apps/shmup/src/ai/parts.rs` at all** — `ai/mod.rs`'s own doc listed
   `parts.js` under "what is deliberately not in this slice". This was a port
   from scratch, not a completion.
2. The same table claims `ai/geo.js` is 31% ported. It was 0% when this slice
   started and 100% by the time it finished — see below.

## The `ai/geo.js` seam, and the rewrite it caused

`parts.js` imports thirteen symbols from `./geo.js` and calls Three.js
directly for the rest, and **none of that existed** when this slice began.
`weapons/geometry/` is no substitute: it is a Three `BufferGeometry` +
`Assembly` + `mergeVertices` kit in `f32` for hard-surface weapon parts, while
`ai/geo.js` is a ring-lofting toolkit over `f64` `{p, n, uv, i}` records with
UVs in metres of surface and no welding. The two share no primitive.

So, per the fan-out brief, the needed subset was first ported **into**
`parts.rs` as a private `geo` submodule. Mid-slice, a sibling agent landed the
real `apps/shmup/src/ai/geo.rs` (the whole file, `CharacterBuilder` included)
and another landed `soldier.rs` calling `crate::ai::parts::*`. **`parts.rs` was
then rewritten against the real `ai::geo`** and the vendored copy deleted.
Shipping a knowing 730-line duplicate would have been exactly the "duplicate
helper definitions" the plan warns the integration pass about, and the seam
was resolvable here for free.

Consequences worth knowing:

* `parts.rs` now uses `geo::{Mesh, Noise, loft, tube, ribbon, box_round,
  ellipsoid, super_ellipse, ellipse_profile, compute_normals, displace, warp,
  transform_mesh, append_mesh, empty_mesh, Ring, LoftOpts, TubeOpts,
  RibbonOpts, BoxRoundOpts, EllipsoidOpts, M4}` and
  `weapons::rig_math::{Q, V3}` (the same import `ai/geo.rs` itself takes).
* **The public signatures match `soldier.rs`'s existing call sites exactly**,
  including options passed by reference (`&JacketOpts`, `&HeadOpts`,
  `&LimbOpts`, `&PouchOpts`) and the `Sunglasses { lens, frame }` /
  `Goggles { frame, strap, down }` field names. `soldier.rs` also calls
  `face_wrap(&nz, head)`, `helmet(&nz, head)` and `plate_carrier(&nz)` with no
  options bag, which is what this port does — see "Divergences" below.
* `geo.rs` renamed its types mid-slice (`AiMesh`/`AiNoise` → `Mesh`/`Noise`,
  `PartOpts` → `PartOptions`, and `compute_normals(m, from)` split into
  `compute_normals`/`compute_normals_from`). `parts.rs` targets the **current**
  names. If it churns again the fix is mechanical.
* **Three small Three.js routines `ai/geo.rs` does not carry** are private to
  `parts.rs`, each with its source citation: `quat_from_euler_yxz`
  (`setFromEuler(…, 'YXZ')` — `rig_math::Q` only has `from_euler_xyz` and
  `to_euler_yxz`), `quat_from_axis_angle` (the boot rings' `+PI/2` about X),
  and `basis_at` (`makeBasis(…).setPosition(…)` for the glove's hand frame,
  written as a literal column-major `M4`). `ai/weapon.rs` carries its own
  identical `quat_from_euler_yxz` for `weapon.js:28`; consolidating the two is
  `ai/geo.rs`'s call to make, not something to decide from a sibling slice.

## What the golden pins

`capture.mjs` imports `src/core/rng.js`, `src/ai/geo.js`, `src/ai/rig.js` and
`src/ai/parts.js` from `C:/dev/Claude-of-Duty` (read-only, untouched) and
emits:

* **`bones`** — the real `RIG.bindPos` entries `soldier.js` feeds the builders
  (`UpperArmR/L`, `ForearmR/L`, `HandR/L`, `UpLegR/L`, `LegR/L`, `FootR/L`,
  `Head`). The Rust test reads them out of the golden, so no coordinate is
  hand-copied across the language boundary.
* **`args`** — the glove grip/palm axes and two sling anchors. `gripAxisR` and
  `palmR` are `soldier.js:661-664`'s literals. The left grip axis is
  `BORE_DIR` there (a `rig.js` computation); this capture substitutes a fixed
  non-unit vector, which also covers `glove`/`knuckleGuard` normalising an
  input that is not already unit length. The sling anchors come from the
  weapon, a different slice, so two fixed points stand in.
* **`hypot`** — 12 `Math.hypot` probes (see below).
* **`noise`** — 9 points × {`n3`, `fbm3` at 2/3/4 octaves}.
* **`parts`** — 60 meshes: every exported builder, at both sides where a
  builder is sided, at every option branch, plus the real `soldier.js`
  argument sets. Full `pos`/`normal`/`uv`/`index` plus vertex and triangle
  counts.

`parts.js` draws no randomness of its own — the only `rng` contact in the
dependency closure is `new Noise(rng)`, exactly 255 `rng.int(0, i)` draws — so
builder call order cannot perturb a value and the capture is
order-independent. `SEED = 20260821`, arbitrary and fixed; re-running the
capture produces a byte-identical file (verified twice).

### Tolerances, and why

Three layers, documented at the top of `ai_parts_port.rs`:

1. **Vertex count, triangle count, index buffer — exact.** Integer-derived.
2. **Every `f64` position/normal/uv, index for index, `< 1e-9`.** The primary
   check, and far stricter than the weapon-geometry suites because it *can*
   be: `ai/geo.js` never welds a vertex away and never merges, so buffer index
   `i` on both sides is the same vertex by construction. None of the weld
   nondeterminism that forced `geometry_assert`'s centroid re-pairing exists
   here. `1e-9` is slack for a residual that should land near `1e-13` (a
   `sin`/`cos`/`pow`/`exp`/`atan2` chain, ~1 ULP each, through a cross product
   whose edge vectors cancel two similar-magnitude coordinates). The assertion
   prints the measured worst deviation and its element index.
3. **The shared `geometry_assert` triangle-soup comparator at `1e-6`.** Reused
   unmodified, as instructed. Its entry point takes `f32`, so the `f64`
   buffers are downcast for it — worth up to ~1.2e-7 absolute at these
   magnitudes, hence `1e-6` and not tighter. Redundant given layer 2, but it
   is the suite-wide instrument and it is ordering-invariant where layer 2 is
   not.

`Noise::n3` is asserted at **exact bit equality** (table lookups plus
`+ - *`); `fbm3` at `1e-12`.

The `hypot` probes are **not asserted by this suite** — `geo.rs`'s `hypot3` is
private to that module (and `ai_geo_port.rs` owns pinning it). They stay in the
golden as the cheapest possible diagnostic if these meshes ever diverge by
~1e-17 in a normal component. They are real V8 output: `sqrt(x²+y²+z²)` fails
the `1e150` probe outright by overflowing to infinity.

## Traps checked by name

* **`Float32Array` storage width.** Grepped: `parts.js` has none, and
  `geo.js`'s only `Float32Array`s are in `CharacterBuilder.build`
  (`geo.js:514-519`) — a different slice. The mesh records are plain JS
  arrays; the ring scratch buffers are explicitly `Float64Array`
  (`geo.js:153,168`). Everything here is `f64`; the only downcast anywhere is
  in the test, feeding the shared comparator.
* **`sign` is not `signum`** — `superEllipse` calls `Math.sign` twice per
  point. Lives in `ai/geo.rs` (`js_sign`, correct there); nothing in
  `parts.js` calls it directly.
* **`Math.hypot` is not `sqrt(x*x+y*y+z*z)`** — likewise `ai/geo.rs`'s, and
  correct there as of this writing (it was the plain root earlier; see the
  wiring queue's `jsmath` note). The 12 golden probes were measured against
  V8, and the max-scaled Kahan transcription was additionally checked
  bit-exact against `Math.hypot` over 200,000 random triples while developing
  this slice.
* **Euler order is a convention.** `place()` uses `'YXZ'` (`parts.js:44`),
  which is neither Three's `'XYZ'` nor `Q::from_euler_xyz` nor
  `axiom_math::Quat::from_euler_xyz`. Transcribed from Three's `'YXZ'` switch
  arm verbatim in `quat_from_euler_yxz`.
* **Matrix storage order.** `basis_at` writes `makeBasis`'s three axes as
  *columns* of `M4::e`, matching Three's column-major `elements` (and
  `M4::compose`'s own layout).
* **Float arithmetic is not associative.** Every expression is transcribed in
  the source's grouping and left-to-right order, including
  `(i / seg) * Math.PI * 2 + rot`, `v.z = bz + z * scale + 0.006 * brow +
  0.004 * chin + 0.008 * occ * -1`, and Three's `divideScalar(s)` =
  `multiplyScalar(1 / s)` (a reciprocal multiply, written as
  `V3::scale(1.0 / d)`, not a division).
* **Dead computation is still the source.** Kept: `shoulderCap`'s
  `v.y *= 1.0` (`parts.js:250`); the `nz` parameters `nose`/`ear` never read;
  the `side` parameters `boot`/`kneePad` never read. `helmet`'s per-ring `t`
  field (`parts.js:494`) that `loft` never reads has nowhere to go on a Rust
  `Ring` and is noted in a comment instead.
* **An enum used as a table index** — none in this file.
* **A matching count is not proof** — layer 2 is a per-element comparison.
* **Your comparator can be the bug** — the whole port was cross-checked
  independently of both comparators; see below.

## Verification actually performed (no `cargo` was run)

The fan-out brief forbids building, so the Rust test cannot have been
executed. Two checks that do not need a compiler were done instead:

1. **`rustc -Zparse-crate-root-only`** on both new `.rs` files — clean. Syntax
   only, not types.
2. **The transcription was re-implemented in JavaScript, from the Rust text,
   and diffed against the golden captured from the original.** Every routine
   that does not depend on `Noise` — `superEllipse` (+ `js_sign`), `loft`
   (+ V8 `hypot`, caps, uv arc lengths), `boxRound`, `ellipsoid`,
   `pathFrames` (+ `setFromRotationMatrix`), `tube`, `ribbon`,
   `computeNormals`, `warp`, `appendMesh`, `bendY`, `place` (Euler `'YXZ'` +
   `compose` + normal matrix, including a non-uniform-scale case) — and then
   `Noise` itself (permutation shuffle, `n3`, `fbm3`) and two of the hardest
   noise-driven builders (`jacketTorso`, and `limbTube` with the full
   arc-length crease pass) reproduce the original at **zero deviation, not
   merely within tolerance**, on `pelvis`, `jacketTorso_bulk`, `sleeveR`,
   `limbTube_capped`, `earR`, `eyeballR`, `sling`, `chinStrap`,
   `carrierWebbing`, `bootLacesR`, `bendY_slab` and `place_scaled_slab`.

   Independently, the vertex and triangle counts the Rust structure implies
   were derived by hand for all 60 parts and matched the golden exactly (e.g.
   `jacketTorso` 13 rings × 27 cols + 1 + 26 cap = 378 verts / 650 tris;
   `gloveR` 119 palm + 4 × 77 fingers + 77 thumb = 504).

   Caveat, stated because the wiring queue already caught one slice where a
   hand transcription shared the port's own bugs: this second implementation
   was written from the Rust, not from the JavaScript, so it is a check on
   *mechanical* faithfulness, not on reading. The mitigation here is that the
   comparison target is the real original's output, not another transcription
   of it — a misreading shared by both sides would still show up as a
   non-zero diff against the golden. It did not.

   What this does **not** cover: Rust type errors, and libm ULP differences
   between V8 and the Rust standard library. Those are what layer 2's `1e-9`
   budget is for. **If integration measures a residual above `1e-9`, check
   whether it is uniform ~`1e-13` noise across many elements (widen and record
   the measurement) or localised to one builder (a real divergence — the
   assertion names the element index).**

## Divergences from the source, and why

* **Option bags that are never read are dropped.** `faceWrap`, `helmet` and
  `plateCarrier` each take a trailing `p = {}` (the variant record) and never
  touch it; an empty Rust options struct would be pure ceremony, and
  `soldier.rs` already calls all three without one. `jacketTorso` keeps
  `JacketOpts { flare, bulk }` and `headMesh` keeps `HeadOpts { wide }`, both
  of which *are* read. (`wide` is never set by any `soldier.js` variant, so
  `1.0` is the only value the game produces — the golden covers `1.08`
  anyway.)
* **`lidTilt` and `bend` are `f64`, not `Option<f64>`.** The source tests them
  for *truthiness* (`o.lidTilt ? … : …`, `if (o.bend)`), under which an absent
  key and a literal `0` are identical. `soldier.js` passes a literal
  `lidTilt: 0` for two of the three mag pouches, so both arms are real call
  sites and both are in the golden.
* **`goggles`' `down` flag is `bool`, defaulting to `false`.** The pushed-up
  variant returns an object with no `down` key at all (`parts.js:618`); the
  capture writes `up.down ?? false` so the absence is pinned rather than
  glossed over.
* **`place`'s nine transform parameters are all passed explicitly** at every
  call site, including the twenty-odd `, 1.0, 1.0, 1.0` scale triples.
  Verbose, but it keeps the port diffable line-for-line, and it is why
  `#[allow(clippy::too_many_arguments)]` sits on `place`.

## No source defects found

Ten defects have been found elsewhere in this port. This file has none I can
identify: every oddity above (the `v.y *= 1.0` no-op, the unread `t` ring
field, the four unread parameters, the unused `V` helper and the unused
`revolve`/`vcount` imports at `parts.js:12-16`) is dead code rather than wrong
code. All of it is carried or explicitly noted, none silently "fixed".

## Coverage of the source

Every exported symbol in `parts.js` is ported and pinned: `bendY`, `mirrorX`,
`place`, `jacketTorso`, `pelvis`, `collar`, `limbTube`, `shoulderCap`,
`headMesh`, `nose`, `ear`, `eyeball`, `faceWrap`, `sunglasses`, `helmet`,
`helmetHardware`, `chinStrap`, `goggles`, `goggleLens`, `headScarf`, `pouch`,
`plateCarrier`, `carrierWebbing`, `sling`, `belt`, `hipPouch`, `kneePad`,
`boot`, `bootSole`, `bootLaces`, `glove`, `knuckleGuard` — plus the private
`radiusAt`, `plate` and `gogglesDown`. `mirrorX` and `place`-with-a-scale are
never exercised by `soldier.js`, so the golden calls them directly rather than
leaving the code unpinned.
