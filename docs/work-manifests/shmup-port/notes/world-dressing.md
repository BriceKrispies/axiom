# `world/dressing.js` → `apps/shmup/src/world/dressing/`

Ported `C:/dev/Claude-of-Duty/src/world/dressing.js` (2,269 lines — the
largest single unported file) into a **directory module** of 20 submodules
plus `mod.rs`, ~4,100 lines. Split the way the source splits itself: one
submodule per prop family, one per scatter pass, matching the house style of
`world/props/` and `world/kit/`.

## Files written

| path | what |
|---|---|
| `apps/shmup/src/world/dressing/` | 21 files (below) |
| `apps/shmup/tests/world_dressing_port.rs` | the golden test |
| `apps/shmup/tests/world_dressing/capture.mjs` | golden capture, runs the original JS under Node |
| `apps/shmup/tests/world_dressing/golden.json` | 1.30 MB, byte-reproducible |
| `docs/work-manifests/shmup-port/notes/world-dressing.md` | this file |

### Where each source section landed

| `dressing.js` | module |
|---|---|
| `inBuilding`/`isOpen`/`groundY`/`groundSkirt`/`nearestWall`/`camClear`/`jitterRig` | `occupancy.rs` |
| `registerDressingProps` | `prototypes.rs` |
| `dressStreet` | `street.rs` |
| `marketStalls` | `stalls.rs` |
| `barriers` | `barriers.rs` |
| `sandbagEmplacements`/`sandbagWall` | `sandbags.rs` |
| `wrecks` | `wrecks.rs` |
| `palms` | `palms.rs` |
| `streetLamps` | `lamps.rs` |
| `overheadLines`/`facadeHangings` | `lines.rs` |
| `rubblePiles` | `rubble.rs` |
| `tyreStack`/`tyreStacks` | `tyres.rs` |
| `coverClusters` | `cover.rs` |
| `streetFloor` | `street_floor.rs` |
| `dressBuildings`/`dressBuilding`/`alleyLines` | `buildings.rs` |
| `scatterDebris` | `scatter.rs` |
| `gateAperture`/`merlonRun`/`buildGate` | `gate.rs` |
| `buildPerimeter` | `perimeter.rs` |

Plus two `src/world/util.js` primitives with no Axiom home yet, in
`berm.rs` and `cable.rs` — see "Divergences" below.

## What was pinned, and at what tolerance

The golden was captured by running the **original** `dressing.js` under Node
24 through a real `Assembler` (materials/render stubbed to `null` — the
dressing pass touches neither). `capture.mjs` wraps `Assembler.prototype.place`
and `.box` rather than reimplementing them, so the jitter and contact-fillet
logic inside `put()` is the real thing. Re-running the capture produces a
byte-identical file (verified twice).

Four instruments, coarsest to finest:

1. **The generator state after every pass — four exact `u32`s.** The sharpest
   check in the file and the cheapest. The whole pass is one shared stream; a
   single extra, missing or reordered draw anywhere leaves the state different
   and no float slack can hide it. Asserted for all eight passes plus
   `registerDressingProps` and each `driftBerm` case.
2. **Exact integer counts** — instances per prototype, collision boxes per
   surface, vertices and triangles per merged static batch, and the
   first-write order of the palette keys.
3. **Every instance matrix, in call order, per prototype**, at `1e-4`
   absolute per element (12 non-constant elements of the column-major TRS
   matrix), plus the per-instance `[wear, grime, ao]` triple at `1e-6`.
4. **Full triangle-soup geometry** for the eight prototypes and the two
   sub-primitives, at `1e-4` position/normal (`2.0` on `uv`, for the
   already-documented `chamfer_box` 45-degree axis tie and `rock_geometry`'s
   absent `uv` column — same treatment and same values as `props_port.rs`).

Merged static batches get a structural fingerprint (exact vertex/triangle
counts, position bbox at `5e-3`, per-component sum of **absolute** values over
every index-expanded **triangle corner** at `1e-4` relative) rather than a
full buffer dump — a full dump of all of these would be tens of megabytes and
the counts already carry the "same algorithm?" signal. Absolute sums, not
plain sums: summing ~1e5 signed coordinates that straddle zero cancels to near
nothing and turns 1e-6 per-term noise into an unbounded relative error;
magnitudes cannot cancel. Over corners, not over the vertex buffer, so the
measure is weld-invariant — see "the one failure was the comparator" below for
the run that forced that.

`1e-4` on matrices comes from the Rust `Assembler` storing instance matrices
as `f32` while the JavaScript computes in `f64`: ~4e-6 on a translation at
this map's coordinate range, ~1e-6 on a rotation/scale entry. It is an order
above the largest of those and every comparison reports the worst *measured*
deviation, so slack can never be mistaken for a pass.

### Placement order really is pinned

`Assembler::finalize` hands matrices back grouped per prototype and bucketed
by 64 m chunk, so the test re-applies exactly that partition to the golden's
global call-order list (`bucketize`, a 12-line mirror of `assembler.rs`'s
rule) before comparing. This is **not** a no-op path: 17 of `dressStreet`'s
47 prototypes and 12 of `scatterDebris`'s 29 split across all four buckets
(e.g. `litter` at 403 instances → 100/107/80/116). Within each bucket up to
116 matrices must appear in exactly the source's relative order, so a single
misordered `put` fails loudly. Combined with the exact rng state, global order
is pinned too: any reordering that survived the per-prototype check would have
had to consume an identical draw sequence.

## Determinism: the two idioms that would have broken it

Both are in the module doc and commented at every site.

1. **`for (let i = 0; i < rng.int(a, b); i++)` re-evaluates its condition
   every iteration**, so `rng.int` is drawn once per *test* — including the
   final failing one. **Sixteen loops in `dressing.js` are written this way.**
   Reading it as "draw a count once, then loop" changes both the iteration
   count and the number of draws consumed, and shifts every subsequent
   placement in the level. Spelled here as
   `while int_loop_continues(rng, i, a, b) { …; i += 1; }`, with `i += 1`
   placed so a `continue` still bumps it.
2. **Short-circuits skip draws.** `isOpen(...) && rng.float() < 0.96`,
   `i > 0 && rng.float() < lyingP`, `!broken && rng.float() < 0.55`,
   `rs.w > 10 && rng.float() < 0.4`, `tallest && rng.float() < 0.55`,
   `opts.pebbles ?? rng.int(4, 8)`, `lying ? 0 : rng.range(-0.03, 0.03)`,
   `rng.float() < 0.3 ? rng.int(0, 5) : -1`, `header ? rng.range(...) : 0`,
   `pick === prev` → a second `rng.int(0, 1)`.

JavaScript evaluates call arguments left-to-right, so a call like
`A.put(rng.pick(ids), x, y, z, rng.float()*6.28, rng.range(...), [1, rng.range(...), 1], rng.range(...), rng.range(...))`
draws in exactly that order. Every such site hoists each draw into a `let` in
the same order rather than relying on Rust's argument evaluation.

`jitterRig()`'s pinned seed is `0x9e3779b1` — note the final `b1`, **not** the
`b9` of `Rng::DEFAULT_SEED`. Pinned by a unit test.

## Source defects and quirks found, and pinned

1. **`ALLEYS[4]` (the east gravel alley) is authored with an inverted Z span**
   — `z0 = -14.2`, `z1 = -30.2`. Every other rect runs `z0 < z1`. Two
   consequences, both reproduced: `isOpen`'s `z > z0 + m && z < z1 - m` is
   unsatisfiable there, so the whole alley reads as closed ground to every
   dressing pass; and `scatterDebris`'s per-alley count
   `Math.round(area * 0.85)` comes out **negative** (-306), so the loop body
   never runs and the alley gets none of the ~300 junk props its neighbours
   get — while the trailing skip-load roll still runs and still consumes its
   draws. Pinned by
   `source_defect_the_east_gravel_alley_rect_is_inverted_in_z`. Fixing the
   rect would silently re-roll the whole scatter pass downstream; it belongs
   in `layout.rs` with a fresh golden, not here.
2. **A market stall is scaled non-uniformly and only `sx` carries the
   authored width.** `dressing.js:719-723` reads `putS('stall', x, y, z, ry,
   s, rng.range(0.94, 1.05), rng.range(0.95, 1.06), …)` with `s = w / 2.3`, so
   `sy`/`sz` are ~1.0 per-instance variation, not `s`-scaled. Looks like a
   typo; ported as written and pinned by
   `source_quirk_stall_scale_is_non_uniform_with_the_width_only_on_x`, which
   reads its expected values from the golden.
3. **`SET_PIECES.lamps` carries an authored `ry` column that `streetLamps`
   throws away.** It destructures only `[x, z]` and derives the yaw from the
   sign of `x` instead. Every authored yaw is `±PI/2`; the pass only ever uses
   `0` or `PI`. Pinned by
   `source_quirk_street_lamps_ignore_the_authored_yaw` so nobody "fixes" the
   port by honouring the data.
4. **`nearestWall` computes an outward normal (`nx`, `nz`) that no caller
   reads** — its only caller uses `near.d`. Ported anyway (the recipe's "dead
   computation is still part of the source"), documented on the struct.
5. **`alleyLines(A, rng, infos)` never reads `infos`.** Dropped from the Rust
   signature, recorded in the doc comment.
6. **`buildGate`'s `const hutX = -span / 2 - 1.2;` is computed and never
   read.** Recorded as a comment rather than an unused `let`.
7. **`block()` inside `buildGate` returns `{cx, w}` that no caller reads.**
   Dropped from the Rust signature, recorded in the doc comment.
8. **`driftBerm` pushes a placeholder `(0,1,0)` normal per vertex and then
   immediately calls `computeVertexNormals()`, which overwrites all of them.**
   Dead write, ported anyway.
9. **`host.plinthKey ?? 'concrete'`** — no `BUILDINGS` entry in the source
   ever declares `plinthKey`, so it always resolves to `'concrete'`, and
   `crate::world::layout::Building` has no such field. Commented at the site.
10. **`gateAperture`'s `opts.sill !== false` guard has no caller that passes
    `false`.** Carried as a real flag anyway rather than hard-coding the
    always-taken arm.
11. Weighted `rng.pick` lists with a deliberate duplicate — `rock_b` twice in
    `groundSkirt`'s pebbles, `litter` twice in `scatterDebris`'s road pass.
    Commented so nobody de-duplicates them.

## Traps checked, by name

- **`Float32Array`** — grepped `dressing.js`: **zero occurrences**. No storage-
  width hazard in this file.
- **`Math.hypot`** — two sites (`overheadLines`'s laundry span,
  `buildPerimeter`'s wall runs). Both now call **`crate::jsmath::hypot2`**.
  The first draft used `f64::hypot` on the reasoning that it is "the same
  largest-magnitude-first algorithm" — **that was wrong**, and `jsmath`'s own
  doc says so: Rust's is a different, correctly-rounded algorithm that
  disagrees with V8's Kahan-compensated form in the last bits. Both of this
  slice's spans happen to be axis-aligned (`hypot(116, 0)`, `hypot(0, 5.2)`),
  where the two agree exactly, so nothing was actually wrong in the emitted
  geometry — but relying on that accident of the data is not the standard
  here, and the sites now use the exact-by-construction primitive.
  The camera-clearance guard `camClear` turned out to be the
  *other* way round — it is written as a raw squared-distance test
  (`dx*dx + dz*dz < (r+1.5)²`), so substituting `hypot` there would have been
  the bug; transcribed as written, with a comment.
- **`sign` is not `signum`** — three sites (`nearestWall` ×2,
  `streetFloor`'s host lookup). JS `Math.sign(0)` is `0`; `f64::signum(0.0)`
  is `1.0`. Hand-rolled three-valued `js_sign` in `occupancy.rs`, with a unit
  test asserting the difference. **See "For the coordinator" — this wants
  folding into the new `crate::jsmath`.**
- **`Math.round`** (the newly-flagged trap) — ten sites reach this slice
  (`len/pitch`, `len/1.1`, `len/1.7`, `roofProps*2.4`, `area*0.85`, `w/1.15`,
  `w*h*0.05`, `len/4`, `len/0.55`, `w/0.38` and `24/bands`). Audited all ten:
  JS breaks ties toward `+Infinity` and Rust away from zero, so the two agree
  for every non-negative argument, and **every argument here is non-negative
  except one** — `area * 0.85` for the inverted `ALLEYS[4]`, which is exactly
  `-306.0`, an exact integer where `round` is the identity on both sides
  (defect 1 above). No site used the naive `floor(x + 0.5)`, so the
  `0.49999999999999994` failure mode never applied, and none was on `f32`.
  **All ten now call `crate::jsmath::round` anyway** — the audit holding true
  is a property of today's data, and the exact primitive is free. The one
  remaining `.floor()` in the slice is `CatmullRomCurve3::get_point`'s
  `Math.floor(p)`, where `f64::floor` is exact.
- **Euler order** — every rotation goes through the existing
  `world::kit::trs`, which is Three's `'YXZ'` (`qy*qx*qz`) and already golden-
  verified. The two prototypes that build their own matrices (`glass_shards`,
  `stool`) use `makeRotationY`/`makeRotationZ` composed with `setPosition`,
  transcribed as explicit column-major `Mat4::from_cols_array` literals rather
  than routed through `trs` — `makeRotationZ(θ).setPosition(...)` is not
  `trs(..., rz = θ)`.
- **Matrix storage order** — Three's `.elements` and `Mat4::as_cols_array`
  are both column-major; the capture drops indices 3/7/11/15 (the constant
  row) and the test's `matrix12` drops exactly the same ones. Verified against
  a known placement: `tyreStack` at `(-5.2, 0.145, 12.5)` lands at golden
  elements 9/10/11 = `(-5.183, 0.145, 12.520)`.
- **Float arithmetic is not associative** — no expression was tidied. The
  exponential wall falloff (`0.12 + Math.abs(rng.gauss()) * 0.75`),
  `CubicPoly::init_nonuniform_catmull_rom`'s two tangent expressions, and the
  catenary droop `(cosh(1.5) - cosh((t-0.5)*3)) / K` are all transcribed with
  the source's grouping and left-to-right order.
- **A matching count is not proof** — counts are asserted exactly *and*
  every matrix and every triangle is compared.
- **Dead computation is still part of the source** — five sites, listed
  above.
- **The comparator can be the bug** — the placement comparison re-applies
  `finalize`'s own bucketing rather than sorting, so no positional sort key is
  involved. Geometry reuses the shared `tests/geometry_assert/` centroid-keyed
  triangle-soup comparator unmodified.

## Divergences from the source, and why

1. **`driftBerm` and `catenaryTube` live in `dressing/berm.rs` and
   `dressing/cable.rs`, not in `world/kit/primitives.rs`.** They are generic
   `util.js` sub-primitives and structurally belong with the rest of that
   file's port; this slice was forbidden from editing any pre-existing file
   under `src/world/`. `dressing.js` is their only caller today. **Move them
   when a second caller appears** — nothing depends on the location. Flagged
   in both module docs.
2. **`catenaryTube` dragged in three Three.js classes.** There was no Axiom
   counterpart for `CatmullRomCurve3` (centripetal, `tension 0.5`),
   `TubeGeometry`, or the `Curve` base-class machinery they need
   (`getLengths` over 200 arc-length divisions, `getUtoTmapping`'s binary
   search + linear interpolation, `getPointAt`, `getTangent`'s
   `delta = 0.0001` finite difference, `computeFrenetFrames`). All ported
   line-by-line in `cable.rs`, entirely in `f64` (the arc-length
   reparameterisation moves the sample points, not just rounds them, so
   narrowing anywhere upstream would be a real change). `closed = true`'s
   frame post-pass is **not** ported: no caller anywhere builds a closed tube,
   so it would be dead untested code — the same call `rock_geometry` already
   makes for its unused subdivision arm. Verified by golden on three cases
   (with and without jitter, three segment counts).
3. **`wheel_flat` reuses `weapons::geometry::primitives::ring`** rather than a
   second `TorusGeometry` transcription. Three's parameter order is
   `(radius, tube, radialSegments, tubularSegments, arc)` and `ring`'s is
   `(radius, thickness, seg, rings, arc)`, so the two counts swap at the call —
   commented at the site. `ring` takes `arc: f32`, so `TAU` arrives ~1.7e-7
   short of the `f64` value; that is a ~6e-8 position error on a 0.35 m torus,
   two orders under the tolerance, and it is a pre-existing property of `ring`
   that this slice does not own.
4. **`registerDressingProps` returns `Vec<RegisteredProto>`** instead of the
   source's bare `return A;`. Exactly the deliberate testability addition
   `world::props::register_props` already makes and documents — `finalize()`
   only surfaces a prototype that has a *placed* instance, and registration
   places none.
5. **`stripedCloth`'s `bands`/`segX` defaults are recomputed in `f64`**
   (`dressing/mod.rs::striped_cloth_defaults`) rather than calling
   `kit::striped_cloth_default_bands`, which divides in `f32`. Rounding a
   division is exactly where an `f32` narrowing flips an integer, and a
   different band count is a different mesh. Justified at the site.
6. **Everything computes in `f64` and narrows to `f32` only at the Assembler
   boundary**, matching the source's JS numbers. The unavoidable exceptions
   are the parameters the existing kit already types as `f32`
   (`chamfer_box`, `cloth_geometry`, `ll`/`trs`, the `BuildingInfo` anchors).

## Known hazard, stated rather than papered over

`Assembler::put`'s contact-fillet yaw is `(x * 2.7 + z * 1.9) % 6.283`,
computed in `f32` here and `f64` in the source. The pre-modulo value reaches
~270, where the `f32` gap is ~1.6e-5; harmless except for a value landing
within 1.6e-5 of a multiple of `6.283`, where the two sides would disagree by
a full period. Odds ~1e-6 per fillet, ~0.3% across the ~2,000 fillets in a
full level build. If `world_dressing_port` ever fails on a single `dust_skirt`
instance with a huge angular residual, **that** is the cause, and the fix
belongs in `Assembler::put` (compute the fillet yaw in `f64`), not in the
tolerance. Written up in the test's module doc as well.

## `dressBuildings` runs against a synthetic fixture, deliberately

`dressBuilding` reads only `info.spec.roofProps`, `info.roofY`,
`info.roofSpec`, `info.windows`, `info.balconies`, `info.awnings` and
`info.doors`. The golden carries a hand-built two-building anchor set
(`buildingFixture`) that the Rust test reads back and rebuilds, instead of
running the real `buildBuilding`. Coupling this slice's golden to
`world/buildings.js`'s port would mean a divergence over *there* surfaces as a
failure over *here*. The fixture exercises every branch: ground-floor windows
(skipped before any draw), upper windows, balconies, awnings, doors both on
and off open ground, a roof plate wider than 10 m (the laundry-line arm) and
one narrower (the `&&` short-circuit).

## Verification status — green

The crate went green partway through the session (other agents' files
cleared), so this slice **has** been compiled and run:

```
cargo test -p axiom-shmup --test world_dressing_port
  test result: ok. 15 passed; 0 failed
cargo test -p axiom-shmup --lib world::dressing
  test result: ok. 19 passed; 0 failed
```

34 tests, zero failures, zero warnings from anything under
`src/world/dressing/`. Run with
`CARGO_TARGET_DIR=…/shmup-agent-targets/dressing` throughout so as not to
contend with the coordinator's target directory.

The golden was verified byte-reproducible four times, including after the
`jsmath` migration and after the comparator fix below.

### 14 of 15 passed first time; the one failure was the comparator

`build_gate` failed on its first-ever run, and it is worth recording exactly
how, because the recipe's "your comparator can be the bug" trap fired and the
diagnosis mattered:

```
buildGate.statics[brick_fine]: vertex count 858 vs golden 1560
buildGate.statics[brick_fine]: absSum[0] 4691.13 vs golden 8529.34 (rel 0.450001)
buildGate.statics[brick_fine]: absSum[1] 6649.63 vs golden 12087.68 (rel 0.449883)
buildGate.statics[brick_fine]: absSum[2] 34558.19 vs golden 62833.08 (rel 0.450000)
```

Triangle count was **not** in the failure list — 520 both sides. And
`858 / 1560 = 0.5500`, which is exactly the ratio all three sums came out at.
So the geometry was identical and the vertex *buffer* was not: `brick_fine`
is every `spallPatch` in the gate, and `spall_patch` reaches
`weapons::geometry::primitives::extrude` through `kit::poly_prism`, which
welds coincident vertices at `1e-6` where `THREE.ExtrudeGeometry` never does.
That is the same already-documented trade `props_port.rs` records for
`jersey` and `slab_shard`.

The fingerprint was summing magnitudes over the **vertex buffer**, which is
not weld-invariant. Fixed at the instrument, not with a tolerance and not with
an exemption: both the capture and the test now sum over **index-expanded
triangle corners**, which undoes exactly what welding collapsed — the same
move `tests/geometry_assert/` already makes for triangle-soup comparison. The
fields are renamed `cornerAbsSum`/`cornerColAbsSum` so a stale golden cannot
be silently misread against the new rule.

Vertex count is inherently a buffer property, so it keeps a named, documented
exemption (`WELDED_VERTEX_COUNT_EXEMPT = ["brick_fine"]`) on the same
precedent — with triangle count, bounding box and the corner sums all still
compared exactly/strictly for that batch. **The port was never wrong here**;
no dressing source was changed to make this pass.

## Wiring — both lines have already landed

```
apps/shmup/src/world/mod.rs:        pub mod dressing;
apps/shmup/src/world/props/mod.rs:  pub(crate) use vehicles::burnt_car;
```

Both were added by the coordinator while this slice was in flight, and both
are confirmed present. The second is required because
`registerDressingProps`'s first call is `burntCar(rng)`, and `burnt_car` was
`pub(crate)` inside the **private** `props::vehicles` module, so it was
unreachable from `world::dressing`. `props/mod.rs`'s own comment anticipated
exactly this ("no re-export here until a real caller needs one") — this is
that caller. `pub(crate) use` is the right form; a plain `pub use` would be
rejected for re-exporting a `pub(crate)` item. Nothing further is needed to
wire this slice in.

## For the coordinator

- **`js_sign` is still local, as instructed.** `occupancy.rs` carries
  `pub(crate) fn js_sign`; three sites use it (`nearestWall` ×2 and
  `streetFloor`'s host lookup), and `street_floor.rs` imports it from
  `occupancy` rather than keeping a second copy — so there is exactly one
  definition to delete. `jsmath::sign` is a drop-in replacement **except** on
  signed zero: `jsmath::sign(-0.0)` returns `-0.0` (matching `Math.sign`),
  where the local one returns `+0.0`. Every use here immediately compares the
  result or multiplies it into a magnitude, so the two are indistinguishable
  at these call sites — but `jsmath`'s is the more faithful one and the
  consolidation is safe. The local copy has a unit test asserting the
  `signum` difference; that test should move with it.
- **`hypot` and `round` are already migrated to `jsmath`** (2 and 10 sites) —
  see the trap list above. `hypot3/4` and `or_one` have no caller here.
- **Nothing outstanding on this slice.** 34 tests green, golden
  byte-reproducible, no source file outside the four deliverables touched, and
  nothing committed. The only thing I could not do is run
  `cargo xtask check-architecture` — `apps/` is outside the Layer and Module
  Laws and this slice adds no crate, manifest or dependency, so there should
  be nothing for it to say, but it has not been run.
- One thing worth knowing when the whole level build is assembled: this slice
  was tested with each pass on its own fresh `Assembler` and its own fresh
  `Rng(PASS_SEED)`. The real `world/index.js` order is
  `registerProps` → `registerDressingProps` → `buildGround` → per-building
  `buildBuilding` → `buildGate` → `buildPerimeter` → `dressStreet` →
  `dressBuildings` → `scatterDebris`, all sharing **one** stream. The per-pass
  goldens pin each pass's own draw sequence exactly, so composing them in that
  order is the remaining unverified step — worth one end-to-end golden once
  `world/index.js` itself is ported.
