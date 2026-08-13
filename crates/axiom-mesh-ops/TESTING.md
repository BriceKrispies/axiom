# Axiom Mesh-Ops — Testing

## Shape of the suite

Every test in this layer is an **inline `#[cfg(test)] mod tests`** beside the
operator it exercises — 311 tests across 28 modules. There is no `tests/`
directory. The one module without tests,
[`marching_cubes_tables.rs`](src/marching_cubes_tables.rs), contains three
`const` arrays and no executable code; it is proven by the extraction tests that
index it.

A geometry library is unusually easy to test *badly*: it is trivial to assert a
vertex count, and a vertex count proves almost nothing. The suite is organised
around properties that a wrong implementation would actually fail.

## The Coverage Law

This layer is inside the engine spine and is held at **100% regions, lines, and
functions** — the Coverage Law, not a target. It meets that bar today. There are
no `#[coverage(off)]` attributes and no coverage-ignore entries (the gate's
sanctioned ignore pattern may not, by construction, name any layer or module
file).

Where a fallback exists that no accepted input can reach — `heightfield`'s
`normalize().unwrap_or(Vec3::UNIT_Y)` on a vector whose `y` component is
literally `1.0`, `icosphere`'s `unit()` on a sum of two non-antipodal unit
vectors — the code says so at the site rather than pretending the arm is live,
and the fallback names the layer's one documented default (`+Y`) so the
un-reachable branch cannot introduce a second convention. Everything genuinely
reachable is reached through the public API by constructing the state that
selects it, never by widening a signature for a test.

## Winding: the CCW proof, applied to every primitive

The layer's central convention is that counter-clockwise is front-facing, with
the geometric normal of triangle `(a, b, c)` being
`(b - a).cross(c - a)`. Every primitive is tested against it directly, in one of
two forms depending on what the shape makes available:

**Against the centroid** — valid for any shape that encloses the origin:

```rust
let geometric = b.subtract(a).cross(c.subtract(a));
let centroid  = a.add(b).add(c).mul_scalar(1.0 / 3.0);
assert!(geometric.dot(centroid) > 0.0, "triangle {t:?} faces the origin");
```

Used by `primitive_box::every_triangle_winds_counter_clockwise_outward` (which
*also* asserts the geometric normal agrees with the stored vertex normal),
`primitive_rounded_box::every_triangle_faces_outward` (a rounded box is convex,
so the origin is inside every face's plane), and
`implicit_surface::the_extracted_sphere_winds_counter_clockwise_outward`.

**Against the summed vertex normals** — valid for a shape whose stored normals
are the authority, including open shells that do not enclose the origin:

```rust
let geometric = p[j].subtract(p[i]).cross(p[k].subtract(p[i]));
let outward   = n[i].add(n[j]).add(n[k]);
assert!(geometric.dot(outward) > 0.0, "triangle {t:?} is not CCW-outward");
```

Used as `assert_ccw_outward` by `primitive_cylinder`, `primitive_cone`,
`primitive_frustum`, `primitive_icosphere`, `primitive_sphere`,
`primitive_capsule`, `primitive_torus`, `primitive_disk`, `primitive_grid`, and
`primitive_quad`.

The constructive operators get the same treatment in the shape their geometry
allows: `revolve::an_outer_wall_faces_away_from_the_axis_and_an_inner_wall_toward_it`,
`extrude::a_clockwise_profile_still_extrudes_outward`,
`loft::a_prisms_side_normals_are_horizontal_and_its_caps_axial`, and
`heightfield::a_flat_field_is_exactly_flat_and_faces_up` (which asserts the face
normal is **exactly** `(0, 1, 0)`, not approximately).

A winding regression cannot pass this suite by producing plausible-looking
geometry; it produces an inside-out solid, and a dot product goes negative.

## Named geometric proofs

These are the tests that exist because a specific wrong implementation is
tempting, common, and otherwise invisible.

### Cylinder side normals have `y == 0` exactly; cone side normals do not

The classic bug in the cylinder/cone/frustum family is to give a cone the
horizontal radial normal that belongs to a cylinder. A cone's surface leans
inward as it rises, so its normal must lean *upward*.

`primitive_cylinder::side_normals_are_horizontal_with_y_exactly_zero` asserts
`assert_eq!(n.y, 0.0)` — exact equality, not a tolerance — for every side
vertex, and additionally that each normal is the radial direction of its own
vertex.

`primitive_cone::side_normals_are_slant_normals_not_radial_ones` is its
deliberate counterpart. For `radius = 2`, `half_height = 3` it asserts every
side normal is unit length, that `n.y > 0.0`, and that `n.y` equals the analytic
`2 / hypot(6, 2)` to `1e-5`. Then it asserts the wrong answer explicitly:

```rust
// A radial normal would have y == 0 — assert we did NOT emit that.
assert!(c.normals().iter().all(|n| n.y != 0.0));
```

`a_flatter_cone_leans_its_normals_further_up` pins the relationship's direction:
a wide, short cone's normals lean further up than a narrow, tall one's. And
`primitive_cone::the_end_cap_is_ignored_and_the_start_cap_is_the_base_disc`
asserts `CapPolicy::End` produces a mesh *equal* to `CapPolicy::None` — the top
of a cone is a point, and a fan of degenerate triangles is not a cap.

### Icosphere level 1 has exactly 42 vertices, not 60

Recursive triangle bisection is correct only if the two triangles sharing an
edge agree on that edge's midpoint. A generator that mints a fresh midpoint per
triangle produces geometry that *looks* right and is silently unwelded.

`primitive_icosphere::level_one_shares_every_edge_midpoint` asserts
`vertex_count() == 42` and `triangle_count() == 80`. The count is the proof: a
naive refinement mints three midpoints per triangle and reports `12 + 20*3 = 72`
vertices (60 of them duplicates). Sharing them gives exactly 42.

`counts_follow_the_geodesic_identity` generalises it, asserting
`10 * 4^n + 2` vertices and `20 * 4^n` triangles for `n` in `0..=3`. The sharing
is implemented with a `BTreeMap` keyed on the *sorted* endpoint pair — ordered,
never a hash map — and `refinement_is_deterministic_across_runs` guards that.
`triangles_are_near_uniform_in_area` (max/min area spread `< 1.4`) pins the
property that motivates an icosphere over a UV sphere in the first place.

### Marching cubes on a sampled sphere lands on the true radius

`implicit_surface::a_sampled_sphere_field_extracts_the_true_sphere` samples the
exact signed-distance field of a unit sphere over `[-2, 2]³` at spacing `0.25`,
extracts the zero level set, and asserts the worst radial error over **every**
extracted vertex is under `SPHERE_SPACING * 0.2` — a fifth of one cell. The
assertion is that the surface *is* the sphere, not a blob near it. It also
asserts `> 100` triangles, so a degenerate extraction cannot pass by producing
nothing.

`a_raised_iso_level_extracts_a_larger_sphere` repeats it at `iso = 0.5` against
radius 1.5, confirming the level-set parameter is honoured rather than ignored.

The supporting extraction tests:

- `the_gradient_normals_on_a_sphere_are_radial_and_unit_length` — each normal is
  unit to `0.05` and dots `> 0.9` with the true radial direction; and
  `assert!(!mesh.has_uvs())`, pinning the deliberate absence.
- `a_flat_step_field_crosses_on_a_plane` — a field equal to `x` extracts the
  plane `x = 0` with every vertex at `|x| < 1e-5` and every normal exactly `+X`.
  This also exercises the equal-corner-values guard on every cube edge running
  along `y` or `z`.
- `welding_joins_the_soup_into_a_shared_surface` — the welded result has fewer
  vertices than `3 × triangles`, so the delegated deduplication actually ran.
- `a_field_wholly_on_one_side_of_the_iso_level_is_empty_not_an_error` — both
  signs; zero triangles, the single origin point, a `+Y` normal.
- `an_over_budget_extraction_is_refused_before_it_allocates` — `BudgetExceeded`
  with a budget of 4 triangles.

### Heightfield normals are analytically exact, at the borders too

`heightfield::a_flat_field_is_exactly_flat_and_faces_up` asserts every normal is
**exactly** `Vec3::new(0.0, 1.0, 0.0)` — `assert_eq!`, no epsilon — and that
every face normal is exactly `+Y`.

`a_linear_ramp_reports_its_analytic_normal_everywhere` is the stronger claim. A
ramp `h = 2x` at `spacing_x = 0.5` has the analytic normal
`normalize(-2, 1, 0)`, and the test asserts every vertex matches it to `1e-6`
— **including the border vertices**, where the central-difference window is
clipped to the grid. That works only because the divisor shrinks to the real
distance actually spanned (`(c1 - c0) as f32 * sx`, which is `1 * sx` at a
border and `2 * sx` inside). An implementation that hard-codes `2 * spacing`
passes in the interior and fails on every edge vertex.

`an_unequal_spacing_ramp_scales_each_axis_independently` catches the other half
of the same bug: with `spacing_x = 100` and `spacing_z = 4`, a `z`-only ramp
must be divided by `spacing_z`. The older shortcut form (`difference / (2 *
spacing)` with `y = 2 * spacing`) silently uses the wrong axis scale and tilts
the shading.

The skirt is tested for what a skirt is *for*:
`a_skirt_adds_an_outward_curtain_hanging_exactly_below_the_border` asserts the
exact added vertex and triangle counts (16 and 16 for a 3×3 grid), that the
lowest point is exactly `-skirt_depth`, that every wall triangle faces away from
the patch centre, and that every wall vertex normal is horizontal
(`n.y.abs() < 1e-6`) and outward. `a_skirt_on_a_non_square_grid_rings_the_whole_border`
pins `border_ring(5, 2)` to a literal index list.

### Sweep frames do not flip through vertical

`sweep_frames::a_vertical_path_frames_without_a_flip` drives the pathological
case for a fixed-up construction: the tangent is exactly the conventional
up-vector, and the caller even asks for `+Y` as the reference. Every frame must
be orthonormal and every consecutive pair must satisfy
`normal[i].dot(normal[i+1]) > 0.0`.

`a_path_climbing_through_vertical_never_flips_its_normal` is the real test. A
Catmull-Rom path runs horizontally, turns straight up, and leaves horizontally
in a new direction. It first asserts the path genuinely passes through vertical
(`frames.iter().any(|f| f.tangent().y > 0.99)`) — so the test cannot pass by
never reaching the hard case — then asserts orthonormality everywhere and a
positive normal-to-normal dot across **every** span, reporting the index where a
flip occurred.

`a_right_angled_corner_turns_the_normal_by_exactly_the_tangents_turn` states the
positive claim rather than just the absence of a flip: across every span the
normal turns *no further than the tangent did*
(`turned >= tangents - 1e-4`), and end to end a `+X → +Y → +Z` path carries a
seeded `+Y` normal to `-X` — a quarter turn, not the half-turn snap a fixed-up
frame suffers, and not the identity either.

`an_inflection_does_not_disturb_the_frame` (an S-curve in the XY plane keeps a
`+Z` reference within `1e-3` of `+Z` throughout — no spurious twist),
`a_helix_accrues_no_spurious_twist`, `a_straight_path_keeps_one_constant_frame`,
and `a_reversing_span_carries_the_normal_rather_than_negating_it` (an exactly
opposed tangent pair has a zero cross product; the carry rule keeps the previous
normal rather than letting a 180° rotation invert it) complete the set.

Seeding is pinned separately:
`a_reference_parallel_to_the_tangent_falls_back_to_a_world_axis`,
`a_zero_reference_requests_the_fallback_and_is_not_an_error`,
`a_non_finite_reference_is_a_degenerate_axis` (`NaN`, `+Inf`, `-Inf`), and
`the_least_aligned_axis_prefers_the_first_minimum` (which fixes the tie-break so
the fallback is a pure function of the tangent).

### Ear clipping produces exactly `n - 2` triangles and rejects a bowtie

`polygon_triangulation::a_many_sided_convex_polygon_yields_n_minus_two_triangles`
triangulates a 12-gon and asserts `triangles.len() == point_count - 2`, plus
that the total area matches the analytic `3.0` for a regular 12-gon inscribed in
radius 1.

`a_self_intersecting_bowtie_fails_as_untriangulatable` builds a polygon whose
edge `(0,0)→(4,4)` crosses edge `(4,0)→(0,1)` and asserts
`MeshErrorCode::TriangulationFailed`. That failure is the *detection* of a
non-simple polygon, not a timeout: the clip is a `try_fold` over exactly
`n - 2` steps, and "a full pass found no ear" is precisely the non-simple
condition, so a bad polygon fails instead of looping forever.

The rest of the triangulation contract:

- `a_counter_clockwise_square_becomes_two_triangles_covering_its_area` and
  `a_clockwise_square_still_yields_counter_clockwise_triangles` — the input
  winding is normalised internally, and the *output* is always CCW.
- `indices_address_the_original_point_order` — the internal re-orientation is
  invisible: the emitted indices are the caller's own point indices, all of
  them, none invented.
- `a_concave_l_shape_yields_four_positive_area_triangles` — plus the assertion
  that **no triangle covers the notch**, which area alone would not catch.
- `a_collinear_vertex_neither_clips_as_an_ear_nor_blocks_one` — a redundant
  midpoint on an edge yields 3 triangles at the same total area.
- `an_open_profile_cannot_be_triangulated` — `InvalidProfile`.
- `triangulation_is_deterministic` — the first-ear-in-ring-order scan means the
  same polygon always decomposes the same way.

### Loop shrinks toward the limit surface; midpoint does not move a vertex

These two operators share a topological step and differ entirely in where the
vertices go, so the tests are written as a *contrast*.

`subdivision::midpoint_never_moves_an_original_vertex_but_loop_does` asserts, on
one octahedron:

```rust
assert_eq!(&midpoint.positions()[..original.vertex_count()], original.positions());
assert_ne!(&looped  .positions()[..original.vertex_count()], original.positions());
// same topology, different geometry — the proof they are different algorithms,
// not one wearing two names.
assert_eq!(looped.triangle_count(), midpoint.triangle_count());
assert_eq!(looped.vertex_count(),   midpoint.vertex_count());
assert_ne!(looped.positions(),      midpoint.positions());
```

`loop_contracts_a_closed_mesh_toward_its_limit_surface` asserts the contraction
is monotone — `max_radius(twice) < max_radius(once) < max_radius(original)` —
and, in the same test, that midpoint subdivision keeps the extreme radius
**exactly** unchanged. One test, both algorithms, no ambiguity about which is
interpolating.

Supporting:

- `one_midpoint_level_shares_edge_vertices_between_neighbours` and
  `midpoint_levels_multiply_the_triangle_count_by_four` — the shared-edge-vertex
  property, the same one the icosphere's 42 pins.
- `midpoint_places_new_vertices_exactly_half_way`,
  `midpoint_interpolates_uvs_at_the_new_vertices`,
  `midpoint_interpolates_every_other_present_stream`.
- `loop_keeps_a_closed_mesh_closed` — after two levels, **every** edge still has
  exactly two adjacent faces.
- `loop_uses_the_boundary_rules_on_an_open_mesh_without_producing_nan` — a lone
  triangle, where every edge is a boundary edge, produces finite positions and
  the exact `1/8, 3/4, 1/8` boundary mask result.
- `loop_applies_the_interior_vertex_mask_at_valence_three_and_above` — Warren's
  `3/16` at valence 3, which the general `3/(8n)` form would get wrong.
- `loop_leaves_an_unreferenced_vertex_where_it_was`,
  `loop_carries_every_attribute_stream_through_the_masks`,
  `loop_leaves_an_absent_normal_stream_absent`,
  `midpoint_normal_blending_falls_back_when_two_normals_cancel`,
  `a_zero_length_tangent_falls_back_to_its_source_direction`,
  `a_non_manifold_edge_reads_its_first_two_faces`,
  `both_schemes_are_reproducible`.

### Simplification is byte-identical across two runs

`simplification::the_same_input_decimates_to_byte_identical_output` runs
`simplify_quadric` twice on the same sphere at `Fraction(0.25)` and asserts both
`Mesh` equality *and* `MeshStreams` equality, plus that the result is genuinely
smaller than the input (so the test cannot pass by doing nothing).

That is a real property to defend. Simplification is a long chain of "pick the
cheapest thing", and floating-point ties plus unordered iteration are exactly
how such a chain becomes irreproducible. Two implementation rules make it hold,
and both are separately tested:

1. **Costs are quantized** to `(cost * 1e6).round() as i64`, so a last-bit
   difference in an `f32` accumulation cannot reorder two collapses.
2. **Ties break on the edge itself** — candidates live in a `BTreeMap` keyed on
   `(quantized_cost, min_index, max_index)`, unique and totally ordered, derived
   only from the mesh's own numbering. `costs_quantize_to_a_reproducible_total_order`
   asserts that directly. No hash map is used anywhere in the module.

The rest of the decimation suite asserts it is a *quadric* decimation and not a
triangle cull:

- `a_sphere_decimates_to_a_quarter_of_its_triangles_and_stays_valid` — hits the
  target, stays a valid `Mesh`, has **no orphaned vertices** (the referenced
  index set equals the vertex count), and keeps the original's extent within
  slack while retaining more than half its width. QEM keeps the silhouette; a
  cull would shave whole regions off.
- `a_flat_sheet_decimates_through_the_singular_midpoint_placement` — every plane
  of a flat sheet is the same plane, so the quadric's 3×3 sub-system is rank one
  and the optimal placement does not exist; the midpoint fallback is the
  *normal* path here, not an error path.
- `a_singular_quadric_falls_back_and_a_full_rank_one_solves` — both branches.
- `every_attribute_stream_survives_decimation` — all six optional streams
  present and correctly sized afterwards.
- `a_collapse_that_would_fold_a_neighbour_over_is_rejected`,
  `a_non_manifold_edge_is_rejected`,
  `a_collapse_that_would_erase_the_last_triangle_is_rejected` — rejection is a
  skip, never a failure: the operator returns the best mesh it could reach.
- `a_triangle_count_below_one_is_rejected`,
  `a_fraction_outside_the_unit_interval_is_rejected`,
  `a_target_at_or_above_the_current_count_returns_the_mesh_unchanged`,
  `a_tiny_fraction_still_asks_for_at_least_one_triangle`.

## The tessellation and profile vocabulary

Every bound is tested at **both** edges of its domain, accepted inside and
rejected outside, always on the exact `MeshErrorCode`:

| Type | Accepted | Rejected |
|---|---|---|
| `Segments` | `3`, `MAX_SEGMENTS` | `2`, `MAX_SEGMENTS + 1` |
| `Rings` | `2`, `MAX_RINGS` | `1`, `MAX_RINGS + 1` |
| `Subdivisions` | `0`, `MAX_SUBDIVISIONS` | `MAX_SUBDIVISIONS + 1` |
| `Samples` | `2`, `MAX_SAMPLES` | `1`, `MAX_SAMPLES + 1` |
| `DetailBudget` | `1`, default `1_000_000` | `0`; `admit` refuses one over |

`Profile` (15 tests) covers each validation arm separately — too few points,
non-finite points, duplicate consecutive points, a closed profile whose last
point repeats its first, a zero-area closed profile — and the accepted case an
adjacent rule would wrongly reject (`an_open_profile_may_be_collinear`). The
constructors are pinned to their geometry: a circle profile is CCW with every
point on the radius and `points()[0] == (r, 0)`; a rectangle is centred with the
analytic signed area; `rotating_preserves_area_and_moves_points`;
`scaling_scales_area_quadratically`.

`CapPolicy`'s four variants are each asserted on `caps_start`, `caps_end` and
`cap_count`, so the two-bit mask arithmetic is exhaustively covered.

## Determinism testing approach

Determinism is asserted **per operator**, not once globally, because each
operator has its own way to lose it. The pattern is always the same — build the
same input twice, run the operator twice, assert full `Mesh` equality:

`generation_is_deterministic` / `..._is_reproducible` exists for `uv_sphere`,
`torus`, `capsule`, `frustum`, `icosphere`, `rounded_box`, `extrude`, `loft`,
`revolve`, `subdivide_midpoint` + `subdivide_loop`, `triangulate_profile`, and
`parallel_transport_frames`; `simplify_quadric` gets the stronger byte-identical
form above.

Those assertions are cheap but not vacuous, because the implementations contain
exactly the constructs that would break them if chosen carelessly. Every
associative container in this layer is a `BTreeMap` or `BTreeSet` — the
icosphere's midpoint cache, the subdivision edge table, the simplification
candidate set — never a `HashMap`, because hash iteration order is not a fact
this layer is allowed to depend on. Every "pick one" is resolved by an explicit
rule (first minimum, lowest index, ring order) rather than by whichever
candidate a traversal happened to reach first. The determinism tests are what
notice if that discipline slips.

## Running the tests

```sh
cargo test -p axiom-mesh-ops
```

All tests are inline and native. Nothing here needs a browser, a GPU, or a wasm
target.

## Running the coverage gate

Coverage is measured across the whole workspace, not per crate:

```sh
bash scripts/coverage.sh          # Linux / CI
scripts/coverage.ps1              # Windows / PowerShell
scripts/coverage.ps1 -Open        # annotated HTML report; red is the work list
```

**Coverage requires an MSVC nightly toolchain on Windows.** This repository's
default toolchain is `stable-x86_64-pc-windows-gnu`, under which the gate fails
with:

```text
error[E0463]: can't find crate for `profiler_builtins`
```

That is the instrumentation runtime, which the GNU toolchain does not ship.
Install and select `nightly-x86_64-pc-windows-msvc` before running the gate.
Nightly is preferable regardless: only nightly populates the true "Branches /
Missed Branches" columns. On stable the script falls back to region coverage,
which still pins the gate at 100%.
