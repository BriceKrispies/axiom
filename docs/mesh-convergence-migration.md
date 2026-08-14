# Mesh convergence — migration

`crates/axiom-mesh` now owns the engine's one canonical CPU triangle mesh
([`Mesh`](../crates/axiom-mesh/src/mesh.rs)) and `crates/axiom-mesh-ops` owns the
one deterministic geometry library that produces it. Neither is yet the *only*
mesh in the tree.

This document is the inventory of what has not converged, why each one should,
and what converging it changes. It is a work list, not a design proposal: the
target representation already exists, is validated by construction, is
serializable and digestible, and is documented in
[`crates/axiom-mesh/ARCHITECTURE.md`](../crates/axiom-mesh/ARCHITECTURE.md).

Nothing in this document has been done. Every item below is present in the tree
as described.

## Why converge at all

Seven mutually incompatible CPU mesh representations existed before
`axiom-mesh`. Two of them — `axiom_proc_mesh::MeshBuffer` and
`axiom::MeshData` — are **field-for-field identical and mutually unnameable**,
because the Module Law forbids the crates holding them from depending on one
another. That is not carelessness; it is the predictable output of a structure
with no shared geometry vocabulary. Every pair of crates that needs to pass a
mesh has to invent a third shape or flatten to untyped floats, and each new
shape makes the next one more likely.

The cost is not aesthetic. It is:

- **untyped seams** — `(u64, Vec<f32>, Vec<u32>)` with a stride known only by
  convention, replicated in five places and contradicted in a sixth;
- **duplicated algorithms** — two central-difference normal formulas that
  disagree, two vertex-welding implementations, two 2-bone IK solvers;
- **defects that cannot be shared away** — `axiom-proc-mesh`'s marching cubes
  emits inward-facing triangles; `axiom-mesh-ops`' does not. Nothing propagates
  the fix.

## Ordering

The items are independent of one another, but two of them unblock the rest:

1. **`axiom-proc-mesh` → `axiom-mesh-ops`** removes the largest duplicated
   algorithm set (primitives, transforms, implicit surfaces) and is the item
   that fixes the inward-winding defect for every recipe consumer at once.
2. **`modules/axiom-resources` → `axiom_mesh::Mesh`** is the one that unblocks
   `modules/axiom/src/mesh_geometry.rs`, `modules/axiom-canvas2d-backend`,
   `modules/axiom-gpu-backend` and the `mesh_set` seam, because all of them
   exist to work around the resources contract.

Everything else can follow in any order.

---

## 1. `modules/axiom-resources` — `Vertex`, `MeshData`, `MeshInputVertex`

### `Vertex`

`modules/axiom-resources/src/vertex.rs:8`

```rust
pub struct Vertex { position: Vec3, normal: Vec3, uv: Vec2, color: Vec4 }
```

An **interleaved** array-of-structs vertex with private fields and accessors.
Interleaving is a GPU vertex-layout decision that belongs to a backend; freezing
one into a CPU representation is exactly what makes it un-shareable. It also
hard-codes four attributes: a mesh here can never carry tangents or a skin
binding.

**Migrate to** `axiom_mesh::MeshStreams` — structure of arrays, eight streams,
absence expressed as an empty stream.

### `MeshData`

`modules/axiom-resources/src/mesh_data.rs:16`

```rust
pub struct MeshData { id: ResourceId, name: &'static str, vertices: Vec<Vertex>, indices: Vec<u32> }
```

**The `name: &'static str` is a structural defect, not a style choice.** A
mesh's name can only ever be a compile-time literal — `MeshData::new` and
`name()` both hard-code the lifetime (lines 26 and 42). A runtime-generated name
(`format!("chunk_{x}_{z}")`) is impossible without leaking memory, which means
**a runtime-loaded asset cannot be named at all**. The umbrella works around it
by passing the constant `"axiom.author.mesh"` for every author mesh
(`modules/axiom/src/mesh_geometry.rs:319`), so every author mesh in the engine is
indistinguishable by name.

**Migrate to**: an `axiom_mesh::Mesh` for the geometry, with identity (`id`,
`name`) staying in the resource layer as a `String` alongside it. Geometry and
identity are different things; the current type conflates them and pays for the
conflation with a lifetime.

### `MeshInputVertex`

A `pub(crate)` type alias appearing in a **public** signature:

```rust
// modules/axiom-resources/src/mesh_data.rs:11
pub(crate) type MeshInputVertex = ([f32; 3], [f32; 3], [f32; 2], [f32; 4]);

// modules/axiom-resources/src/resources_api.rs:51
pub fn register_mesh(&self, table: &mut ResourceTable, name: &'static str,
                     vertices: &[MeshInputVertex], indices: &[u32]) -> ResourceId
```

This compiles only because a type alias is transparent. External callers cannot
*name* the type and must spell the raw four-tuple by hand — which is exactly
what `modules/axiom/src/mesh_geometry.rs:303` does:

```rust
Vec<([f32; 3], [f32; 3], [f32; 2], [f32; 4])>
```

The alias also appears at `cube_mesh.rs:17,42`, `cylinder_mesh.rs:27,46,52,53,55`,
`plane_mesh.rs:24`, and `sphere_mesh.rs:23,24`.

**Migrate to**: `register_mesh(&self, table, name, mesh: &axiom_mesh::Mesh)`. A
public signature should name a public type.

---

## 2. `modules/axiom/src/mesh_geometry.rs` — the copy-paste the isolation caused

583 lines (354 non-test). Its own header states the cause:

> This mapping lives in the umbrella because it bridges the umbrella's `Mesh`
> enum to an `axiom-resources` primitive — neither module can name the other's
> contract types, so the composition is the feature module's job. The resources
> table/resolve types are not nameable across the module boundary, so the
> read-back is repeated per primitive (kept as inferred locals) rather than
> factored into a shared helper.

Five near-identical functions — `cube_geometry` (:45), `plane_geometry` (:87),
`sphere_geometry` (:127), `cylinder_geometry` (:167), `resolve_author_geometry`
(:300) — each of which:

1. constructs a **throwaway** `ResourcesApi::new()` and `empty_table()`;
2. registers a mesh into that table;
3. reads it back **one scalar at a time** in a `(0..vertex_count).for_each`
   loop, calling `resolved_mesh_position_at` / `resolved_mesh_normal_at` /
   `resolved_mesh_uv_at` per vertex with `.expect("vertex in range")`;
4. throws the table away.

The bodies are byte-identical apart from the `register_*_mesh` call and the
`expect` string. Dispatch is a `[fn() -> MeshGeometry; 4]` table indexed by
`*mesh as usize` (:30), with a comment noting that adding a `Mesh` variant
requires adding a generator at the same index — a silent-misindex hazard.

There is a second asymmetry: `skinned_author_geometry` (:228) **bypasses the
resources round-trip entirely**, because skinned meshes carry only
position/normal/uv through resources. Static and skinned author meshes therefore
take two different code paths for the same data.

`pub(crate) struct MeshGeometry` (:20) is itself another parallel-stream mesh
(`positions`, `normals`, `uvs`, `indices`, `joints`, `weights`), and its name
collides with `axiom_canvas2d_backend::MeshGeometry` (different fields).

**Migrate to**: delete the file. Once `axiom-resources` traffics in
`axiom_mesh::Mesh`, the umbrella hands one across instead of registering,
reading back scalar-by-scalar, and discarding a table. The four primitive
generators become `axiom_mesh_ops::{cube, quad, uv_sphere, cylinder}` calls.
This is the largest single deletion the convergence enables.

---

## 3. `modules/axiom/src/mesh_data.rs` — `axiom::MeshData`

`modules/axiom/src/mesh_data.rs:55`

```rust
pub struct MeshData {
    positions: Vec<Vec3>, normals: Vec<Vec3>, uvs: Vec<Vec2>,
    indices: Vec<u32>, joints: Vec<[u16; 4]>, weights: Vec<[f32; 4]>,
}
```

**Field-for-field identical to `axiom_proc_mesh::MeshBuffer`.** The same struct,
written twice, in two crates that may not depend on one another. No colours.
Constructors `new` (:72) and `new_skinned` (:94); a `MeshDataError` enum at :24
with 9 variants that duplicates part of `MeshErrorCode`'s job.

The name also collides with `axiom_resources::MeshData` — two distinct
`MeshData` types in the engine, with different fields.

**Migrate to**: `axiom_mesh::MeshStreams` + `Mesh::from_streams`, which is the
same six streams plus tangents and colours, validated, digestible, and
serializable. `MeshDataError` folds into `MeshErrorCode`.

---

## 4. `modules/axiom-terrain-mesh` — `GridMesh` and the `impl Fn` callback

### `GridMesh`

`modules/axiom-terrain-mesh/src/ids.rs:19`

```rust
pub struct GridMesh { positions: Vec<Vec3>, normals: Vec<Vec3>, indices: Vec<u32> }
```

**No UVs.** A terrain patch out of this module cannot be textured with anything
but a shader-side projection, because the parameterization was never emitted.

### The `impl Fn` height callback

```rust
// modules/axiom-terrain-mesh/src/terrain_mesh_api.rs:35
pub fn heightfield_grid_mesh<H>(center: (Meters, Meters), radius: Meters,
                                spacing: Meters, height: H) -> GridMesh
where H: Fn(Meters, Meters) -> Meters

// :104
pub fn heightfield_grid_mesh_rect<H>(center: (Meters, Meters), half_extent: (Meters, Meters),
                                     spacing: (Meters, Meters), height: H) -> GridMesh
where H: Fn(Meters, Meters) -> Meters
```

This is the `generic-behavior-state` shape the **State Law** bans on a public
boundary (`tools/lints/engine_no_retained_state`). A closure is an opaque
capability: the operator cannot know whether it reads a clock, a global, or an
RNG, so it cannot claim its output is a function of its inputs.
`axiom_mesh_ops::heightfield_mesh` deliberately takes `&HeightfieldSamples` — a
`Vec<Meters>` — for exactly this reason.

### Two central-difference normal formulas that disagree

The callback shape is *why* this module drifted. The two public functions
compute the normal differently:

```rust
// terrain_mesh_api.rs:67-69  (square variant)
let nx = -(hx1 - hx0);
let nz = -(hz1 - hz0);
let ny = 2.0 * s;

// terrain_mesh_api.rs:134-136  (rect variant)
let nx = -(sample(x + sx, z) - sample(x - sx, z)) / (2.0 * sx);
let nz = -(sample(x, z + sz) - sample(x, z - sz)) / (2.0 * sz);
let ny = 1.0;
```

They agree up to a positive scale only when `sx == sz`, and the shared
`.max(MIN_NORMAL_LEN)` clamp (`MIN_NORMAL_LEN = 1.0e-6`, :10) means something
different in each: in the rect form `len >= 1.0` always, so the guard at :137 is
**dead code that can never fire**; in the square form `len ≈ 2·spacing`, so it
fires only for sub-micrometre spacing. The doc comments disagree with each other
and with `ids.rs`: :26-28 calls it "central-difference" with `ny = 2·spacing`,
:98 calls it a "gradient normal `(−∂h/∂x, 1, −∂h/∂z)`", and `ids.rs:11`
documents *both* outputs as "the unit central-difference surface normal". The
triangulation (`[i0, i2, i1, i1, i2, i3]`) and the sampling closure are also
duplicated between the two.

**Migrate to**: `axiom_mesh_ops::heightfield_mesh` with `HeightfieldSamples`.
One formula, divided by the true distance spanned on each axis independently,
correct at the borders, tested against the analytic normal at every vertex
including edges (`a_linear_ramp_reports_its_analytic_normal_everywhere`), with
UVs, an optional skirt, and a selectable quad diagonal. The caller keeps the
height function — it just evaluates it into an array first.

---

## 5. `modules/axiom-forest` — a bare tuple with a magic stride

`modules/axiom-forest/src/forest_api.rs:52`

```rust
pub fn tree_mesh() -> (Vec<f32>, Vec<u32>)
```

No `self`, no named type: a raw `(vertices, indices)` pair. The 12-float
interleaved stride is a closure literal at :56-60:

```rust
let vert = |x: f32, y: f32, z: f32, n: [f32; 3], uv: [f32; 2], c: [f32; 3]| -> [f32; 12] {
    [x, y, z, n[0], n[1], n[2], uv[0], uv[1], c[0], c[1], c[2], 1.0]
};
```

The twelve floats are `position.xyz` (3) + `normal.xyz` (3) + `uv` (2) +
`colour.rgb` (3) + a hard-coded alpha of `1.0` (1). Geometry: two crossed
vertical quads — 8 vertices, 24 indices.

**Nothing in the signature or in any type records the stride.** A consumer that
reads this as anything but 12 floats produces garbage, and the compiler cannot
help.

**Migrate to**: `axiom_mesh::Mesh` with `positions`, `normals`, `uvs` and
`colors` streams. The alpha column disappears (it is a constant), the stride
disappears (there is none), and the geometry becomes two
`axiom_mesh_ops::quad` calls through `axiom_mesh::combine`.

---

## 6. `crates/axiom-proc-mesh` — `MeshBuffer` and the welded generators

`crates/axiom-proc-mesh/src/mesh_buffer.rs:18`

```rust
pub struct MeshBuffer {
    positions: Vec<Vec3>, normals: Vec<Vec3>, uvs: Vec<Vec2>,
    indices: Vec<u32>, joints: Vec<[u16; 4]>, weights: Vec<[f32; 4]>,
}
```

The third parallel-stream CPU mesh type, and identical to `axiom::MeshData`.
`pub const MAX_VERTS: usize = 100_000` (:8) is a hard-coded cap where
`axiom_mesh_ops::DetailBudget` is a caller-chosen one. Constructors return
`Option<Self>`, so a rejection carries no reason — where `MeshResult<Mesh>`
carries a specific `MeshErrorCode`.

The generators are `pub(crate)`, reachable only by baking a `RecipeGraph`:

| File | Functions |
|---|---|
| `src/primitives.rs` | `cube` (:42), `grid` (:75), `cylinder` (:108), `sphere` (:154) |
| `src/transforms.rs` | `transform` (:21), `extrude` (:48), `bevel` (:82), `bend` (:111), `displace` (:137), `uv_project` (:161), `triangulate` (:181) |
| `src/implicit.rs` | `meta_surface` (:217) |
| `src/dispatch.rs` | `mesh_eval` (:32) |

All take `NodeEval<'_, MeshBuffer> -> Option<MeshBuffer>`; the only `pub` entry
point is `ProcMeshApi::bake(&self, recipe: &RecipeGraph, seed: u64)`. A test, an
importer or an app that wants a cylinder must construct a recipe graph to get
one.

Its marching cubes also emits a **fully unwelded soup**: `cell_vertices`
(`implicit.rs:153-188`) returns a flat `Vec<Vec3>` of positions and
`meta_surface` then does `let indices = (0..positions.len() as u32).collect();`
(`implicit.rs:246`). One unique vertex per triangle corner, never deduplicated
across shared edges, so a `res = 64` bake burns the 100k vertex budget roughly
six times faster than a welded mesh — and `from_parts_skinned` then returns
`None`, so the bake fails without saying why.

**Desired end state**: `axiom-proc-mesh` keeps what is genuinely its own — the
recipe graph, operator codes, `Param` words, per-node entropy, graph baking —
and **consumes `axiom-mesh-ops`**, returning `axiom_mesh::Mesh`. Every
`pub(crate)` generator above already has a public, tested, budgeted counterpart
in `mesh-ops`. The recipe layer becomes an interpreter over a library instead of
an interpreter with a private library inside it, and the winding defect below
disappears with the reimplementation.

---

## 7. `modules/axiom-canvas2d-backend` — `MeshGeometry` and `decimate`

`modules/axiom-canvas2d-backend/src/mesh_cache.rs:18`

```rust
pub(crate) struct MeshGeometry {
    positions: Vec<[f32; 3]>, colors: Vec<[f32; 4]>, indices: Vec<u32>,
}
```

A fourth CPU mesh shape, **lossy by construction**: `from_interleaved` (:27)
reads floats `0..3` and `8..12` of the 12-float stride and **drops normals and
UVs entirely**. The name collides with `axiom::mesh_geometry::MeshGeometry`.

`modules/axiom-canvas2d-backend/src/mesh_skinning.rs:127`

```rust
fn decimate(verts: &[f32], indices: &[u32]) -> (Vec<f32>, Vec<u32>)
```

**A second vertex-welding implementation, living inside a rasterizer.** It snaps
each vertex to a per-axis grid of `CLUSTER_CELLS_PER_AXIS` cells across the
mesh's own AABB (cell sizes floored at `1e-4`, :143-147), welds every vertex in
a cell to the **first one seen** (`reps.entry(key(v)).or_insert_with(…)`,
:162-166 — the representative's position, normal, uv and skin binding are the
first vertex's, not an average), remaps triangles, and drops any whose three
corners are no longer distinct (:176). It operates on the 20-float *skinned*
stride (`SKINNED_VERTEX_STRIDE = 20`, :23), not the 12-float static one.

Two defects follow from where it lives: welding "first seen" makes the result
depend on **vertex order**, and the `HashMap<(i32, i32, i32), u32>` key makes
the weld sensitive to `as i32` truncation at cell boundaries. `axiom_mesh::weld`
has neither problem — it keeps the **lowest original index** (a pure function of
the input, independent of traversal) and uses a `BTreeMap` lattice with an
explicit 27-cell neighbour scan so a pair straddling a cell boundary still
merges.

**Migrate to**: `axiom_mesh::weld` for the welding, and `axiom_mesh::Mesh` for
the cached geometry — which also restores the normals and UVs
`from_interleaved` currently discards.

---

## 8. The untyped `(u64, Vec<f32>, Vec<u32>)` seam

`modules/axiom/src/app/resources.rs:35`

```rust
pub fn mesh_set(&self) -> Vec<(u64, Vec<f32>, Vec<u32>)>
```

with two siblings on the same seam: `mesh_vertex_stream(&self) -> (Vec<f32>, Vec<u32>)`
(:21) and `skinned_mesh_set(&self) -> Vec<(u64, Vec<f32>, Vec<u32>)>` (:50) —
**the same type, a different stride**, distinguished only by which method you
called. Re-exported unchanged at `apps/axiom-game-runtime/src/bridge.rs:198`.

The 12-float stride is asserted independently in five places:

| # | Site | Form |
|---|---|---|
| 1 | `modules/axiom/src/app/resources.rs:95,102` | producer: `with_capacity(positions.len() * 12)`, then a 12-element `extend_from_slice` |
| 2 | `modules/axiom-canvas2d-backend/src/mesh_cache.rs:13` | `const VERTEX_STRIDE: usize = 12;` |
| 3 | `modules/axiom-gpu-backend/src/scene_renderer.rs:1013` | `const VERTEX_STRIDE: u64 = 12 * 4;` |
| 4 | `modules/axiom-gpu-backend/src/scene_renderer.rs:2266` | `const MESH_VERTEX_FLOATS: usize = 12;` |
| 5 | `modules/axiom-forest/src/forest_api.rs:56` | `-> [f32; 12]` |

And contradicted in a sixth — `modules/axiom-webgpu/src/live_present.rs:234`:

```rust
array_stride: 3 * 4,
```

with a single `VertexFormat::Float32x3` attribute at offset 0 (:236-239). **Five
sites say a vertex is 12 floats; this one says 3.** Nothing in
`(u64, Vec<f32>, Vec<u32>)` records which is true, so the mismatch is invisible
to the compiler and surfaces only as garbage geometry at draw time.

(A related pair, distinct from this: `mesh_skinning.rs:23` and
`scene_renderer.rs:1018` each independently declare the 20-float skinned
stride.)

**Migrate to**: `Vec<(MeshId, axiom_mesh::Mesh)>`. The stride stops existing —
`positions()`, `normals()`, `uvs()` and `colors()` are separate typed slices —
so there is nothing for six sites to disagree about, and a backend that wants an
interleaved buffer builds the interleaving it actually needs at the point of
upload, which is where that decision belongs.

---

## Known follow-ups — NOT done in this change

These are real, verified findings that the mesh/mesh-ops work surfaced and did
**not** address. They are recorded here so they are not rediscovered from
scratch.

### A. `Segments` has a minimum of 3, and `grid` inherits it

`crates/axiom-mesh-ops/src/tessellation.rs:27-46` documents the bound as a
radial argument:

> At least 3 — two segments cannot enclose an area, so a 2-segment cylinder is a
> degenerate sliver rather than a coarse cylinder.

That argument is correct **for a radial count** and does not apply to **linear
subdivision**. `grid(half_width, half_depth, cols: Segments, rows: Segments)`
(`primitive_grid.rs:29`) uses the same type for a linear division count, so
`grid` cannot be built with 1 or 2 divisions per axis — even though a 1×1 grid
is a perfectly well-formed quad and a 2×2 grid is a perfectly well-formed
four-cell sheet. There is no geometric reason to refuse them.

**The fix is a separate `Divisions` type with a minimum of 1**, used by the
linear operators, *not* stretching `Segments`' domain down to 1. Lowering
`Segments` would silently re-admit the 2-segment cylinder the bound exists to
reject; the two quantities are genuinely different and should be two types, the
same way `Rings` (minimum 2) is already separate from `Segments`.

### B. `axiom-proc-mesh`'s marching cubes emits inward-facing triangles

`crates/axiom-proc-mesh/src/implicit.rs` emits each triangle in the **raw Paul
Bourke table order** (`mc_tables.rs:5`), with the standard "bit `i` set when
corner `i` is below the iso value" configuration convention.

That combination winds every triangle so its geometric normal points **down the
field gradient — into the solid**. Verified exhaustively against every linear
field direction in `[-1, 0, 1]³`:

| Triple order | Faces down-gradient (inward) | Faces outward |
|---|---:|---:|
| raw Bourke order | 409 | 0 |
| each triple reversed | 0 | 409 |

The result is unambiguous: **`MeshOp::MetaSurface` has been producing inside-out
surfaces.** Every implicit surface baked through a recipe graph has had its
faces culled backwards, or lit from behind, depending on the backend.

`axiom_mesh_ops::implicit_surface_mesh` reverses each triple correctly
(`implicit_surface.rs`, `row[k + 2 - 2 * (k % 3)]`, with the reasoning recorded
at the site) and is tested by
`the_extracted_sphere_winds_counter_clockwise_outward`. Converging item 6 above
fixes the defect by deletion; fixing it in place in the meantime means reversing
the triple order in `cell_vertices` and adding the outward-winding assertion to
its tests.

### C. A generic 2-bone IK + skeleton/gait primitive is missing

Two apps have independently written the same primitive, because an app may not
depend on another app:

- `apps/end-zone/src/presentation/locomotion/leg.rs` — 166 lines. Header: *"A
  small, explicit two-segment (thigh + shin) analytic leg solver for the
  locomotion animator — NOT a general IK engine."* Carries
  `pub struct LegDims { pub thigh: f32, pub shin: f32 }` reading bone lengths
  from `crate::player::model::PARTS`.
- `apps/dog/src/leg_ik.rs` — 289 lines. Header: *"Two-bone
  analytic inverse kinematics, and the distance-driven stride cycle that feeds
  it."* Law of cosines, `cos A = (a² + d² − b²) / (2 a d)`, with `d` clamped into
  `[|a − b| + ε, (a + b) − ε]`.

Two apps have now paid separately for the same primitive, and the second was
written knowing the first existed and being unable to use it. That is the exact
signal the Module Law describes: a capability that two leaves both need belongs
**below** them.

**It belongs in `modules/axiom-animation`, or in a layer** if physics and
animation both come to need it. This is a placement decision that needs making
before a third app writes a third copy; it is deliberately *not* bundled into
the mesh convergence, because IK is not geometry construction and does not
belong in `axiom-mesh-ops`.
