# Curve / Profile / Sampling / Arc-Length / Frame foundation — work manifest

Reconnaissance date: 2026-08-14. Baseline: `origin/main` = `1f22049a`.

> **Read this first.** A large part of this foundation is **already shipped on
> `origin/main`**. This manifest is a *convergence and gap-filling* plan, not a
> greenfield design. Section 2 states exactly what exists.

---

## 1. Executive recommendation

**No new layer. The whole foundation belongs in `axiom-math`, and two pieces
must move *down* into it from `axiom-mesh-ops`.**

The layer DAG does not change:

```
kernel ──▶ runtime
   │          │
   └────┬─────┘
        ▼
      math ──────▶ mesh ──────▶ mesh-ops
```

- `crates/axiom-math/layer.toml` — `depends_on = ["kernel", "runtime"]`
- `crates/axiom-mesh/layer.toml` — `depends_on = ["kernel", "math"]`
- `crates/axiom-mesh-ops/layer.toml` — `depends_on = ["kernel", "math", "mesh"]`

### Final placement, one decision per capability

| Capability | Home | Status |
|---|---|---|
| Curve representation | `axiom-math` | **exists** (`curve.rs`) — extend |
| Curve evaluation | `axiom-math` | **exists** — extend (Hermite, 2nd derivative) |
| Derivatives / tangents | `axiom-math` | **exists**, closed-form, correct |
| Profile representation | `axiom-math` | ⚠ **MOVE DOWN** from `axiom-mesh-ops` |
| Curve sampling | `axiom-math` | partial — needs a typed policy |
| Profile sampling / resampling | `axiom-math` | **does not exist** |
| Arc-length table | `axiom-math` | private + rebuilt per call — promote |
| Distance ↔ parameter conversion | `axiom-math` | **does not exist** publicly |
| Moving-frame construction | `axiom-math` | ⚠ **MOVE DOWN** from `axiom-mesh-ops` |
| Twist / bank modifier | `axiom-math` | ⚠ **MOVE DOWN** (today a `SweepOptions` field) |
| Adaptive sampling | **nowhere — deliberately deferred** | banned by `engine_no_recursion`; zero consumers |
| Closed-loop handling | `axiom-math` | **does not exist** (today a *sweep* flag) |
| Sweep / loft / revolve / extrude | `axiom-mesh-ops` | **exists**, stays |
| Triangulation, caps, UV, winding normalisation | `axiom-mesh-ops` | **exists**, stays |
| `Segments`/`Rings`/`Subdivisions`/`DetailBudget` | `axiom-mesh-ops` | **exists**, stays |

### Why no new layer

`crates/axiom-math/ARCHITECTURE.md:135-138` states the rule directly: *"If a
future system needs a primitive that does not exist here, the correct response
is to add it to this layer, not to define it in a higher layer."* Every
capability above is a total function over `Vec2`/`Vec3`/`Quat` and kernel
quantities. A layer holding only those would introduce no new *kind* of
dependency and would be exactly the "tiny ceremonial layer just to feel
organized" the Layer Rules forbid. `axiom-geosphere` (`depends_on = ["math"]`)
is a layer because it owns a **topology** — CSR adjacency, manifold validation —
that is not expressible as math primitives. Curves and profiles are.

### Why `Profile` and frames must move down

Both are pure geometry that today can only be *named* by depending on the mesh
library.

- `Profile` (`crates/axiom-mesh-ops/src/profile.rs`) is `Vec<Vec2>` + a `closed`
  flag + shoelace area + rotate/scale/reverse. There is nothing mesh in it, yet
  constructing one requires `MeshError` (`profile.rs:6`), so a UI outline, a
  physics hull, or a road cross-section must depend on marching cubes and
  quadric simplification to name a polygon.
- Frame construction (`sweep_frames.rs`) is Rodrigues rotation plus
  Gram-Schmidt over a tangent sequence. Its consumers are camera dollies (6
  apps), AI paths, animation channels, ribbon trails, `apps/dog`'s locomotion,
  and burnt-rubber's road — which feeds **physics and gameplay**, not a mesh.
  The sweep is *one* consumer of frames, not their owner.

The shipped rationale (`crates/axiom-mesh-ops/ARCHITECTURE.md:254-262`) argues a
`Curve` used for a camera path *"would inherit a sweep-shaped basis for no
reason."* That defends the **API shape** — frames must not be a method on
`Curve` — and the shipped code already got that right: it is a free function
over `&[CurveSample]`. It does not defend the **crate**, because a free function
in `axiom-math` gives `Curve` no opinion either.

The class-of-problem signal is already visible: **four independent
"orthonormal basis from one direction" implementations exist**, and one of them
is a private copy inside `axiom-physics` made *because* math's version was
unusable (pole-biased to +Y):

| Implementation | File | Degeneracy strategy |
|---|---|---|
| `geo::tangent_basis` | `crates/axiom-math/src/geo.rs:50` | length ε `1e-5` + table select, +Y-biased |
| `contact_solver::tangent_basis` | `modules/axiom-physics/src/contact_solver.rs:253` | least-aligned axis — **no degenerate case at all** |
| `perpendicular_basis` | `apps/burnt-rubber/src/render/surface_builder.rs:238` | `if axis.y.abs() > 0.9` (branchy, app tier) |
| CSS3D plane basis | `packages/axiom-web-engine/src/backend-css.ts:444` | `0.9` threshold |

Module Law #2: *"If two engine modules want to share a primitive, the primitive
belongs in a lower layer."* Physics already had to copy it.

---

## 2. Repository evidence — what exists today

### 2.1 The stale-checkout trap (read before anything else)

| Ref | Commit |
|---|---|
| local `main` | `b1d449ff` |
| `origin/main` | `1f22049a` |
| `feature/procedural-mesh` | `1f22049a` (identical) |

`git merge-base --is-ancestor 69ee2174 origin/main` → **YES**.
`git merge-base --is-ancestor 69ee2174 HEAD` → **NO**.

`ls crates/` on the local tree shows no `axiom-mesh`, and the local root
`Cargo.toml` has no mesh member. **The crates exist.** Any agent reading the
working tree cold will conclude otherwise and duplicate merged work.

**WP-00 exists solely to close this.**

### 2.2 What `69ee2174` landed

`crates/axiom-math` (+964 lines):

- `curve.rs` (787) — `Curve { kind: CurveKind, points: Vec<Vec3> }`.
  `Curve::polyline` / `cubic_bezier` / `catmull_rom` are the only constructors;
  `build` (`:230`) is the one validation gate — kind-specific point count, all
  coordinates finite, no two *consecutive* points coincident, with a **table
  index** selecting the failure message (`:245-250`), not a chain of `if`s.
- `curve_kind.rs` (63) — `Polyline=0`, `CubicBezier=1`, `CatmullRom=2`. Catmull-Rom
  is **uniform (α=0)**; centripetal is documented as a future *kind*, not a
  silent change (`curve.rs:15-21`).
- `curve_sample.rs` (106) — `CurveSample { position: Vec3, tangent: Vec3,
  parameter: Ratio, distance: Meters }`. `new` is `pub(crate)` so a sample in a
  caller's hands is always self-consistent.
- Dispatch is a `const [fn(&[Vec3], f32) -> Vec3; 3]` table indexed by
  `kind.raw()` (`curve.rs:61-65`) — **no `match` anywhere in the file**.
- Every kind has a **closed-form derivative**; no finite differencing at all.
- `MathErrorCode::InvalidCurve` added; `tests/architecture.rs` export list
  extended by 3.

`crates/axiom-mesh` (~4,700 lines) — `Mesh` (structure-of-arrays, absence = an
empty stream), `MeshStreams`, 15 error codes, `validate_streams`, `aabb`,
`bounding_sphere`, `generate_normals`, `generate_flat_normals`,
`generate_tangents`, `transform`, `reverse_winding`, `combine`, `weld`,
`remove_degenerate_triangles`, `digest`, `encode_mesh`/`decode_mesh`.
`Mesh::from_streams` is the only constructor.

`crates/axiom-mesh-ops` (~8,600 lines) — 15 primitives, ear-clipping
`triangulate_profile`, `extrude`, `sweep`, `loft`, `revolve`,
`tessellate_surface`, `heightfield_mesh`, `implicit_surface_mesh`,
`subdivide_loop`/`_midpoint`, `simplify_quadric`, plus `Profile`,
`ProfileWinding`, `SweepFrame`, `parallel_transport_frames`, `CapPolicy`,
`Segments`/`Rings`/`Subdivisions`/`Samples`/`DetailBudget`.

`docs/mesh-convergence-migration.md` (512) — inventories **seven** incompatible
CPU mesh representations with migration targets, and states plainly that
*nothing on it is done*.

### 2.3 What the spine still does not have

Confirmed by exhaustive grep across `crates/`, `modules/`, `packages/`:

- **`frenet`, `binormal`, `parallel_transport`, `rotation_minimizing`,
  `arc_length`, `nurbs`, `bspline` — zero occurrences outside `axiom-mesh-ops`.**
- No `SamplingPolicy` of any kind. `Curve::sample_uniform(count: u32)` is the
  entire vocabulary.
- No public arc-length table, no `distance → parameter`, no
  `parameter → distance`.
- No closed-curve concept. Closure is `SweepOptions::closed_path`, a *sweep*
  flag.
- No closed-loop frame seam correction (see §3.3 — this is a live defect).
- No profile resampling and no profile correspondence.
- No `Quat::slerp` (only `nlerp`, `quat.rs:245`), no `Quat::from_rotation_arc`.
- No `Curve` or `Profile` serialization or digest, though `Mesh` has all three.
- No nearest-point-on-path query anywhere in the spine.

### 2.4 The duplication the foundation absorbs

| Capability | Implementations | Where |
|---|---|---|
| Scalar `lerp` | **~24** | spine + apps + TS |
| `smoothstep` | **8** | `frame_sky.rs:411`, `ease.rs:27`, `growth/curves.rs:35`, `stride.rs:15`, `road.rs:523`, 2 texture files, `easing.ts:11` |
| Ease-curve family | **3** | `axiom-tween/src/curve.rs` (7), `axiom-animation-authoring/src/ease.rs` (4, a strict subset), `casino-games/.../easing.ts` (7, same `1.70158`) |
| Orthonormal basis from a direction | **4** | see §1 table |
| Arc-length accumulation | **4** | `bend-it` ×3, `burnt-rubber` (implicit) |
| Polyline even-spacing resample | **3** | `trajectory.rs:119`, `stroke/line.rs:160`, `shot/path.rs:77` |
| Arc-length → parameter inversion | **3** | all in `apps/bend-it` |
| Swept tube / ribbon | **6** | `meshgen.ts:281`, `road_mesh.rs`, 3 tori, wedding-ring fake |
| Cubic Bézier evaluation | **1** | `apps/bend-it/src/shot/curve.rs:33` |
| Nearest point on a path | **1** | `apps/burnt-rubber/src/track/mod.rs:262` |

A load-bearing detail: the reason nobody uses the spine's scalar lerp is
recorded in code. `apps/axiom-growth/src/curves.rs:12-15` refuses
`MathApi::lerp` (`math_api.rs:271`) because it is **fallible and handle-based** —
*"a poor fit for the tight, always-finite inner loops here."* **The new API's
hot path must be total and free-function-shaped.**

---

## 3. Existing path/curve implementations

### 3.1 Burnt Rubber — the most complete path system in the repo

**It has no spline and no control points.** The previous control-point walk was
deleted (`track/mod.rs:11-15`). A course is authored as an ordered list of road
primitives in a `.brc` DSL (`courses/burning_coast.brc`):

```rust
// apps/burnt-rubber/src/course/specification/road.rs:75-152
pub enum RoadPrimitiveSpec {
    Straight { length_m }, Turn { length_m, radius_m, direction },
    SBend { .. }, Crest { .. }, Dip { .. },
    BankTransition { .. }, LaneTransition { .. }, WidthTransition { .. },
}
```

Each exposes signal functions of a normalised fraction — `heading_rate(t) -> κ`
(rad/m), `grade(t)`, `bank_rad(t)`, `lanes(t)`, `half_width_m(t)`
(`road.rs:196-265`). The signals are conditioned (clamp → rate-limit → box
smooth; `relax`/`rate_limit`/`smooth` at `geometry/mod.rs:496-537`, with
`CORRECTION_PASSES = 6`, `BANK_SMOOTHING_PASSES = 8`) and then **forward-Euler
integrated exactly once** (`geometry/mod.rs:540-581`). This is a
clothoid-family integrated-heading construction, not Catmull-Rom.

Its frame carries **no state between samples** and therefore structurally cannot
accumulate twist:

```rust
// geometry/mod.rs:550-563
let tangent    = Vec3::new(heading.sin()*pitch.cos(), pitch.sin(), heading.cos()*pitch.cos());
let flat_right = unit_or(Vec3::UNIT_Y.cross(tangent), Vec3::UNIT_X);
let flat_up    = unit_or(tangent.cross(flat_right),   Vec3::UNIT_Y);
let (sin_b, cos_b) = bank.sin_cos();
right: flat_right.mul_scalar(cos_b).add(flat_up.mul_scalar(sin_b)),
up:    flat_up.mul_scalar(cos_b).subtract(flat_right.mul_scalar(sin_b)),
```

Arc length is **asserted, not accumulated**: `distance = i * spacing` and
`position += tangent * spacing` with `tangent` exactly unit, so the 3-D chord
between consecutive samples is exactly `spacing` (`sample_spacing = 2.0` m,
`tuning.rs:782`). The course is strictly **open** — `index_at`,
`interpolated_at`, `localise`, `progress` all clamp; there is no modulo on
distance anywhere.

`TrackSample` (`track/mod.rs:27-63`) has **14 fields** in three tiers:

- neutral curve facts — `position`, `tangent`, `distance`
- frame + derived road scalars — `right`, `up`, `heading`, `curvature`, `grade`,
  `bank`, `half_width`
- **game meaning** — `section: SectionKind`, `section_index: u16`,
  `expected_speed: f32`

Only the first tier belongs in `CurveSample`. The third must never descend
below the app.

Three functions in it are genuinely general and have no spine equivalent:

- `Track::localise(position, hint, window) -> (arc_length, lateral)`
  (`track/mod.rs:262-287`) — windowed nearest sample, then projection onto the
  local frame. The window is what keeps it `O(1)` and what stops a hairpin
  snapping the car onto the wrong lap.
- `Track::interpolated_at(distance)` (`:224-252`) — interpolates a whole frame:
  componentwise lerp + renormalise of the three axes (**not** slerp, so
  orthogonality is not preserved), `shortest_angle` for heading, nearest-take
  for discrete labels.
- `shortest_angle` (`:346`) and `unit_or` (`:356`).

### 3.2 bend-it — the only Bézier and the only arc-length inversion

`apps/bend-it/src/shot/curve.rs:33-59` — cubic Bernstein basis with an analytic
derivative. `shot/trajectory.rs:105-134` — oversample 6×, prefix-sum the chord
lengths by `fold`, then walk a cursor placing points at even arc length.
`stroke/line.rs:105-184` — the same algorithm again in 2D (`length`,
`at_length`, `resampled`). `stroke/fit.rs:26-53` — `progress_at_fraction`
inverts arc length to parameter. ~1,600 lines total, in a penalty-kick game.

**It has no golden harness and no `slice.toml`** — only 26 unit tests across
those files. That is why it is not the WP-08 target.

### 3.3 The live defect in the shipped sweep

`SweepOptions::closed_path` (`sweep.rs:80-88`) bridges the last ring back to the
first, but `parallel_transport_frames` (`sweep_frames.rs:123`) **does not know
the path is closed**. After transporting around a loop the final normal differs
from the seed by the holonomy angle, and nothing corrects it. A closed sweep
therefore has an uncorrected twist discontinuity at the seam. `grep` for
`is_closed|seam|loop_closure` in the frame module returns nothing.

**WP-07 fixes this. It is the single highest-value correctness item in the
manifest.**

---

## 4. Semantic decomposition

| Concern | Definition | Depends on |
|---|---|---|
| **Curve** | A validated kind + control points. Pure representation. Pointwise evaluation only. | `Vec3` |
| **Profile** | An ordered 2D section in a local XY plane, open or closed, with an outward orientation intent. A closed profile is mathematically a closed planar curve. | `Vec2` |
| **Sampling** | A typed, explicit *policy* turning a continuous definition into an ordered array. Deterministic; no heuristics. | `Curve`/`Profile` |
| **Arc length** | A derived numerical artifact — a cumulative table plus both inversion directions. Approximate by nature for cubics. | sampling |
| **Frames** | A stable orthonormal basis carried along a tangent sequence. Sequential, seeded, twist-free. | `Vec3`, tangents |
| **Twist** | An authored roll about the tangent, keyed on **arc length**. Content, layered on top of frames. | frames, `Radians` |
| **Mesh consumers** | Winding normalisation, seam duplication, UV assignment, capping, triangulation, tessellation budgets. | all of the above + `Mesh` |

The cut line between the bottom six and the last row is: **anything that names
a triangle, an index buffer, a UV, or a cap is mesh work.** Nothing above that
row does.

---

## 5. Lowest-layer placement proof

Applied to every capability, both directions.

**Downward test — could it move one level lower?**

`axiom-math` is the floor for all of it. Everything here is expressed over
`Vec2`/`Vec3`/`Quat`, which live in `math`, not the kernel. The kernel's own
rules admit core math *"only if it is required broadly across the engine"* —
`Vec3` itself already failed that bar. So nothing descends past `math`.

The one genuine kernel candidate is the shared scalar vocabulary already
present: `Meters`, `Radians`, `Ratio`. No new kernel type is needed.

**Upward test — would placing it higher force unrelated systems to depend on mesh/procgen?**

| Capability | If left in `mesh-ops`, who is contaminated? |
|---|---|
| `Profile` | draw2d outlines, a future physics convex hull, UI rounded rects, glyph contours — all would need the mesh library to name a polygon |
| Frames | camera dollies (6 apps), AI path following, animation channels, ribbon trails, `apps/dog` locomotion, burnt-rubber's **physics-facing** road frame, a future scatter-along-a-curve in `modules/axiom-scatter` |
| Twist | same set as frames |
| Arc-length / sampling | `apps/dog/src/locomotion.rs` already consumes `sample_uniform` for gait with **zero mesh concepts** |

**Neutrality is proven, not asserted.** `apps/dog/src/locomotion.rs:113-140`
builds walking rings from `Curve::catmull_rom` + `sample_uniform`, reading only
`position`, `tangent`, `distance`. Its module doc gives the same justification a
sweep gives: *"A spline's parameter is not proportional to its length, so a
walker advanced by parameter would speed up and slow down for no reason a viewer
could see."*

**Capabilities that correctly stay in `mesh-ops`** (they fail the upward test in
reverse — moving them down would push triangle concepts into math):

`sweep`, `loft`, `revolve`, `extrude`, `triangulate_profile`, `CapPolicy`,
`Segments`, `Rings`, `Subdivisions`, `DetailBudget`, the CCW normalisation, the
seam-vertex duplication, and both UV parameterisations.

`Samples` is the one exception in that list: it is a *curve* sampling count, not
a mesh tessellation count. It is subsumed by `SamplingPolicy` (WP-03).

---

## 6. Proposed public model

Conceptual signatures. Not implementation.

### 6.1 `axiom-math` — curve

```rust
pub enum CurveKind { Polyline = 0, CubicBezier = 1, CatmullRom = 2, Hermite = 3 }

/// Whether a closing span joins the last control point back to the first.
pub enum Closure { Open, Closed }

pub struct Curve { /* kind, points, closure — all private */ }

impl Curve {
    pub fn polyline(points: Vec<Vec3>, closure: Closure) -> MathResult<Curve>;
    pub fn cubic_bezier(points: Vec<Vec3>, closure: Closure) -> MathResult<Curve>;
    pub fn catmull_rom(points: Vec<Vec3>, closure: Closure) -> MathResult<Curve>;
    pub fn hermite(points: Vec<Vec3>, tangents: Vec<Vec3>) -> MathResult<Curve>;

    pub const fn kind(&self) -> CurveKind;
    pub const fn closure(&self) -> Closure;
    pub fn points(&self) -> &[Vec3];
    pub fn span_count(&self) -> usize;

    pub fn position_at(&self, t: Ratio) -> Vec3;
    pub fn derivative_at(&self, t: Ratio) -> Vec3;          // un-normalised dP/dt
    pub fn second_derivative_at(&self, t: Ratio) -> Vec3;
    pub fn tangent_at(&self, t: Ratio) -> MathResult<Vec3>; // unit
    pub fn curvature_at(&self, t: Ratio) -> MathResult<Ratio>;

    pub fn encode(&self, w: &mut BinaryWriter);
    pub fn decode(r: &mut BinaryReader) -> MathResult<Curve>;
    pub fn digest(&self) -> StableHash;
}
```

### 6.2 `axiom-math` — sampling policy

**The obvious shape — a data-carrying enum — is unimplementable here.** Consuming
`SamplingPolicy::Count(n)` requires destructuring, and `engine_no_branching`
bans `match` unconditionally, including exhaustive fieldless matches, with **no
escape hatch of any kind**. The repo's sanctioned form is a **fieldless
`#[repr(uN)]` enum with explicit discriminants carrying the *discriminant only*,
plus always-present fields on a carrier struct, dispatched through a `const`
table indexed by `self as usize`**. `crates/axiom-state/src/state_op_kind.rs:5-32`
states the reason verbatim: *"the payload a given kind needs lives in
`StateOp`'s always-present fields rather than in enum variants, because
destructuring a data-carrying enum is the one thing safe Rust offers no
combinator for."*

```rust
/// Which rule turns a continuous definition into an ordered array.
/// Fieldless so `self as usize` indexes the resolver table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum SamplingMode {
    /// Exactly `count` samples, spaced uniformly in the parameter domain.
    UniformParameter = 0,
    /// Exactly `count` samples, spaced (approximately) uniformly by arc length.
    UniformDistance = 1,
    /// Enough uniform-arc-length samples that no span exceeds `max_segment`.
    MaxSegmentLength = 2,
}

/// A complete, explicit, deterministic sampling request. Both fields are always
/// present; `mode` says which one is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SamplingPolicy { mode: SamplingMode, count: SampleCount, max_segment: Meters }

impl SamplingPolicy {
    pub fn uniform_parameter(count: SampleCount) -> SamplingPolicy;
    pub fn uniform_distance(count: SampleCount) -> SamplingPolicy;
    pub fn max_segment_length(max: Meters) -> MathResult<SamplingPolicy>;
    pub const fn mode(self) -> SamplingMode;
    /// Resolve to a concrete count. Table dispatch: `RESOLVE[mode as usize](..)`.
    pub fn resolve(self, table: &ArcTable) -> MathResult<SampleCount>;
}

pub struct SampleCount(u32);   // 2 ..= 65_536, validated
impl SampleCount { pub fn new(v: u32) -> MathResult<SampleCount>; pub const fn get(self) -> u32; }
```

`SamplingPolicy` derives `Eq` and `Hash` (it holds `Meters`, which wraps `f32`,
so `Eq` requires a hand-written impl over the raw bits or a `Meters`-level
decision — resolve this in WP-03). It must be a plain, diffable, serializable
value an agent can author, so the derive set is a deliberate acceptance
criterion, not an afterthought.

Note the earlier draft's `Count` and `UniformParameter` collapsed into one
variant: an explicit count *is* uniform-parameter sampling. Three modes, not
four.

### 6.3 `axiom-math` — sampled curve and arc length

```rust
pub struct CurveSample {   // exists today; unchanged
    position: Vec3, tangent: Vec3, parameter: Ratio, distance: Meters,
}

/// The immutable derived artifact. Built once, queried many times.
pub struct SampledCurve { /* Vec<CurveSample>, total: Meters, closure */ }

impl SampledCurve {
    pub fn build(curve: &Curve, policy: SamplingPolicy) -> MathResult<SampledCurve>;
    pub fn samples(&self) -> &[CurveSample];
    pub fn total_length(&self) -> Meters;
    pub const fn closure(&self) -> Closure;

    pub fn sample_at_distance(&self, d: Meters) -> CurveSample;   // total, lerped
    pub fn parameter_at_distance(&self, d: Meters) -> Ratio;
    pub fn distance_at_parameter(&self, t: Ratio) -> Meters;
    pub fn digest(&self) -> StableHash;
}

/// The cumulative table, exposed so a caller can cache and reuse it.
pub struct ArcTable { /* nodes: u32, cumulative: Vec<u64> micrometres */ }

impl ArcTable {
    pub fn build(curve: &Curve, nodes: ArcTableDensity) -> ArcTable;
    pub fn total(&self) -> Meters;
    pub fn parameter_at(&self, d: Meters) -> Ratio;
    pub fn distance_at(&self, t: Ratio) -> Meters;
    /// Stated worst-case error for this table's density.
    pub fn max_error(&self) -> Meters;
}
```

### 6.4 `axiom-math` — profile

```rust
pub enum ProfileWinding { CounterClockwise, Clockwise }

pub struct Profile { /* points: Vec<Vec2>, closure: Closure */ }

impl Profile {
    pub fn open(points: Vec<Vec2>) -> MathResult<Profile>;
    pub fn closed(points: Vec<Vec2>) -> MathResult<Profile>;
    pub fn circle(radius: Meters, count: SampleCount) -> MathResult<Profile>;
    pub fn rectangle(half_width: Meters, half_height: Meters) -> MathResult<Profile>;

    pub fn points(&self) -> &[Vec2];
    pub const fn closure(&self) -> Closure;
    pub fn edge_count(&self) -> usize;
    pub fn winding(&self) -> ProfileWinding;
    pub fn signed_area(&self) -> Meters;        // was pub(crate) + f32
    pub fn perimeter(&self) -> Meters;
    pub fn point_at_perimeter(&self, d: Meters) -> Vec2;

    pub fn reversed(&self) -> Profile;
    pub fn rotated(&self, angle: Radians) -> Profile;
    pub fn scaled(&self, factor: Ratio) -> Profile;   // was Meters — a units bug
    pub fn oriented(&self, want: ProfileWinding) -> Profile;

    pub fn encode(&self, w: &mut BinaryWriter);
    pub fn decode(r: &mut BinaryReader) -> MathResult<Profile>;
    pub fn digest(&self) -> StableHash;
}

pub struct SampledProfile { /* points: Vec<Vec2>, perimeter: Vec<Meters>, closure */ }

impl SampledProfile {
    pub fn build(profile: &Profile, policy: SamplingPolicy) -> MathResult<SampledProfile>;
    pub fn points(&self) -> &[Vec2];
    pub fn perimeter_at(&self, index: usize) -> Meters;
    pub fn total_perimeter(&self) -> Meters;
}

/// Deterministic loft compatibility: resample every profile to one shared
/// point count by normalised perimeter, from a shared seam.
pub fn correspond(profiles: &[Profile], policy: SamplingPolicy)
    -> MathResult<Vec<SampledProfile>>;
```

### 6.5 `axiom-math` — frames

```rust
/// One orthonormal station. `binormal == tangent.cross(normal)`.
pub struct Frame { position: Vec3, tangent: Vec3, normal: Vec3, binormal: Vec3 }

impl Frame {
    /// TOTAL. A zero or non-finite tangent yields the documented identity
    /// frame (t=+Z, n=+X, b=+Y) rather than failing.
    pub fn seed(position: Vec3, tangent: Vec3, reference: Vec3) -> Frame;
    /// One double-reflection transport step. Total.
    pub fn transport(self, next_position: Vec3, next_tangent: Vec3) -> Frame;
    /// Roll about the tangent. Leaves `tangent` untouched.
    pub fn with_twist(self, angle: Radians) -> Frame;
    /// Opt-in ground reference for roads/terrain, named so it is a choice.
    pub fn projected_to_up(self, up: Vec3) -> Frame;
    pub fn to_quat(self) -> Quat;

    pub const fn position(&self) -> Vec3;
    pub const fn tangent(&self)  -> Vec3;
    pub const fn normal(&self)   -> Vec3;
    pub const fn binormal(&self) -> Vec3;
}

/// Twist is plain data, not an enum — one authored angle per sample, keyed on
/// arc length by position in the slice. An **empty slice means no twist**, so
/// there is no variant to destructure and no `match` to write. A caller wanting
/// a linear ramp calls the builder below.
pub fn linear_twist(total: Radians, samples: &SampledCurve) -> Vec<Radians>;

/// The sequence. Seeds once, transports, applies twist, and — for a closed
/// path — distributes the holonomy residual by cumulative arc length.
/// `twist` is either empty or exactly `samples.samples().len()` long.
pub fn path_frames(samples: &SampledCurve, reference: Vec3, twist: &[Radians])
    -> MathResult<Vec<Frame>>;

/// Frame at an arbitrary distance: slerp of the frame rotations, so
/// orthonormality is preserved.
pub fn frame_at_distance(frames: &[Frame], samples: &SampledCurve, d: Meters) -> Frame;

/// Windowed nearest point. Returns (arc length along, signed lateral offset
/// along the frame's normal).
pub fn locate(frames: &[Frame], samples: &SampledCurve,
              point: Vec3, hint: Meters, window: Meters) -> (Meters, Meters);
```

### 6.6 `axiom-math` — the sibling primitives this exposes as missing

```rust
impl Quat {
    pub fn slerp(self, other: Quat, t: Ratio) -> MathResult<Quat>;
    pub fn from_rotation_arc(from: Vec3, to: Vec3) -> MathResult<Quat>;
}
```

Both are currently re-implemented per-app in Rust **and** TypeScript
(`apps/end-zone/src/presentation/locomotion/leg.rs:61`,
`apps/end-zone/src/football/model.rs:11`,
`apps/axiom-home-run/web/src/figure-math.ts:81`).

---

## 7. Curve mathematics

### 7.1 Parameter domain — settled

- `t` is a `Ratio` spanning the **whole** curve, `0 ..= 1`, split **evenly
  across spans**. A straight span and a tight curl consume the same `t`. This
  is deliberate and already documented (`curve.rs:7-13`); arc-length uniformity
  is what `SampledCurve` restores.
- Span mapping is `scaled = t * span_count`, `index = floor(scaled)` clamped to
  the last span, `u = scaled - index` (`span_of`, `curve.rs:88-92`).
- Segment-local `u` is **not** separately exposed. No consumer asked for it, and
  exposing it would fix the even-split convention in the public API forever.
- Out of range **clamps, never extrapolates** (`curve.rs:320,325`). This
  deliberately diverges from `MathApi::lerp` (`math_api.rs:271`), which
  extrapolates by design; the doc comment must say so.
- `t` is dimensionless — `Ratio`.
- Arc length is `Meters`.
- Closed curves: `Closure::Closed` adds one closing span. `t = 1` returns to the
  start point exactly. Distance beyond the total **wraps** by `rem_euclid` for a
  closed curve and **clamps** for an open one — the two policies are selected by
  the stored `Closure`, never inferred from `first ≈ last`. Inference is both a
  branch and a guess.

### 7.2 The four kinds

| Kind | Point rule | Span count | Basis |
|---|---|---|---|
| `Polyline` | `n >= 2` | `n - 1` (`n` if closed) | linear |
| `CubicBezier` | `3k + 1`, `k >= 1` | `k` | cubic Bernstein |
| `CatmullRom` | `n >= 4` | `n - 3` | uniform (α = 0) |
| `Hermite` | `n >= 2` points + `n` tangents | `n - 1` | cubic Hermite |

**Hermite is included on evidence, not for completeness.** It is the only kind
that accepts an *authored tangent*, which is exactly what a keyframe track needs
(`modules/axiom-animation/src/track.rs:53` interpolates poses and has no
tangent control) and what an importer needs for glTF `CUBICSPLINE`. It is also
the basis Catmull-Rom is defined *in terms of*, so the two share one evaluator.

**NURBS and B-splines are excluded.** Zero occurrences repo-wide, zero
consumers, and no demonstrated need. Centripetal Catmull-Rom (α = 0.5) is
reserved as a future *kind*, never a silent change to the existing one.

### 7.3 Derivatives

- **Analytical for every kind.** No finite differencing anywhere. The shipped
  code already does this correctly (`bezier_derivative`, `catmull_derivative`,
  `polyline_derivative`), and burnt-rubber independently reached the same
  conclusion (`geometry/mod.rs:547-549`: *"a central difference across a clamped
  signal is neither [unit nor continuous]"*).
- Second derivative added for `curvature_at`; polyline's is the zero vector,
  which is correct and makes curvature zero.
- `tangent_at` normalises and surfaces a vanishing derivative as
  `MathErrorCode::NormalizeZeroLength` — reported by the primitive that
  discovered it rather than relabelled. Keep this.
- **At an interior polyline joint the *outgoing* segment is chosen**, so a
  corner's tangent is its outgoing direction (`curve.rs:100-103`). Documented,
  deliberate, keep.
- At a Bézier joint the two segments' derivatives generally differ; the span
  index selects the later one at the exact boundary. A `C1`-continuous chain is
  the author's responsibility, not the primitive's.
- Construction rejects **coincident consecutive** control points, so a polyline
  derivative can never vanish. A spline whose control points double back *within
  one span* can still vanish — that is a genuine undefined direction and is an
  error, not a fallback (`curve.rs:564-578` pins it).

---

## 8. Sampling model

### 8.1 The vocabulary, and what is deferred

Ship: `Count`, `UniformParameter`, `UniformDistance`, `MaxSegmentLength`.

**Defer adaptive error-tolerance subdivision.** Three independent reasons:

1. **Zero consumers.** No adaptive subdivision exists anywhere in the repo.
2. **`engine_no_recursion` bans it** (`tools/lints/engine_no_recursion/src/lib.rs:64`),
   and `docs/unbranching.md:47-48` closes the door: *"Recursion is banned, so a
   loop with no iterator form is irreducible."* Recursive flatness subdivision
   has no iterator form.
3. **The Coverage Law makes data-dependent depth expensive** — every depth arm
   must be reachable by a test.

`MaxSegmentLength` covers the real need (bounded chord error) with a
**closed-form** count. The one precedent for error-driven density in the repo is
also closed-form: `apps/axiom-growth/src/gameworld.rs:176-190` derives an octave
count from a feature-size target with a single `log2`, no recursion.

`SamplingPolicy` is a plain enum with no `f32` payload other than dimensioned
quantities. **There is no `quality: f32` anywhere.**

### 8.2 Determinism

- A sample array is a pure function of `(Curve, SamplingPolicy)`. Same inputs →
  byte-identical output. Already pinned for the existing sampler
  (`curve.rs:691-696`).
- `MaxSegmentLength` resolves its count from the **arc-length table**, which is
  itself a pure function of the curve, so the resolved count is reproducible.
- The count is clamped to `SampleCount`'s `2..=65_536` range, so an absurd
  request fails rather than allocating unboundedly.

---

## 9. Arc-length model

### 9.1 Exactness

| Kind | Exact closed form? |
|---|---|
| `Polyline` | **Yes** — the sum of chord lengths |
| `CubicBezier` | **No.** The arc length of a general cubic is an elliptic integral with no elementary closed form |
| `CatmullRom` | **No** — same reason |
| `Hermite` | **No** — same reason |

**The engine standardises on deterministic numerical approximation for all four
kinds uniformly.** A per-kind split (exact for polyline, approximate elsewhere)
would make the error semantics kind-dependent and the goldens kind-dependent.
Uniform treatment costs nothing on a polyline: with nodes placed at span
boundaries the chord sum *is* exact there.

### 9.2 The table

Chord-length prefix sum over a dense, evenly-parameterized node grid. Density
today: `ARC_TABLE_DENSITY = 16` chords per requested sample, clamped to
`64..=8192` nodes (`curve.rs:48-54`). Keep those constants; promote the table to
a public `ArcTable`.

**Accumulate in fixed-point integer micrometres (`u64`), not `f32`.** Two
reasons, both grounded in existing repo practice:

- `f32` prefix sums are not associative, so the total depends on accumulation
  order, and a byte-golden over a sample array becomes fragile.
- The repo already does exactly this where determinism matters:
  `crates/axiom-entropy/src/entropy_stream.rs:90-94` keeps its cumulative table
  integer-only for this reason, and
  `modules/axiom-physics/src/physics_world.rs:502-515` divides a total into `N`
  pieces by integer division plus remainder distribution so the pieces sum
  **exactly** to the total.

Convert to `Meters` only at the public boundary.

### 9.3 Inversion

**Primary: `partition_point` + in-bucket linear interpolation.** The template
already exists — `modules/axiom-animation/src/track.rs:57` is the repo's only
`partition_point` and is structurally identical (locate the bracketing pair,
clamp the index, lerp the fraction).

This replaces the shipped implementation's `table.iter().position(|&d| d >= target)`
(`curve.rs:365-368`), which is an **O(nodes) linear scan per sample** — so
`resample` is O(count × nodes), up to 2000 × 8192 ≈ 16M comparisons for a large
request. That is a real performance defect, not a style point.

**Never fixed-iteration Newton.** It has no error bound without a convergence
test, and a convergence test is a branch the Branchless Law forbids.

### 9.4 Error semantics, stated

- A chord sum **under-measures** a curve and converges from below
  (pinned at `curve.rs:598-604`).
- Documented bound: for a table of `N` uniform nodes over a curve whose maximum
  curvature is `κ`, the relative chord-vs-arc error per span is
  `O((κ·L/N)²/24)`. At the default `64..=8192` nodes this is below `1e-4`
  relative for any curve a game authors. **State this number in the doc
  comment and pin it with a test** comparing `nodes = N` against `nodes = 4N`.
- `ArcTable::max_error()` returns this bound so a caller can reason about it.

### 9.5 Wrapping

- Open: `sample_at_distance` clamps to `[0, total]`.
- Closed: wraps by `d.rem_euclid(total)`.
- Selected by the stored `Closure`, branchlessly by table index.

---

## 10. Profile model

### 10.1 Semantics

- **Open and closed are both required and both first-class.** Closed: every ring
  generator (cylinder, capsule, cone, torus, swept tube). Open: burnt-rubber's
  road cross-section (verge → shoulder → tarmac → shoulder → verge is a strip
  that terminates), gravix's half-pipe, every `Stroke`, every open `Path2d`.
- **Never conflate two meanings of "closed."** `Closure::Closed` on a profile
  means the point ring wraps. `MeshData.closed`
  (`packages/axiom-web-engine/src/api.ts:68`) means the *solid* is watertight.
  A closed profile swept along an open path is not a closed solid unless capped.
- **Winding is measured and reported, never trusted from the author.** This is
  the repo's hard-won answer. `Profile::winding()` reports; `oriented(want)`
  normalises. Consumers derive triangle winding from an explicit outward intent,
  the way `SurfaceBuilder::quad_with_uvs`
  (`apps/burnt-rubber/src/render/surface_builder.rs:76-102`) already does — it
  takes a facing direction, tests `cross(b−a, c−a)·facing`, and reverses the
  cycle when negative, carrying the UVs through the reversal (pinned at
  `:317-351`). The cost of trusting author order is documented:
  `modules/axiom-resources/src/plane_mesh.rs:13-23` records that a winding
  disagreeing with its authored normals lit **every ground plane in every app**
  from below on the Canvas2D backend.
- **Perimeter parameterisation belongs on `Profile`.** Today it lives inside
  `sweep.rs::column_arc` (`:286-299`), which is why nothing but the sweep can
  use it.
- **Holes are excluded.** Zero consumers repo-wide — verified exhaustively. No
  multi-contour, sub-path, or nonzero-vs-evenodd selection exists in any Rust
  geometry code; physics has no polygon shape at all; the only two "holes" in
  the repo are a CSS `clip-path` and a texture op.
- **Per-point normals are excluded from v1, deliberately, with a named
  alternative.** Three different derivation rules are live today (analytic
  radial, caller-supplied closure, wall-vs-cap arithmetic select), and
  `apps/burnt-rubber/src/render/prop_meshes.rs:82-88` deliberately stores a
  *false* normal so palm fronds do not go black in a low sun. The repo's
  existing way to express a hard corner is **duplicating the point at the
  seam** (`cylinder_mesh.rs:48-51`, `meshes.ts:164-166`), which a plain point
  list already supports. Adding an `Option<Vec2>` normal per point is a
  followable extension; adding a corner *classifier* is not — it has zero
  consumers and would need a smoothing-angle policy nobody has asked for.
- **Per-point material tags and UV coordinates are excluded from the profile.**
  They are real requirements (`ChunkMeshes` splits one road profile into four
  meshes; `paving_uvs` keys on lateral **metres**), but they are consumed only
  by the mesh tier. The neutral primitive supplies **perimeter distance in
  `Meters`**; `mesh-ops` derives `u` from it, exactly as it derives `v` from
  `CurveSample::distance` today (`sweep.rs:22-41`).

### 10.2 Validation

Enough points (`>= 2` open, `>= 3` closed), all finite, no duplicate
neighbours beyond `PROFILE_EPSILON = 1e-6`, and for a closed profile a non-zero
enclosed area and a non-degenerate closing edge. All four already exist in
`profile.rs:186-217` and are correct — they move down unchanged.

**Self-intersection is not checked.** It is `O(n²)` in the general case, no
consumer requires it, and `triangulate_profile`'s ear clipping already fails
loudly on a polygon it cannot triangulate.

### 10.3 Correspondence for lofting

The shipped `loft` requires identical point counts and hard-errors otherwise
(`IncompatibleProfiles`, `loft.rs:98-113`). Its stated rationale is right and
must be preserved: index correspondence is *"the only correspondence rule that
is stateless and reproducible"*, and heuristic rematching would make topology
depend on geometry so a small edit could re-thread the whole surface.

**So `correspond` is a separate, explicit, caller-invoked step — never
automatic inside `loft`.** It resamples every profile to one shared point count
by **normalised perimeter distance** from a shared seam:

- **Seam** for a closed profile is **index 0**, always. Not nearest-point, not
  a feature match. The author placed it.
- **Winding** is normalised to a single target before resampling; that is the
  one property fixable without guessing at correspondence, and `loft` already
  does it.
- **Corner preservation is explicitly out of scope for v1.** Uniform perimeter
  resampling will round a sharp corner that does not land on a sample. The
  named alternative for an author who needs a corner kept is to place a control
  point there — which is what index correspondence already honours.
- Sophisticated shape matching (feature detection, optimal seam rotation,
  minimal-twist correspondence) is a **future** capability with zero current
  consumers, and must not be smuggled in.

---

## 11. Moving-frame model

### 11.1 Algorithm — **double-reflection rotation-minimizing frames**

(Wang, Jüttler, Zheng & Liu 2008, *ACM TOG* 27(1).)

Chosen over the shipped Rodrigues formulation (`sweep_frames.rs:204-218`) for
four concrete reasons:

1. **No transcendentals.** Only `+ − × ÷`, dot, cross. The Rodrigues version
   uses `atan2`, `sin`, `cos` (`:213`, `:222`), which
   `engine_no_unportable_float` bans inside any `#[sim]` zone
   (`tools/lints/engine_no_unportable_float/src/lib.rs:82-86`). A road frame
   feeding deterministic simulation needs the portable subset.
2. **Second-order accurate** vs. the rotation method's first order — better
   frame quality at the same sample count.
3. **Better conditioned exactly where Rodrigues is worst.** Rodrigues' axis
   `cross(t_i, t_{i+1})` vanishes at *both* the identity case *and* the 180°
   reversal, and the gate at `sweep_frames.rs:211` cannot tell them apart.
   Double reflection's second denominator is **maximal (≈4) at reversal** and
   zero **only** at the identity — the benign case.
4. Fewer floating-point operations per step.

**Per step.** Given frame `(x_i, t_i, r_i)` advancing to `x_{i+1}` with unit
tangent `t_{i+1}`:

```
Reflection 1 — bisecting plane of the two points
  v1  = x_{i+1} − x_i
  c1  = v1 · v1
  rL  = r_i − (2/c1)(v1 · r_i) v1
  tL  = t_i − (2/c1)(v1 · t_i) v1

Reflection 2 — bisecting plane of tL and t_{i+1}
  v2  = t_{i+1} − tL
  c2  = v2 · v2
  r_{i+1} = rL − (2/c2)(v2 · rL) v2
  s_{i+1} = t_{i+1} × r_{i+1}
```

Two reflections compose to a rotation, so the result is exactly the minimal-twist
rotation carrying `t_i` onto `t_{i+1}`.

**Branchless degeneracy.** Each division uses one multiplicative gate:

```
gate = f32::from(u8::from(c > EPS));
divisor = [1.0, c][usize::from(c > EPS)];
term_scale = gate / divisor;      // 0 when degenerate → the reflection is skipped exactly
```

Two gates, two reachable arms each — four tests give 100% coverage with no dead
arm. This is recipe 2 from `docs/unbranching.md`, already used at
`sweep_frames.rs:211-212`.

Close each step with Gram-Schmidt re-orthogonalisation and a **total**
normalise (`v * (1.0 / v.length().max(f32::MIN_POSITIVE))`, the pattern at
`modules/axiom-physics/src/contact_solver.rs:271-272`) so drift cannot make the
basis skew over thousands of samples.

### 11.2 Seeding

The **least-aligned world axis**, Gram-Schmidt'd against `t₀`: score
`[|t.x|, |t.y|, |t.z|]`, take the first minimum via a branchless `fold` argmin.
A unit vector cannot be within ~54.7° of all three axes, so the perpendicular is
always well-conditioned — **no threshold, no fallback, no degenerate case.**
This is exactly `contact_solver.rs:253-266` and `sweep_frames.rs:172-176`; both
already agree, and this is the rule the four duplicates collapse onto.

A caller-supplied `reference` is orthogonalised against `t₀` and used when its
residual exceeds ε. `Vec3::ZERO` is the honest way to say *"no preference."* A
**non-finite** reference is an error, not a request for the fallback — keep the
shipped behaviour (`sweep_frames.rs:114-122`).

### 11.3 Every edge case, with a stated policy

| Case | Policy |
|---|---|
| Zero / near-zero tangent | Not the frame's problem. `Frame::seed` is **total**: a zero or non-finite tangent yields the documented identity frame (`t=+Z, n=+X, b=+Y`). This is `engine_no_unwrap_or`'s sanctioned remedy — push the named default down behind a signature that says what it means. Coincident points are absorbed by gate 1 (`c1 ≈ 0` → frame carried unchanged). |
| Nearly straight segments | The *correct* case. `c2 ≈ 0` → gate 2 = 0 → frame carried through exactly unchanged. A straight run yields a constant cross-section. |
| Inflection points | **A non-event.** RMF never consults curvature, so there is nothing to flip. This is the strongest single argument against Frenet. |
| Fully vertical paths | **A non-event.** There is no world-up anywhere in the algorithm. A vertical climb transports like any other segment — the loop-the-loop case today's `Vec3::UNIT_Y.cross(tangent)` cannot express. |
| 180° tangent reversal | Best-conditioned case (`c2 ≈ 4`); no special handling. The normal is carried and the binormal flips with the tangent, which is geometrically correct. A **true cusp** is a tangent discontinuity where no method can define a continuous frame: `path_frames` fails with `FrameDiscontinuity` when a span's turn exceeds a stated threshold (150°). A cusp is a topology fact, not a numerical one. |
| Closed loops | After transporting around, `r_N ≠ r_0`. The mismatch is the holonomy angle `θ = atan2((r_0 × r_N) · t_0, r_0 · r_N)`. Distribute as an added twist `−θ · (s_i / S)` where `s_i` is **cumulative arc length** and `S` the total. **Arc length, not parameter** — uniform-in-parameter concentrates visible twist wherever sampling is dense. State the tradeoff: a closed frame is no longer strictly rotation-minimizing; it carries a constant twist rate `−θ/S`. That is unavoidable unless `θ = 0`. |
| Accumulated twist | **Zero by construction** — that is the definition of rotation-minimizing. The only accumulation is float drift, bounded per step by the re-orthogonalise, and it does not compound into twist. |
| Explicit banking / twist | A **separate later stage**: `Frame::with_twist(Radians)` rotates `(n, b)` about `t`. Composes with the seam correction (both are twists) and with per-sample authored roll. This is what burnt-rubber already does (`geometry/mod.rs:562-563`), but on a stable base instead of a world-up base. |
| Start frame | §11.2. Pure function of `t₀` ⇒ deterministic and replayable. |
| End frame | Open path: none — the last frame is the last transport. Closure correction applies **only** when `Closure::Closed` is declared, never inferred from `first ≈ last`. |

### 11.4 Rejected alternatives, on the record

- **Frenet**: needs a second derivative; normal is **undefined at zero
  curvature** (every straight segment — most of a race track) and **flips 180°
  at every inflection**. Non-starter.
- **Fixed-up**: what the repo does today, everywhere, with four different
  thresholds (`0.9`, `0.99`, `1e-5`, `1e-6`). Undefined exactly when `t ∥ up`;
  cannot express a vertical climb. **Survives only as the opt-in, explicitly
  named `Frame::projected_to_up(up)`** for ground-referenced content, where
  "up" is a genuine physical constraint rather than a mathematical crutch.

### 11.5 Architectural verdict

**Split, but cut differently from the shipped code.**

- `Frame` (the value type), `seed`, `transport`, `with_twist`,
  `projected_to_up`, `to_quat`, plus the sequence builder `path_frames` and the
  queries `frame_at_distance` / `locate` → **`axiom-math`**.
- Winding normalisation, seam-vertex duplication, profile placement, capping,
  UV assignment → **`axiom-mesh-ops`**. The shipped code is right about *this*
  half.

---

## 12. Determinism and serialization

Requirements, derived from what `axiom-mesh` already does (it has `digest`,
`encode_mesh`, `decode_mesh`; `Curve` has none of the three — an inconsistency
inside one change).

| Type | `Debug/Clone/PartialEq` | `Eq/Hash` | Binary codec | `StableHash` digest |
|---|---|---|---|---|
| `Curve` | yes | no (holds `f32`) | **yes** | **yes** |
| `CurveKind`, `Closure`, `ProfileWinding` | yes | **yes** | via discriminant | — |
| `SamplingPolicy`, `SampleCount` | yes | **yes** | **yes** | — |
| `Profile` | yes | no | **yes** | **yes** |
| `CurveSample`, `Frame` | yes | no | no | — |
| `SampledCurve`, `SampledProfile` | yes | no | no | **yes** |
| `ArcTable` | yes | no | no | — |

Conventions to follow, all with existing precedent:

- **No `serde`.** Verified: zero `#[derive(Serialize)]`/`#[derive(Deserialize)]`
  in `crates/**/src` + `modules/**/src`. The only serde in the workspace is
  `serde_json` in `axiom-crypto` (JWT payloads) and xtask's manifest parsing.
- **Implement the kernel's `Reflect` trait** (`crates/axiom-kernel/src/reflect.rs:10-27`)
  — `const SCHEMA: TypeSchema`, `reflect_write(&self, &mut BinaryWriter)`,
  `reflect_read(&mut BinaryReader) -> KernelResult<Self>`. There is **no derive
  macro**; ~40 hand-written impls exist. `Meters` itself is one
  (`meters.rs:54-66`).
  **Prefer a `Reflect` impl over inherent `encode`/`decode` methods.** A public
  inherent `fn encode(&self, w: &mut BinaryWriter)` takes a public `&mut T` and
  is a `mutable-engine-api` finding under the State Law (§13.1); a trait-impl
  method is the established shape and is what every existing serializer uses.
- `StableHash::of_words` (`crates/axiom-kernel/src/stable_hash.rs`, FNV-1a) over
  the canonical byte encoding, as `mesh_digest.rs` does. It is *"a diagnostic
  index, never the proof"* — byte equality remains the source of truth.
- Decode failure wraps a `KernelError` as `MathErrorCode::DeserializationFailed`
  (`math_error.rs:80`). Sequential reads chain via nested `and_then` + `map_err`,
  never `?` — the model is `crates/axiom-math/src/aabb.rs:146-157`.
- **No `impl From<XError> for YError` anywhere in the spine.** `From` exists to
  power `?`, which is banned. Each boundary writes an explicit
  `.map_err(|cause| Upper::with_lower(msg, cause))`.
- Derive sets follow the measured house convention: a float-bearing value type
  gets `#[derive(Debug, Clone, Copy, PartialEq)]` (no `Eq`/`Ord`/`Hash`); a
  fieldless enum or id newtype gets
  `#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]` +
  `#[repr(uN)]` + explicit discriminants; an owned collection type gets
  `#[derive(Debug, Clone, PartialEq)]`.

This is what makes the foundation suitable for deterministic replay, golden
fixtures, generated-data caching, agent editing, and regression diffing.

---

## 13. Error model

### 13.1 First — the State Law, which constrains every signature below

`engine_no_retained_state` is a **zero-tolerance** lint deliberately absent from
`dylint-baseline.txt`, so the gate allows zero findings. The engine has **787
existing violations** inventoried in `docs/audits/retained-state-audit.md` —
a migration work list, explicitly *not* an allowed baseline. **The dylint gate
therefore fails today regardless of your change.** Judge a change by whether it
*adds* findings, not by the exit code.

Every new signature in this manifest must be clean. The categories that bite
here:

| Category | Forbidden on a public boundary | Consequence for this design |
|---|---|---|
| `mutable-engine-api` | `&mut T`, `&mut self`, `-> &mut T` (incl. through `Option`/slices/tuples) | No builder that mutates in place. Serialization goes through a `Reflect` **trait impl**, not an inherent `encode(&mut BinaryWriter)`. |
| `stateful-callback-boundary` | `dyn Fn`/`FnMut`/`FnOnce`, `impl Fn*` | **No callback-shaped sampling API.** A plain `fn(..) -> ..` pointer *is* legal — which is exactly what the `const` dispatch table needs. |
| `generic-behavior-state` | a public generic bounded by `Fn`/`Future` | Rules out `sample_with<F: Fn(Ratio) -> Vec3>`. Data generics stay legal. |
| `interior-mutability` | `Cell`, `RefCell`, `OnceCell`, `Mutex`, `RwLock`, every atomic | **No cached arc-length table inside `Curve`.** `ArcTable` must be an explicit value the caller owns — which §14 already requires. |
| `shared-state-ownership` | `Rc`, `Arc`, `Weak` | Sampled artifacts are owned `Vec`s, never shared handles. `Box`/`Vec` are fine. |
| `static-storage` | every user `static`, **including immutable ones** | Dispatch tables and epsilons must be `const`, never `static`. |
| `stateful-trait-implementation` | engine-defined `Future`/`Iterator` impls | No `CurveSampler: Iterator`. Return `Vec<T>` or `impl Iterator` (the latter is not currently flagged). |
| `drop-side-effect-hole` | a custom `Drop` | None needed. |

The prescribed shape is exactly what this manifest already proposes:
`fn f(state: &S, input: &I) -> S`.

**One trap:** `SourceHygieneStateLawSuppression` scans **raw** text — comments
included — for the literals `engine_no_retained_state`, `allow(warnings)` and
`expect(warnings)`. A doc comment that names the lint fails the architecture
checker. The same applies to `coverage(off)`.

Two further signature rules from the same sweep:

- **`.unwrap()` is banned at zero** (`no_unwrap_in_engine`). `.expect("<why it
  cannot fail>")` is the sanctioned escape hatch — 157 uses in the spine — *"because
  it documents the invariant at the call site."* (That lint's help text
  recommends `?`, which `engine_no_branching` bans; only the `.expect` half is
  actionable.)
- **Free functions must be reachable at the crate root.** The public-path check
  is a shape heuristic: `prefix::module::item` reads as a `PrivatePathImport` at
  every consumer. So `correspond`, `path_frames`, `frame_at_distance`,
  `locate` and `linear_twist` must all be `pub use`d from `lib.rs`.

Size budgets, exact: **1000 lines/file**, **120 lines/function**, **24
fields/struct**, **24 variants/enum**, **30 items/impl block**. `axiom-math`'s
`Mat4` impl block is already one of the two grandfathered
`engine_no_large_impl_blocks` findings — **do not grow it**, and do not put
curve constructors on `MathApi` (616/1000 lines).

### 13.2 Error codes

Extend `MathErrorCode` (`math_error_code.rs`). Discriminants are
`#[repr(u16)]`, sparse, and never reused — the kernel's registry skips 8–12 for
retired codes, and math must follow the same discipline.

| Condition | Code | Hard error? |
|---|---|---|
| Too few control points for the kind | `InvalidCurve` (10, exists) | **yes** |
| Bézier point count not `3k+1` | `InvalidCurve` | **yes** |
| Non-finite control point | `InvalidCurve` | **yes** |
| Coincident consecutive control points | `InvalidCurve` | **yes** |
| Hermite tangent count ≠ point count | `InvalidCurve` | **yes** |
| Derivative vanishes at a sample | `NormalizeZeroLength` (2, exists) | **yes** — a genuinely undefined direction |
| Sample count out of `2..=65_536` | `InvalidSampleCount` (**new**) | **yes** |
| `MaxSegmentLength` ≤ 0 or non-finite | `InvalidSampleCount` | **yes** |
| Arc-table density out of range | `InvalidSampleCount` | **yes** |
| Profile: too few points / non-finite / duplicate neighbours / zero area | `InvalidProfile` (**new**) | **yes** |
| Loft sections disagree on count or closure | *stays* `MeshErrorCode::IncompatibleProfiles` | **yes** — a mesh-tier concern |
| Span turn exceeds the cusp threshold | `FrameDiscontinuity` (**new**) | **yes** |
| Non-finite frame reference | `DegenerateAxis` (**new** in math) | **yes** |
| Zero / parallel frame reference | — | **no** — a well-formed request for the deterministic fallback |
| Zero-length tangent at `Frame::seed` | — | **no** — total, yields the documented identity frame |
| Collinear consecutive tangents | — | **no** — the frame is carried unchanged; this is the *correct* result |
| Distance outside `[0, total]` | — | **no** — clamps (open) or wraps (closed) per `Closure` |
| `t` outside `[0, 1]` | — | **no** — clamps |
| Self-intersecting profile | — | **not checked** — see §10.2 |

**Threading `Result` without `?`.** The Branchless Law bans `?`. The shipped
code's pattern is the model and must be followed:
`cond.then_some(v).ok_or_else(|| MathError::…).and_then(|v| …).map(|v| …)`
(`curve.rs:251-254`, `308-315`). Note that `.unwrap_or(..)` is now **banned**
(`engine_no_unwrap_or`, added 2026-08-14) — `unwrap_or_else`,
`unwrap_or_default`, and `map_or` are not.

---

## 14. Performance model

Intended lifecycle:

```
authoring description   (Curve, Profile, SamplingPolicy — small, immutable, serializable)
        │
        ▼   sample once
derived artifact        (SampledCurve, ArcTable, Vec<Frame>, SampledProfile)
        │
        ▼   query many times
consumers               (sweep, locomotion, camera, AI, localisation)
```

| Operation | Cost | Precompute? |
|---|---|---|
| `position_at` / `derivative_at` | O(1), ~10 flops | no |
| `ArcTable::build` | O(nodes) evaluations, 64–8192 | **yes — the expensive one** |
| `SampledCurve::build` | O(nodes + count·log nodes) | **yes** |
| `parameter_at_distance` | O(log nodes) with `partition_point` | no |
| `path_frames` | O(count), one pass, no transcendentals | **yes** |
| `locate` | O(window / spacing) — bounded by the window, not the path | no |
| `Profile::perimeter` | O(n) | cache in `SampledProfile` |
| `correspond` | O(sections · count) | **yes** |

Rules:

- **No caching, no interior mutability, no retained state in the primitives.**
  `engine_no_retained_state` is zero-tolerance and deliberately absent from the
  baseline. A pure `fn(&[Sample], seed) -> Vec<Frame>` is legal; a stateful
  `FrameWalker` with `&mut self` is not.
- **The arc-length table is an explicit derived artifact the caller owns.** This
  is the fix for the shipped `sample_uniform`, which rebuilds the table on every
  call and cannot be reused.
- `SampledCurve`, `SampledProfile`, `ArcTable` and `Vec<Frame>` are immutable
  once built and carry no `&mut` on their public boundary — the same rule
  `axiom-mesh` follows.
- **Fix the O(n·m) inversion.** `curve.rs:365-368`'s linear scan makes
  `resample` O(count × nodes); `partition_point` makes it O(count × log nodes).

---

## 15. Existing integration target

**`apps/burnt-rubber` — specifically `Track`'s sampled-path queries, not its
course compiler.**

Why it and not `bend-it`, which has more curve mathematics:

| | burnt-rubber | bend-it |
|---|---|---|
| Byte-exact goldens | **15 `.bin` artifacts**, 5 checkpoints | none |
| `slice.toml` SHA pins | **yes**, enforced by `cargo xtask check-slices` | none |
| Behavioural pins | 25 + 17 + 9 + 17 + 12 in-module tests | 26 unit tests |
| Proves | sampled-path + frames + localisation | `Curve` + arc-length + resampling |

Equivalence can be *proven* for burnt-rubber and only *argued* for bend-it. So
burnt-rubber is WP-08 and bend-it is migration candidate #1 in §16.

**The smallest slice — four functions, no course-compiler change:**

1. `Track::interpolated_at` → `frame_at_distance` + app-side scalar lerp for the
   road metadata. **Behaviour change to accept deliberately:** the engine slerps
   the frame rotation instead of lerping three loose vectors, so orthogonality
   is preserved where today it is not (`track/mod.rs:234-239`). This will move
   goldens; see WP-11.
2. `Track::localise` → `locate`.
3. `Track::index_at` → `SampledCurve::sample_at_distance`'s index step.
4. `shortest_angle` and `unit_or` (`track/mod.rs:346-361`) → `axiom-math` free
   functions, deleting the app copies.

**What must stay in the app, permanently:** the `.brc` DSL, the eight road
primitives, the signal conditioning (`relax`/`rate_limit`/`smooth`), the
forward-Euler integrator, `SectionKind`, `section_index`, `expected_speed`,
lanes, banking-as-a-road-concept, barriers, and every consumer in
`render/road_mesh.rs`. **No road vocabulary enters the engine.**

**What is explicitly NOT migrated:** the course compiler. Burnt Rubber has no
spline and replacing its integrator with a `Curve` would be a rewrite, not a
migration.

---

## 16. Future consumers

Ranked by code deleted. **None of these is an implementation requirement.**

1. **`apps/bend-it`** — ~450 lines (`trajectory.rs:105-206`, `stroke/line.rs:160-184`,
   `stroke/fit.rs:26-53`), plus `BendCurve` becomes a `Curve`.
2. **`crates/axiom-proc-mesh` → `axiom-mesh-ops`** — the largest duplicated
   algorithm set; also propagates a live fix (proc-mesh's marching cubes emits
   inward-facing triangles, mesh-ops' does not). Already the first item in
   `docs/mesh-convergence-migration.md`.
3. **The four `tangent_basis` duplicates** collapse onto `Frame::seed`.
4. **`axiom-tween` + `axiom-animation-authoring`** — one canonical ease family
   deletes `ease.rs` outright (a strict subset duplicate).
5. **Three torus generators + `segmentedAppendage`** (`meshgen.ts:250,281`,
   `sky-drop`, `three-point`) → `sweep`/`revolve`.
6. **`crates/axiom-hydrology` river centrelines** — the highest-value *new*
   capability. Rivers are a receiver-pointer chain plus a scalar field today
   (`drainage.rs:26,49`) and **no centreline polyline is extracted anywhere**.
   Threshold flow → trace chains → `Curve::polyline` → width-from-flow →
   `sweep`.
7. **`modules/axiom-scatter`** — area-only today; `scatter_along(curve, spacing)`
   needs frames to orient instances.
8. **`modules/axiom-draw2d`** — both rasterisers re-derive per-edge direction and
   neither has miter joins, so a stroked polyline has visible corner gaps.
9. **Camera rigs across six apps** — four `CameraPose::lerp` copies.
10. **`modules/axiom-grid`** — `GridApi::path` returns a raw lattice `Vec<Cell>`;
    smoothing + even-arc-length resampling makes it steerable.

---

## 17. Concrete work manifest

Shared prerequisites for every package: WP-00.

---

### WP-00 — Sync the checkout and establish the baseline

- **Location:** repo tooling. No engine code.
- **Files:** none created. Local `main` fast-forwarded to `origin/main`.
- **Prerequisites:** none.
- **Responsibility:** `git fetch origin && git merge --ff-only origin/main`.
  Confirm `crates/axiom-mesh` and `crates/axiom-mesh-ops` are present and that
  `crates/axiom-math/src/curve.rs` exists. Run the gates **one at a time** and
  record every result — in particular the **per-lint finding counts** for
  `engine_no_retained_state` and `engine_no_unwrap_or`, which are the delta
  baselines every later package is judged against (§20). `dylint-gate.sh` will
  fail; that is expected and is not a blocker.
- **Acceptance:** `git rev-parse main == git rev-parse origin/main`;
  `cargo xtask check-architecture` exits 0; `bash scripts/coverage.sh` reports
  100%; the two per-lint counts are written into this manifest as the recorded
  baseline.
- **Blocks:** everything.
- **Note:** never run two gates concurrently — the dylint gate fabricates a
  `cargo metadata` error under memory pressure and masks the real finding.

---

### WP-01 — Core curve representation

- **Location:** Layer: `math`.
- **Files:** modify `crates/axiom-math/src/curve.rs`, `curve_kind.rs`,
  `math_error_code.rs`, `math_error.rs`, `lib.rs`,
  `crates/axiom-math/tests/architecture.rs`, `crates/axiom-math/layer.toml`.
  Create `crates/axiom-math/src/closure.rs`,
  `crates/axiom-math/src/curve_binary.rs`.
- **Prerequisites:** WP-00.
- **Public API:** `Closure`; `CurveKind::Hermite`; `Curve::hermite`; `closure`
  parameter on the three existing constructors; `Curve::closure`,
  `Curve::span_count`, `Curve::encode`, `Curve::decode`, `Curve::digest`.
- **Implementation:** extend the one validation gate (`build`, `curve.rs:230`)
  with the Hermite point/tangent-count rule and the closed-curve rule (a closed
  polyline must not repeat its first point as its last — the closing span is
  implied). Extend the failure-message table index rather than adding a branch.
  Versioned little-endian codec mirroring `mesh_binary.rs`; `StableHash` digest
  mirroring `mesh_digest.rs`.
- **Algorithms:** none beyond validation and encoding.
- **Edge cases:** every `InvalidCurve` row in §13; a closed curve with the
  minimum point count per kind; Hermite with mismatched tangent count; codec
  round-trip of all four kinds × both closures; a truncated buffer;
  a schema-version mismatch.
- **Unit tests:** one per rejection with its exact message; round-trip
  equality for all eight kind × closure combinations; digest stability across
  two independent constructions; digest inequality for a one-`f32` change.
- **Golden tests:** none yet (WP-11 owns goldens).
- **Lint/architecture:** `Closure` and `CurveKind` must be added to
  `layer.toml`'s `introduced_capabilities` **and** to the pinned export list in
  `tests/architecture.rs:248-293` in the same change. No file may exceed 1000
  lines — `curve.rs` is at 787, so the codec goes in its own file. No `match`
  on `CurveKind`; extend the `const` fn tables to 4 entries.
- **Non-goals:** evaluation of the new kinds (WP-02); sampling (WP-03); NURBS;
  B-splines; centripetal Catmull-Rom.
- **Acceptance:** all four kinds constructible and rejectable; codec round-trips
  byte-exactly; `cargo xtask check-architecture` green; 100% coverage of the
  new regions.
- **Parallel with:** WP-03, WP-05.
- **Blocks:** WP-02, WP-04.

---

### WP-02 — Curve evaluation and derivatives

- **Location:** Layer: `math`.
- **Files:** modify `crates/axiom-math/src/curve.rs`. Create
  `crates/axiom-math/src/curve_basis.rs` (the four bases and their first and
  second derivatives) if `curve.rs` approaches the 1000-line cap.
- **Prerequisites:** WP-01.
- **Public API:** `Curve::derivative_at`, `Curve::second_derivative_at`,
  `Curve::curvature_at`. Hermite added to the `EVAL` and `DERIVATIVE` tables.
- **Implementation:** cubic Hermite basis `h00 = 2u³−3u²+1`, `h10 = u³−2u²+u`,
  `h01 = −2u³+3u²`, `h11 = u³−u²`, and its analytic derivative. Second
  derivatives for all four kinds (polyline's is `Vec3::ZERO`). Closed-curve span
  mapping wraps the index by `rem_euclid` instead of clamping — selected by a
  table index on `Closure`, not a branch.
- **Algorithms:** cubic Hermite; Bernstein and Catmull-Rom second derivatives;
  curvature `κ = ‖P′ × P″‖ / ‖P′‖³`.
- **Edge cases:** `t = 0` and `t = 1` on every kind × closure; the joint between
  two Bézier segments (outgoing span wins); a polyline corner (outgoing
  direction, already pinned at `curve.rs:468-475`); a vanishing derivative
  inside one span (`curve.rs:564-578`); zero curvature on a straight span;
  out-of-range `t` clamping.
- **Unit tests:** Hermite interpolates its endpoints and matches its authored
  tangents there; every kind returns a unit tangent at 11 parameters (extend
  `curve.rs:554-561`); curvature of a circular-arc Bézier matches `1/r` to
  `1e-3`; polyline curvature is exactly zero; closed-curve `t = 1` equals
  `t = 0`; the second derivative of a polyline is exactly zero.
- **Golden tests:** none.
- **Lint/architecture:** the dispatch tables stay `const` arrays indexed by
  `kind.raw()` — **no `match`**. Function bodies under 120 lines. Closures inside
  iterator chains must contain no branches.
- **Non-goals:** arc length (WP-04); frames (WP-07); finite-difference fallback
  (there is none, by design).
- **Acceptance:** all four kinds evaluate position, first and second derivative,
  tangent and curvature; closed curves wrap; 100% coverage.
- **Parallel with:** WP-03, WP-05, WP-06.
- **Blocks:** WP-04, WP-07.

---

### WP-03 — Sampling policy

- **Location:** Layer: `math`.
- **Files:** create `crates/axiom-math/src/sampling_policy.rs`,
  `crates/axiom-math/src/sample_count.rs`. Modify `lib.rs`,
  `math_error_code.rs`, `tests/architecture.rs`, `layer.toml`.
- **Prerequisites:** WP-00.
- **Public API:** `SamplingMode` (3 fieldless variants), `SamplingPolicy`
  (carrier struct), `SampleCount`, `MathErrorCode::InvalidSampleCount`,
  `MAX_SAMPLE_COUNT = 65_536`.
- **Implementation:** validated newtype in the shipped style
  (`tessellation.rs:88-101` is the model: `((v >= 2) & (v <= MAX)).then_some(..).ok_or_else(..)`).
  **`SamplingMode` must be fieldless `#[repr(u8)]` with explicit discriminants**;
  `SamplingPolicy` carries `count` and `max_segment` as always-present fields and
  resolves through `const RESOLVE: [fn(&SamplingPolicy, &ArcTable) -> MathResult<SampleCount>; 3]`
  indexed by `mode as usize` — the `state_op_kind.rs:82-101` pattern. Decide and
  document the `Eq`/`Hash` question: `Meters` wraps `f32`, so either hash the raw
  bits or store `max_segment` as an integer micrometre count internally.
  A `Reflect` impl so a policy is serializable alongside the curve it applies to.
- **Algorithms:** none beyond validation. This package is deliberately pure data.
  `resolve` for `MaxSegmentLength` needs `ArcTable`, so the *table* argument is
  supplied by WP-04 — WP-03 defines the signature and the two count-only
  resolvers; WP-04 fills the third.
- **Edge cases:** count `0`, `1`, `2`, `65_536`, `65_537`; `MaxSegmentLength`
  of `0`, negative, `NaN`, `+∞`; the codec round-trip of every mode; every one
  of the three `RESOLVE` table entries must be executed by a test (Coverage Law
  counts each as its own function).
- **Unit tests:** one rejection per invalid input with its exact message; `Eq`
  and `Hash` agree; round-trip of all three modes; a grep-style test or a review
  note asserting no `match` in the file.
- **Golden tests:** none.
- **Lint/architecture:** `SampleCount` must not expose a naked `u32` setter.
  `SamplingPolicy` carries `Meters`, never a raw `f32`. Both added to
  `introduced_capabilities` and the pinned export list.
- **Non-goals:** *resolving* a policy to a count (that needs the arc table —
  WP-04); adaptive tolerance (deferred, §8.1); any `quality` scalar.
- **Acceptance:** the type compiles, validates, serializes, and hashes; 100%
  coverage; no naked float on the public boundary.
- **Parallel with:** WP-01, WP-02, WP-05.
- **Blocks:** WP-04, WP-06.

---

### WP-04 — Arc-length system

- **Location:** Layer: `math`.
- **Files:** create `crates/axiom-math/src/arc_table.rs`,
  `crates/axiom-math/src/sampled_curve.rs`. Modify `curve.rs` (remove the
  private `arc_table`/`resample`/`sample_at_distance` and re-point
  `sample_uniform` at the new type), `lib.rs`, `tests/architecture.rs`,
  `layer.toml`.
- **Prerequisites:** WP-02, WP-03.
- **Public API:** `ArcTable` (`build`, `total`, `parameter_at`, `distance_at`,
  `max_error`), `ArcTableDensity`, `SampledCurve` (`build`, `samples`,
  `total_length`, `closure`, `sample_at_distance`, `parameter_at_distance`,
  `distance_at_parameter`, `digest`). `Curve::arc_length(samples: u32)` is
  replaced by `Curve::arc_length(density: ArcTableDensity) -> Meters`.
- **Implementation:** cumulative chord table over a dense uniform node grid,
  accumulated in **`u64` micrometres** (§9.2), converted to `Meters` only at the
  boundary. Inversion by `partition_point` + in-bucket lerp, replacing the
  shipped O(nodes) linear scan (`curve.rs:365-368`). `MaxSegmentLength` resolves
  its count as `ceil(total / max) + 1` clamped into `SampleCount`. Closed curves
  wrap by `rem_euclid`; open curves clamp — table-selected on `Closure`.
- **Algorithms:** prefix sum in fixed point; `partition_point` bisection
  (template: `modules/axiom-animation/src/track.rs:57`); integer
  division-with-remainder distribution for exact even spacing (template:
  `modules/axiom-physics/src/physics_world.rs:502-515`).
- **Edge cases:** density below and above its bounds; a curve of near-zero
  length; the first and last sample pinned exactly to `0` and `total`;
  `distance = 0`, `= total`, `> total` (clamp vs wrap); `t` outside `[0,1]`;
  a huge count saturating the node clamp (`curve.rs:722-729` already pins this);
  a vanishing tangent propagating out of sampling (`curve.rs:707-720`).
- **Unit tests:** keep and extend every test in `curve.rs:580-729`; the L-shaped
  polyline arc-uniformity proof (`:606-640`) must still pass byte-for-byte;
  `parameter_at_distance(distance_at_parameter(t)) ≈ t` for 20 parameters on
  each kind; a chord sum under-measures and converges from below; `nodes = N`
  vs `nodes = 4N` agree within `max_error()`; two independent builds are
  bit-equal.
- **Golden tests:** a committed `.bin` of one `SampledCurve` per kind at a fixed
  policy, plus a **mandatory negative** (a deliberately perturbed control point
  must change the bytes).
- **Lint/architecture:** no `unwrap_or`; no `?`; no `match` on `Closure`; the
  new files stay under 1000 lines; `ArcTable` holds no interior mutability
  (`engine_no_retained_state`).
- **Non-goals:** adaptive subdivision; frames; profile perimeter (WP-06).
- **Acceptance:** all five sampling behaviours work; inversion is `O(log n)`;
  the stated error bound is pinned by a test; goldens committed; 100% coverage.
- **Parallel with:** WP-05, WP-06.
- **Blocks:** WP-07, WP-08, WP-09.

---

### WP-05 — Profile representation

- **Location:** Layer: `math`. **This package MOVES code down a layer.**
- **Files:** create `crates/axiom-math/src/profile.rs`,
  `crates/axiom-math/src/profile_winding.rs`,
  `crates/axiom-math/src/profile_binary.rs`. Modify `lib.rs`,
  `math_error_code.rs`, `tests/architecture.rs`, `layer.toml`. **Delete**
  `crates/axiom-mesh-ops/src/profile.rs`; modify
  `crates/axiom-mesh-ops/src/lib.rs` and `layer.toml` to drop the re-export.
- **Prerequisites:** WP-00. (Independent of WP-01/02/03.)
- **Public API:** `Profile`, `ProfileWinding`, `PROFILE_EPSILON`, plus the new
  `perimeter`, `point_at_perimeter`, `oriented`, `encode`/`decode`/`digest`.
  `signed_area` becomes **public** and returns `Meters`.
  `scaled` takes `Ratio`, **not** `Meters` — a units bug in the shipped code
  (`profile.rs:172`).
- **Implementation:** port `profile.rs:1-217` unchanged in behaviour, swapping
  `MeshError`/`MeshErrorCode::InvalidProfile` for
  `MathError`/`MathErrorCode::InvalidProfile`, and `Segments` for `SampleCount`
  in `Profile::circle`. Add cumulative-perimeter computation in the same
  fixed-point style as WP-04.
- **Algorithms:** shoelace signed area (exists); cumulative perimeter; walk-and-lerp
  `point_at_perimeter` (template: `apps/bend-it/src/stroke/line.rs:173-184`).
- **Edge cases:** open with `< 2` points; closed with `< 3`; non-finite; duplicate
  neighbours; a closed profile whose last point sits on its first; zero enclosed
  area; both windings; `reversed()` flipping winding; perimeter of an open vs a
  closed profile (the closed one includes the closing edge);
  `point_at_perimeter` at `0`, at the total, and beyond.
- **Unit tests:** every existing test in `mesh-ops`' `profile.rs` moved and
  passing; winding of a CCW and a CW square; `perimeter` of a unit square is
  exactly `4` closed and `3` open; codec round-trip; digest stability.
- **Golden tests:** none (WP-11).
- **Lint/architecture:** `Profile` and `ProfileWinding` move from mesh-ops'
  `introduced_capabilities` to math's — **both manifests change in the same
  commit** or `CapabilityNotExported` fires. mesh-ops must add `Profile` to
  `consumed_capabilities`. Its `[[proof_exports]]` block for `Profile`
  (`layer.toml`) must be replaced with one for a symbol it still owns
  (`CapPolicy` or `Segments`).
- **Non-goals:** resampling (WP-06); triangulation (stays in mesh-ops); holes;
  per-point normals; per-point material tags; self-intersection checking.
- **Acceptance:** `Profile` lives in math; mesh-ops compiles against it with no
  behaviour change; both `layer.toml`s valid; `cargo xtask check-architecture`
  green; 100% coverage.
- **Parallel with:** WP-01, WP-02, WP-03, WP-04.
- **Blocks:** WP-06, WP-09.

---

### WP-06 — Profile resampling and correspondence

- **Location:** Layer: `math`.
- **Files:** create `crates/axiom-math/src/sampled_profile.rs`,
  `crates/axiom-math/src/profile_correspondence.rs`. Modify `lib.rs`,
  `tests/architecture.rs`, `layer.toml`.
- **Prerequisites:** WP-03, WP-05.
- **Public API:** `SampledProfile` (`build`, `points`, `perimeter_at`,
  `total_perimeter`), `correspond(&[Profile], SamplingPolicy) -> MathResult<Vec<SampledProfile>>`.
- **Implementation:** resample by **normalised perimeter distance** from index 0.
  `correspond` normalises winding to the first profile's, then resamples every
  profile to one shared count.
- **Algorithms:** cumulative perimeter (WP-05) + walk-and-lerp; the same
  integer-exact even-division as WP-04.
- **Edge cases:** open and closed profiles; a target count below the source
  count (decimation) and above it (interpolation); a profile whose points are
  wildly unevenly spaced; sections disagreeing on `Closure` (**error**); a
  single section (**error** — correspondence needs ≥ 2); mixed windings
  (normalised, not an error).
- **Unit tests:** a 7-point and a 19-point profile both resample to 24 points
  with matching indices; resampling a regular polygon to its own count is
  idempotent to `1e-5`; total perimeter is preserved to `1e-4`; resampling is
  sampling-density independent (template: `stroke/line.rs:280-300`); winding
  normalisation actually flips.
- **Golden tests:** a committed `.bin` of the 7↔19 correspondence result.
- **Lint/architecture:** `correspond` is a free function — the `geo.rs`
  precedent (`lib.rs:8-18`) sanctions free functions in math where there is no
  type to hang them on. No branches in the resampling closures.
- **Non-goals:** feature/corner preservation; optimal seam rotation; nearest-point
  or minimal-twist matching; **automatic** invocation from `loft` — the caller
  invokes `correspond` explicitly, and `loft` keeps its hard
  `IncompatibleProfiles` error (§10.3).
- **Acceptance:** two profiles of different point counts become loft-compatible
  deterministically; `loft` accepts the result; 100% coverage.
- **Parallel with:** WP-04, WP-07.
- **Blocks:** WP-09.

---

### WP-07 — Moving frames

- **Location:** Layer: `math`. **This package MOVES code down a layer and
  changes its algorithm.**
- **Files:** create `crates/axiom-math/src/frame.rs`,
  `crates/axiom-math/src/path_frames.rs`, `crates/axiom-math/src/twist.rs`,
  `crates/axiom-math/src/path_query.rs`. Modify `quat.rs` (add `slerp`,
  `from_rotation_arc`), `geo.rs` (re-express `tangent_basis` on `Frame::seed`),
  `lib.rs`, `math_error_code.rs`, `tests/architecture.rs`, `layer.toml`.
  **Delete** `crates/axiom-mesh-ops/src/sweep_frames.rs`; modify
  `crates/axiom-mesh-ops/src/{lib.rs,sweep.rs,revolve.rs,layer.toml}`.
  Modify `modules/axiom-physics/src/contact_solver.rs` to delete its private
  `tangent_basis` duplicate.
- **Prerequisites:** WP-02, WP-04.
- **Public API:** `Frame`, `linear_twist`, `path_frames`, `frame_at_distance`,
  `locate`, `MathErrorCode::{FrameDiscontinuity, DegenerateAxis}`,
  `Quat::slerp`, `Quat::from_rotation_arc`. Twist is a `&[Radians]` slice —
  empty means none — **not** an enum, so nothing destructures.
- **Implementation:** §11. Double-reflection RMF, least-aligned-axis seeding,
  arc-length-distributed holonomy correction for closed paths, twist as a
  separate composable stage. The sequence is `windows(2).scan(seed, …)` — the
  only legal expression of a sequential carry under `engine_no_recursion`.
- **Algorithms:** double reflection (§11.1); branchless argmin over three axis
  scores; holonomy angle `θ = atan2((r₀ × r_N)·t₀, r₀·r_N)`; slerp;
  shortest-arc rotation between two vectors; windowed nearest-point.
- **Edge cases:** every row of §11.3, each with its own test. Additionally: a
  two-sample path (the minimum); a path of 10,000 samples (drift bound); a
  closed loop whose holonomy is exactly zero (correction is a no-op); a closed
  loop with a large holonomy; a `Vec3::ZERO` reference (fallback) vs a
  non-finite one (error); a 179° turn (allowed) vs a 151° turn (error).
- **Unit tests:** orthonormality of every frame on eight representative paths
  (helix, vertical climb, hairpin, straight, closed circle, figure-eight,
  single-span, 10k-sample); **no normal flips** — `n_i · n_{i+1} > 0` for every
  consecutive pair; a straight run produces a constant normal exactly; a closed
  loop's last frame matches its first within `1e-4` after correction; twist
  composes additively; `projected_to_up` reproduces burnt-rubber's fixed-up
  frame on a non-vertical path; `locate` round-trips a point placed at a known
  `(distance, lateral)`.
- **Golden tests:** a committed `.bin` of the frame array for a helix and for a
  closed circle, with a mandatory negative.
- **Lint/architecture:** **`Frame::seed` and `Frame::transport` must be total** —
  no `Result`, no `Option`, no `.unwrap_or`. `engine_no_unwrap_or` (baseline 36,
  counted per compilation unit) means math is already a flagged unit, but new
  `unwrap_or` is still wrong here and the total-primitive design removes the
  need. Note the shipped `sweep_frames.rs:162,217` uses `.unwrap_or` and its
  `atan2`/`sin`/`cos` (`:213,222`) bar it from any `#[sim]` zone — the
  double-reflection rewrite removes both problems. `engine_no_branching` honors
  **no** zone marker, so there is no escape hatch. Deleting `sweep_frames.rs`
  removes `SweepFrame` from mesh-ops' `introduced_capabilities`.
- **Non-goals:** Frenet frames; a stateful frame walker; per-sample authored
  normals beyond `Twist::PerSample`; curvature-adaptive re-framing.
- **Acceptance:** frames are flip-free on all eight representative paths; the
  closed-loop seam is corrected; `sweep` and `revolve` compile against
  `axiom_math::Frame` with byte-identical output on open paths; the physics
  duplicate is deleted; 100% coverage.
- **Parallel with:** WP-06.
- **Blocks:** WP-08, WP-09.

---

### WP-08 — Burnt Rubber integration proof

- **Location:** App: `burnt-rubber`. **No engine code changes here.**
- **Files:** modify `apps/burnt-rubber/src/track/mod.rs`,
  `apps/burnt-rubber/src/sim/controller.rs`,
  `apps/burnt-rubber/Cargo.toml`, `apps/burnt-rubber/app.toml`.
- **Prerequisites:** WP-04, WP-07, WP-11 (the baseline capture must exist first).
- **Public API:** none — the app's own `Track` API is unchanged so no consumer
  outside `track/mod.rs` is touched.
- **Implementation:** the four-function slice in §15. `Track` gains a
  `SampledCurve` + `Vec<Frame>` built once in the preparation phase from the
  existing integrator output, and `interpolated_at`/`localise`/`index_at`
  delegate to the engine. `shortest_angle` and `unit_or` are deleted and
  imported from `axiom-math`.
- **Algorithms:** none new.
- **Edge cases:** distance `0` (the grid sits at `GRID_DISTANCE = 30.0`, not
  zero); distance at and past `track.length()`; `localise` at both ends
  (`track/mod.rs:471-480` pins clamping); the 80 m window; a sample exactly on
  a section boundary.
- **Unit tests:** every existing test in `track/mod.rs:363-731` must pass, with
  **two documented exceptions**: `interpolated_at`'s frame now slerps rather
  than lerping loose vectors, so the orthonormality assertions
  (`:402-418`) tighten and the interpolation test (`:495-504`) needs its
  tolerance restated.
- **Golden tests:** WP-11's fifteen `.bin` artifacts. **Expect the render and
  resources goldens to move** — the frame interpolation genuinely changes. Any
  movement must be explained, screenshotted, and re-golded deliberately, never
  with a blind `AXIOM_REGOLD`.
- **Lint/architecture:** apps are outside the coverage gate and the Branchless
  Law, so the app-side code has no new lint burden. `app.toml` must list `math`
  in `allowed_layers` (it already does transitively — verify).
- **Non-goals:** the `.brc` DSL; the eight road primitives; the signal
  conditioning; the forward-Euler integrator; lanes; banking; barriers; the road
  mesh. **No road vocabulary may enter the engine.**
- **Acceptance:** burnt-rubber compiles with four app functions replaced by
  engine calls; `cargo test -p axiom-burnt-rubber` green; the fifteen goldens
  either match or have documented, reviewed movement; `cargo xtask check-slices`
  green.
- **Parallel with:** WP-09, WP-10.
- **Blocks:** nothing.

---

### WP-09 — Mesh-ops contract proof

- **Location:** Layer: `mesh-ops`.
- **Files:** modify `crates/axiom-mesh-ops/src/{lib.rs,sweep.rs,loft.rs,revolve.rs,extrude.rs,polygon_triangulation.rs,tessellation.rs,cap_policy.rs,primitive_*.rs}`,
  `crates/axiom-mesh-ops/layer.toml`,
  `crates/axiom-mesh-ops/{ARCHITECTURE.md,TESTING.md}`. **Deleted by WP-05/07:**
  `profile.rs`, `sweep_frames.rs`.
- **Prerequisites:** WP-05, WP-06, WP-07.
- **Public API:** unchanged **except** `Profile`, `ProfileWinding`, `SweepFrame`
  and `parallel_transport_frames` are no longer exported here.
  `SweepOptions::closed_path` is replaced by reading the path's own `Closure`.
  `Samples` is replaced by `SamplingPolicy`. `sweep` gains a `Twist` parameter
  in place of the scalar `twist: Radians`.
- **Implementation:** rewire onto the moved-down types. The neutral contract
  mesh-ops consumes is exactly:
  `SampledCurve` → `&[CurveSample] { position, tangent, parameter, distance }`,
  `&[Frame] { position, tangent, normal, binormal }`, and
  `SampledProfile { points, cumulative perimeter }`. **Nothing else.**
- **Algorithms:** none new. `column_arc`'s perimeter computation
  (`sweep.rs:286-299`) is deleted and read from `SampledProfile`.
- **Edge cases:** a closed path now carries a corrected seam — the previously
  broken case (§3.3) becomes correct and must be tested; caps on an open profile
  (ignored, not an error); a cap on a closed path (ignored); mismatched loft
  sections still hard-error.
- **Unit tests:** every existing mesh-ops test must pass unchanged for open
  paths (**byte-identical meshes** — this is the proof that the move changed
  nothing); a new test proving a closed sweep's seam ring aligns with its first
  ring within `1e-4`.
- **Golden tests:** a committed `.bin` mesh digest for one sweep, one loft, one
  revolve and one extrude, with a mandatory negative.
- **Lint/architecture:** mesh-ops' `layer.toml` must drop `Profile`,
  `ProfileWinding` from `introduced_capabilities`, add them to
  `consumed_capabilities`, and replace the `[[proof_exports]]` block naming
  `Profile`. It keeps `depends_on = ["kernel", "math", "mesh"]` — unchanged.
- **Non-goals:** implementing new mesh operators; the `axiom-proc-mesh` →
  `axiom-mesh-ops` convergence (a separate initiative,
  `docs/mesh-convergence-migration.md`); road/tree/building generators.
- **Acceptance:** mesh-ops exports only mesh concerns; every open-path mesh is
  byte-identical to the pre-change output; the closed-sweep seam defect is
  fixed and pinned; 100% coverage.
- **Parallel with:** WP-08, WP-10.
- **Blocks:** nothing.

---

### WP-10 — Documentation and architecture enforcement

- **Location:** Tooling + docs.
- **Files:** create `crates/axiom-math/CURVES.md`. Modify
  `crates/axiom-math/ARCHITECTURE.md` (the "eight error codes" line is already
  stale — there are nine; this change makes it thirteen),
  `crates/axiom-math/layer.toml`, `crates/axiom-mesh-ops/ARCHITECTURE.md`
  (§"Curve and sweep framing policy" at `:238-262` must be rewritten — its
  conclusion is reversed by WP-07), `docs/unbranching.md` (recipe 1 at `:32`
  recommends `cond.then_some(a).unwrap_or(b)`, which `engine_no_unwrap_or` now
  bans — a live contradiction), `CLAUDE.md` (the same recipe, and the claim
  that CI runs on every push — CI has been `workflow_dispatch`-only since
  2026-07-14).
- **Prerequisites:** WP-05, WP-07, WP-09.
- **Public API:** none.
- **Implementation:** document the parameter model, the error bound, the frame
  algorithm and its edge-case policies, and the neutral contract mesh-ops
  consumes. Verify both `layer.toml`s and the pinned export test.
- **Edge cases:** the pinned export list in `tests/architecture.rs:248-293`
  grows from 27 to roughly 40 entries — it must be updated exactly once, at the
  end, not per package.
- **Unit tests:** `real_repo_layers_pass` and `real_repo_class_aware_check_passes`.
- **Lint/architecture:** this package **owns** the final manifest reconciliation.
- **Non-goals:** rewriting the Layer Law or Module Law; adding a new lint.
- **Acceptance:** `cargo xtask check-architecture` green; no doc in the repo
  recommends a banned idiom; the mesh-ops architecture doc no longer asserts a
  placement the code contradicts.
- **Parallel with:** WP-08, WP-09.
- **Blocks:** nothing.

---

### WP-11 — Golden / regression harness

- **Location:** Test/Harness.
- **Files:** create `apps/burnt-rubber/tests/golden/track_samples.bin`,
  `apps/burnt-rubber/tests/track_golden.rs`. Modify
  `apps/burnt-rubber/slice.toml`. Create the WP-04/06/07/09 `.bin` fixtures
  under `crates/axiom-math/tests/golden/` and
  `crates/axiom-mesh-ops/tests/golden/`.
- **Prerequisites:** WP-00. **Must complete before WP-08.**
- **Public API:** none.
- **Implementation:** the mechanism already exists and needs no invention.
  `apps/burnt-rubber/tests/agent_golden.rs` pins fifteen byte-exact artifacts
  (`agent_{grid,opening,esses,canyon,finish}_{state,render,resources}.bin`) at
  five checkpoints, SHA-pinned in `apps/burnt-rubber/slice.toml` and enforced by
  `cargo xtask check-slices`. Its own doc explains why the resources artifact
  matters: *"a road chunk built from a stale track, an off-by-one sample range,
  a seam that stops being bit-identical … all render a visibly different game
  while leaving the draw list byte-identical."*
  **Add a sixteenth artifact: a byte dump of `Track::samples()` for
  `DEFAULT_SEED`.** This is the gap — every existing guard is a *tolerance*
  assertion and none would catch a frame that shifted by `1e-4` everywhere. A
  diff localised to "the curve changed" is worth far more than one saying "the
  render changed."
- **Algorithms:** none.
- **Edge cases:** every golden needs a **mandatory negative** — a deliberately
  perturbed input must change the bytes, or the golden proves nothing.
- **Unit tests:** the goldens are the tests.
- **Lint/architecture:** goldens live under `tests/`, which is exempt from the
  Branchless Law and the coverage gate.
- **Non-goals:** screenshot comparison; a new harness; visual convergence.
- **Acceptance:** the sixteenth artifact is committed and SHA-pinned **before**
  WP-08 changes anything; `cargo xtask check-slices` green; each golden has a
  passing negative.
- **Parallel with:** WP-01 … WP-07.
- **Blocks:** WP-08.

---

## 18. Parallel execution graph

```
                            WP-00 (sync + baseline)
                                    │
        ┌──────────────┬────────────┼────────────┬──────────────┐
        ▼              ▼            ▼            ▼              ▼
      WP-01          WP-03        WP-05        WP-11          (docs recon)
   (curve repr)    (policy)     (profile)    (goldens)
        │              │            │            │
        ▼              │            ▼            │
      WP-02            │          WP-06          │
   (evaluation)        │      (resample)         │
        │              │            │            │
        └──────┬───────┘            │            │
               ▼                    │            │
             WP-04                  │            │
        (arc length)                │            │
               │                    │            │
               ▼                    │            │
             WP-07                  │            │
           (frames)                 │            │
               │                    │            │
        ┌──────┴────────────────────┴────────────┘
        ▼                    ▼
      WP-08                WP-09  ──▶  WP-10
   (burnt rubber)        (mesh-ops)    (docs)
```

**Maximum concurrency, wave by wave:**

| Wave | Packages that may run at once |
|---|---|
| 1 | WP-00 alone |
| 2 | **WP-01, WP-03, WP-05, WP-11** (four agents) |
| 3 | **WP-02, WP-06** (WP-06 needs WP-03 + WP-05) |
| 4 | **WP-04** |
| 5 | **WP-07** |
| 6 | **WP-08, WP-09** (two agents) |
| 7 | **WP-10** |

WP-05 is the longest independent chain start and should be dispatched first in
wave 2 — it touches two crates and two manifests.

**File ownership is exclusive.** The only files touched by more than one package
are `crates/axiom-math/src/lib.rs`, `crates/axiom-math/layer.toml`, and
`crates/axiom-math/tests/architecture.rs`. **WP-10 owns the final reconciliation
of all three**; earlier packages append their exports and re-run the checker,
but WP-10 is responsible for the end state.

---

## 19. Files expected to change

**Created — `crates/axiom-math/src/`** (13):
`closure.rs`, `curve_binary.rs`, `curve_basis.rs`, `sampling_policy.rs`,
`sample_count.rs`, `arc_table.rs`, `sampled_curve.rs`, `profile.rs`,
`profile_winding.rs`, `profile_binary.rs`, `sampled_profile.rs`,
`profile_correspondence.rs`, `frame.rs`, `path_frames.rs`, `twist.rs`,
`path_query.rs`

**Created — docs and fixtures:**
`crates/axiom-math/CURVES.md`,
`crates/axiom-math/tests/golden/*.bin`,
`crates/axiom-mesh-ops/tests/golden/*.bin`,
`apps/burnt-rubber/tests/track_golden.rs`,
`apps/burnt-rubber/tests/golden/track_samples.bin`

**Modified — `crates/axiom-math/`:**
`src/curve.rs`, `src/curve_kind.rs`, `src/quat.rs`, `src/geo.rs`,
`src/math_error.rs`, `src/math_error_code.rs`, `src/lib.rs`,
`tests/architecture.rs`, `layer.toml`, `ARCHITECTURE.md`

**Modified — `crates/axiom-mesh-ops/`:**
`src/lib.rs`, `src/sweep.rs`, `src/loft.rs`, `src/revolve.rs`, `src/extrude.rs`,
`src/tessellation.rs`, `src/cap_policy.rs`, `src/polygon_triangulation.rs`,
`src/primitive_*.rs` (the `Profile::circle` callers), `layer.toml`,
`ARCHITECTURE.md`, `TESTING.md`

**Deleted:**
`crates/axiom-mesh-ops/src/profile.rs`,
`crates/axiom-mesh-ops/src/sweep_frames.rs`

**Modified — elsewhere:**
`modules/axiom-physics/src/contact_solver.rs` (delete the duplicate
`tangent_basis`), `apps/burnt-rubber/src/track/mod.rs`,
`apps/burnt-rubber/src/sim/controller.rs`, `apps/burnt-rubber/Cargo.toml`,
`apps/burnt-rubber/app.toml`, `apps/burnt-rubber/slice.toml`,
`docs/unbranching.md`, `CLAUDE.md`

---

## 20. Validation commands

Run from `C:\dev\axiom`. **Never run two gates concurrently** — under memory
pressure the dylint gate fabricates a `cargo metadata` error and masks the real
finding, and `link.exe` exits `0xc0000142` when RAM is exhausted.

```sh
# 0. Baseline (WP-00)
git fetch origin && git merge --ff-only origin/main

# 1. Tests
cargo test --workspace
cargo test -p axiom-math
cargo test -p axiom-mesh-ops
cargo test -p axiom-burnt-rubber --test agent_golden

# 2. Architecture (Layer Law + Module Law + coverage-scope)
cargo xtask check-architecture

# 3. Slice / golden SHA pins
cargo xtask check-slices

# 4. Coverage (100% regions/lines/functions on layers + modules)
bash scripts/coverage.sh          # Linux/CI
scripts/coverage.ps1              # Windows, this repo's primary dev shell
scripts/coverage.ps1 -Open        # annotated HTML — red is the work list

# 5. Dylint rulebook ratchet (Branchless Law + the rest)
bash scripts/dylint-gate.sh

# 6. TypeScript SDK gate — unaffected by this work, run once before pushing
bash scripts/ts-gate.sh

# 7. wasm build
cargo build --target wasm32-unknown-unknown -p axiom-kernel
```

**Caveats that will otherwise cost time:**

- **CI does not run automatically.** `.github/workflows/ci.yml` is
  `on: workflow_dispatch` only, disabled 2026-07-14 because the runners' stable
  toolchain moved to clippy 1.96 and fails on ~100 pre-existing spine warnings,
  *"including lints that conflict with the Branchless Law's canonical idioms."*
  **CLAUDE.md's claim that CI gates every push to `main` is stale.** The four
  local gates are the real gate.
- **`cargo fmt --all --check`** is in the CI job but CI does not run; it is
  known to hang on `frame_report.rs`. It is **not** a gate — do not block on it.
- **`cargo clippy --workspace --all-targets -- -D warnings`** currently fails on
  pre-existing warnings. Do not treat a clippy failure as caused by this work
  without bisecting.
- **`engine_no_unwrap_or` counts compilation units, not findings** (baseline 36).
  A new `unwrap_or` inside `axiom-math` will *not* trip the ratchet because math
  is already flagged — that is a gap in the gate, not a licence. Write total
  primitives anyway. The lint crate is currently **untracked**
  (`?? tools/lints/engine_no_unwrap_or/`) while `tools/lints/Cargo.toml` and the
  baseline that register it are tracked-and-modified; it is nonetheless live
  under `cargo dylint --all`.
- **`bash scripts/dylint-gate.sh` FAILS today no matter what you do.**
  `engine_no_retained_state` has 787 findings and deliberately no baseline entry.
  The correct check is a **before/after delta**, not the exit code:

  ```sh
  cargo dylint --lib engine_no_retained_state -- --all-targets 2>&1 | grep -c '^warning'
  ```

  Run it on `origin/main` first, record the number, and require the post-change
  number to be **equal or lower**. Do the same for `engine_no_unwrap_or`. Never
  add an `engine_no_retained_state=N` line to the baseline — the baseline file
  says so in as many words.
- **The dylint gate needs an exact toolchain**: `cargo-dylint` and `dylint-link`
  at **6.0.1**, on nightly `nightly-2026-04-16` with `rustc-dev` and
  `llvm-tools-preview` (`tools/lints/rust-toolchain`).
- **wasm-only code is invisible to every native gate.** `axiom-windowing`'s
  `wasm32` arm and `axiom-gpu-backend`'s `offscreen` feature are compiled out, so
  dylint, coverage and `cargo test` all miss them — which is why ~67 `?`
  operators survive in the spine while `engine_no_branching` reads 0.
  `engine_no_branching=0` is true of the *compiled* spine, not the whole spine.
- **`bash scripts/ts-gate.sh` covers three packages** — `axiom-client`,
  `axiom-game`, `axiom-web-engine` — each needing `node_modules` present.
  CLAUDE.md mentions only `axiom-client`.

---

## 21. Explicit non-goals

Out of scope for this manifest, in full:

- **Mesh generation itself** — beyond rewiring the existing operators (WP-09)
  and using them as test glue.
- **Roads, tracks, lanes, banking-as-a-road-concept, barriers, tunnels.** No
  road vocabulary enters the engine, ever.
- **Trees, branches, cables, rivers, buildings, cars.** Every one is a
  *composition* and belongs to an app or a module.
- **Renderer, GPU, shader, material, texture work.** None.
- **Scene, ECS, physics, animation-system changes** — except deleting one
  duplicated `tangent_basis` in `axiom-physics`.
- **Browser and platform APIs.** None.
- **Generalized animation systems**; the ease-curve consolidation is listed as a
  future consumer, not a work package.
- **NURBS, B-splines, CAD kernels, boolean/CSG operations, offsetting,
  self-intersection resolution.**
- **Polygon holes, multi-contour profiles, per-point normals, per-point material
  tags, corner classification.**
- **Adaptive recursive subdivision** — banned by `engine_no_recursion`, zero
  consumers.
- **Sophisticated profile shape matching** — feature detection, optimal seam
  rotation, minimal-twist correspondence.
- **A visual editor or authoring UI.**
- **The `axiom-proc-mesh` → `axiom-mesh-ops` convergence** — tracked separately
  in `docs/mesh-convergence-migration.md`.
- **Migrating every consumer in §16.** One consumer is migrated and proven; the
  rest are listed and deferred.

---

## 22. Acceptance criteria

Objective, checkable, and complete.

1. **Deterministic evaluation** — `position_at`, `derivative_at`,
   `second_derivative_at`, `tangent_at`, `curvature_at` are pure functions of
   `(Curve, t)`; two independent constructions produce bit-equal results for all
   four kinds × both closures.
2. **Deterministic sampling** — `SampledCurve::build(curve, policy)` is
   byte-identical across runs and across processes for every policy variant; a
   committed `.bin` golden per kind passes, and its mandatory negative fails.
3. **Arc-length stability** — an L-shaped polyline samples to exactly equal
   spacing within `1e-3`; a curved Bézier's consecutive gaps stay within 2% of
   their mean; `parameter_at_distance(distance_at_parameter(t)) ≈ t` for 20
   parameters on every kind; `nodes = N` and `nodes = 4N` agree within
   `ArcTable::max_error()`; inversion is `O(log n)`, verified by a test that
   would time out under the old linear scan.
4. **No moving-frame flips** — on all eight representative paths (helix,
   vertical climb, hairpin, straight run, closed circle, figure-eight,
   two-sample minimum, 10,000-sample drift test), every consecutive normal pair
   satisfies `n_i · n_{i+1} > 0`, and every frame is orthonormal to `1e-4`.
5. **Stable closed-loop seam** — for a closed path, `frames.last()` matches
   `frames.first()` within `1e-4` after holonomy distribution, and a closed
   sweep's final ring aligns with its first.
6. **Profile resampling correctness** — a 7-point and a 19-point profile
   resample to a shared 24 points with matching perimeter fractions; total
   perimeter is preserved within `1e-4`; resampling a regular polygon to its own
   count is idempotent within `1e-5`; the result is accepted by `loft`.
7. **Real consumer migrated** — burnt-rubber's `interpolated_at`, `localise`,
   `index_at`, `shortest_angle` and `unit_or` are engine calls; the app compiles
   with no road vocabulary added to any layer; all fifteen existing goldens
   either match byte-for-byte or have documented, individually reviewed
   movement; the sixteenth (`track_samples.bin`) is committed and pinned.
8. **Mesh-ops output unchanged for open paths** — every existing mesh-ops test
   produces a byte-identical mesh after the rewiring. This is the proof that
   moving `Profile` and the frames down changed nothing.
9. **All gates green, run one at a time** — `cargo test --workspace`;
   `cargo xtask check-architecture` exit 0; `cargo xtask check-slices` exit 0;
   `bash scripts/coverage.sh` at **100.00%** regions, lines and functions.
   For dylint, the criterion is a **delta**, not an exit code: the gate fails
   today on `engine_no_retained_state`'s 787 pre-existing findings, so the
   requirement is that the per-lint finding counts for
   `engine_no_retained_state` and `engine_no_unwrap_or` are **no higher than
   the `origin/main` baseline recorded in WP-00**, and that
   `engine_no_branching` and `engine_no_large_files` are still exactly **0**.
10. **Structural hygiene** — no file in `crates/` exceeds 1000 lines; no
    function exceeds 120 lines; no struct exceeds 24 fields; no enum exceeds 24
    variants; no impl block exceeds 30 items (and `Mat4`'s does not grow); no
    new `match`, `if`, `if let`, `let-else`, `for`, `while`, `?`, `&&` or `||`
    in spine non-test code; no new `unwrap_or` and no `unwrap()` at all; no
    naked `f32` on any public boundary above the scalar floor; **no public
    `&mut`, no `dyn Fn`/`impl Fn`, no `Rc`/`Arc`, no interior mutability, no
    `static`** in the new code; no source file contains the literals
    `coverage(off)`, `engine_no_retained_state`, `allow(warnings)` or
    `expect(warnings)` — comments included; every new `pub mod` is
    doc-commented; no wildcard imports; every new free function is `pub use`d
    at the crate root; `Profile`, `ProfileWinding`, `SweepFrame` and
    `parallel_transport_frames` no longer appear in
    `crates/axiom-mesh-ops/src/lib.rs`; `contact_solver.rs` has no private
    `tangent_basis`.
11. **No data-carrying enum is destructured anywhere** — `CurveKind`,
    `Closure`, `SamplingMode` and `ProfileWinding` are all fieldless
    `#[repr(uN)]` enums with explicit discriminants, consumed **only** through
    `const` tables indexed by `self as usize`. Grep the new files: zero
    occurrences of `match`.
12. **Documentation is not contradicted by code** — `axiom-mesh-ops`'
    `ARCHITECTURE.md` no longer asserts that framing belongs in mesh-ops;
    `docs/unbranching.md` and `CLAUDE.md` no longer recommend
    `unwrap_or`; the parameter model, the error bound and every frame edge-case
    policy are documented at the site.

---

## 23. Verdict

`VERDICT: No new layer. The entire curve/profile/sampling/arc-length/frame foundation belongs in axiom-math, which is already its correct home for Curve/CurveKind/CurveSample and is the lowest layer at which Vec2/Vec3/Quat are nameable; Profile + ProfileWinding and the moving-frame system (as a total Frame value type plus the free functions path_frames, frame_at_distance and locate) must MOVE DOWN into axiom-math from axiom-mesh-ops, which retains only genuine mesh concerns — sweep, loft, revolve, extrude, triangulation, caps, UV assignment, winding normalisation, seam duplication, and the Segments/Rings/Subdivisions/DetailBudget tessellation budgets; the frame algorithm changes from Rodrigues rotation to double-reflection rotation-minimizing frames so it is transcendental-free and therefore #[sim]-legal; sampling gains a typed SamplingPolicy (Count, UniformParameter, UniformDistance, MaxSegmentLength) with adaptive recursive subdivision deliberately deferred as banned by engine_no_recursion and consumer-free; arc length standardises on a deterministic chord prefix-sum accumulated in fixed-point integer micrometres with partition_point inversion; closed-loop handling becomes a Closure property of the curve rather than a sweep flag, and the currently-uncorrected closed-sweep holonomy seam is fixed; the layer DAG kernel -> math -> mesh -> mesh-ops is UNCHANGED; and the integration proof is apps/burnt-rubber's four sampled-path query functions, gated by its existing fifteen byte-exact goldens plus one new track_samples.bin.`
