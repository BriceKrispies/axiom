# Weapons geometry: the hard-surface primitive kit

Ported `src/weapons/geometry.js:51-357` (every primitive builder between the
`normalizeAttributes` helper and the `Assembly` class) into
`apps/claude-of-duty/src/weapons/geometry/primitives/` per
`docs/work-manifests/claude-of-duty-port/03-weapon-geometry-api.md`. This is
the "primitives" half of the contract; `Geo`, `merge_all`, and `Assembly`
(the other half) landed concurrently from a different agent in
`geometry/geo.rs`, `geometry/merge.rs`, `geometry/assembly.rs`.

## Files

- `primitives/xform.rs` — local `translate`/`rotateX`/`rotateZ`/`scale`
  helpers, all thin wrappers around `Geo::apply(&Mat4)` (built from
  `axiom_math::{Mat4, Quat, Vec3}`) rather than a second hand-rolled
  transform path. `axiom-math` (`crates/axiom-math`, a real engine layer) was
  added to `apps/claude-of-duty/Cargo.toml` for this — the other agent's
  `Geo::apply` already depended on it, so the dependency line was shared
  rather than duplicated; both changes landed as a single addition.
- `primitives/earcut.rs` — a full port of `mapbox/earcut` v3.0.1 (vendored
  into Three.js as `three/src/extras/lib/earcut.js`, MIT licensed) —
  ear-slicing with a z-order spatial index, self-intersection curing,
  polygon splitting, and hole-bridge elimination. Specialized to `dim = 2`
  and point-indexed holes (see the file's module doc); otherwise a 1:1
  translation using an arena `Vec<Node>` addressed by index in place of the
  source's mutable object-reference graph.
- `primitives/rounded_box.rs` — `box_geo`/`blob`, porting `RoundedBoxGeometry`
  (chamfered corners via a per-vertex outward push, with the `getUv` face-UV
  formulas) and the plain `BoxGeometry` fallback/base builder.
- `primitives/lathe.rs` — `lathe_z`/`tube_z`/`rod_z`, porting
  `LatheGeometry`.
- `primitives/sphere.rs` — `dome`, porting `SphereGeometry`.
- `primitives/torus.rs` — `ring`, porting `TorusGeometry`.
- `primitives/octahedron.rs` — the `detail = 0` case of
  `PolyhedronGeometry`/`OctahedronGeometry`, specialized (with the
  subdivision-collapse reasoning documented in the file) since `knurlBand`'s
  cell is the only caller and always uses `detail = 0`.
- `primitives/extrude.rs` — `extrude`/`round_rect`, porting `ExtrudeGeometry`
  (bevel **and** holes) plus `ShapeUtils.triangulateShape`, and a local
  `weld_vertices` (a duplicate of the other agent's private
  `merge::merge_vertices` — same algorithm, same `1e-6` tolerance, kept local
  because `extrude()` welds inline, independent of `mergeAll`, exactly as
  `geometry.js` does).
- `primitives/parts.rs` — `screw`/`knurl_band`/`serrations`/`picatinny`/
  `mlok_slot`, the primitives that compose sub-pieces through `merge_all`,
  plus `Axis` and `PicatinnyOpts`.

## Contract deviation: `round_rect` does not return `Geo`

`03-weapon-geometry-api.md` line 50 declares `round_rect(...) -> Geo`. This
cannot be correct: `mlokSlot` (the function's only real caller,
`geometry.js:352-353`) feeds `roundRect(...)`'s return straight into
`extrude(pts, ...)` as the *point-list* argument, and the JS `roundRect`
itself (`geometry.js:188-205`) builds and returns a flat array of `[x, y]`
points — it never touches `THREE.BufferGeometry`. A `Geo`-returning
`round_rect` could not compile against `extrude`'s signature (`pts: &[[f32;
2]]`) and would contradict the source it is supposed to port. Implemented as
`pub fn round_rect(w: f32, h: f32, r: f32, seg: u32) -> Vec<[f32; 2]>`,
matching both the JS semantics and the contract's own `extrude` signature.

## Source quirks pinned, not fixed

- **`RoundedBoxGeometry`'s zero-segment unit-box quirk.** `box_geo(w, h, d,
  chamfer, 0)` (i.e. `seg = 0`, giving `totalSegments = 1`) returns a plain
  **unit** (`1×1×1`) box, silently discarding the requested `w`/`h`/`d` —
  `RoundedBoxGeometry`'s constructor builds its base `BoxGeometry` at
  `(1, 1, 1)` always, and returns early (before the per-vertex remap that
  would apply the real dimensions) exactly when `totalSegments === 1`. Pinned
  by `box_geo_zero_segments_reproduces_the_rounded_box_geometry_unit_box_quirk`.
- **`LatheGeometry`'s unnormalized last-vertex normal.** Every meridian
  vertex's normal is unit length *except* the last, which reuses the
  second-to-last segment's raw (pre-normalize) edge vector
  (`LatheGeometry.js:103-107` has no `.normalize()` call in that one branch,
  unlike the first-vertex and default-vertex branches). Ported as-is in
  `lathe.rs`'s `lathe_geometry`.

## A real, understood precision boundary (not a bug)

`extrude()`'s bevel construction (`get_bevel_vec`) is **provably bit-exact**
against the JavaScript: fed the exact same corner coordinates in full `f64`
precision (verified directly against `roundRect(0.021, 0.011, 0.0021,
2)`'s twelve corners during debugging — every component matched to the last
bit), it reproduces `getBevelVec`'s output exactly. The contract fixes
`extrude`'s `pts` at `&[[f32; 2]]`, though, and `round_rect` (like every
other point-list producer here) truncates to `f32` — about 7 significant
decimal digits — before `extrude` widens back to `f64`. The JavaScript never
does this: `roundRect`/`Shape.moveTo`/`lineTo` keep plain (`f64`) numbers all
the way through. `get_bevel_vec`'s shift-and-intersect construction divides
by `v_prev_x*v_next_y - v_prev_y*v_next_x` — small for the shallow per-corner
turns a multi-point rounded outline takes — which amplifies that `f32`
rounding noise from ~`1e-7` relative up past `1e-6` absolute in the emitted
bevel vector, specifically for inputs shaped like `round_rect`'s regular,
symmetric-angle corners (and `picatinny`'s mirror-symmetric tooth profile).

That can tip `weld_vertices`'/`mergeVertices`' `1e-6` quantization hash to a
different bucket than the source — the same coordinate, off by roughly the
size of the grid it's snapped to. **What stays exact regardless:**
`Geo::tri_count`, fixed by `earcut`'s triangulation of the un-bevelled
contour (never goes through the amplifying division) — asserted exactly for
every case in `tests/weapons_geometry_primitives_port.rs`, including every
`round_rect`-shaped one. **What is only bounded:** `Geo::vert_count` (a
handful of weld ties can go either way) for `extrude_normal`, `picatinny`,
and `mlok_slot` specifically — via `assert_geo_topology_matches`, documented
in-file. Every other case (20 of 23 tests, including a bevelled
non-`round_rect` extrude, a bevel-disabled extrude, and a holed extrude) hits
full exact position/normal/uv/index fidelity via `assert_geo_matches`.

Also fixed along the way: an earlier draft of `extrude_shape` skipped
`THREE.Path.closePath()`'s "append a closing point equal to the first if not
already closed" step (reasoning it was a no-op, since
`mergeOverlappingPoints` deletes the duplicate straight back out). It **is**
a no-op for the point *set*, but not for point *order*: which of the two
coincident points `mergeOverlappingPoints` deletes determines where the
final contour starts, corrupting every downstream vertex index whenever
`reverse` was `false`. Fixed via `close_ring` in `extrude.rs`; this is what
made `extrude_with_a_hole_exercises_earcuts_bridge_elimination` and
`extrude_with_bevel_disabled_skips_the_contraction_pass` pass at full exact
fidelity.

## Verification

`apps/claude-of-duty/tests/weapons_geometry_primitives_port.rs`, 23 tests,
golden data in `apps/claude-of-duty/tests/geometry/golden.json` (captured
from the real `THREE.RoundedBoxGeometry`/`LatheGeometry`/`SphereGeometry`/
`TorusGeometry`/`ExtrudeGeometry`/`OctahedronGeometry`/`Earcut` under Node
v24; the capture script was not committed, per the port recipe). Covers
every primitive at least once, plus the recipe's three named degenerate
cases (a zero-`phi_length` lathe, a single-segment ring, a zero-chamfer box)
and two more (a zero-segment box — the quirk above — and a zero-`cut` dome),
plus a bevel-disabled extrude and a holed extrude (the only exerciser of
`earcut`'s hole-elimination path in this kit today).

## Not ported

Nothing in the assigned primitive list was skipped. `Shape`/`Path`/
`CurvePath`/`Curve` (Three's general 2-D-path curve machinery — arcs,
Bezier/quadratic curves, splines) were **not** ported, because
`geometry.js`'s `extrude()` only ever builds straight-line shapes
(`moveTo`/`lineTo`), for which that machinery collapses to "the input
points plus a closing duplicate" (documented in `extrude.rs`'s module doc).
If a future port needs `extrude` fed a curved path, that machinery would
need to land first.
