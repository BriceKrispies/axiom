# SPEC-03 — Math & spatial queries

> Status: Landed
> Landed (2026-06-28): `axiom-math::MathApi::{clamp, lerp, normalize_angle}` + `axiom-scene::SceneApi::overlap_circle`; `@axiom/game` exports `clamp`/`lerp`/`normalizeAngle`/`overlapCircle` and the `v2` helpers, routed to native math. The §2 gaps below are now closed.
> Contract: §5   Vocabulary: clamp, lerp, normalizeAngle, Vec/Mat4/Quat, AABB/point-in-rect/circle overlap, Raycast   Determinism: sim

## 1. Summary

The authoritative numeric + spatial vocabulary every simulation reaches for:
clamp a health bar, lerp a value, wrap an angle, add/normalize a vector, test
two rects/circles, and ask the scene "what is in this box / under this ray".
All 11 games use the scalar/vector helpers; the spatial queries are the
substrate for collision, line-of-sight, picking, and proximity. The native
algebra and geometry already exist — what is missing is **three scalar helpers**
and the **entire TS projection** of the whole §5 surface (consistent with
SPEC-00: 0 of the contract's entry points exist in TS today).

## 2. Current state (verified)

- **Vector/matrix/quat/transform algebra: present.** `axiom-math`
  (facade `MathApi`) exposes `Vec2`/`Vec3`/`Vec4`, `Mat4`, `Quat`, `Transform`
  with full algebra (constructors, dot/normalize/length, perspective/ortho/
  look-at, axis-angle, TRS compose).
- **Pure geometry primitives: present.** `Aabb` (`contains_point`, `overlaps`,
  `contains_aabb`, `merge`, `expand`), `Sphere` (`contains_point`, `overlaps`,
  `intersects_ray`), `Ray` (`intersect_aabb`, `intersect_aabb_entry`,
  `intersect_sphere`). All checked-finite, all reached through `MathApi`.
- **Scene spatial queries: mostly present.** `axiom-scene` (`SceneApi`) exposes
  `raycast`/`raycast_hit` (→ `SceneNodeId` (+ entry point), `Meters` range) and
  `overlap_box(center, half_extents)`. There is **no `overlap_circle` /
  `overlap_sphere`** — a native gap against contract §5.2 `overlapCircle`.
- **Scalar helpers: MISSING.** `axiom-math` exposes the finite-validation
  `Scalar` policy but **no `clamp`, `lerp`, `normalize_angle`, or trig**
  (verified: grep for `fn (clamp|lerp|normalize_angle|sin|cos)` in
  `crates/axiom-math/src` returns nothing). Games today fall back to
  `f32::clamp`/`f32::sin` from `std` — i.e. they leave the engine vocabulary.
  *(The capability-inventory doc lists these as "have"; it is wrong — this spec
  is the source of truth.)*
- **TS projection: absent.** No `clamp`/`lerp`/`normalizeAngle`, no `v2`
  namespace, no `aabbOverlap`/`pointInRect`/`circleOverlap`, no
  `overlapBox`/`overlapCircle`/`raycast`. The whole §5 author surface is unbuilt.

## 3. Architectural placement

One layer extension, one module extension, one projection — **no new layer**.

1. **Scalar helpers — extend the `axiom-math` layer.** Add `clamp`,
   `lerp`, `normalize_angle` to the `MathApi` facade. Under the Layer Law this is
   the lowest correct home: `axiom-math` already *owns the scalar policy* (the
   `Scalar` finite-validation discipline), and these are operations over that
   scalar — derived math, not a new primitive type. They are broadly used by
   every later layer, which is exactly what makes `axiom-math` (the substrate
   every layer already depends on) the right place, not a ceremonial
   `axiom-scalar` micro-layer (forbidden — a layer that only re-wraps `f32` does
   not "meaningfully transform the layers it declares").
2. **Not the kernel.** The kernel holds the dimensioned scalar *types*
   (`Meters`/`Radians`/`Ratio`) because they are shared primitives no single
   layer owns. `clamp`/`lerp`/`normalize_angle` are *core math operations*, and
   the kernel rules bar "core math" and "convenience utilities" from the kernel
   unless no layer can own them — `axiom-math` plainly owns them. Pushing the
   ops down to sit beside the types would pull math into the kernel; that is the
   inward leak the kernel rules exist to stop.
3. **`overlap_circle` — extend the `axiom-scene` module.** Add the missing
   radial overlap query next to `overlap_box`/`raycast`, built on the existing
   `Sphere`/`Aabb` primitives. It is a scene query (reads node bounds), so it
   belongs in the scene facade, not in `axiom-math`.
4. **TS projection — the `@axiom/game` SDK + `apps/axiom-game-runtime`
   (SPEC-00).** The pure helpers and the scene queries are marshalled across the
   wasm boundary by the runtime app and re-exported by the SDK. The pure helpers
   route through the **native** `MathApi`/`Aabb`/`Sphere` (one source of truth,
   deterministic arithmetic) rather than being re-implemented in TS — a sim-class
   value must be computed once, in the authoritative core.

## 4. API surface

### 4.1 Native (extends `MathApi`, sim-class, checked, branchless)

```rust
impl MathApi {
    // Scalar helpers (new). Finite-checked, consistent with the layer's policy.
    pub fn clamp(&self, v: f32, lo: f32, hi: f32) -> MathResult<f32>;
    pub fn lerp(&self, a: f32, b: f32, t: f32) -> MathResult<f32>;     // a + (b-a)*t
    pub fn normalize_angle(&self, angle: Radians) -> Radians;          // wrap to (-π, π]
}
```

`axiom-scene` (new facade method):

```rust
impl SceneApi {
    /// Every bounded node whose world bounds intersect the sphere
    /// (center, radius), ascending node-id order. Radial sibling of overlap_box.
    pub fn overlap_circle(&self, center: Vec3, radius: Meters) -> Vec<SceneNodeId>;
}
```

The existing `Aabb`/`Sphere`/`Ray`/`Vec*` algebra and
`raycast`/`raycast_hit`/`overlap_box` are unchanged — they are *projected*, not
re-implemented.

### 4.2 TS authoring projection (contract §5)

```ts
function clamp(v: number, lo: number, hi: number): number;
function lerp(a: number, b: number, t: number): number;
function normalizeAngle(a: number): number;                    // wrap to (-π, π]

const v2: {
  add(a: Vec2, b: Vec2): Vec2; sub(a: Vec2, b: Vec2): Vec2;
  scale(a: Vec2, s: number): Vec2; dot(a: Vec2, b: Vec2): number;
  len(a: Vec2): number; normalize(a: Vec2): Vec2;
  dist(a: Vec2, b: Vec2): number; lerp(a: Vec2, b: Vec2, t: number): Vec2;
};
// v3 / mat4 / quat: full 3D equivalents — projected by SPEC-11 (3D scene surface).

// Pure predicates (stateless):
function aabbOverlap(a: Rect, b: Rect): boolean;
function pointInRect(p: Vec2, r: Rect): boolean;
function circleOverlap(aCenter: Vec2, aR: number, bCenter: Vec2, bR: number): boolean;

// Scene queries (read committed transforms for the current tick — see §6):
function overlapBox(center: Vec3, halfExtents: Vec3): Entity[];
function overlapCircle(center: Vec2, radius: number): Entity[];
interface RayHit { entity: Entity; point: Vec3; distance: number }
function raycast(origin: Vec3, dir: Vec3, maxDistance: number): Result<RayHit>;  // nearest
```

## 5. Data contracts

- **Value records crossing the boundary:** `Vec2`/`Vec3`/`Rect` (plain
  number records, §0.2) and `RayHit { entity, point, distance }`. `Vec*` map to
  the native `Vec2`/`Vec3`; `Rect` is a 2D AABB projected through `Aabb`.
- **`Entity`** is the opaque handle from SPEC-00/02's handle table; scene queries
  return `Entity[]` resolved from `SceneNodeId`. Handles are never serialized
  into sim state.
- **`Result<RayHit> = RayHit | null`** — a miss is `null`, never a throw (§0.2).

## 6. Determinism (sim)

- **Pure helpers** (`clamp`/`lerp`/`normalize_angle`, `aabbOverlap`/
  `pointInRect`/`circleOverlap`, all `Vec*` algebra) are stateless deterministic
  `f32` arithmetic — no wall clock, no RNG, no ambient state. `normalize_angle`
  is implemented by arithmetic range-reduction (floor/remainder), **not** `sin`/
  `cos`, so it is exact and portable across machines (§17.6).
- **Scene queries** (`overlapBox`/`overlapCircle`/`raycast`) read the scene's
  **committed, propagated world transforms for the current tick** — they must run
  after `SceneApi::advance` / `update_world_transforms` for that tick. Within a
  tick they are read-only and order-stable (ascending node id), so identical
  `(seed, config, input)` yields identical query results and an identical
  per-tick state hash on replay (§17.4).
- Nothing here reads presentation time or feeds presentation values back into
  sim.

## 7. Acceptance / proof

- **Native, 100% covered + branchless** (the layer's standing discipline):
  finite-rejection arms for `clamp`/`lerp`; mutation-killing identities
  (`lerp(a,b,0)=a`, `lerp(a,b,1)=b`, `clamp` at both bounds and the interior);
  `normalize_angle` range `(-π, π]`, idempotence (`f(f(x)) == f(x)`), and
  equivalence at ±π boundary. `overlap_circle` parity tests against `overlap_box`
  on inscribed/circumscribed cases, plus a propagation-order test (query before
  vs after `advance`).
- **Replay/golden (sim):** a held scene advanced to tick N produces a byte-equal
  query result on a second run; the per-tick state hash sequence reproduces.
- **TS projection:** tsgo + Oxlint (branch ban) + 100% `node:test` coverage. A
  cross-check test asserts the TS surface and the native core agree on a vector
  of sample inputs (the projection adds no second implementation to drift).

## 8. Dependencies & order

- **Scalar helpers have no new dependency** — they extend `MathApi` and can land
  natively immediately (contract §18 step 3, "entities, hierarchy & math").
- **`overlap_circle`** depends only on the existing scene bounds + `Sphere`.
- **The TS projection depends on SPEC-00** (handle tables, the wasm boundary app,
  the `@axiom/game` package) and on **SPEC-02** for `Entity`/`World` so scene
  queries can return `Entity[]`.
- **3D math namespaces** (`v3`/`mat4`/`quat`) are projected by **SPEC-11**, not
  here — this spec projects the scalars, `v2`, the pure predicates, and the scene
  queries.

## 9. Open questions

- **Naked `f32` vs the house rule.** Public engine APIs must not take naked
  `f32` where a dimensioned/typed quantity fits. `normalize_angle` resolves
  cleanly (`Radians → Radians`). `clamp`/`lerp` are deliberately
  *dimension-generic* — they apply to any scalar quantity, so no single
  dimension fits and a naked `f32` is the honest type. The alternative — a
  generic `Scalar<Q>` clamp/lerp over `Meters`/`Radians`/`Ratio` — buys type
  safety at the cost of a much larger surface. Lean: keep `f32` for the two
  generic ops, dimensioned types where a dimension genuinely fits, and revisit if
  a caller is shown mixing units through `lerp`.
- **`overlapCircle` in a 3D scene.** The contract types it as 2D
  (`center: Vec2, radius`). Native maps it to a sphere/`Vec3` overlap; the open
  question is which plane the 2D circle inhabits (z = 0? the camera plane?) and
  whether the native method should be `overlap_circle` (2D) or `overlap_sphere`
  (3D) with the SDK projecting the 2D form. Decide alongside SPEC-04 (the 2D
  surface) which fixes the 2D world convention.
- **`clamp` with `lo > hi`.** Define as a checked error vs. silent swap.
  Lean: checked `MathResult` error, consistent with the layer's reject-bad-input
  policy (no silent correction).
- **Pure-predicate placement.** `aabbOverlap`/`pointInRect`/`circleOverlap` route
  through native for one deterministic source of truth; if profiling shows the
  boundary cost dominates for hot per-frame UI checks, a *presentation*-only TS
  fast-path could be added — but never for a value that re-enters sim.
