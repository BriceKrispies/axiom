# Axiom Math — Testing Discipline

`axiom-math` owns the engine's deterministic math substrate. Its testing rules
are deliberately conservative — once rendering, physics, and scene layers
start trusting these primitives, regressions here are very expensive.

## Every public math concept needs direct tests

A "public concept" is any item that can be reached from outside the math
crate. In practice that is:

- every method on `MathApi` (the only thing `lib.rs` exports),
- every public constant, constructor, and method on each module's primary
  public type (`Vec2`, `Vec3`, `Vec4`, `Quat`, `Mat4`, `Transform`, `Aabb`,
  `Sphere`, `Ray`, `Plane`, `PlaneSide`, `Frustum`, `Epsilon`, `Scalar`,
  `MathError`, `MathErrorCode`, `MathResult`),
- the `ApproxEq` impl for each of those types.

The discipline:

> **If a public item is not directly unit-tested, it is removed.**

A test that only proves "this method does not panic" is not enough. Each
test must assert a specific deterministic outcome — a returned value, a
specific error code, an equality, an intersection result. Boolean
`is_some()`/`is_err()` checks are acceptable only when paired with an
assertion on the value or code inside.

## Approximate equality policy

Approximate comparisons go through `ApproxEq` with an explicit `Epsilon`.
The default tolerance is `Scalar::DEFAULT_EPSILON = 1e-6`; tests that
compose several float operations (matrix multiplications, quaternion
rotations) use a slightly looser tolerance — typically `1e-5` or `1e-4`
— constructed via `Epsilon::new(...)` so the choice is explicit.

`ApproxEq` returns `false` whenever either operand contains a `NaN` or
`±Inf` component. Tests rely on this directly to prove that non-finite
inputs cannot be compared — they have to be rejected through the
validation paths instead.

## Finite scalar policy

Every checked constructor validates finiteness before it does anything
else. Tests are required for:

- **happy path** — a finite input produces the expected value;
- **`NaN`** — rejected with `MathErrorCode::NonFiniteScalar`;
- **`±Inf`** — rejected with `MathErrorCode::NonFiniteScalar`;
- **structural failure** — the specific code documented for that
  operation (e.g. `DivideByZero`, `NormalizeZeroLength`,
  `InvalidAabbBounds`, `InvalidSphereRadius`, `InvalidRayDirection`,
  `InvalidMatrixOperation`).

These tests are how the layer guarantees that nothing reaches a higher
layer with a silently-wrong value.

## Deterministic geometry tests

Geometry primitives (`Aabb`, `Sphere`, `Ray`, `Plane`, `Frustum`) test
both the *positive* and *negative* shape of every predicate:

- containment — point inside, point outside, point on the boundary;
- overlap — touching, fully disjoint, partially overlapping;
- ray intersection — direct hit, behind-origin miss, side-glance miss,
  origin-inside hit;
- frustum culling — point/AABB/sphere in front of camera, behind camera,
  beyond the far plane.

Each test uses fixed numeric inputs so it is byte-for-byte reproducible
across platforms.

## Serialization round-trip requirements

Every type that exposes `write_to` / `read_from` must have a round-trip
test:

1. construct a value with non-trivial components,
2. serialize it via `KernelApi::binary_writer()`,
3. deserialize it via `KernelApi::binary_reader(bytes)`,
4. assert the recovered value `approx_eq` the original.

Constructors that validate (e.g. `Aabb`, `Sphere`) also have a *negative*
round-trip test that proves a hand-crafted byte sequence with an invalid
shape is rejected with the correct math error code, not silently
accepted.

## Architecture tests

`tests/architecture.rs` mechanically enforces the rules in this layer's
`ARCHITECTURE.md` by scanning the source tree (with comments and string
literals stripped so that forbidden tokens in documentation cannot fail
the build). It asserts:

- no browser / DOM / JS / WebGPU / WebGL / `wgpu` / `winit` / `bevy`
  references,
- no wall-clock APIs (`std::time`, `SystemTime`, `Instant::now`,
  `chrono`),
- no randomness (`rand`, `thread_rng`, `getrandom`, `fastrand`),
- no console output or placeholder macros (`println!`, `eprintln!`,
  `print!`, `eprint!`, `dbg!`, `todo!`, `unimplemented!`),
- no global mutable state (`static mut`, `lazy_static`),
- no module named `utils`, `helpers`, `common`, or `misc`,
- `lib.rs` exports exactly one item: `MathApi`,
- `axiom-kernel` and `axiom-runtime` do not import `axiom_math`,
- `axiom-math` imports only `axiom_kernel` and `axiom_runtime`,
- no symbol from a higher engine layer (`World`, `Scene`, `Renderer`,
  `Material`, `Mesh`, `Asset`, `Physics`, `Animator`, `Audio`, `Input*`,
  `Plugin`, `EditorPanel`, `GameLoop`) appears anywhere in math source.

The workspace's `cargo xtask check-architecture` test
(`real_repo_layers_pass`) validates the real `crates/axiom-math/layer.toml`
against the Axiom Layer Law on every workspace test run, so the manifest
on disk cannot drift from the code.
