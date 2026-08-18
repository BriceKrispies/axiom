# `weapons::geometry`: the buffer, the merge layer, and `Assembly`

Ported from `C:/dev/Claude-of-Duty/src/weapons/geometry.js`. This note covers
the half of the API contract owned by this pass: `Geo`, `merge_all` (plus the
two Three.js `BufferGeometryUtils` functions it depends on), and `Assembly`.
Primitive builders (`box_geo`, `blob`, `lathe_z`, …) are a separate,
concurrent pass against the same fixed contract
(`docs/work-manifests/shmup-port/03-weapon-geometry-api.md`).

Files:

- `apps/shmup/src/weapons/geometry/geo.rs` — `Geo`
- `apps/shmup/src/weapons/geometry/merge.rs` — `merge_all`,
  `merge_geometries`, `merge_vertices`, `to_non_indexed`
- `apps/shmup/src/weapons/geometry/assembly.rs` — `Assembly`, `Xform`,
  `Node`, `euler_xyz_quat`
- `apps/shmup/src/weapons/geometry/mod.rs` — re-exports
- `apps/shmup/tests/weapons_geometry_port.rs` — the golden-capture
  pins

## What was ported, and how it was pinned

All goldens were captured with small throwaway Node scripts run against the
real `three@0.180.0` installed in `C:/dev/Claude-of-Duty` (deleted after use,
per the recipe). Values are pasted into the Rust test file as literal
constants, not recomputed.

1. **`Geo`** — `pos`/`normal`/`uv`/`index`, plus `vert_count`, `tri_count`
   (`geometry.js:441-444`), `apply` (transforms positions directly and
   normals via the **normal matrix**, `transpose(inverse(upperLeft3x3(m)))`
   — ported from `Matrix3.getNormalMatrix` + `Vector3.applyNormalMatrix` in
   `three/src/math/{Matrix3,Vector3}.js`, not the raw matrix), `flip_winding`
   (`geometry.js:82-100`), and `normalize_attributes`
   (`geometry.js:32-45`, backed by a from-scratch port of
   `BufferGeometry.computeVertexNormals()`'s "no existing normal" branch,
   `three/src/core/BufferGeometry.js:975-1063`).

   The normal-matrix requirement in the API contract is real, not
   decoration: `Geo::apply`'s test
   (`apply_transforms_points_directly_and_normals_via_the_inverse_transpose`)
   uses a non-uniform scale specifically chosen so the raw-matrix and
   normal-matrix answers visibly diverge, and asserts the raw-matrix answer
   is *wrong*.

   `apply`'s normal matrix is computed directly as `cofactor(A) / det(A)`
   (for the upper-left 3x3 `A`) rather than through
   `axiom_math::Mat4::inverse()`, because that returns `None` on a singular
   matrix, where the JavaScript silently produces `Infinity`/`NaN` through
   plain float division. The direct cofactor formula is total, matching that
   behavior, and is derivable in one line from
   `transpose(inverse(A)) == cofactor(A) / det(A)` (adjugate is
   `transpose(cofactor)`, and `inverse = adjugate / det`).

2. **`merge_all`** (`geometry.js:423-438`) plus `mergeGeometries` and
   `mergeVertices`, ported from
   `three/examples/jsm/utils/BufferGeometryUtils.js` (MIT, Three.js
   authors), attributed in `merge.rs`'s module doc. The exact sequence is
   preserved: empty-list -> `None`; single-geometry -> returned **as-is**,
   skipping non-indexing/normalize/weld entirely (pinned by
   `merge_all_of_a_single_geometry_returns_it_unchanged`, using a
   deliberately "dirty" single input to prove nothing touches it);
   otherwise: `toNonIndexed()` per input (ported as `to_non_indexed`, no
   direct JS counterpart since `mergeAll` calls the Three method), then
   `normalize_attributes`, then concatenate (`merge_geometries` — for
   non-interleaved `Float32Array` attributes this is exactly array
   concatenation in argument order), then weld
   (`merge_vertices`, tolerance fixed at `1e-6`), then `normalize_attributes`
   once more.

   `merge_vertices` ports the source's truncating hash
   (`~~(value * hashMultiplier + hashAdditive)`, i.e. `ToInt32`) as
   `.trunc() as i64` — equivalent for any value in realistic geometry range;
   the `~~` operator's mod-2^32 wraparound far outside that range is not
   modeled. Welding is pinned two ways: disjoint triangles concatenate with
   an identity index and no collisions
   (`merge_all_concatenates_disjoint_triangles_with_identity_index`), and a
   unit square split on its diagonal into two *consistently-wound*
   (matching-normal) triangles welds its shared edge from 6 vertices to 4
   with index `[0,1,2,0,2,3]`
   (`merge_all_welds_coincident_position_and_normal_vertices`) — this is the
   case that actually exercises the position+normal+uv joint hash, unlike a
   naive "two triangles sharing an edge with opposite winding" case (tried
   first, and it does **not** weld — differing normals block the weld, per
   the source's own comment at `geometry.js:180-181`).

3. **`Assembly`** (`geometry.js:368-421`) — `BTreeMap`, not `HashMap`, for
   both `buckets` and `nodes` (a Rust `HashMap`'s per-process randomization
   would make the merged output hash non-reproducible across runs of the
   same build). `add` composes translate x rotate x scale exactly as
   `THREE.Matrix4.compose(pos, quat, scale)` does, applies it, and flips
   winding when `sx*sy*sz < 0`; `add_mirrored` relies on exactly that.
   `build()` drains `buckets` via `merge_all` per material, leaving `nodes`
   untouched — matching the source, which only clears `this.buckets`.

## The one real divergence risk this pass found: Euler order

`geometry.js` builds the per-instance rotation via
`new THREE.Euler(rx, ry, rz, 'XYZ')` then `Quaternion.setFromEuler`.
`axiom_math::Quat::from_euler_xyz` looked like the obvious reuse target — it
even documents itself as "X-then-Y-then-Z order" — but a golden capture
proved it composes in the **opposite** order from `THREE`'s actual `'XYZ'`:

- `THREE.Euler(0.3, -0.5, 0.7, 'XYZ')` -> quaternion
  `(0.052132410889547995, -0.2794438940784743, 0.29377717233096856,
  0.9126271389863014)`.
- That value is reproduced exactly by composing `qx * qy * qz` (Hamilton
  product, per-axis quaternions).
- `axiom_math::Quat::from_euler_xyz` composes `qz * qy * qx` for the same
  angles, giving `(0.21989576632910457, -0.1801458579968856,
  0.36323736972823584, 0.8872721876797527)` — a genuinely different
  rotation, not the same one written differently (confirmed by rotating
  `(0,0,1)` through both: `(-0.479, -0.259, 0.838)` vs `(-0.160, -0.521,
  0.838)`).

So `Assembly::add` does **not** call `axiom_math::Quat::from_euler_xyz`. It
builds the rotation with a small private `euler_xyz_quat` in `assembly.rs`
that mirrors `from_euler_xyz`'s own per-axis half-angle construction but
composes `qx.multiply(qy).multiply(qz)` — the order that actually matches
`THREE`. This is pinned end-to-end through the public API in
`add_rotation_matches_three_euler_xyz_order_not_axiom_math_from_euler_xyz`,
which also asserts the wrong order would visibly fail (a rotated point and a
rotated normal both compared against the captured golden, plus a sanity
check that the two orders genuinely disagree by more than a rounding error).

This is worth flagging to whoever next reaches for
`axiom_math::Quat::from_euler_xyz`: it is not wrong on its own terms (it is
an internally consistent X-then-Y-then-Z *intrinsic* convention), but it is
not `THREE.Euler`'s `'XYZ'`, and the two will silently diverge for any
non-trivial rotation.

## `axiom-math` dependency

Added `axiom-math` to `apps/shmup/Cargo.toml` and `"math"` to
`allowed_layers` in `app.toml` (concurrently, the primitives pass added the
same lines — the duplicate `[dependencies]` entry was resolved by keeping
one, since both agents needed the same crate for the same reason: `Mat4`,
`Quat`, `Vec3` for the transform/point/normal math throughout this module).

## What could not be ported / left for later

- Primitive builders (`box_geo`, `blob`, `lathe_z`, `tube_z`, `rod_z`,
  `dome`, `extrude`, `round_rect`, `ring`, `screw`, `knurl_band`,
  `serrations`, `picatinny`, `mlok_slot`) are out of scope for this pass —
  owned by the concurrent primitives agent against the same fixed contract.
- No `docs/work-manifests/shmup-port/03-weapon-geometry-api.md`
  divergence: every signature in the contract's `Geo`/`Assembly` sections
  was implementable as written.
