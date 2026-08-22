# `ai/geo.js` -> `apps/shmup/src/ai/geo.rs`

Ported from Claude-of-Duty `src/ai/geo.js:1-754` — **the whole file**, from
scratch.

## Status of what was here before

The parallel-port plan's hazard table (`06-parallel-port-plan.md`) lists
`ai/geo.js` as ~31% ported (231 of 754 lines) and already on `main`. That is
**wrong**: `apps/shmup/src/ai/geo.rs` did not exist, and `apps/shmup/src/ai/mod.rs`
explicitly lists `geo.js` under "what is deliberately not in this slice". There
was nothing to finish and nothing to delete. This is a clean port.

## Files

| file | what it is |
|---|---|
| `apps/shmup/src/ai/geo.rs` | the port |
| `apps/shmup/tests/ai_geo_port.rs` | the golden test |
| `apps/shmup/tests/ai_geo/capture.mjs` | the capture script (runs the original under Node 24) |
| `apps/shmup/tests/ai_geo/golden.json` | 778 KB, byte-reproducible |

Regenerate the golden with `node capture.mjs > golden.json` from
`apps/shmup/tests/ai_geo/`.

## Naming — and a mid-slice course correction

The brief flagged a naming hazard (`weapons::geometry::Geo`, `world::geo`) and
asked me to pick non-colliding names. I first shipped `AiMesh`/`AiNoise`/
`PartOpts`/`BuiltCharacter`.

Then I checked what else had landed in the tree while I was working, and found
that **three sibling slices were already written against a specific
`ai::geo` API**: `ai/weapon.rs` (`use super::geo::{…, Mesh, Noise, Q, Ring, V3, M4}`),
`ai/soldier.rs` (`use crate::ai::geo::{CharacterBuilder, Mesh, Noise, PartOptions}`
plus `CharacterGeometry` and `PartRange`), and `tests/ai_weapon_port.rs`. So I
renamed to match them. The names are the source's own, and none of them actually
collides in Rust — there is no other `Mesh` in the crate, and
`fx::noise::Noise` is a different path. Final surface:

| this module | note |
|---|---|
| `Mesh`, `empty_mesh()` | the `{p,n,uv,i}` record; `f64`, always indexed |
| `Noise` | the 3-D Perlin class; **not** `fx::noise::Noise` (2-D, Worley, different draws) |
| `PartOptions`, `CharacterGeometry`, `PartRange`, `Group`, `IndexBuffer` | the builder's vocabulary |
| `MaterialTiles<'a>` = `&'a [(&'a str, f64)]` | the materials table |
| `M4` | the `f64` `THREE.Matrix4`; consistent with `rig_math`'s `V3`/`Q`, distinct from `axiom_math::Mat4` |
| `pub use weapons::rig_math::{Q, V3}` | re-exported, because `ai/weapon.rs` imports them from here |

The three "geo" types stay deliberately separate, exactly as the three source
files are:

| type | source | shape |
|---|---|---|
| `weapons::geometry::Geo` | `weapons/geometry.js` | `f32` pos/normal/uv/index |
| `world::geo::WorldGeo` | `world/util.js` | the above plus a `color` mask |
| **`ai::geo::Mesh`** | `ai/geo.js` | **`f64`** `p`/`n`/`uv`/`i` |

Two more shapes were matched to the consumers rather than to my first draft:

- **`compute_normals(m)` / `displace(m, f)` / `warp(m, f)`** are the primary
  arity, because that is what every real call site (in the source and in the
  ported siblings) uses — the source's `from` parameter is a default no caller
  overrides. `compute_normals_from` / `displace_from` / `warp_from` keep the
  parameterised form, and the golden exercises it.
- **`CharacterBuilder` takes `&crate::ai::rig::Rig` concretely**, not a trait.
  See below.

## What was reused rather than re-derived

`weapons::rig_math::{V3, Q}` — the `f64` THREE `Vector3`/`Quaternion` kit.
`Q::from_basis` is exactly `Quaternion.setFromRotationMatrix(Matrix4.makeBasis(...))`,
which is what `pathFrames` calls; `V3::apply_quat` is `Vector3.applyQuaternion`,
which is what `loft` calls. Both were **verified against this golden at zero
deviation** before being relied on, so this is reuse of a checked component,
not an assumption.

The shared geometry kit (`weapons::geometry::{Geo, merge, primitives}`) has
**nothing** `ai/geo.js` needs: that kit is a `f32` `THREE.BufferGeometry`
wrapper over Three's own primitive constructors (`RoundedBoxGeometry`,
`LatheGeometry`, `ExtrudeGeometry`, …), while `ai/geo.js` is a hand-rolled
`f64` ring-lofting toolkit that constructs every vertex itself. There is no
overlap to share, and nothing in the shared kit was touched.

`M4` (Matrix4 compose / applyMatrix4 / getNormalMatrix) had to be written here
because `rig_math` has no matrix type. It is transcribed from three@0.180's
`Matrix3.invert()` + `transpose()` **step for step** rather than folded into the
cofactor shortcut `weapons::geometry::geo` and `world::geo` use for the same
job — those are `f32` and can afford an algebraically-equivalent regrouping;
this pipeline is `f64` and float arithmetic is not associative.

## The rig dependency: a trait, then not a trait

`CharacterBuilder` calls exactly two things on the rig: `rig.index(name)` and
`rig.distanceToBone(i, x, y, z)`. I first declared a `CharacterRig` trait naming
those two calls, on the `grounding::FootSource` precedent `ai/mod.rs` documents,
because `src/ai/rig.js` was an unported slice.

It is no longer unported — `apps/shmup/src/ai/rig.rs` landed in the same wave,
with `Rig::index(&self, &str) -> usize`, `Rig::distance_to_bone(&self, usize,
f64, f64, f64) -> f64` and a `pub static RIG: LazyLock<Rig>`. With one skeleton
and one caller (`ai::soldier`), a type parameter with a single instantiation is
ceremony, and `soldier.rs` had already written `CharacterBuilder<'a>` with no
type argument. So the trait is gone and the builder takes `&Rig`.

The golden still carries the real `RIG`'s bone names, bind-pose segments and 28
`distanceToBone` probes, and
`the_ported_rig_reproduces_the_javascripts_bone_indices_and_distances` checks
the ported `RIG` against them **before** any skin weight is compared. That is
now a genuine cross-slice check rather than a stub self-test: a wrong bone
position in `rig.rs` would otherwise surface only as an inscrutable
skin-weight mismatch further down the file.

## The real defect found: `Math.hypot` is not `sqrt(x*x + y*y + z*z)`

The port recipe names this trap. It bit here, and it is now fixed at the root.

Method: after writing the port I re-implemented the Rust line-for-line in
JavaScript and diffed it against the golden (a "shadow port" — it catches
transcription errors that a golden alone cannot, because it isolates *which*
routine diverged). Everything matched at exactly zero **except six normal
components**, which came out at `-6.594e-17` where the original produced
`-6.941e-17`. Those are the z-normals of loft-seam vertices whose true value is
zero, so the absolute error was ~3.5e-17 — invisible under any sane tolerance,
and exactly the kind of thing a port ships by mistake.

Tracing it: replacing the shadow's `sqrt(a²+b²+c²)` with the real `Math.hypot`
took the difference to zero. Measured directly, `sqrt(a²+b²+c²)` disagrees with
`Math.hypot` on **164,284 of 400,204** randomly-drawn triples (41%). The
mechanism at these magnitudes is not overflow — it is that `weldNormals` sums
normals that cancel almost exactly, so a 1-ULP difference in the divisor becomes
a ~5% difference in the residue.

The fix landed in a shared module, not here. While I was tracing this, another
slice created `apps/shmup/src/jsmath.rs` — one V8-transcribed
`Math.hypot`/`Math.sign`/`Math.round`/`|| 1` for the whole crate, pinned against
its own golden in `tests/jsmath_port.rs`. Its module doc records that the crate
previously held **six** independent `hypot3`s using three different algorithms,
and names this module as one that shipped the plain root and cited
`audio::spatial`'s "within a couple of ULP" comment as justification. That is
exactly right, and it is the correct home. `ai::geo` now calls
`crate::jsmath::{hypot3, sign, round}` and defines none of its own.

Independently verified before switching: V8's `MathHypot` (normalize by the
largest magnitude, Kahan-compensate the sum of squares, `sqrt(sum) * max`,
`Infinity` checked before `NaN`) is bit-identical to `Math.hypot` on 400,204
cases — 0 mismatches — while the plain root misses 164,284 of them.
`jsmath::hypot` is that algorithm.

Note the whole thing is deliberately V8-specific. `Math.hypot` is
"implementation-approximated" in ECMA-262, so there is no canonical answer to
match — only the oracle this port is checked against, which is Node/V8.

## Other traps checked by name

- **`Float32Array` storage width.** Grepped, and it is here:
  `CharacterBuilder.build` allocates `Float32Array` for
  position/normal/uv/color/skinWeight, `Uint16Array` for skinIndex, and
  `Uint16Array`-or-`Uint32Array` for the index (`vTotal > 65535`). Crucially the
  source then *reads back out of them* — `_bind`, `_shade` and
  `computeBoundingSphere` all take their `x,y,z,nx,ny,nz` from the already
  narrowed buffers. So the port stores `Vec<f32>` and widens to `f64` for the
  arithmetic, and the index element width is modelled as an `IndexBuffer` enum
  rather than flattened to `u32`. An all-`f64` port would have diverged in the
  vertex colours and the bounds.
  `Float64Array` also appears (loft's `uArr` and per-ring `arr`, `_bind`'s
  `wBuf`) — those stay `f64`.
- **`sign` is not `signum`.** `superEllipse` multiplies `Math.sign(cos t)`
  straight into a radius, so a zero cosine must contribute nothing.
  `jsmath::sign`.
- **`Math.round` is not `f64::round`.** `weldNormals`'s bucket key is
  `Math.round(p * 1e4)`, which rounds a half toward `+Infinity` where Rust
  rounds away from zero (`-2.5` -> `-2` vs `-3`). `jsmath::round`. JS's
  `-0`/`0` collapsing to the same key string falls out of the `as i64` cast.
- **Euler order.** Not applicable — nothing in `ai/geo.js` builds a quaternion
  from Euler angles. (`parts.js`'s `place()` does, with `'YXZ'`; that is the
  next slice's problem, and `M4::compose` here takes a quaternion, not angles.)
- **Matrix storage order.** `M4::e` is THREE's column-major `elements`, and the
  `Matrix3` in `normal_matrix` is likewise column-major. Verified against the
  golden's 16 captured matrix elements.
- **Float arithmetic is not associative.** No expression was regrouped. The
  colour chain in `_shade` in particular is transcribed line for line even where
  a common factor could be hoisted.
- **Enum used as a table index.** Not applicable — no enums here.
- **Matching counts are not proof.** Counts *and* full buffers *and* the index
  buffer are compared; and the character geometry additionally goes through the
  shared triangle-soup comparator, which is what would catch a triangle in the
  right place with the wrong winding.
- **Dead computation in the source.** Two instances, both preserved as comments
  rather than dropped silently: `build()`'s `const rig = this.rig` (never read),
  and `_bind`'s `sum` accumulator (never read). A third — `_shade`'s
  `Math.max(0, Math.abs(nz3))`, a redundant clamp — is kept literally in the
  code.

## Source quirks ported faithfully and pinned by test

1. **`displace`'s falsy guard** (`geo.js:418`): `if (!d) continue` skips on `0`
   **and on `NaN`**. A Rust `if d != 0.0` would drive the vertex to `NaN`.
   Pinned by `displace_skips_a_nan_displacement_the_way_the_source_does`, with a
   golden captured from a callback that returns `0` / `NaN` / a real value in
   rotation.
2. **`computeNormals(m, from)`'s partial recompute** (`geo.js:360-377`): only
   vertices at or above `from` are zeroed, and only they are renormalized — but
   a triangle is skipped only when **all three** corners are below `from`. So a
   straddling triangle accumulates into already-unit, never-renormalized
   vertices. Measured in the golden: 19 of 100 below-`from` vertices are
   touched, worst `|n| = 0.99898`. Pinned by
   `compute_normals_from_leaves_straddled_vertices_unnormalized`, which also
   asserts the golden itself exhibits the quirk so the test cannot pass
   vacuously.
3. **`revolve`'s truthy `squash`** (`geo.js:289`): `opts.squash ? r * opts.squash : r`
   means a squash of exactly `0` falls back to the unsquashed radius instead of
   collapsing the ring. Modelled as `Option<f64>` plus an explicit `!= 0.0`
   check, and pinned by `revolve_treats_a_zero_squash_as_absent`.
4. **`build()`'s trailing unconditional `groups.push`** with `curMat === null`
   when there are no parts at all, giving `materialIndex = -1`. Reproduced via
   `Group::material_index: i32` and `index_of`'s `-1`; not driven by a test
   (an empty builder is not a case any caller reaches), but the shape is there
   rather than papered over with a `usize`.
5. **`if (part.bone)` is a truthy test**, so an empty bone name would take the
   smooth-binding path in JS where `Some("")` takes the rigid path here. No call
   site passes one and `rig.index("")` throws either way, so this changes which
   error you get, not any reachable behaviour. Noted at the site.

## Divergences from the source's shape (and why)

- **`segDist` returns `(distance, closest_point)`** instead of stashing the
  closest point in three module-level globals for the caller to read
  (`geo.js:740`). Same information, same arithmetic, no shared mutable state
  (Axiom's determinism rules).
- **`opts.into` is a separate entry point** (`loft_into`, `tube_into`) rather
  than an option field, so the borrow is explicit. No call site in the source
  actually uses `into`, but it is part of `loft`'s contract, so it is ported
  and pinned by the `loft_into` golden case.
- **Options are per-function structs** (`LoftOpts`, `TubeOpts`, `RevolveOpts`,
  `BoxRoundOpts`, `EllipsoidOpts`, `RibbonOpts`) rather than one bag, with the
  source's `??` defaults in each `Default` impl. `boxRound` forcing
  `capStart/capEnd = false` and `ribbon` forcing them `true` are applied where
  the source's object spread applies them.
- **`add` takes the mesh by value.** The source mutates the caller's object in
  place inside `add` (`computeNormals`, `weldNormals`); every call site hands
  over a freshly built mesh, so ownership is the honest model.

## What the golden covers

- the full 512-entry permutation table (exact), 140 `n3` samples over a grid
  including negative and >256 coordinates, 15 `fbm3`, 2 `fbm3` with non-default
  `lac`/`gain`, 12 `ridge3`
- 6 `superEllipse` rings (n = 2, 3, 4, 5.5, 12, with and without `rot`, plus a
  `seg = 4` case where the sampled cosine lands at ~6e-17 and `Math.sign`
  matters) and 2 `ellipseProfile`
- `pathFrames` for 4 polylines x 2 up-references, including both degenerate
  fallbacks (`lengthSq < 1e-12` and `|dot| > 0.97`)
- 5 `segDist` cases, including a zero-length segment, both clamped ends, and a
  zero distance
- 15 meshes, each dumped **raw** (straight out of the builder, normals still
  zero) and again after `computeNormals` + `weldNormals`: `loft` open / capped /
  appended-into, 3 `boxRound`, 2 `ellipsoid`, 3 `revolve`, 2 `tube`, 2 `ribbon`
- the ops pipeline: `warp` (parts.js's real `bendY`), `displace` (parts.js:124's
  real `fbm3` bump), `transformMesh` through a `Matrix4.compose` with a
  **negative, non-uniform** scale (so the normal matrix is genuinely exercised —
  a port using the raw 3x3 fails here, not merely drifts), `appendMesh`, and
  `computeNormals(m, 100)`
- the real `RIG`'s 25 bone names, 25 bind-pose segments, and 28
  `distanceToBone` probes
- a full `CharacterBuilder.build()`: 439 vertices, 676 triangles, 5 parts across
  3 materials added deliberately out of material order, one material with no
  `tile` entry (the `?? 0.4` fallback), a per-part `tile` and `uvOffset`, a
  `bone` part, `bones`+`bias`+`power` parts (with `bias` deliberately shorter
  than `bones`, exercising `bias[c] ?? 1`), a default-`['Hips']` part, a
  `weld: false` part, and 3 AO occluders — compared on position, normal, uv,
  color, skinIndex, skinWeight, index buffer + element width, groups, part
  ranges, bounding box and the 1.45-inflated bounding sphere

## Tolerances

- **Exact**: every count, every index buffer, the permutation table, the index
  element width, group and part records, bone indices.
- **`1e-12`**: `f64` values — the port's established figure.
- **`1e-6`**: the `f32` artifacts from `CharacterBuilder::build`.
- **`1e-8`, on a host whose libm is worse than V8's at a zero of `cos`/`sin`,
  and only for trig-derived geometry.** See the next section — this is a
  toolchain split, selected by a runtime probe, not a blanket loosening.

On `stable-x86_64-pc-windows-msvc` the whole suite passes at `1e-12` with
**zero** measured deviation on every value. The shadow-port cross-check
(re-implementing the Rust line-for-line in JS and diffing) also found zero
everywhere. The tolerances are headroom, not residual.

## The toolchain split: `cos`/`sin` at a zero

Integration on `stable-x86_64-pc-windows-gnu` produced three numeric failures
at ~2e-12, which is twice the stated tolerance. The coordinator flagged the
storage-width trap (`f32` transforms via `Geo::apply` / `primitives/xform`) as
the likely cause, since that shape had already been found three times this
wave. **It was not that** — `ai/geo.rs` routes no transform through the
`weapons/geometry` kit; its `M4` is `f64` throughout and was already verified
against the golden's 16 captured matrix elements. Nor was it a
reciprocal-multiply-where-the-source-divides (checked: `compute_normals`,
`weld_normals` and `loft` all divide, and `V3::normalize` multiplies by a
reciprocal exactly as THREE's `divideScalar` does).

It is the host libm. At `t = (15/20) * PI * 2` — the `f64` nearest `3*pi/2`,
and **bit-identical** in Rust and Node (`0x4012d97c7f3321d2`):

| | `cos(t)` | relative error |
|---|---|---|
| true value (60-digit) | `-1.836970198721029766e-16` | — |
| V8 / Node 24 | `0xbcaa79394c9e8a0a` | `3.6e-17` |
| `stable-x86_64-pc-windows-msvc` | `0xbcaa79394c9e8a0a` | `3.6e-17` |
| `stable-x86_64-pc-windows-gnu` | `0xbcaa790000000000` | **`3.3e-5`** |

The gnu result has its low **40 mantissa bits zeroed** — a truncated argument
reduction, accurate to about 66 bits absolutely, which is worthless when the
answer itself is `1e-16`. Same for `sin(PI)`. MSVC agrees with V8 bit for bit,
and so does this port.

**Why a `1e-16`-scale error becomes `1e-9`.** `superEllipse` computes
`r * sign(cos t) * |cos t|^(2/n)`. At a sample that lands on an axis the true
coordinate is `0`, and both sides produce `r * |eps|^(2/n)` from their own
residue `eps`. A *fractional power inflates* the cancelled near-zero: for
`n = 12`, `|1.84e-16|^0.1667` is `2.4e-3`, so a coordinate whose true value is
zero comes out at `4.8e-4`, carrying `(2/n) * 3.3e-5` of relative error —
`2.6e-9` absolute. The measured worst across the whole suite is `2.62e-9`, at
exactly that point. Smaller `n` (a bigger exponent) shrinks it: the `boxRound`s
at `n = 5`/`n = 7` land at `1e-12`..`1e-11`, and `n = 2` (`ellipseProfile`,
exponent `1`) is unaffected entirely because `powf(x, 1.0)` is the identity.

The mechanism is confirmed by what *doesn't* diverge: 13 of the 15 meshes are
identical to better than `1e-13` on gnu. `box_round_custom` has a fractional
exponent but `seg = 18`, which never samples a multiple of `pi/2` — bit-exact
on both hosts. Only the profiles that are *both* fractional-exponent *and*
axis-sampling move at all.

**What the test does about it.** `host_trig_matches_v8()` probes `cos`/`sin`
against the two known-good bit patterns and returns `1e-12` or `1e-8`
accordingly, printing a four-line explanation when it degrades. The relaxed
figure is applied *only* to trig-derived geometry — never to the noise tables,
`segDist`, the compose matrix, the skin weights or the bounds, which reach the
golden without a `sin`/`cos` and stay at `1e-12` on every host. A regression on
MSVC still has to clear `1e-12`.

The alternative — transcribing V8's fdlibm `cos`/`sin` into `jsmath` — would be
~250 lines to work around a defect in one toolchain's libm that the toolchain
the repo already mandates for the coverage gate does not have. The port recipe
explicitly sanctions a stated tolerance for `sin`/`cos`; this states it, bounds
it by derivation rather than by fitting, and confines it to the hosts and the
values that need it.

## Comparators

Two, deliberately:

- **`tests/geometry_assert`'s weld-invariant triangle soup** for
  `CharacterBuilder::build`'s `f32` output — the artifact it was built for. It
  asserts triangle count exactly and pairs triangles by centroid, which is what
  catches a triangle in the right place wound the wrong way.
- **Direct, index-aligned `f64` comparison** for every intermediate `AiMesh`.
  This is *stronger*, and it is legitimate here for a structural reason:
  **nothing in `geo.js` ever welds a vertex.** `weldNormals` averages *normals*
  across a position bucket and writes them back in place — it never merges,
  removes or reorders a vertex and never touches the index buffer. Vertex count,
  vertex order and the index buffer are fully determined by the algorithm, so
  the weld ambiguity the soup comparator exists to absorb cannot arise. Using
  the soup comparator on these would have meant first narrowing an `f64`
  pipeline to `f32` to satisfy its signature.

`geometry_assert/mod.rs` was read and reused unchanged, as instructed.

## Not done / handed on

- **Wired and green.** `cargo test -p axiom-shmup --test ai_geo_port`:
  **25 passed, 0 failed** on both `stable-x86_64-pc-windows-gnu` and
  `stable-x86_64-pc-windows-msvc`. On MSVC every comparison runs at `1e-12`
  and every value is bit-exact.
- Two integration failures, both resolved:
  - **Three "golden decode" failures were mine** — `f64s` was walking
    `[[x, z], ...]` profiles and `[[x, y, z, w], ...]` frames as flat numbers.
    Fixed with `flat_f64s`, which flattens one level and rejects a golden that
    is not nested. (No non-finite probes exist in this golden: the one `NaN`
    the suite exercises is produced inside a `displace` callback on both sides
    and never crosses the file, so the `JSON.stringify(NaN) === null` hazard
    does not apply here.)
  - **Three ~2e-12 numeric failures were the toolchain, not the port.** See
    "The toolchain split" above.

### Cross-slice findings for the orchestrator

**Run the port's goldens on `stable-x86_64-pc-windows-msvc`.** This is the
finding worth propagating. The default `stable-x86_64-pc-windows-gnu`
toolchain's `cos`/`sin` are wrong by `3.3e-5` *relative* at a zero of the
function (low 40 mantissa bits zeroed — see "The toolchain split" above); MSVC
is bit-identical to V8. Any slice that evaluates trig at an axis angle and then
feeds the result through a power, a normalize, or a cross product will see
unexplained deviations one to three orders above the `1e-12` figure, and will
be tempted to blame its own arithmetic. Three of this slice's six integration
failures were exactly that. The repo already mandates MSVC for the coverage
gate (`05-port-status.md`), so this costs nothing to adopt:

```sh
RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc CARGO_BUILD_JOBS=2 \n  cargo test -p axiom-shmup
```

A cheap diagnostic for any slice that suspects it:
`((15.0f64/20.0) * PI * 2.0).cos().to_bits()` must be `0xbcaa79394c9e8a0a`.

**Resolved during integration** (recorded because my pre-integration report
raised them, and all three are now fixed — no action left):

1. `ai/parts.rs`'s 730-line vendored copy of this module is gone; it now does
   `use crate::ai::geo::{…}`. Good — a second copy of a primitive is exactly
   how the `Math.hypot` bug got in.
2. `ai/soldier.rs:647` now reads `Noise::new(&mut rng.fork())`.
3. `ai/soldier.rs`'s `CharacterBuilder<'a>` and its `MATERIALS: [(&str, f64); 9]`
   both line up with this module's final shape; `lib.rs` has `pub mod jsmath;`.

**Kept from integration, do not revert:** `TubeOpts { loft: LoftOpts { … } }`
and `RibbonOpts { tube: … }` stay nested. `geo.js`'s `tube` forwards its whole
opts bag to `loft` and `ribbon` forwards its own to `tube`, so the nesting is
the faithful shape — flattening it would quietly drop `closed`/`into` on the
way through.

### For the next slices

- `parts.js`'s `place()` is `Matrix4.compose` from a **`'YXZ'`** Euler.
  `M4::compose` here takes a quaternion, so that slice needs a
  `Q::from_euler_yxz` — `rig_math::Q` currently has only `from_euler_xyz` and
  `to_euler_yxz`, so it does not exist yet. (Euler order is a convention, not a
  spelling: `'YXZ'` composes differently from `'XYZ'`.)
- `weapons/rig_math.rs:112-123`'s `V3::apply_quat` groups Three's
  `applyQuaternion` as `vx + qw*tx + (qy*tz - qz*ty)`; three@0.180 writes
  `vx + qw*tx + qy*tz - qz*ty`, which evaluates left-to-right. The wiring queue
  already carries this (raised by the `ai/weapon.js` agent). **Measured here:
  it makes no difference to any value in this slice** — both groupings
  reproduce this golden at zero deviation across every mesh and the whole
  character. Worth fixing anyway, but it is not urgent on this slice's account.
