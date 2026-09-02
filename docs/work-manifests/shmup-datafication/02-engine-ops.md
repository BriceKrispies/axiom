# Engine ops — the vocabulary gap, with evidence

**Every op in this list carries a measured call-site count from the app. An op
with no evidence does not get built.** That rule exists because W2b lands ~10
capabilities before a single app line moves because of them (see
`00-manifest.md`), so a wrong call about an op's shape is discovered late and
across many consumers.

Counts are `ax q … --path apps/axiom-shmup/src`, measured 2026-09-01.

## `crates/axiom-field` — the algebra

The existing 27 ops (`FieldOp::ALL`, `field_op.rs`):

```
Const Point Uv Normal Time Param Add Sub Mul Min Max Abs Clamp Mix
Smoothstep Dot Length Normalize Compose Component Noise Fbm Transform
Sin Cos Pow Exp
```

The vocabulary is closer than it looks. `Smoothstep` (373 app sites) and `Mix`
(171) already exist. What is missing:

| op | app sites | note |
|---|---|---|
| `Floor` / `Fract` / `Mod` | **109** | `.floor()`, `.fract()`, `.rem_euclid()`. The tiling/repeat primitive — `sky/luts.rs:77,89`, `world/props/cover.rs:140`, `world/kit/window.rs:327` |
| `Round` | **72** | `.round()`. **Must be `round_ties_up`**, not Rust's `round` — see `axiom_math::round_ties_up`, and the latent-defect note below |
| `Atan2` | **58** | `world/props/cover.rs:135`, `world/props/mesh.rs:220`. Polar addressing |
| `Sign` | **15** | **JS `sign(0)` is `0`; `f64::signum(0.0)` is `1.0`.** Bind to `axiom_math::signum_with_zero`, never to `signum` |
| period on `Noise` / `Fbm` | — | tiling noise; today's are unbounded |
| `Worley` / `Hash` / `Srgb` | — | evidence to be gathered before building; **do not build on this line alone** |

Appending is mandatory: `FieldOp::ALL` is a positional table and its index *is*
the opcode. The orchestrator assigns `Floor = 27, Fract = 28, Mod = 29, …` in the
wave brief so the vocabulary agent and the body agents never negotiate.

`axiom-field` compiles to WGSL and **WGSL has no `f64`** — so no field op ever
declares a 2-word (f64) param slot. That keeps its CPU↔GPU parity corpus valid by
construction rather than by discipline.

### The `round` trap, recorded because it already bit once

`world/noise.rs` defined `round_half_up` as `(v + 0.5).floor()`, which is wrong at
`0.49999999999999994` — that value plus 0.5 rounds *up* to exactly 1.0 in
binary64, so the "round" returns 1 where JS returns 0. `jsmath` had documented
this and implemented it correctly; the wrong one got promoted anyway.

The goldens did not move, because the pathological input never arose. **That is
exactly how a latent defect survives a golden suite**, and it is why `Round` binds
to a named, tested primitive rather than to whatever the porting agent reached
for.

## `crates/axiom-proc-mesh`

| op | why | status |
|---|---|---|
| `MeshOp::PaintColor` | `MeshBuffer` now carries a colour stream (`colors`, `with_colors`, `respecified`) and **no `MeshOp` writes it**. A procedural kit paints wear, grime and baked AO into vertex colour; without this a recipe can produce the *shape* of a weathered wall but not its weathering. | blocker 3 |
| primitive exposures | `Cube Cylinder Grid Transform Extrude Bevel Bend Displace UVProject Triangulate Sphere MetaSurface Merge Trs` — `Merge` and `Trs` landed this cycle | — |

`PaintColor`'s precedent is exact: `Displace` binds a `FieldGraph` evaluated with
`point`/`normal` in scope. `PaintColor` is the same evaluation writing a different
output stream.

## `crates/axiom-mesh-ops`

Hole-bridged triangulation · extrude bevel · revolve `phi_start` · partial sphere ·
torus arc · chamfer style · public `concat`/`weld`.

**Evidence to gather before building.** `weapons/geometry/`'s `ring_geometry` and
`circle_geometry` already duplicate `annulus`/`disk`, so at least the deletion
half of this is proven.

## `crates/axiom-noise`

1-D lattice; the GLSL hash family. Two bases (`unit_noise`, `value_fbm_01`,
`perlin_2d`, `cellular_2d`, …) landed this cycle with Node-captured goldens
asserted by `assert_eq!`, not tolerance.

## `modules/axiom-animation`

Two-bone IK · look-at · blend tree. There is **no IK today** — the only `two_bone`
hits are a test fixture (`skeleton_two_bones()`).

## `modules/axiom-physics`

`PhysicsShapeKind::TriangleMesh = 5` + a BVH. The enum is table-ordered
(`Sphere=0 … Heightfield=4`), so appending is safe. **The only `bvh` hits in
`crates/` + `modules/` today are doc comments written during this programme's
audit** — there is no BVH in the spine.

This belongs to the *promotion* programme, not this one; it is listed here because
it is the blocker that gates ~2,400 physics lines and someone will look for it.

### One real defect already found here

The AABB slab test's `0 × ∞ = NaN` made a comparison-based test **miss a box the
ray passes through** — a grounded character's downward probe, every frame. Fixed
by substituting ∓∞ for NaN bounds. Ray/AABB is the BVH's inner loop, so the fix
had to land before the BVH, not after.

## `modules/axiom-grid`

Weighted 8-connected pathfinding. `axiom-grid::path` today is gradient descent
over a BFS distance field — the **unweighted case of the same operation**, which
is why this extends `axiom-grid` rather than becoming `axiom-nav`.
(`axiom-agent` *prohibits* pathfinding by an enforced architecture test; do not
put it there.)

## `crates/axiom-host`

`FrameSky` already exists — "a vertical gradient with an optional celestial body",
plumbed through `axiom-windowing` and `FrameOutcome`. The shmup sky/celestial port
extends it. It does **not** become `axiom-atmosphere`, and it does **not** go to
`axiom-space` (that layer is only `Address`/`SpaceApi`).

Blocker 4 lands here: `DVec3::normalize` is fallible and the sky ports are
infallible, pinned to JS goldens that yield `Infinity`/`NaN` on degenerate input.
8 call sites, but a **semantics** change, not a rename. Needs
`DVec3::normalize_or_zero`.

## Duplication to delete, not promote

- `physics/ragdoll.rs` redefines `hypot3`; `axiom_math::hypot3` exists — 26 sites.
- `level_xform`/`transform_box` duplicated verbatim in `world/system.rs` and
  `scene/wiring/ai.rs` (whose own comment admits it). Needs `Aabb::transform`,
  ~12 lines.
- Two `Mat4`s (`weapons/rig_math.rs`, `ai/animator.rs`); three f64 `Vec3`s.
- `weapons/geometry/`: `ring_geometry`/`circle_geometry` re-implement
  `axiom-mesh-ops::annulus`/`disk`.
- The `Subsystem` 4-method stub is hand-written **11 times** in `src/`.

## Out of scope

**`materials/surfaces/` (1,799 lines).** `MAX_NODES = 256` against 2.1k–43.4k-node
generator graphs, and an algebra deliberately without loops, division, `floor` or
`fract`. Adding `Floor`/`Fract`/`Mod` does not close a 43,000-node gap by two
orders of magnitude. Hand-written WGSL is the route and another session is already
taking it.

This was the first draft's highest-value target. An agent proved it infeasible and
the correction stands.
