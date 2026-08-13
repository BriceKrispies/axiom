# Axiom Mesh — Architecture

## What this layer is

`axiom-mesh` (`crates/axiom-mesh/`, layer name `mesh`) owns **the** engine's
neutral CPU-side representation of triangle geometry — the [`Mesh`](src/mesh.rs)
value type — and the operations that are *intrinsic to that representation*:
validation, derived bounds, generated normals and tangents, affine
transformation, combination, welding, a deterministic digest, and versioned
binary serialization.

It declares `depends_on = ["kernel", "math"]`
([`layer.toml`](layer.toml)) and imports nothing else. It owns **no generator**:
producing geometry from a description is `axiom-mesh-ops`' job. This layer would
be byte-for-byte identical if the engine had no procedural generation at all.

| File | Owns |
|---|---|
| [`src/mesh.rs`](src/mesh.rs) | `Mesh` — the type, its accessors, its one constructor |
| [`src/mesh_streams.rs`](src/mesh_streams.rs) | `MeshStreams` — the pre-validation attribute streams |
| [`src/mesh_validation.rs`](src/mesh_validation.rs) | `validate_streams`, `SKIN_WEIGHT_TOLERANCE` — the contract |
| [`src/mesh_error.rs`](src/mesh_error.rs), [`src/mesh_error_code.rs`](src/mesh_error_code.rs), [`src/mesh_result.rs`](src/mesh_result.rs) | `MeshError`, `MeshErrorCode`, `MeshResult` |
| [`src/mesh_bounds.rs`](src/mesh_bounds.rs) | `aabb`, `bounding_sphere` |
| [`src/mesh_normals.rs`](src/mesh_normals.rs) | `generate_normals`, `generate_flat_normals` |
| [`src/mesh_tangents.rs`](src/mesh_tangents.rs) | `generate_tangents` |
| [`src/mesh_transform.rs`](src/mesh_transform.rs) | `transform`, `reverse_winding` |
| [`src/mesh_combine.rs`](src/mesh_combine.rs) | `combine` |
| [`src/mesh_weld.rs`](src/mesh_weld.rs) | `weld`, `remove_degenerate_triangles` |
| [`src/mesh_binary.rs`](src/mesh_binary.rs) | `encode_mesh`, `decode_mesh`, `MESH_SCHEMA_VERSION` |
| [`src/mesh_digest.rs`](src/mesh_digest.rs) | `digest` |

## Why one canonical mesh type

Before this layer existed, the repository carried **seven mutually incompatible
CPU mesh representations**. Not seven views of one type — seven types, none of
which could name any of the others:

| Representation | Where it lives | Shape |
|---|---|---|
| `Vertex` + `MeshData` | `modules/axiom-resources` | interleaved struct-of-vertex, `name: &'static str` |
| `MeshInputVertex` | `modules/axiom-resources` | a `pub(crate)` alias appearing in a *public* signature |
| `GridMesh` | `modules/axiom-terrain-mesh` | positions + normals + indices, **no UVs** |
| `(Vec<f32>, Vec<u32>)` | `modules/axiom-forest` | a bare tuple, 12-float interleaved stride |
| `MeshBuffer` | `crates/axiom-proc-mesh` | positions/normals/uvs/indices |
| `MeshData` | `modules/axiom/src/mesh_data.rs` | field-for-field identical to `MeshBuffer` |
| `MeshGeometry` | `modules/axiom-canvas2d-backend` | rasterizer-side geometry + its own welding |

Two of those seven — `axiom_proc_mesh::MeshBuffer` and `axiom::MeshData` — were
**field-for-field identical and mutually unnameable**: the same struct, written
twice, in two crates that the Module Law forbids from depending on one another.
That is not a coincidence; it is the predictable output of a structure with no
shared vocabulary for geometry. Every pair of crates that needed to hand a mesh
across had to invent a third shape or flatten to floats, and each new
representation made the next one more likely.

`Mesh` is the answer to the question that fragmentation asks: *what is the one
value that all of them are trying to be?* The migration path from each of the
seven onto it is recorded in
[`docs/mesh-convergence-migration.md`](../../docs/mesh-convergence-migration.md).

## Why a layer and not a module

This is the decisive rule, and it is mechanical, not stylistic.

**Seven engine modules need to name triangle geometry**: `resources`,
`terrain-mesh`, `figure`, `physics`, `draw2d`, `text`, and `gpu-backend`. Under
**Module Law #2**, an engine module may never depend on another module — its
`allowed_modules` must be empty, and the checker rejects a violation as
`ModuleDependsOnModule` (`crates/xtask/src/class_check.rs`). If `Mesh` lived in
a module, at most one of those seven could ever use it, and the other six would
be back to inventing their own. The Module Law's own remedy states the
conclusion outright: *"If two engine modules want to share a primitive, the
primitive belongs in a lower **layer**, not a third module."*

There is a second, independent reason. **Module Law #8** allows a module's
`lib.rs` to expose exactly one public facade (plus its `ids` vocabulary). A
geometry vocabulary does not fit through a facade: callers must be able to
*name* `Mesh`, construct `MeshStreams` field by field, match on
`MeshErrorCode`, and hold a `MeshResult<Mesh>` in a signature. Squeezing that
through a single `MeshApi` handle would either hide the type (making it
unnameable, which is exactly the disease) or widen the facade rule for
everybody.

A layer has neither restriction: it publishes a curated set of primitives, and
any layer above it may depend on it. That is the shape the problem has.

### Why it depends on kernel and math, and genuinely uses both

- **math** — every attribute *value* is a math type (`Vec2`, `Vec3`, `Vec4`),
  every derived bound is a math volume (`Aabb`, `Sphere`), and placement is a
  math `Mat4`. Math owns points and volumes but has no notion of connectivity;
  this layer adds the index buffer and the stream-alignment contract on top.
- **kernel** — `StableHash` for the digest, `BinaryWriter`/`BinaryReader` +
  `SchemaVersion` for versioned little-endian serialization, `KernelError` as a
  wrapped deserialization cause, and `Meters` so a weld tolerance crosses the
  public boundary as a dimensioned quantity rather than a naked `f32`.

## The attribute model: structure of arrays

`MeshStreams` is a plain value struct of parallel `Vec`s:

```rust
positions: Vec<Vec3>     // required, non-empty
indices:   Vec<u32>      // required, triangle-list
normals:   Vec<Vec3>
uvs:       Vec<Vec2>
tangents:  Vec<Vec4>     // xyz direction, w handedness
colors:    Vec<Vec4>     // linear RGBA
joints:    Vec<[u16; 4]>
weights:   Vec<[f32; 4]>
```

Three properties of that shape are load-bearing.

**An empty stream means the attribute is absent.** There is no `Option<Vec<_>>`
and no presence bitflag in the type. `normals: vec![]` and "this mesh has no
normals" are the same statement, so absence needs no separate encoding, no
`unwrap`, and no "present but empty" third state to reason about. A present
stream must be exactly `positions.len()` long — never a prefix.

The payoff shows up in operations that would otherwise carry a special case for
every optional stream. `mesh_normals::gather` copies an attribute through a
corner list with `stream.get(i)`: an absent stream misses on every lookup and
produces an empty result, so absence propagates for free. `mesh_weld::picked`
does the same on the surviving-vertex list.

**Structure of arrays, never interleaved.** Interleaving is a *GPU vertex-layout
decision* — which attributes share a buffer, in what order, with what padding —
and that decision belongs to a backend, which knows the pipeline it is feeding.
Baking one interleaving into the representation is precisely what produced the
fragmentation above: `axiom-forest`'s 12-float stride and
`axiom-resources`' `Vertex` are each a frozen answer to a question a backend
should have been asked. SoA has no answer to freeze, so every backend can build
the layout it wants without the representation objecting.

**Public fields, no builder.** Construction stays immutable and
machine-authorable: an agent or a generator fills the streams it has and leaves
the rest empty, using struct-update syntax to name only the present attributes:

```rust
Mesh::from_streams(MeshStreams {
    uvs,
    ..MeshStreams::new(positions, indices)
})
```

## The topology model

**`u32` triangle-list indices, and nothing else.** No strips, no fans, no quads,
no polygon lists, no primitive-topology enum. A strip is a rendering-side
optimisation; a quad mesh is an authoring-side convenience; both are lossy or
ambiguous under the operations this layer performs (welding, transforming,
combining). One topology means every operation is written once.

An **empty index buffer is legal**: zero triangles is a whole number of
triangles, and a mesh with positions but no faces is a structurally valid point
set awaiting topology. `remove_degenerate_triangles` can legitimately produce
one.

## Invariants

`validate_streams` ([`src/mesh_validation.rs`](src/mesh_validation.rs)) is the
single gate. It enforces, in this order — the first failure wins, so the
reported code is a stable function of the input:

| # | Check | Code on failure |
|---|---|---|
| 1 | `positions` is non-empty | `EmptyPositions` |
| 2 | every position component is finite (no `NaN`, no `±Inf`) | `NonFinitePosition` |
| 3 | `indices.len()` is divisible by 3 | `IndexCountNotTriangular` |
| 4 | every index `< positions.len()` | `IndexOutOfRange` |
| 5 | each of `normals`/`uvs`/`tangents`/`colors` is empty **or** exactly `positions.len()` long | `AttributeLengthMismatch` |
| 6 | every component of those streams is finite | `NonFiniteAttribute` |
| 7 | `joints` and `weights` are both absent, or both exactly `positions.len()` long | `SkinStreamMismatch` |
| 8 | every weight row is finite, non-negative, and sums to `1.0 ± SKIN_WEIGHT_TOLERANCE` (`1.0e-3`) | `SkinWeightsNotNormalized` |

**`Mesh::from_streams` is the only constructor.** `Mesh` has one private field
and no other way in, so an invalid mesh is *unrepresentable* — not "discouraged",
not "checked at the boundary", but impossible to hold. Every generator,
importer, refinement, and decoder in the engine funnels through that one call,
which means the invariant list above is the complete set of assumptions any
downstream consumer may make about any mesh it is handed, from any source.

`validate_streams` is also public, so a caller can test candidate streams
without constructing, and so the invariants are testable in isolation from the
type.

## Winding convention

**Right-handed, Y-up. Counter-clockwise triangles are front-facing.**

For triangle `(a, b, c)` the geometric normal is:

```text
(p[b] - p[a]).cross(p[c] - p[a])
```

That expression is the definition, not a description of it — it is what
`mesh_normals::face_cross` computes, and every generator in `axiom-mesh-ops` is
tested against it (see [`TESTING.md`](TESTING.md)).

**UV origin `(0, 0)` is the lower-left**; `v` increases upward.

**Tangent `w` is the bitangent handedness**: `+1` when
`bitangent == normal.cross(tangent.xyz)`, `-1` otherwise. A shader rebuilds the
third axis as `w * (n × t)`. A mirrored UV island flips `w`, which is exactly
the information a shader would otherwise have to guess.

Two operations depend on the convention being *stated* rather than assumed:

- `transform` reverses every triangle's index order when the matrix's linear
  part has a **negative determinant**, because a mirror reverses the orientation
  of every triangle and a CCW-front mesh would otherwise cull backwards after
  being mirrored. It negates each tangent's `w` for the same reason.
- `reverse_winding` turns a solid into its own interior (a box becomes a room, a
  sphere becomes a skydome) by swapping corners 2 and 3 of every triangle and
  negating normals and tangent directions. It leaves `w` alone: reversing the
  surface negates both the normal and the tangent, so their cross product — and
  therefore the convention `w` records — is unchanged.

## Determinism: the digest

`digest(&Mesh) -> StableHash` is how the engine *names* geometry it did not
author by hand: an asset cache key, a golden-artifact fingerprint, a provenance
record for a generated mesh, a "did this operation change anything" check.

**It reuses the serializer.** `digest` hashes the bytes `write_mesh` produces —
it does not define its own encoding, and it must never be allowed to. Two
encodings of the same value are two definitions of that value's identity, and
they drift the moment a stream is added. Because the digest is literally
`StableHash::of_bytes` over the canonical encoding, *"these meshes serialize the
same"* and *"these meshes digest the same"* cannot disagree.

Every byte of the encoding is hashed, so the digest changes with any change to a
position component, an index, an attribute value, a count, or the **presence**
of an optional stream. Presence is explicit in the encoding's bitmask, so a mesh
carrying an all-zero colour stream and a mesh carrying no colours digest
differently — as they must, since they are different meshes.

**The honest `-0.0` note.** `f32` components are hashed as their IEEE-754 bit
patterns. `-0.0` and `+0.0` have different bit patterns and therefore **digest
differently**, even though they compare equal as numbers and even though the two
meshes compare `PartialEq`-equal. This is deliberate and documented rather than
convenient: the digest reports the bytes the mesh actually holds. Normalizing
the sign would make the digest claim that two different byte sequences are the
same value, which is the one thing an identity function may not do.

**It is an index, not a proof.** Following the kernel's stance on `StableHash`:
byte equality is the verdict, a digest match is a hint. FNV-1a is a 64-bit
non-cryptographic hash; collisions are astronomically unlikely, not impossible.
Use `digest` to label, key, and locate geometry — not to certify that two meshes
are identical.

## Serialization

[`src/mesh_binary.rs`](src/mesh_binary.rs) owns **the** byte shape of a mesh.
There is deliberately no second encoding.

```text
SchemaVersion   major: u16, minor: u16      (MESH_SCHEMA_VERSION = 1.0)
vertex_count    u32
index_count     u32
presence        u32  bitmask
positions       vertex_count x Vec3   (3 x f32)      always present
indices         index_count  x u32                   always present
normals         vertex_count x Vec3                  iff bit 0
uvs             vertex_count x Vec2                  iff bit 1
tangents        vertex_count x Vec4                  iff bit 2
colors          vertex_count x Vec4                  iff bit 3
joints          vertex_count x [u16; 4]              iff bit 4
weights         vertex_count x [f32; 4]              iff bit 4
```

Every value is written through a kernel `BinaryWriter` primitive, so the
encoding is **little-endian on every platform** and no memory representation,
padding byte, or pointer ever reaches the buffer. Feeding the same mesh in twice
yields byte-identical output on every target, which is what makes `digest`
stable.

**The presence bitmask** is what makes stream *absence* an encoded fact rather
than an inference from a length. Joints and weights share bit 4 because the mesh
contract guarantees they are present together or absent together.

**`MESH_SCHEMA_VERSION`** is `SchemaVersion::new(1, 0)`. `decode_mesh` accepts
any buffer sharing this *major* version (the kernel's
`SchemaVersion::is_compatible_with` rule): a minor bump may append data a reader
can ignore, a major bump may not.

**`decode_mesh` ends at `Mesh::from_streams`.** That is the structurally
important detail. Three things can go wrong, and each is deterministic:

1. the declared version is incompatible — `DeserializationFailed`, with **no**
   kernel cause, because nothing in the kernel failed;
2. the buffer is short at any point — `DeserializationFailed` **with** the
   kernel reader's fault as the wrapped cause, and the reader is left parked at
   the failing read so `BinaryReader::position` says where the data ran out;
3. the bytes decode but describe an **illegal** mesh — an out-of-range index, a
   misaligned stream, unnormalized skin weights. Because the last step of
   decoding is the same constructor everything else uses, a corrupt-but-readable
   buffer is rejected with the *specific structural code* (`IndexOutOfRange`,
   `SkinWeightsNotNormalized`, …) and **no invalid `Mesh` can be produced by
   decoding**.

A hostile buffer declaring a four-billion-vertex mesh does not get to ask the
allocator for the fiction up front: `read_stream` clamps its reserved capacity
by the reader's remaining byte count (every item this module reads occupies at
least four bytes), so the read fails on a bounds check instead.

## Bounds

`aabb` is the tight component-wise envelope of every position, seeded with the
first position (which always exists, by invariant #1) and grown one point at a
time. A single-vertex mesh yields a degenerate box whose `min` equals its `max`
rather than an error.

`bounding_sphere` is centred on the AABB centre with the radius that just
reaches the furthest position. **It is deliberately not the minimal enclosing
sphere.** Ritter's algorithm and Welzl's exact solution both produce a smaller
sphere for most inputs; this construction can be up to `sqrt(3)` times too large
in the worst case (a single point in the corner of a cubic box).

The trade is made on purpose: this is a **closed-form, order-independent
function of the positions**. Welzl's algorithm depends on a randomized or
order-dependent incremental pass, and Ritter's depends on which extreme point
happens to be found first — both make the result a function of traversal order
as well as data, which is exactly the kind of thing that stops replaying
byte-identically. The construction here is tight enough for the culling and
broad-phase uses a derived bound serves, and a caller who genuinely needs the
minimal sphere can compute one. **This layer will not silently trade determinism
for tightness.**

Neither bound reads topology. A vertex that no triangle references still counts,
because bounds answer *"where is this data"*, not *"where is this surface"*.

## What it deliberately does not know

A `Mesh` names no material, no texture, no shader, no GPU buffer, no vertex
layout, no scene node, no entity, no resource id, no asset origin, no LOD
policy, and no browser type.

That list is not modesty; it is the **convergence principle** the type exists
for:

> An imported mesh and a procedurally generated mesh are the same kind of value
> here, and nothing downstream can tell them apart.

A glTF importer, a marching-cubes extraction, a hand-authored primitive, a
decoded cache entry and a welded triangle soup all produce the same type,
satisfying the same invariants, with the same digest rule. Every consumer —
renderer, physics collider builder, text mesher, canvas rasterizer — is written
once, against one contract, and gains every producer for free. The moment the
type learned about materials or resource ids, an "imported" mesh and a
"generated" mesh would become distinguishable, and the consumers would start
carrying a branch for the difference.

## The public boundary carries no `&mut`

The **State Law** (`tools/lints/engine_no_retained_state`) bans mutable engine
APIs on the public surface: a public method taking `&mut self`, or a public
function taking a caller-supplied `&mut` sink, is retained state the caller can
observe between calls.

`encode_mesh(&Mesh) -> Vec<u8>` and `decode_mesh(&[u8]) -> MeshResult<Mesh>` are
shaped specifically for that. The natural signatures would have been
`write_mesh(&Mesh, &mut BinaryWriter)` and `read_mesh(&mut BinaryReader)` — and
those functions do exist, as **`pub(crate)`**, because `digest` needs to hash
into a writer it owns. What crosses the *public* boundary hands back an owned
`Vec<u8>` and takes an owned `&[u8]`: the writer and reader are implementation
details of the encoding, not part of the contract.

The result is that this layer adds **no findings to the State Law inventory**.
Every public function is a pure value transform: streams in, mesh out; mesh in,
bytes out. The `&mut` that does exist (`accumulate_face_normals`,
`accumulate_triangle`, the weld scan's buckets) is confined to private helpers
building a return value, which the law explicitly permits.

## Related documents

- [`TESTING.md`](TESTING.md) — what is tested, what each test proves, how to run
  it.
- [`../axiom-mesh-ops/ARCHITECTURE.md`](../axiom-mesh-ops/ARCHITECTURE.md) — the
  layer that *produces* meshes.
- [`../../docs/mesh-convergence-migration.md`](../../docs/mesh-convergence-migration.md)
  — the representations that have not converged yet.
