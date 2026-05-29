# Axiom Math — Layer 02 Architecture

`axiom-math` is the deterministic numeric and geometric substrate of the
Axiom engine. It is the third layer in the chain:

```
Layer 00  axiom-kernel    (time, identity, errors, binary, logging, telemetry)
Layer 01  axiom-runtime   (lifecycle, fixed-step scheduling, queues, context)
Layer 02  axiom-math      ← this crate
```

Every later layer (rendering, scene, physics, animation, culling, picking,
tooling, editor) will build on the primitives this layer defines. Layer 02 is
therefore the last place where it is reasonable to enforce determinism,
checked failure, and a small public surface "by hand"; once higher layers
start importing math, those properties have to hold uniformly.

## What Layer 02 is

A small library of *deterministic, allocation-free, browser-free* math types
and the checked operations on them:

- **Scalar policy** — `Scalar`, `Epsilon`, `ApproxEq` define the engine's
  finite-`f32` discipline and the rule for approximate equality.
- **Math error model** — `MathError`, `MathErrorCode`, `MathResult` give
  every fallible math operation the same `(code, optional kernel cause)`
  identity. The kernel binary-reader's `KernelError` is preserved by value
  so deserialization failures keep their root cause.
- **Vectors** — `Vec2`, `Vec3`, `Vec4` with component-wise IEEE-754
  arithmetic, checked scalar division, checked normalization, dot/cross,
  and binary round-trip through the kernel's `BinaryWriter`/`BinaryReader`.
- **Quaternion** — `Quat` with the canonical Hamilton product, axis-angle
  construction, checked inverse/normalize, and a unit-quaternion `rotate`.
- **4×4 matrix** — `Mat4` in column-major storage with translation, scale,
  rotation-from-quaternion, right-handed `perspective` / `orthographic` /
  `look_at`, and homogeneous `transform_point` / `transform_vector`.
- **Transform** — `Transform` is the compact `T * R * S` form. It composes,
  expands to a `Mat4`, and inverts when the scale is uniform (the TRS
  structure is not closed under non-uniform inverse, so that case is
  rejected explicitly).
- **Geometry primitives** — `Aabb`, `Sphere`, `Ray`, `Plane`, `PlaneSide`,
  and `Frustum`. Each carries the validation that keeps it sound (`min <=
  max`, `radius >= 0`, unit ray direction, unit plane normal, six-plane
  frustum extracted from a clip-from-world matrix), and the small set of
  intersection/containment tests rendering and culling layers will need.

The entire public surface is the single facade `MathApi`. `lib.rs` exports
nothing else.

## What Layer 02 is not allowed to know

The kernel's hard rules apply to math directly, and the layer's tests
(`tests/architecture.rs`) enforce them mechanically:

- **No browser / DOM / JS APIs.** No `wasm_bindgen`, no `web_sys`, no
  `Math.random`, no `window`/`document`.
- **No WebGPU / WebGL.** No `wgpu`, no `WebGL`, no GPU resource types.
- **No higher engine concepts.** No world/scene, no renderer/material/mesh,
  no asset loader, no physics body/collider, no animator, no audio source,
  no input/key/mouse types, no plugin/editor/game-loop scaffolding. Any of
  these appearing as a symbol fails the build.
- **No wall-clock time.** No `std::time::SystemTime`, no `Instant::now`, no
  `chrono`.
- **No randomness.** No `rand`, `thread_rng`, `getrandom`, `fastrand`.
- **No global mutable state.** No `static mut`, no `lazy_static`.
- **No console output / placeholder macros.** No `println!`, `eprintln!`,
  `dbg!`, `todo!`, `unimplemented!`.
- **No misc/utils junk drawers.** No `utils`, `helpers`, `common`, or
  `misc` module — the project's agentic-development discipline forbids them.

The kernel and runtime crates must not import math, and math must not
import any layer above `axiom-runtime`. Both directions are scanned in
`tests/architecture.rs`.

## Deterministic scalar policy

Axiom is an `f32` engine. The `Scalar` policy holder:

- pins `DEFAULT_EPSILON = 1.0e-6` as the engine-wide default tolerance,
- defines `Scalar::validate_finite(v)` as the single rejection path for
  `NaN` and `±Inf` (every checked constructor delegates to it before any
  other check),
- defines `is_finite_value(v)` as the corresponding predicate.

`Epsilon::new` rejects negative, `NaN`, and infinite tolerances; the
default `Epsilon::DEFAULT` value is `Scalar::DEFAULT_EPSILON`. `ApproxEq`
is implemented for `f32` first and reduced to component-wise `f32`
comparisons for every higher type. `ApproxEq` always returns `false` if
either operand contains a non-finite component — non-finite values cannot
be meaningfully compared, so they have to surface through a validation
path instead.

## Checked math failures

Every fallible math operation returns `MathResult<T>` (an alias for
`Result<T, MathError>`). The error identity is the pair
`(MathErrorCode, Option<KernelError>)`; the human message is metadata and
plays no role in comparison. Tests assert against codes, never against
strings, so failure paths stay machine-stable across builds.

The eight error codes (`MathErrorCode`) cover:

- `DivideByZero` — checked scalar / vector / transform division by zero.
- `NormalizeZeroLength` — normalizing a zero-length vector, quaternion, or
  plane normal.
- `NonFiniteScalar` — any scalar argument is `NaN` or `±Inf`.
- `InvalidAabbBounds` — `min > max` (or negative extents) for an AABB.
- `InvalidSphereRadius` — a negative sphere radius.
- `InvalidRayDirection` — a zero-length ray direction.
- `InvalidMatrixOperation` — degenerate `perspective`/`orthographic`/`look_at`
  parameters, or a non-uniform-scale `Transform::inverse`.
- `DeserializationFailed` — wraps a kernel binary-reader error so the cause
  is preserved.

There are no panics in production code. Every public method has a direct
unit test for the success path and at least one direct unit test per
documented failure path.

## Why math has no rendering / scene / ECS / physics concepts

Those are higher-layer responsibilities. Allowing them inside math would
have three concrete consequences and we reject all three:

1. **Determinism would weaken.** Rendering depends on a backend (WebGPU)
   that math is not allowed to know about. Scene/ECS state is global to
   the engine; math is component-level and stateless. Physics integrates
   non-deterministic vendor solvers; math is closed and reproducible.
2. **The layer law would break.** A math import of `Renderer` would be a
   forward import; the architecture checker would refuse the build.
3. **Future layers would be forced to depend on the wrong abstraction.**
   A culling layer wants `Frustum::intersects_aabb`, not a renderer
   pipeline. A picking layer wants `Ray::intersect_sphere`, not a scene
   query. Keeping math small keeps everyone else honest.

If a future system needs a primitive that does not exist here, the
correct response is to add it to this layer, not to define it in a higher
layer.

## How future layers should consume `MathApi`

`MathApi` is a zero-sized facade. Construct one once per system (or pass
the engine's existing instance) and call the constructors:

```rust
use axiom_math::MathApi;

let m = MathApi::new();
let translation = m.vec3(1.0, 2.0, 3.0);
let rotation   = m.quat_from_axis_angle(m.vec3_unit_y(), 0.7)?;
let transform  = m.transform(translation, rotation, m.vec3_one());
let camera     = m.mat4_look_at(m.vec3(0.0, 0.0, 5.0), m.vec3_zero(), m.vec3_unit_y())?;
let frustum    = m.frustum_from_view_projection(
    m.mat4_perspective(std::f32::consts::FRAC_PI_2, 16.0 / 9.0, 0.1, 1000.0)?
)?;
```

Math telemetry is deterministic and explicit. A higher layer that wants
to surface its workload through the runtime's sinks calls
`MathApi::record_validation_failure(&mut ctx)` or
`MathApi::record_intersection_test(&mut ctx)` with a `RuntimeContext`; the
metric is named, counted, and tick-stamped through the kernel facade.
There is no other logging or telemetry path in this layer — math does not
print, does not open files, and does not allocate background work.
