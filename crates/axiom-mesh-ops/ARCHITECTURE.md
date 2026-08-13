# Axiom Mesh-Ops — Architecture

## What this layer is

`axiom-mesh-ops` (`crates/axiom-mesh-ops/`, layer name `mesh-ops`) is the
engine's **deterministic geometry library**. It constructs
[`axiom_mesh::Mesh`](../axiom-mesh/src/mesh.rs) values and nothing else.

It declares `depends_on = ["kernel", "math", "mesh"]`
([`layer.toml`](layer.toml)). The `mesh` layer can only validate and manipulate
geometry it is handed; this layer is what *produces* it.

## The operator contract

Every public entry point in this layer has the same shape:

```text
explicit deterministic input data  ->  MeshResult<Mesh>
```

That is the whole contract, and each word in it is load-bearing:

- **no ambient state** — no operator reads or writes anything outside its
  arguments;
- **no global state** — there is not a `static` in the layer;
- **no wall clock** — nothing here can tell you what time it is;
- **no unseeded randomness** — nothing here has an RNG at all, seeded or not;
  entropy, where a caller wants it, is *sampled data* the caller supplies;
- **no hidden lookup** — no resource table, no asset registry, no cache;
- **no callbacks** — no operator takes a closure or a `dyn Fn` (see below);
- **no scene access** — this layer cannot name a node, an entity, or a
  transform hierarchy;
- **no GPU access** — no device, no buffer, no vertex layout, no capability
  query.

An operator handed the same inputs twice produces **byte-identical** output.
That is what makes generated geometry replayable, diffable, and cacheable by
`axiom_mesh::digest`. It is also what makes a generated mesh and an imported
mesh indistinguishable downstream — the convergence property the `mesh` layer
exists for.

Local mutation *inside* an operator is fine and common (an accumulator, an edge
cache, a `Vec` being filled). The rule is about what crosses the boundary.

## Operator families

Everything below is a `pub` item of [`src/lib.rs`](src/lib.rs).

### Primitives

| Operator | File | Notes |
|---|---|---|
| `triangle` | [`primitive_triangle.rs`](src/primitive_triangle.rs) | three explicit corners; rejects collinear and coincident |
| `quad` | [`primitive_quad.rs`](src/primitive_quad.rs) | two triangles in XZ, `+Y` normals |
| `grid` | [`primitive_grid.rs`](src/primitive_grid.rs) | tessellated rectangle — the input every vertex-displacing operator wants |
| `box_mesh`, `cube` | [`primitive_box.rs`](src/primitive_box.rs) | 24 hard-creased vertices, 12 triangles, a full UV chart per face |
| `disk`, `annulus` | [`primitive_disk.rs`](src/primitive_disk.rs) | fan and quad band |
| `uv_sphere` | [`primitive_sphere.rs`](src/primitive_sphere.rs) | ring/segment lattice with a duplicated seam and clean rectangular wrap |
| `icosphere` | [`primitive_icosphere.rs`](src/primitive_icosphere.rs) | geodesic; near-uniform triangle area, no poles, no clean wrap |
| `frustum` | [`primitive_frustum.rs`](src/primitive_frustum.rs) | the family's real generator |
| `cylinder` | [`primitive_cylinder.rs`](src/primitive_cylinder.rs) | `frustum(r, r, …)` |
| `cone` | [`primitive_cone.rs`](src/primitive_cone.rs) | `frustum(r, 0, …)` |
| `capsule` | [`primitive_capsule.rs`](src/primitive_capsule.rs) | cylinder plus two hemispheres |
| `torus` | [`primitive_torus.rs`](src/primitive_torus.rs) | two duplicated seams |
| `rounded_box` | [`primitive_rounded_box.rs`](src/primitive_rounded_box.rs) | the Minkowski sum of a box and a ball |

`cylinder` and `cone` **delegate** to `frustum` rather than re-deriving the ring
trigonometry. That is not code-golf: it is why the three can never drift apart
on the seam rule, the cap winding, or the slant normal.

### Constructive

| Operator | File | Turns |
|---|---|---|
| `triangulate_profile` | [`polygon_triangulation.rs`](src/polygon_triangulation.rs) | a closed 2D outline into `n - 2` CCW triangles |
| `extrude` | [`extrude.rs`](src/extrude.rs) | a profile + a distance into a solid with walls and caps |
| `sweep` | [`sweep.rs`](src/sweep.rs) | a profile carried along a `Curve` |
| `loft` | [`loft.rs`](src/loft.rs) | an ordered series of placed cross-sections into a skin |
| `revolve` | [`revolve.rs`](src/revolve.rs) | a silhouette spun about an axis — the lathe |

`sweep.rs` also owns the shared ring-lattice mechanics (`oriented_ccw`,
`column_points`, `column_arc`, `column_normals`, `stitch_rings`, `cap_mesh`),
crate-visible, because a sweep is the canonical ring-lattice operator. `loft`
and `revolve` call them rather than re-deriving the winding and the seam rule
and risking disagreeing with the sweep about either.

### Sampled-data surfaces

| Operator | File | Input |
|---|---|---|
| `tessellate_surface` | [`surface_tessellation.rs`](src/surface_tessellation.rs) | `SurfaceGrid` — a row-major lattice of already-sampled `Vec3` |
| `heightfield_mesh` | [`heightfield.rs`](src/heightfield.rs) | `HeightfieldSamples` — a row-major grid of `Meters` |
| `implicit_surface_mesh` | [`implicit_surface.rs`](src/implicit_surface.rs) | `ScalarField` — a sampled 3D lattice of `f32` |

### Refinement

| Operator | File | Behaviour |
|---|---|---|
| `subdivide_midpoint` | [`subdivision.rs`](src/subdivision.rs) | **interpolating** — new vertex at the exact edge midpoint, no original vertex moves |
| `subdivide_loop` | [`subdivision.rs`](src/subdivision.rs) | **approximating** — Charles Loop's masks move both odd and even vertices toward the limit surface |
| `simplify_quadric` | [`simplification.rs`](src/simplification.rs) | Garland & Heckbert quadric-error-metric edge collapse |

## Why sampled data instead of callbacks

The natural signature for a parametric surface operator is a callback:
`tessellate(f: impl Fn(f32, f32) -> Vec3, …)`. This layer does not use it
anywhere, for two reasons that agree.

**The structural reason.** The **State Law**
(`tools/lints/engine_no_retained_state`) bans a public `impl Fn` / `Fn` generic
parameter outright (`generic-behavior-state`), along with `Box<dyn Fn>`
(`stateful-callback-boundary`) and `&dyn Trait` (`opaque-behavior-state`). A
callback is an *opaque capability*: from inside the operator there is no way to
know whether it reads a clock, a global, an RNG, or a file. An operator that
accepts one cannot honestly claim its output is a function of its inputs,
because the caller can smuggle arbitrary hidden state through the closure. That
is the whole disease the law exists to prevent.

**The design reason, which is the same reason.** A sampled lattice is a
**value**: hashable, diffable, serializable, loggable, and reproducible. If a
generated surface looks wrong, you can print the samples that produced it, save
them as a fixture, and replay them a year later. A closure cannot be any of
those things. It is also the shape a machine author wants: an agent composing
geometry emits data, not Rust closures.

So the three sampled-data operators take arrays:

```rust
pub fn heightfield_mesh(samples: &HeightfieldSamples, options: HeightfieldOptions) -> MeshResult<Mesh>
pub fn implicit_surface_mesh(field: &ScalarField, iso: IsoValue, options: ImplicitSurfaceOptions) -> MeshResult<Mesh>
pub fn tessellate_surface(grid: &SurfaceGrid, wrap_u: bool, wrap_v: bool) -> MeshResult<Mesh>
```

`HeightfieldSamples` carries `Vec<Meters>` — dimensioned and finite by
construction, so the operator needs no finiteness check on the heights at all.
`ScalarField` carries `Vec<f32>` and validates finiteness itself. Evaluation
policy is the caller's; geometry is ours.

### The shape this deliberately does not copy

`modules/axiom-terrain-mesh` is the counter-example, and it is in the tree
today:

```rust
// modules/axiom-terrain-mesh/src/terrain_mesh_api.rs:35
pub fn heightfield_grid_mesh<H>(center: (Meters, Meters), radius: Meters,
                                spacing: Meters, height: H) -> GridMesh
where H: Fn(Meters, Meters) -> Meters
```

That is the `generic-behavior-state` shape. It is also *why* that module drifted:
because the height source is a closure rather than data, its two public
functions ended up with **two different central-difference normal formulas**
that disagree (`terrain_mesh_api.rs:67-69` versus `:134-136`) with no way for a
test to compare them on the same input without writing two closures. Sampled
data makes divergence like that visible; a callback hides it. The migration is
tracked in
[`docs/mesh-convergence-migration.md`](../../docs/mesh-convergence-migration.md).

## The tessellation / detail policy

Detail is expressed in one small validated vocabulary
([`tessellation.rs`](src/tessellation.rs)), which every generator speaks:

| Type | Domain | Meaning |
|---|---|---|
| `Segments` | `3..=4096` | radial or linear divisions around/along a surface |
| `Rings` | `2..=4096` | latitudinal divisions pole to pole |
| `Subdivisions` | `0..=8` | levels of recursive refinement (each ×4 triangles) |
| `Samples` | `2..=65536` | points a curve or path is sampled at |
| `DetailBudget` | `>= 1` triangle, default `1_000_000` | a ceiling an operator checks *before* allocating |

Every one of these is **caller-chosen, bounded, backend-neutral, and
deterministic**. The module does exactly what its own header says it does and
nothing more:

> This module never asks what device it is on, never reads a frame time, and
> never consults a hardware capability.

There is **no device query, no FPS input, no hardware capability check, and no
adaptive tier** anywhere in this layer. Choosing a budget is a *policy* decision
that belongs to whoever is composing the geometry — an app that knows it is on a
phone, a build step that knows it is producing a lightmap. What lives here is
only the bounded, validated vocabulary for *expressing* that choice, so an
operator can refuse an absurd request instead of allocating without limit.

The lower bounds are geometric arguments, not taste: two segments cannot enclose
an area, so a 2-segment cylinder is a degenerate sliver; one ring degenerates a
sphere into a disc; a single sample has no extent to sweep along. (One
consequence of `Segments`' minimum of 3 is recorded as a known follow-up in the
migration document — the radial argument does not apply to linear subdivision.)

`DetailBudget` exists for the operators whose output size is not obvious from
their inputs. `implicit_surface_mesh` **counts the triangles its configuration
table will emit before emitting any of them**, admits the total against the
budget, and only then allocates — so an over-budget extraction is refused rather
than half-built.

## Input validation

Every operator validates its inputs and returns a `MeshResult<Mesh>`. **Nothing
in this layer panics on data it was handed.** There is no `unwrap` on caller
data, no `assert!` on a parameter, no `todo!`, and no `unimplemented!` (the last
two are banned outright in the spine by Module Law #10).

The layer reuses `axiom_mesh::MeshErrorCode` rather than defining a second error
vocabulary; the operator-facing codes live there alongside the
representation-facing ones:

| Code | Raised when |
|---|---|
| `InvalidParameter` | a negative radius, a zero extent, a zero extrusion distance, a non-positive spacing, an out-of-domain simplify target |
| `InvalidTessellation` | a `Segments`/`Rings`/`Subdivisions`/`Samples`/`DetailBudget` outside its domain |
| `InvalidProfile` | too few points, duplicate neighbours, a zero-area closed outline, triangulating an open polyline |
| `InvalidPath` | fewer than two path samples, or a path that cannot be sampled into stations with defined tangents |
| `InvalidGridDimensions` | a lattice dimension below the minimum, or a sample count that does not match the declared shape |
| `IncompatibleProfiles` | loft sections disagreeing on point count or open/closed policy |
| `TriangulationFailed` | ear clipping found no ear — the outline is self-intersecting |
| `DegenerateAxis` | a zero-length or non-finite revolution axis, or a non-finite sweep reference |
| `BudgetExceeded` | the operation would produce more triangles than the caller's `DetailBudget` allows |
| `NonFiniteAttribute` / `NonFinitePosition` | a non-finite sampled value |

Two validation choices are worth naming because they are policy, not mechanism:

- **A non-finite value is a caller bug, not a request for a default.**
  `parallel_transport_frames` accepts `Vec3::ZERO` as "no preference, pick for
  me" and returns `DegenerateAxis` for a `NaN` reference. Silently substituting
  a fallback for `NaN` would hide the caller's bug and hand back frames nothing
  asked for.
- **An empty result is not an error.** A scalar field that never crosses its iso
  level extracts a mesh with zero triangles (holding the single point
  `options.origin`, because the `Mesh` contract requires at least one position),
  not a failure. A cap request on an open profile is ignored rather than
  rejected — the caller asking for a capped sweep of a polyline wants the
  ribbon, and refusing would push an `is_closed()` test into every call site for
  no gain.

## Curve and sweep framing policy

The split between `axiom-math` and this layer is deliberate and precise.

**`axiom_math::Curve` owns the mathematics of a curve** — where it is
(`position_at`), which way it points (`tangent_at`), how long it is
(`arc_length`), and how to sample it at equal arc length (`sample_uniform`,
returning `CurveSample`s of position, tangent, parameter and distance). Three
kinds: polyline, chained cubic Bézier, uniform Catmull-Rom. That is all
mathematics; it has no opinion about meshing.

**This layer owns the framing** ([`sweep_frames.rs`](src/sweep_frames.rs)):
`parallel_transport_frames(&[CurveSample], Vec3) -> MeshResult<Vec<SweepFrame>>`,
where a `SweepFrame` is an orthonormal `(position, tangent, normal, binormal)`
station with `binormal == tangent.cross(normal)`.

Framing lives here, not in `math`, because **which orthonormal basis a swept
cross-section should ride in is a geometry-construction policy, not a property
of the curve**. The seeding rule, the collinear-carry rule and the
re-orthogonalisation are choices this layer makes so that `sweep` and `revolve`
agree with each other. Pushing them down into `math` would give the curve
primitive an opinion about meshing that nothing else in `math` needs — a
`Curve` used for a camera path or an animation channel would inherit a
sweep-shaped basis for no reason.

### The anti-flip reasoning: there is no global up-vector

The classic wrong answer is a **fixed-up frame**: `binormal = up.cross(tangent)`
for some global `up` (almost always `+Y`). That construction is *undefined
exactly where the tangent becomes parallel to `up`*. A path that climbs through
vertical therefore does not degrade gracefully — the cross-section snaps through
a half-turn in one span and the swept surface tears. It is the most common sweep
bug there is, and the failure only appears on the one path shape an author is
most likely to try (a road going over a crest, a pipe turning upward).

What this module builds instead is a **rotation-minimising frame** (parallel
transport; a Bishop frame). Frame `0` is seeded once, and every later frame is
the previous one carried by the *minimal* rotation taking `tangent[i-1]` onto
`tangent[i]` — a rotation about `cross(t[i-1], t[i])` by the angle between them,
applied with Rodrigues' formula. Because that rotation is minimal:

- the frame never spins about its own tangent, so it accrues **no twist the path
  did not ask for**;
- it is **defined for every tangent**, including a vertical one;
- **consecutive normals can never flip sign**, because a minimal rotation
  between two unit vectors is at most a half-turn and is continuous in them.

Where two consecutive tangents are collinear (the cross product falls below
`FRAME_EPSILON`) the rotation is exactly the identity and the previous normal is
carried through unchanged — which is what makes a straight run, and a straight
*vertical* run, produce a constant cross-section rather than an undefined one.

**There is no global up-vector anywhere in the file, by design.** The one place
a direction must be invented is the seed, and the rule there is a pure function
of the first tangent: the caller's `initial_reference` with its tangent-parallel
component removed, or — when that leaves nothing usable — the **world axis least
aligned with the first tangent**, scored over `[+X, +Y, +Z]` taking the first
minimum. A unit tangent cannot be within 55° of all three axes at once, so the
fallback always yields a healthy perpendicular, and being a pure function of the
tangent it is deterministic and replayable.

`revolve` shares `seed_normal` for exactly the same reason the ring mechanics
are shared: it needs a deterministic unit vector perpendicular to a given axis,
and would otherwise re-derive the rule and risk disagreeing with the sweep about
it. That is why revolving about a tilted axis is as well-defined as revolving
about `+Y`.

## Deterministic surface extraction

`implicit_surface_mesh` is marching cubes over an **explicitly sampled** field.
The caller evaluates whatever it likes — a signed distance function, a metaball
sum, a noise volume, a medical scan, a voxel occupancy grid — onto a
`cols × rows × depth` lattice and hands the values over. The operator has no
recipe graph, no entropy source, no callback, and no opinion about what the
numbers mean.

Three properties make it deterministic and safe:

1. **The configuration index is arithmetic.** Bit `c` of the config is
   `usize::from(values[c] < iso) << c`, summed — no branch chain, no early exit.
   The Bourke triangle table is a `const [[i8; 16]; 256]`
   ([`marching_cubes_tables.rs`](src/marching_cubes_tables.rs)).
2. **The budget is checked before allocation.** `extract` walks every cell and
   sums `triangles_in(configuration(…))` *first*, calls
   `options.budget.admit(total)`, and only then emits geometry. A field that
   would blow the budget costs one counting pass, not a partial allocation.
3. **Deduplication is delegated, not reinvented.** The extraction emits an
   unindexed triangle soup — marching cubes computes each edge vertex once per
   cell, and an interior vertex appears in two to four cells — and then runs it
   through `axiom_mesh::weld` at a tolerance of `1e-3` of the finest cell
   spacing. That is safe because duplicates of the same edge vertex are computed
   from the same two corner values and are therefore **bit-identical**, while
   genuinely distinct vertices are a large fraction of a cell apart. A
   hand-rolled edge-to-index cache inside the operator would reimplement, less
   well, the deduplication the `mesh` layer already owns — and would be a second
   place for the weld rule to drift.

**Normals are gradients, not face normals.** Each cube corner carries the
central-difference gradient of the sampled field, clipped to the lattice at the
borders, and an emitted edge vertex interpolates its two corners' gradients by
the same parameter that placed it. That is what makes an implicit surface shade
smoothly instead of showing the marching-cubes staircase: the gradient is the
surface's real normal field, sampled where the surface actually is. The field
convention is signed-distance-like — values rise going outward, so the gradient
points outward and *is* the outward normal.

**The winding is reversed from the raw table, deliberately.** The Paul Bourke
table, driven by the "bit set when the corner is below the iso value"
convention, winds each triangle so its geometric normal points *down* the field
gradient — into the solid. Axiom's convention is the opposite, so each run of
three edge indices is reversed in place (`row[k + 2 - 2 * (k % 3)]`). This was
verified exhaustively against every linear field direction; the corresponding
defect in `axiom-proc-mesh`, which does *not* reverse, is recorded in the
migration document.

**No UVs.** A volumetric field has no intrinsic surface parameterization, and
inventing one (a planar or spherical projection) would be a rendering policy
this layer has no business choosing.

## Why semantic generators do not belong here

There is no `road`, `tree`, `building`, `car`, `creature`, or `terrain` in this
layer, and there must never be.

A road is a sweep of a particular profile along a particular curve. A tree is a
tapered sweep plus some lofted crowns. A car is a rounded box, some revolved
wheels and a lofted cabin. Those are **compositions**, and the composition is
exactly where the domain meaning lives — which lane width, which trunk taper,
which wheelbase. Meaning belongs to an app or a module, not to a geometry
library. Admitting one semantic generator here would make this layer the junk
drawer every future domain reaches into, and the first argument about what a
"road" is would be an argument about an engine layer.

The claim that the generic set is *sufficient* is not an assertion; it is
demonstrated. **`apps/procedural-mesh-crucible`** builds a road and a tunnel
(`road.rs`), a vehicle (`vehicle.rs`), trees (`flora.rs`), a building
(`building.rs`), terrain (`terrain.rs`), a sculpture (`sculpture.rs`), a dog
(`creature_dog.rs`) and a human (`creature_human.rs`) — assembled by
`crucible_scene` in `scene.rs` — **without a single operator being added to this
layer**. That app's own scene doc states the invariant it is proving:

> Every object in the returned vector was produced by an `axiom-mesh-ops`
> operator and validated by `axiom-mesh` on the way out of it; nothing here
> hand-writes a vertex.

If a future domain finds it *cannot* be built from this vocabulary, the correct
response is to identify the missing **generic** operator (a chamfer, a boolean,
a shell) — not to add the domain.

## Why recipe and proc-core semantics stay above it

`crates/axiom-proc-mesh` owns the recipe graph: operator codes, `Param` words,
per-node entropy, graph baking, `ProcMeshApi::bake(&RecipeGraph, seed)`. That is
a **data-driven front end to geometry**, not geometry itself. The distinction is
the same one that separates an interpreter from the functions it calls.

Exposing the algorithms here as ordinary typed functions means a recipe
interpreter, an importer, an app and a test can all reach the same code without
any of them needing to construct a `RecipeGraph`:

| Caller | What it wants |
|---|---|
| a recipe interpreter | to evaluate node `Cylinder` with params from the graph |
| a glTF importer | to weld and re-normal an imported buffer |
| an app | `cylinder(radius, half_height, segments, caps)` in one line |
| a test | to assert one operator's winding without building a graph |

If the algorithms lived behind the recipe API, three of those four would have to
fabricate a graph to reach a cylinder. The desired end state — recorded in
[`docs/mesh-convergence-migration.md`](../../docs/mesh-convergence-migration.md)
— is that `axiom-proc-mesh` keeps the recipe vocabulary and **consumes** this
layer, returning `axiom_mesh::Mesh`, so there is one implementation of each
algorithm and one representation for its result.

## Related documents

- [`TESTING.md`](TESTING.md) — what each operator family asserts, and how.
- [`../axiom-mesh/ARCHITECTURE.md`](../axiom-mesh/ARCHITECTURE.md) — the
  representation every operator here produces.
- [`../../docs/mesh-convergence-migration.md`](../../docs/mesh-convergence-migration.md)
  — what has not converged yet, and the open follow-ups.
