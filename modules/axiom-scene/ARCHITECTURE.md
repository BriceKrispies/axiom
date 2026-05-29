# Axiom Scene — Engine Module Architecture

`axiom-scene` is **a module, not a layer**. It is the first real
engine-feature module in the Axiom workspace and the template every
future module should follow.

```
Layer 00  axiom-kernel
Layer 01  axiom-runtime
Layer 02  axiom-math
Layer 03  axiom-host
Layer 04  axiom-frame
─────────── ↓ modules read from layers, never the other way around ───────────
Module    axiom-scene   (this crate)
```

## What `axiom-scene` is

The deterministic 3D scene graph. The module adapts the layer spine
([`axiom_math::MathApi`] transforms + [`axiom_frame::FrameContext`]
stepping) into concrete spatial state:

```text
MathApi transforms + Frame/Runtime stepping
    →  deterministic scene graph state and stable scene snapshots
```

Concretely the module owns:

- node storage and stable [`SceneNodeId`]s (monotonic, never reused),
- parent/child topology with cycle and self-parenting detection,
- per-node local + cached world [`axiom_math::Transform`]s,
- deterministic world-transform propagation
  (`world = parent_world * local`),
- perspective cameras (intrinsics validated through
  [`axiom_math::MathApi::mat4_perspective`]),
- directional and point lights (colour and intensity validated
  through [`axiom_math::MathApi::validate_finite`]),
- opaque mesh / material references plus a visibility flag,
- deterministic `SceneSnapshot`s built from any of the above,
- a tiny opt-in `SceneApi::advance(&mut Scene, &FrameContext)`
  integration that re-propagates world transforms on each non-skipped
  engine frame.

The single public export from `lib.rs` is `SceneApi`. Every other type
is reachable only through the facade.

## What `axiom-scene` is not allowed to know

- Rendering / render-graph / shaders / materials data / GPU resources —
  scene only carries opaque `MeshRef` / `MaterialRef` u64s that a
  future renderer or app resolves.
- Assets / file loading — no `std::fs`, no `OpenOptions`, no asset
  loader concepts.
- WebGPU / WebGL / `wgpu` / `winit` / `egui` / `bevy` — the module
  compiles to a pure Rust `rlib`.
- Browser APIs — no `web_sys`, no `js_sys`, no `wasm_bindgen`, no
  DOM/canvas, no `requestAnimationFrame`, no `performance.now`.
- Wall-clock time — no `std::time`, no `SystemTime`, no `Instant::now`,
  no `chrono`.
- Randomness — no `rand`, no `thread_rng`, no `getrandom`.
- ECS / world / entity-component frameworks — scene is a typed,
  hand-rolled graph, not a generic ECS.
- Physics / animation / audio / input / plugin / editor / gameplay —
  every one of these is a separate concern that belongs in its own
  future module or layer.
- Global mutable state — no `static mut`, no `lazy_static`.

The module's `tests/architecture.rs` scans the source tree for every
one of these and fails the build if a regression appears. The
workspace-level `cargo xtask check-architecture` enforces the same
class-aware rules through the module law.

## Why this module does not import other modules

The Axiom Module Law (see repo-root `CLAUDE.md`) says **modules may
not depend on other modules** today. The rationale, applied here:

- A render module that needs scene data should consume a
  `SceneSnapshot`, **not** the scene crate itself. Snapshots are
  plain values that don't drag in the scene module's mutation API.
- An asset module that wants to expose mesh data to scene should hand
  out `u64` resource ids (resolvable to its own internal handles);
  scene carries those as opaque `MeshRef` / `MaterialRef` values
  without knowing what they mean.
- An app, on the other hand, is the *only* place where multiple
  modules legally compose. A future "rotating cube" app might pull
  `axiom-scene`, `axiom-render`, and `axiom-assets`, and wire them
  together — but each of those modules stays unaware of the others.

If two modules ever need to share a primitive, that primitive belongs
in a lower **layer**, not in a third module.

## Why render / assets are represented only as opaque references

A scene needs to *describe* what to draw — "the node at id 7 has mesh
ref 42 and material ref 99" — without owning either the GPU mesh or
the material's shader graph. Owning either would:

- pull rendering / asset / I/O concerns into the scene crate, and
- prevent the scene from being usable as a pure data input to replay
  tests, headless harnesses, and parallel renderers.

`MeshRef(u64)` and `MaterialRef(u64)` are deliberate stand-ins. A
future resource module will define what those IDs mean. The scene
module itself only checks they are non-zero (the sentinel `0` is
rejected as `InvalidRenderableReference`).

## How it consumes `axiom-math`

Math is the foundation of every spatial concept here:

- `Transform` is the per-node local and world transform type.
- `Transform::combine(parent, child)` is the propagation primitive
  used by `Scene::update_world_transforms`.
- `MathApi::mat4_perspective` is the **single** path camera intrinsics
  are validated through; failures are wrapped as
  `SceneError::invalid_camera_parameters` with the underlying
  `MathError` preserved.
- `MathApi::validate_finite` is the single path light colour and
  intensity components are checked through.
- `Vec3` is the colour and translation primitive.
- `Mat4` is the projection-matrix output of
  `SceneApi::camera_projection_matrix`.

There is no parallel finite-validation or projection-math
implementation in this crate.

## How it may consume `axiom-runtime` / `axiom-frame`

`axiom-frame` is the integration point: `SceneApi::advance(&mut Scene,
&FrameContext)` reads the engine frame's lifecycle / step-count facts
and re-propagates world transforms only when the frame is active and
ran at least one runtime step. Skipped frames (hidden, suspended,
shutdown-requested) leave the scene untouched.

The `axiom-runtime` dependency is currently transitive (through
`axiom-frame`'s `RuntimeStepRecord` summaries). The scene module does
not call `Runtime::step` itself — the host boundary already does that;
scene only consumes the resulting frame contract.

The integration is **clean and opt-in**: an app or future module that
wants per-frame propagation calls `advance`; one that wants explicit
control calls `update_world_transforms` directly. Neither path is
forced on the other.

## Deterministic transform propagation

`Scene::update_world_transforms` is the deterministic propagation
core. It:

1. collects roots in ascending `SceneNodeId` order,
2. walks each subtree with an iterative LIFO stack so traversal depth
   is bounded by the tree's depth, not the call stack,
3. processes siblings in ascending `SceneNodeId` order so two scenes
   constructed by the same operation sequence produce byte-identical
   world transforms,
4. computes `world = parent_world * local` via `Transform::combine`,
5. surfaces any structural error (missing node visited during the
   walk) as `SceneError::hierarchy_update_failed`.

Children are stored in a `BTreeSet<SceneNodeId>` so iteration order
is by ascending id on every platform.

## Hierarchy and cycle rules

- A node has at most one parent.
- A node has zero or more children.
- `set_parent(child, parent)` rejects:
  - `child == parent` → `SelfParenting`
  - any cycle (walking from `parent` upward reaches `child`) →
    `HierarchyCycle`
  - missing `child` or `parent` id → `MissingNode`
- `clear_parent` always succeeds for a valid node id; the node's
  local transform is preserved (only world propagation changes on the
  next update).
- `remove_node` detaches the node's children (they become roots),
  removes the node from its parent's child set, and removes every
  camera, light, and renderable attached to it. The ID is never
  reused.

## How future apps and modules should consume `SceneSnapshot`

`SceneSnapshot` is the typed contract every future renderer, debug
overlay, replay sink, picking system, or test harness should read
from:

```rust
use axiom_scene::SceneApi;
use axiom_math::MathApi;

let scene_api = SceneApi::new();
let math      = MathApi::new();

let mut scene = scene_api.empty_scene();
let root = scene_api.create_node(&mut scene);
let child = scene_api.create_node_with_transform(
    &mut scene,
    /* local */ axiom_math::Transform::IDENTITY,
);
scene_api.set_parent(&mut scene, child, root)?;
scene_api.update_world_transforms(&mut scene)?;

let snapshot = scene_api.snapshot(&scene);
for node in snapshot.nodes() {
    // every consumer reads frame-stable, deterministic data here
}
```

A future renderer:

1. receives a `&SceneSnapshot` for the engine frame,
2. iterates `snapshot.cameras()`, `snapshot.lights()`,
   `snapshot.renderables()` in their (deterministic) order,
3. resolves each `MeshRef` / `MaterialRef` through whatever resource
   system the app composed,
4. submits draw work to the GPU.

A future test harness:

1. builds two scenes from the same `SceneApi` operation sequence,
2. takes both snapshots,
3. asserts they are byte-equal as plain values.

The snapshot has no other shape because it owns no other concerns.
