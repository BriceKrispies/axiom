# Physics collision world — port notes

## What was ported

`apps/claude-of-duty/src/physics/`:

- `math.rs` — `src/physics/math.js:1-400`, the whole file. `EPS`, `clamp`,
  `Closest`/`HitRecord` (mirroring `makeClosest()`/`makeHitRecord()`),
  `ray_triangle` (Möller–Trumbore), `ray_aabb`, `closest_pt_point_triangle`,
  `closest_pt_seg_seg`, `seg_triangle_closest`, and the three analytic sweeps
  `ray_sphere`/`ray_capsule`/`ray_obb`. All `f64`, matching the source's own
  JS-number precision exactly.
- `surfaces.rs` — `src/physics/surfaces.js:1-143`, the whole file, *except*
  the 12-entry `SURFACE_NAMES`/`SURFACE` taxonomy itself, which is not
  re-declared: `apps/claude-of-duty/src/world/palette.rs` already has a
  `Surface` enum whose declaration order matches `SURFACE_NAMES` exactly, so
  `surfaces.rs` adds `Surface::index()`/`::from_index()`/`::name()` (in
  `palette.rs`) and reuses it. Everything else — `SurfaceProps`/
  `SURFACE_PROPS`, `surface_index`, `guess_surface` (regex alternations
  ported as case-insensitive substring keyword tables — every `GUESS` row in
  the source is a plain `word|word|word` alternation with no other regex
  metacharacters, so this is behaviourally identical without pulling in a
  regex crate), and the `layer`/`mask` bitflag modules — is ported.
- `bvh.rs` — `src/physics/bvh.js:1-933`. `StaticWorld`'s registration
  (`add_triangles`/`remove_object`), `build()` and its binned-SAH node
  construction (`_buildNodes`, `_nodeBoundsFromRange`), and every query:
  `raycast`, `raycast_any`, `query_aabb`, `overlap_capsule`, and the
  conservative-advancement `sweep_capsule`.

## What was NOT ported, and why

`bakeMesh` and `StaticWorld.addMesh` (`bvh.js:104-125, 836-933`) are not
ported. Both flatten a live `THREE.Mesh`/`InstancedMesh` — reading
`geometry.attributes.position`, `geometry.groups`, `updateWorldMatrix`,
per-instance matrices — into the same flat triangle layout `add_triangles`
already accepts directly. This app has no `THREE.Mesh`- or Axiom-mesh
scene-graph arm yet. When one lands, its baker should reproduce `bakeMesh`'s
algorithm (per-group surface resolution via `guess_surface`, degenerate-
triangle drop, instance flattening), writing into the same
`positions`/`count`/`surface` shape `add_triangles` takes. The BVH and every
query are unchanged either way — this is exactly why the recipe describes
this slice as "a pure algorithm over flat typed arrays with no rendering
contact."

`StaticWorld.findByMesh`, `.objectOf` (returns the whole object incl.
`mesh`/`userData`, neither of which exist yet), and the telemetry-only
`buildMs`/`stats.{rayTests,nodeTests,triTests}` fields are also dropped —
none are read by any ported query, and `buildMs` specifically would need
wall-clock time the port's determinism rules avoid wherever the value isn't
load-bearing.

## The precision finding: computes in f64, stores f32

The source's baked world data — `pos`, `nrm`, `nodeBounds`, and the scratch
arrays `_cent`/`_taabb` — are `Float32Array`, even though every arithmetic
step that touches them runs as an ordinary JS double. A `Float32Array` read
widens to a full-precision `f64` carrying only `f32` content; arithmetic
proceeds in double precision; the *write* back into the array re-truncates to
`f32`. `overlapCapsule`'s contact buffer (`nx/ny/nz/px/py/pz/depth/s`) is the
same shape. `HitRecord` (`makeHitRecord()`) is the one exception — a plain
object literal, not a typed array, so query *results* keep full `f64`
precision even though the geometry they were computed from did not.

This matters beyond bookkeeping: `_nodeBoundsFromRange` pads every node AABB
by `1e-5`, a constant with no exact binary representation in either width, so
the `f32`-truncated bound and the `f64` value it was computed from are
genuinely different numbers. An early draft of this port stored everything
in pure `f64` and would have silently diverged from the source's actual
stored geometry the first time a real (non-integer) triangle soup was baked —
caught before committing by noticing the padded node bounds in the golden
capture (`-0.000009999999747378752`, not `-0.00001`) don't match a pure-`f64`
computation. Fixed structurally: `StaticWorld::pos`/`nrm`/`node_bounds`/
`cent`/`taabb`, and every `Contacts` field but `tri`, are now `f32`, widened
to `f64` at every read site and narrowed only where the source's own
`Float32Array` assignment would narrow. This is the same "computes in f64,
stores f32" discipline already established for the weapon geometry port
(`apps/claude-of-duty/src/weapons/geometry`, commit `2fc45570`).

`StaticObject::tris` (this port's `add_triangles` staging, before `build()`
copies it into `pos`) is kept at full `f64` — the source's `addTriangles`
itself accepts whatever typed array a caller passes; the `f32` truncation
that matters is the one `build()` performs when copying into `this.pos`,
which is exactly where this port applies it too.

## Golden capture

Method: a Node script (deleted after use, per the recipe) imported the
*unmodified* `StaticWorld` from `C:/dev/Claude-of-Duty/src/physics/bvh.js`
(via `file://` URL imports — plain relative/absolute-path imports fail under
Node 24's ESM loader on Windows), built a fixed 35-triangle soup via
`addTriangles` (bypassing `bakeMesh` entirely, so no `THREE` mesh dependency
in the capture other than the module's own top-level `import * as THREE`,
which resolves fine since the source repo has `three` installed), and
printed every query result as JSON.

The soup (documented in full in `tests/physics_port.rs`'s module doc
comment): a 4x4 floor grid (32 triangles, diagonal-split per cell), a
vertical wall quad (2 triangles), and one degenerate colinear triangle placed
off to the side (1 triangle) — every vertex coordinate a small integer, so
`f32` truncation is a no-op everywhere in the pipeline except the `±1e-5`
node-bounds padding (see above), which is exactly the fact the goldens now
pin.

Pinned in `apps/claude-of-duty/tests/physics_port.rs` (19 tests, all
passing against the port on the first run after the `f32`-storage fix):

- **Exact equality** (`build()`'s binned-SAH construction is pure
  `+ - * /` and comparisons): triangle/node counts, max depth, world AABB,
  every one of the 19 nodes' bounds and `[leftFirst, count]` meta, the
  degenerate triangle's fallback `(0,1,0)` normal, `raycast`/`raycast_any`/
  `query_aabb` results (no `sqrt` anywhere in Möller–Trumbore's or the slab
  test's decision path for a hit/miss or an axis-aligned normal), and
  `overlap_capsule`'s contact list/normals/points/`s` (all exact for this
  soup's axis-aligned geometry).
- **`1e-12` tolerance** (the established figure, `sqrt` involved via
  `seg_triangle_closest`'s distance and the CA loop's separating-axis
  normalisation): `sweep_capsule`'s time-of-impact `t` in the "approach from
  above" case.

Covers every case the recipe calls out by name: a ray parallel to a triangle
(`raycast_parallel_to_the_floor_plane_misses`), a sweep starting already
overlapping (`sweep_capsule_already_overlapping_the_floor_hits_at_t_zero`,
hits at `t=0` via the CA loop's "already touching" branch), a hit exactly on
a shared edge (`raycast_hits_exactly_on_a_shared_triangle_edge`, straight
down through floor cell (0,0)'s diagonal midpoint), and an empty/degenerate
soup (`build_on_an_empty_world_...` for the true-empty case,
`build_falls_back_the_degenerate_triangles_normal_to_plus_y` for the
zero-area case — `build()` itself never drops degenerate triangles, only the
unported `bakeMesh` does, so it stays in the soup and its fallback-normal
path is exercised directly).

`surfaces.rs` is pinned separately: `surface_index`/`guess_surface` per
source keyword row, every `layer`/`mask` bit against its JS literal, and two
representative `SURFACE_PROPS` rows (concrete, glass) against the source
table's literal values (no capture needed — these are the source's own hand-
authored constants, transcribed and cross-checked by eye against
`surfaces.js:44-69`).

## Return values instead of out-parameters

Both `math.rs` and `bvh.rs` return their result structs by value instead of
writing into a caller-supplied "out" record the way every source function
does. The source's convention exists to dodge V8 GC pressure inside a
per-frame hot loop; `Closest`/`HitRecord` are a handful of `f64`s each,
happily returned on the stack, so there's no equivalent pressure to dodge in
Rust. Documented at the top of both files rather than at every call site.

## Divergences from a literal line-for-line translation, and why

- Traversal stacks (`raycast`, `raycast_any`, `query_aabb`) are growable
  `Vec`s instead of the source's fixed-capacity typed-array stacks sized from
  the tree's max depth. The push/pop *order* is preserved exactly (same LIFO
  discipline, same near/far child ordering) because it's load-bearing for
  which node index a split's children land at — only the fixed-vs-growable
  capacity choice differs, which cannot change any query's result.
- `Closest::t` is effectively dead in the source (only the plane-straddle
  fast path in `seg_triangle_closest` ever writes it; the endpoint-vs-face
  and edge-vs-edge branches leave it holding whatever a *previous, unrelated*
  query wrote via the shared/reused record) and nothing in `bvh.js` ever
  reads it back out. This port returns a fresh `Closest` per call rather than
  mutating a shared one, so there's no "previous query" to inherit a stale
  value from; `t` is simply left at `0.0` (`Default`) except on the one path
  the source explicitly sets it.

## Verification

- `cargo test -p axiom-claude-of-duty --lib --test physics_port --test core_port`:
  pass (53 lib tests, 19 physics-port golden tests, 12 core_port tests).
- `cargo xtask check-architecture`: pass.
- `cargo test -p axiom-claude-of-duty` (the whole crate, every test binary)
  currently fails to *compile* — `tests/weapons_models_port.rs` and
  `src/weapons/models/` are untracked, in-progress work from a concurrent
  agent (per the port's concurrency assignment: `src/weapons/models/` is
  explicitly someone else's territory), unrelated to this slice. Confirmed
  via `git status --short` before commit: both paths are `??`, and this
  slice touches neither.
