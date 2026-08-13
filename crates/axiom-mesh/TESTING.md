# Axiom Mesh — Testing

## Shape of the suite

Every test in this layer is an **inline `#[cfg(test)] mod tests`** beside the
code it exercises — 121 tests across 13 modules. There is no `tests/` directory
and no shared fixture crate. A module's tests are the executable statement of
that module's contract, and they move with it.

| Module | Tests | What the suite proves |
|---|---:|---|
| [`mesh.rs`](src/mesh.rs) | 6 | counts and accessors; absent-by-default optional streams; `into_streams` round-trips every attribute; construction rejects a contract break; `Mesh` compares by value |
| [`mesh_streams.rs`](src/mesh_streams.rs) | 3 | `new` populates only positions and indices; `default` is entirely empty; struct-update syntax names only present attributes |
| [`mesh_validation.rs`](src/mesh_validation.rs) | 12 | **every error arm** — see below |
| [`mesh_error.rs`](src/mesh_error.rs) | 4 | identity is `(code, kernel-cause)` and ignores the message; a plain error carries no cause; a wrapped one carries and compares it |
| [`mesh_error_code.rs`](src/mesh_error_code.rs) | 2 | discriminants are stable (`EmptyPositions == 1` … `TangentGenerationFailed == 20`); codes order by discriminant |
| [`mesh_bounds.rs`](src/mesh_bounds.rs) | 7 | box is the component-wise envelope; single vertex → degenerate box; unreferenced vertices still widen it; sphere reaches the furthest corner and is *not* minimal; an overflowing box centre reports unrepresentable bounds |
| [`mesh_normals.rs`](src/mesh_normals.rs) | 13 | area weighting, winding-decides-sign, `+Y` fallbacks, flat unwelding, attribute carry-through |
| [`mesh_tangents.rs`](src/mesh_tangents.rs) | 12 | tangent follows increasing `u`; orthonormality; handedness; every fallback and every failure arm |
| [`mesh_transform.rs`](src/mesh_transform.rs) | 14 | the normal matrix, the mirror rule, singular rejection, `reverse_winding` involution |
| [`mesh_combine.rs`](src/mesh_combine.rs) | 7 | index offsetting; the present-on-all attribute policy and its order-independence |
| [`mesh_weld.rs`](src/mesh_weld.rs) | 16 | lowest-index-wins merging, lattice-boundary merges, seam erasure, determinism, degenerate-triangle removal |
| [`mesh_digest.rs`](src/mesh_digest.rs) | 9 | stability and sensitivity, including the `-0.0` case |
| [`mesh_binary.rs`](src/mesh_binary.rs) | 16 | layout, full round-trip, and rejection at every truncation prefix |

## The Coverage Law

This layer is inside the engine spine, so it is held at **100% regions, lines,
and functions** — the Coverage Law, not a target. It meets that bar today, and
every change to it ships with the tests that keep it there. There are no
`#[coverage(off)]` attributes, no `cfg(test)` carve-outs, and no entries in the
coverage gate's ignore pattern (which by construction may not name any layer or
module file).

Where a fallback arm exists that runtime cannot reach — `unit_or_up`'s `+Y`
default, `renormalize`'s zero-length passthrough — the test suite reaches it
through the *public* API by constructing the state that selects it
(`exactly_cancelling_faces_fall_back_to_up`,
`a_degenerate_zero_normal_survives_as_zero_rather_than_becoming_nan`). Nothing
is widened or shimmed to make a region reachable.

## The specific properties asserted

### Every error arm of `validate_streams`

Each of the eight checks in the validation contract has at least one test that
provokes it and asserts on the exact `MeshErrorCode`:

| Code | Test |
|---|---|
| — (success) | `a_minimal_triangle_validates`, `aligned_streams_of_every_kind_validate` |
| — (success) | `an_index_buffer_may_be_empty` — zero triangles is a whole number of triangles |
| `EmptyPositions` | `empty_positions_are_rejected` |
| `NonFinitePosition` | `a_non_finite_position_is_rejected` — both `NaN` and `Inf` |
| `IndexCountNotTriangular` | `a_non_triangular_index_count_is_rejected` |
| `IndexOutOfRange` | `an_out_of_range_index_is_rejected` |
| `AttributeLengthMismatch` | `a_misaligned_attribute_stream_is_rejected` |
| `NonFiniteAttribute` | `a_non_finite_normal_uv_tangent_or_colour_is_rejected` — all four streams, `NaN`, `+Inf` and `-Inf` |
| `SkinStreamMismatch` | `skin_streams_must_be_paired` — joints only, weights only, and both-but-short |
| `SkinWeightsNotNormalized` | `unnormalized_negative_or_non_finite_weights_are_rejected` — sums-to-half, a negative row, a `NaN` row |

`a_weight_row_within_tolerance_is_accepted` pins the other side of check 8: a
row off by half of `SKIN_WEIGHT_TOLERANCE` is accepted, so the tolerance is a
tested boundary rather than an arbitrary constant.

Because `Mesh::from_streams` is the only constructor,
`construction_rejects_streams_that_break_the_contract` closes the loop: the
validator's verdict is the type's verdict.

### Digest stability and sensitivity

Stability:

- `the_same_mesh_digests_the_same_every_time` — repeated calls agree.
- `independently_built_identical_meshes_digest_the_same` — two separately
  constructed equal meshes agree, so the digest is a function of the value, not
  of an allocation.
- `the_digest_is_the_hash_of_the_canonical_serialized_bytes` — the digest is
  asserted to equal `StableHash::of_bytes` over `write_mesh`'s output. This is
  the test that makes drift between digest and serialization *impossible to
  introduce silently*: change the encoding and this assertion fails.

Sensitivity — each of these asserts the digest **changes**:

- `changing_one_position_component_changes_the_digest` (a `1e-6` nudge),
- `changing_one_index_changes_the_digest` (rewinding a triangle),
- `changing_one_attribute_value_changes_the_digest` (a UV and a colour,
  independently, and the two mutations distinguish from each other),
- `adding_a_normal_stream_changes_the_digest`,
- `a_present_all_zero_stream_differs_from_an_absent_one` — the values would be
  zero either way; only the presence bitmask separates them, which is why the
  encoding records it,
- `negative_zero_digests_differently_from_positive_zero` — this test *asserts
  the documented surprise*: `plus.positions()[0] == minus.positions()[0]` while
  `digest(&plus) != digest(&minus)`. The behaviour is pinned so it cannot be
  "fixed" into a silent normalization.

### Full round-trip, including stream absence

`a_mesh_with_every_stream_round_trips_exactly` builds a mesh carrying all six
optional streams and asserts equality of the recovered mesh *and* spot-checks
each stream's values (an off-by-one in the fixed stream order would round-trip
the counts but scramble the values).

`a_minimal_mesh_round_trips_and_keeps_its_streams_absent` is the other half, and
the reason presence is encoded: after a round trip, `has_normals()`,
`has_uvs()`, `has_tangents()`, `has_colors()` and `is_skinned()` are all still
`false`. Absence survives serialization as a fact, not as an accident of a zero
length.

Supporting assertions:

- `the_header_is_version_then_counts_then_the_presence_mask` pins the first 16
  bytes literally and the total size (64 bytes for a bare triangle).
- `the_presence_mask_records_each_stream_in_its_own_bit` pins the mask as
  `0b0001_1111` for a full mesh and the exact byte total (292), so a change to
  the layout has to be deliberate.
- `re_encoding_a_decoded_mesh_reproduces_the_same_bytes` — encode/decode is a
  fixed point.
- `signed_zero_survives_the_round_trip_unnormalized`.
- `skin_streams_share_one_presence_bit`.

### Truncation at every prefix length

`truncating_the_buffer_anywhere_fails_with_a_kernel_cause` is a loop over
**every** prefix `0..bytes.len()` of a fully-populated mesh's encoding. For each
one it asserts both that the code is `DeserializationFailed` **and** that
`err.kernel().is_some()` — the kernel reader's fault is preserved as the wrapped
cause, so a caller can ask the reader where the data ran out. The loop ends by
asserting the untruncated buffer *does* decode, so the test cannot pass by
rejecting everything.

The neighbouring cases separate the three failure kinds:

- `an_incompatible_major_version_is_rejected_without_a_kernel_cause` — code
  `DeserializationFailed`, `kernel() == None`, because nothing in the kernel
  failed.
- `a_differing_minor_version_is_still_decodable` — the compatibility rule works
  in both directions.
- `a_readable_buffer_that_breaks_the_mesh_contract_is_rejected_structurally` —
  a valid-length buffer whose last index is rewritten to `9` fails with
  `IndexOutOfRange`, not with a generic decode error. This is the assertion that
  `decode_mesh` really does end at `Mesh::from_streams`.
- `a_readable_payload_with_unnormalized_skin_weights_is_rejected` — same, via
  `SkinWeightsNotNormalized`.
- `a_buffer_declaring_no_vertices_is_rejected_as_an_empty_mesh` —
  `EmptyPositions`.
- `an_absurd_declared_vertex_count_fails_on_a_bounds_check` — a header claiming
  `u32::MAX` vertices fails on a bounds-checked read rather than an allocation.

### Weld determinism across repeated runs

`welding_is_deterministic_across_repeated_runs` welds the same input twice and
asserts full mesh equality. That is cheap but not vacuous: the implementation's
determinism rests on two choices the test would catch the loss of — the spatial
lattice is a `BTreeMap` (ordered iteration), and among all candidates within
tolerance the merge keeps the one with the **lowest original vertex index**, so
the surviving vertex is a pure function of the input rather than of traversal
order.

The surrounding tests pin what welding actually does:

- `two_coincident_vertices_become_one` and
  `every_attribute_stream_follows_the_surviving_vertices` — the lower-indexed
  vertex survives and carries *its own* attributes.
- `welding_erases_a_uv_seam_and_the_first_vertex_wins` — the documented
  destructive behaviour, asserted rather than left as prose. Six vertices at
  four distinct positions collapse to four, and the seam's second UV is gone.
- `welding_across_a_lattice_boundary_still_merges` — `0.999999` and `1.000001`
  straddle the cell boundary at a `1e-3` tolerance, so only the 27-cell
  neighbour scan finds the pair.
- `a_tolerance_smaller_than_the_gap_merges_nothing`,
  `a_triangle_that_collapses_while_welding_is_dropped`,
  `welding_may_legally_remove_every_triangle`.
- `lattice_cells_and_their_neighbourhood_are_addressed_consistently` — the
  quantization and the 27-step neighbour walk, including that step 13 is the
  centre.
- `a_non_positive_weld_tolerance_is_rejected` (zero and negative).

`remove_degenerate_triangles` is tested separately for the property that
distinguishes it from welding: `a_zero_area_triangle_is_removed_and_every_vertex_is_kept`
asserts the vertex count is **unchanged** (5 in, 5 out) and every attribute
stream is still 5 long, so an index a caller is holding stays valid.
`the_tolerance_is_an_area_threshold_of_tolerance_squared` pins the threshold on
both sides of a triangle of area `0.005` (kept at `0.07`, dropped at `0.08`).

### Normals: area weighting

`a_shared_vertex_is_weighted_by_face_area_not_face_count` is the load-bearing
test. Vertex 0 is shared by a large `+Y` face (twice-area 100) and a tiny `+X`
face (twice-area 1). Equal weighting would give a 45° blend — `~0.707` on both
axes; area weighting must land the result almost exactly on `+Y`. The test
asserts `n.y > 0.999`, `0.0 < n.x < 0.02`, and explicitly that `n.x < 0.5`,
naming the wrong answer so a regression to per-face averaging cannot pass.

That property is why `face_cross` deliberately keeps the *un-normalized* cross
product: its magnitude is twice the triangle's area, so accumulating raw vectors
weights each face by its area for free. Normalizing first would make a mesh's
shading depend on how finely it happened to be cut.

The rest of the normal suite:

- `a_flat_quads_smooth_normals_all_point_up` and
  `reversing_the_winding_flips_the_generated_normal` — winding, not position
  order, decides the sign.
- `a_vertex_no_triangle_references_falls_back_to_up` and
  `exactly_cancelling_faces_fall_back_to_up` — the two reachable routes to a
  zero accumulation, both landing on the documented `+Y`.
- `flat_generation_unwelds_to_three_vertices_per_triangle` — 4 vertices become
  6, indices become `0..3n`.
- `flat_normals_differ_per_face_across_a_crease` — a roof: the two slopes keep
  distinct normals where smoothing would have averaged them.
- `flat_generation_carries_every_present_attribute_through_the_unweld` and
  `flat_generation_leaves_absent_attributes_absent` — the two halves of the
  empty-stream-means-absent contract, through the same code path.
- `unwelding_a_mesh_with_no_triangles_reports_an_empty_mesh` — the documented
  `EmptyPositions` outcome rather than an undrawable mesh.
- `both_generators_are_deterministic`.

### Tangents: the fallbacks

`generate_tangents` has three distinct fallback behaviours, and each is asserted
separately rather than lumped into "it doesn't crash":

1. **A vertex with no usable accumulation** receives a deterministic orthonormal
   companion of its normal — the same east axis `axiom_math::tangent_basis`
   builds. `a_vertex_outside_every_triangle_gets_a_companion_of_its_normal`
   asserts the orphan's tangent is exactly `Vec4::new(1, 0, 0, 1)` for a `+Y`
   normal, *and* that the triangulated vertices are unaffected by its presence.
2. **A single UV-degenerate triangle** contributes a zero vector (selected
   without a branch and without ever multiplying by an infinite reciprocal) so
   its neighbours still decide the shared vertices' frames.
   `one_degenerate_triangle_does_not_spoil_its_neighbours` asserts the shared
   vertex still gets `+X` while the vertex touched *only* by the degenerate face
   falls back to the normal's companion.
3. **A wholly degenerate parameterization** — every triangle collapsed onto one
   UV — is a failure, not an invention:
   `a_wholly_degenerate_parameterization_is_reported_not_invented` expects
   `TangentGenerationFailed`.

Plus the two missing-input arms (`a_mesh_without_uvs_cannot_produce_tangents`,
`a_mesh_without_normals_cannot_produce_tangents`) and the correctness
assertions: `the_tangent_follows_increasing_u`,
`the_stored_tangent_is_unit_length_and_perpendicular_to_the_normal` (with
normals deliberately leaned into the raw tangent so Gram-Schmidt must remove a
*real* component, not a rounding error), `a_right_handed_mapping_reports_positive_handedness`,
`a_mirrored_v_axis_reports_negative_handedness` (`w == -1` while the tangent
direction is unchanged — exactly what `w` exists to record), and
`a_rotated_mapping_rotates_the_tangent`.

### Transform: the normal matrix and the mirror

`non_uniform_scale_keeps_the_normal_perpendicular_to_its_face` is the test that
catches the single most common mesh-transform bug. A 45° face is squashed on X
by 4×. The test asserts the transformed normal is unit length, that it is
genuinely parallel to the *recomputed geometric* normal of the transformed face
(`|dot| > 0.9999`), and — the tell-tale — that `n.x > n.y`, which is the
*opposite* of the direction the position matrix would have moved it. Running
normals through the position matrix passes a "looks normalized" check and fails
this one.

`a_mirror_reverses_winding_and_flips_tangent_handedness` asserts all three
consequences of a negative determinant together: indices become `[0, 2, 1]`, the
tangent becomes `(-1, 0, 0, -1)`, and the normal still agrees with the re-wound
face. `a_singular_matrix_is_rejected` covers the `InvalidParameter` arm.
`reversing_twice_returns_the_original_mesh` pins `reverse_winding` as an
involution.

## Running the tests

```sh
cargo test -p axiom-mesh
```

Every test is inline and native; nothing here needs a browser, a GPU, or a wasm
target.

## Running the coverage gate

Coverage is measured across the whole workspace, not per crate:

```sh
bash scripts/coverage.sh          # Linux / CI
scripts/coverage.ps1              # Windows / PowerShell
scripts/coverage.ps1 -Open        # annotated HTML report; red = your work list
```

**Coverage requires an MSVC nightly toolchain on Windows.** The repository's
default toolchain here is `stable-x86_64-pc-windows-gnu`, and running the gate
under it fails with:

```text
error[E0463]: can't find crate for `profiler_builtins`
```

That is the instrumentation runtime, which the GNU toolchain does not ship.
Install and select an MSVC nightly (`nightly-x86_64-pc-windows-msvc`) before
running the gate; the script prefers nightly anyway, because only nightly
populates the true "Branches / Missed Branches" columns. On stable it falls back
to region coverage, which still pins the gate at 100%.
