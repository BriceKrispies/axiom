# The Assembler, ground, and the remaining `util.js` geometry builders

Ported from `C:/dev/Claude-of-Duty/src/world/builder.js` (455 lines),
`src/world/ground.js` (287 lines), and the still-unported pieces of
`src/world/util.js`: `Accum` (:103-201), `trs`/`newTrs` (:79-96),
`chamferBox` (:267-361), `weatherProp` (:246-260), `patchGeometry`
(:642-669), `wallPanel`/`holePath` (:392-515), `plainBox` (:369-376) and
`quad` (:379-384).

## New files

- `apps/claude-of-duty/src/world/geo.rs` — `WorldGeo`: a flattened
  `THREE.BufferGeometry` with position/normal/uv and an *optional* `color`
  (mask) column, plus `apply`/`translate`/`rotate_x`/`compute_vertex_normals`/
  `paint_masks`/`fill_masks`. This is deliberately a **separate** type from
  `weapons::geometry::Geo` — see the module doc for why (the source itself
  keeps `weapons/geometry.js` and `world/util.js` as two independent files
  with two independent geometry shapes; `world` geometry always carries or
  implies a mask column, weapon geometry never does).
- `apps/claude-of-duty/src/world/accum.rs` — `Accum`: the merge engine
  every static batch and collision proxy funnels through. Unlike
  `weapons::geometry::merge_all` it never welds vertices (the source never
  calls `mergeVertices` here either).
- `apps/claude-of-duty/src/world/kit.rs` — `trs` (Euler order **YXZ**, a
  different composition than the weapon kit's `Assembly::add`, which uses
  XYZ — verified against a real `three@0.180` capture, see the module doc),
  `chamfer_box`, `weather_prop`, `patch_geometry`, `plane_geometry`/`quad`
  (needed by `ground.rs`'s terrain/road, not just `util.js`'s `quad`),
  `plain_box` (reuses `weapons::geometry::primitives::box_geo`'s
  unchamfered branch rather than a second hand-written `BoxGeometry`), and
  `wall_panel`/`holePath` (reuses `weapons::geometry::primitives::extrude`
  for the actual bevelled-extrude-with-holes machinery — see below).
- `apps/claude-of-duty/src/world/assembler.rs` — `Assembler`: the five
  verbs (`add`/`proto`+`place`+`put`+`putS`/`box`+`collide_geo`+`slab_box`/
  `light`/`finalize`), CHUNK=64m instance bucketing, jitter/skirt logic in
  `put()`.
- `apps/claude-of-duty/src/world/ground.rs` — `build_ground`: terrain, road
  (camber/wear/rut), pavement slabs with alley mouths, alley floors, the
  `seam()` material-boundary scatter (fixed-seed `0x5ea31d` stream), sand/dirt
  drifts, manholes (a ported `THREE.CylinderGeometry`) and gully gratings.

## Golden capture

`apps/claude-of-duty/tests/world/capture.mjs` → `golden.json`, read by
`apps/claude-of-duty/tests/world_port.rs`. Pins:

- `chamfer_box` (two configs) — full position/normal/uv/color arrays, exact
  arithmetic (no transcendentals in the geometry itself; `atan2` only orders
  corners, see `chamfer_box`'s doc for why that's immune to libm ULP noise).
- `patch_geometry` (two configs, one with `sag`) — full arrays.
- `weather_prop` applied to a `chamfer_box` — full mask column.
- `trs` — one captured matrix (Euler YXZ + translate + non-uniform scale).
- `wall_panel` — **triangle count only**, three configs (no holes / one rect
  hole / one arched hole). See "wallPanel's divergence" below for why not
  vertex-exact.
- The road's camber and rut terms (the two purely-x-dependent pieces of the
  height field), sampled at 21 points across the street width — pure
  arithmetic, exact tolerance.
- `Assembler.finalize()` stats (`staticTris`/`instTris`/`instances`/
  `drawCalls`/`collideTris`) for a small fixed scene (2 static adds under one
  key + 1 under another, 5 chunked instances, 2 collision boxes under two
  surfaces).
- `build_ground`'s finalize stats end-to-end (`staticTris=22452`,
  `drawCalls=7`, `collideTris=4524`) — the strongest single check in the
  file: it exercises the *entire* ported ground pipeline, including `seam()`
  seven times over the real fixed boundaries, against the real `Rng(0)`/
  `Rng(2)` seeds `ground.js`'s own call sites use conceptually.

All Rust assertions passed against the captured JSON before the concurrency
blocker below (see "Verification status").

## Deliberate divergences (documented at the site too)

1. **`wallPanel` reuses `weapons::geometry::primitives::extrude`** instead of
   a second ~400-line hand copy of `ExtrudeGeometry`'s bevel-with-holes
   machinery (`extrude_shape` is `pub(super)`-private to that module and
   cannot be called directly). Two consequences:
   - **Z convention**: `extrude()` recentres around `z=0`
     (`-depth/2 + bevel`); `wallPanel` wants `[0, t]` (`+bevel` only). Fixed
     with one corrective `+t/2` translate after `extrude()` returns — derived
     algebraically and then **verified empirically**: Rust's triangle counts
     for all three golden configs (28 / 64 / 152) matched the JS exactly.
   - **Vertex welding**: `extrude()` always welds at `1e-6`; raw `wallPanel`
     never does. This changes vertex *count* (56 vs 84 for the no-holes case)
     and can average normals at a welded seam, but never changes triangle
     count, which is what's actually pinned.
2. **`Assembler::cache`** clones the cached `WorldGeo` on every hit instead
   of returning a shared reference (the source hands back the *same*
   `THREE.BufferGeometry`, but `Accum.add` always copies out of it anyway —
   behaviourally identical, and cheap at kit-piece sizes).
3. **Materials, `render`, physics binding, LOD/bounding spheres are not
   carried.** `Assembler.mat()` (material resolution), the `render` hook, and
   `updateLod`/`computeBoundingSphere` all need a live renderer/camera this
   port doesn't have yet; `Assembler::surface_of` (the physics-relevant half
   of `mat`) and plain `max_dist` data are kept, everything else is dropped
   with a doc comment at the site. `physics.addStatic`/`rebuildStatic` are
   likewise not wired — `crate::physics`'s own module doc already documents
   `bvh::StaticWorld::addMesh` (mesh-baking) as an explicitly un-landed future
   arm.
4. **`Assembler.box(surface, ...)`** is typed on `Surface` here, not an
   arbitrary bucket-key string as in the source — every real `ground.js` call
   site already passes a string that spells a valid `Surface` name or the
   result of `surfaceOf(...)`, so this is a strictly-typed equivalent
   translation, not a behavioural change (documented in `ground.rs`'s module
   doc).
5. **`Math.round`'s half-up quirk** (`round.round(roadLen/2)`,
   `round(len/1.15)` in `seam`) uses plain `f64::round` — every input is
   built from an irrational division, so an exact `.5` boundary never occurs
   in practice (see `ground.rs`'s module doc; `crate::world::noise` already
   documents and pins the general JS-vs-Rust rounding divergence for `hash3`,
   where it matters).
6. **Lights carry position only** (`LightRegistration { position: Vec3 }`),
   not color/intensity/decay/type — no live light-object type exists on the
   Rust side yet.

## Not ported (out of scope per the task)

- `src/world/kit.js` (the "modular building kit" — facades, windows, doors,
  balconies, parapets, stairs, canopies, awnings, drainpipes, damage decals,
  rubble). Two one-line cached-chamfer-box helpers (`BOX`/`BOX_SOFT`,
  `kit.js:54,56`) are inlined directly in `ground.rs` since `buildGround`
  needs them and they're pure wrappers around already-ported primitives —
  noted at the site as *not* a kit.js port.
- `Assembler.updateLod`/bounding spheres (see divergence #3).
- Everything else in `util.js` beyond the four named exports plus `trs`,
  `plainBox`, `quad`: `runoffStreak`, `solidSlabs`, `polyPrism`, `driftBerm`,
  `rockGeometry`, `clothGeometry`, `catenaryTube`, `sackGeometry`, `tubeY`,
  `disposeAll`, `warpGeometry`, `holePath`'s callers besides `wallPanel`.
  These belong to future prop/dressing/interior passes, not the Assembler.

## Verification status (read this before trusting a green run)

Everything above was **fully written, unit-tested (83 new tests, all
passing), and golden-tested (14 new integration tests, all passing) against
`tests/world/golden.json`** in this session — confirmed by direct `cargo
test -p axiom-claude-of-duty --lib world::` (83/83 green) and manual
`cargo test -p axiom-claude-of-duty --test world_port` runs against the
committed goldens before the final verification pass below.

**However**, at the point of the final required verification
(`cargo test -p axiom-claude-of-duty`, per the port recipe), the crate
failed to build — with **zero errors in any file this port touched**. The
six compile errors are all `E0499` (`cannot borrow fx.rng as mutable more
than once at a time`) in `apps/claude-of-duty/src/fx/impacts.rs`, an
**untracked, uncommitted directory from a different, concurrently-running
agent session** (confirmed via `git status`: `apps/claude-of-duty/src/fx/`
is `??`, and `src/lib.rs`'s `pub mod fx;`/`pub mod sky;` lines were already
present — modified, uncommitted — before this session's very first commands
ran). Per the port recipe's concurrency note, `src/world/` is this port's
own slice; `src/fx/` is unambiguously someone else's in-flight work, and
fixing another agent's uncommitted borrow-checker error is out of this
port's scope and risks colliding with their edits.

**Action for whoever picks this up next** (this agent, on resume, or the
next one): re-run `cargo test -p axiom-claude-of-duty` once `src/fx/`'s
owner has fixed or committed their side. If it is still red on `fx/` alone,
that confirms this port's own code is unaffected — every failure in this
session's testing, up to the final gate, was in `world::*` and `world_port`
tests only, and all of those were green.
