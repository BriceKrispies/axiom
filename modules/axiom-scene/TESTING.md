# Axiom Scene — Testing Discipline

`axiom-scene` is the first real engine module. Once renderer / picking
/ debug overlay layers depend on the deterministic snapshot contract,
any nondeterminism here multiplies through every consumer. The tests
have to prove that the module's contract is deterministic — same
operations in, same `SceneSnapshot` out, on every platform.

## Direct tests for every public scene concept

A "public concept" is any item reachable from outside the crate.
After lib.rs curation that is:

- every method on `SceneApi` (the only crate-level export),
- every method on the internal types reached *through* the facade:
  `Scene`, `SceneNode`, `SceneNodeId`, `Camera`, `CameraId`, `Light`,
  `LightKind`, `LightId`, `Renderable`, `RenderableId`, `MeshRef`,
  `MaterialRef`, `SceneSnapshot`, `NodeSnapshot`, `CameraSnapshot`,
  `LightSnapshot`, `RenderableSnapshot`, `SceneError`,
  `SceneErrorCode`, `SceneResult`.

The rule:

> **If a public item is not directly unit-tested, it is removed.**

Tests that only prove "this method does not panic" are not enough.
Every assertion is on a deterministic outcome — a returned id, a
specific error code, an equality, a snapshot.

## Hierarchy tests

`SceneApi::set_parent` and `SceneApi::clear_parent` are tested for:

- both-sides linking (`child.parent == Some(p)` and `p.children
  contains child`);
- detachment leaves the child's local transform untouched;
- self-parenting fails with `SelfParenting`;
- cycle creation fails with `HierarchyCycle` (the chain
  `a → b → c`, then `set_parent(a, c)`);
- missing parent / missing child fails with `MissingNode`;
- `remove_node` detaches the node's children (they become roots),
  removes any attached cameras/lights/renderables, and never reuses
  the id.

## Transform propagation tests

`SceneApi::update_world_transforms` is tested for:

- child inherits parent translation (`p (1,0,0) + c (0,2,0) →
  world (1,2,0)`);
- child inherits parent uniform scale (`p scale 2 × c translate (1,0,0)
  → world (2,0,0)`);
- repeated propagation with no changes is idempotent;
- the test-suite-level "identical construction produces identical
  snapshots" run proves byte-equal output across two builds.

The propagation algorithm visits parents before children and processes
siblings in ascending [`SceneNodeId`] order, both of which are
mechanically enforced by the iterator and the `BTreeMap`/`BTreeSet`
storage backing it.

## Camera tests

`Camera::perspective` and `SceneApi::add_perspective_camera` are tested
for:

- happy path with valid intrinsics produces a camera whose accessors
  echo the supplied values;
- `near <= 0` is rejected as `InvalidCameraParameters`;
- `far <= near` is rejected;
- `aspect <= 0` is rejected;
- `fovy <= 0` and `fovy == NaN` are rejected;
- a camera attached to a node id that does not exist fails with
  `MissingNode`;
- `camera_projection_matrix` produces byte-identical `[f32; 16]`
  arrays across two calls.

## Light tests

`Light::directional` / `Light::point` / `SceneApi::add_directional_light`
/ `SceneApi::add_point_light` are tested for:

- both kinds construct cleanly with valid parameters;
- negative intensity fails with `InvalidLightParameters`;
- `NaN` intensity fails;
- negative or `NaN` colour components fail;
- a zero-intensity light is accepted (it is a valid no-op);
- a light attached to a missing node fails with `MissingNode`;
- removed lights disappear from the snapshot.

## Renderable / mesh / material reference tests

`Renderable::new` / `SceneApi::add_renderable` are tested for:

- happy path with valid refs;
- the sentinel `MeshRef::INVALID` is rejected;
- the sentinel `MaterialRef::INVALID` is rejected;
- attachment to a missing node fails with `MissingNode`;
- toggling `set_visible` round-trips;
- `MeshRef` and `MaterialRef` `from_raw` / `raw` round-trips are
  byte-stable.

## Deterministic snapshot tests

`SceneSnapshot::from_scene` (reached through `SceneApi::snapshot`) is
tested for:

- empty scene → empty snapshot;
- node entries are in ascending `SceneNodeId` order;
- camera / light / renderable entries are present and counted
  correctly;
- the child's world transform on the snapshot reflects the
  propagation (`world (1, 2, 0)`);
- identical scene construction produces equal snapshots
  (`assert_eq!(make(), make())`).

## Frame integration tests

`SceneApi::advance(&mut Scene, &FrameContext)` is tested through real
host frame reports (using the `axiom-host` dev-dependency to build
input):

- an active host frame with one runtime step → world transforms are
  propagated;
- a skipped host frame (hidden lifecycle) → world transforms stay at
  their default values, propagation does **not** run;
- in both cases the snapshot returned reflects the post-advance state.

## Error path tests

Every error code has a direct test:

- `MissingNode` — through `node()`, `set_parent`, `clear_parent`,
  `set_local`, `add_camera`/`add_light`/`add_renderable`, `remove_node`.
- `MissingCamera` / `MissingLight` / `MissingRenderable` — through
  `remove_camera`/`remove_light`/`remove_renderable`.
- `SelfParenting` — through `set_parent(n, n)`.
- `HierarchyCycle` — through the `a → b → c` chain.
- `InvalidCameraParameters` — through `add_perspective_camera` with
  bad intrinsics; the wrapped `MathError` is preserved.
- `InvalidLightParameters` — through `add_*_light` with negative or
  non-finite values.
- `InvalidRenderableReference` — through `add_renderable` with the
  sentinel ref.
- `HierarchyUpdateFailed` — covered by the structural invariants of
  `Scene::update_world_transforms`; the error shape is pinned via the
  shorthand constructor.

## Logging / telemetry determinism

The module ships **zero** ambient logging or telemetry. Diagnostics
flow from the runtime through the frame contract; scene only acts on
the data the frame contract carries. If a future iteration adds
scene-level telemetry it must follow the same rule as math/host/frame:
counts only, tick-stamped, routed through `KernelApi` and
`TelemetrySink`, and proven deterministic with a counter-equality
test.

## Architecture / boundary tests

`tests/architecture.rs` mechanically enforces the module law inside
this crate by scanning the source tree (comments and string literals
stripped). It asserts:

- `module.toml` exists and declares `allowed_modules = []`;
- `lib.rs` publicly exports exactly `pub use scene_api::SceneApi;`;
- the source tree imports only `axiom_kernel`, `axiom_runtime`,
  `axiom_math`, `axiom_frame`, and `axiom_host` (only as a test-only
  dev-dependency);
- no other module's import prefix appears in the source;
- no layer imports `axiom_scene` (kernel, runtime, math, host,
  frame source trees are all scanned);
- no `web_sys`, `js_sys`, `wasm_bindgen` / `wasm-bindgen`;
- no DOM / canvas / browser globals;
- no `wgpu`, `webgpu`, `WebGL`, `webgl`, `GPUDevice`;
- no `std::time`, `SystemTime`, `Instant::now`, `chrono`;
- no randomness (`rand::`, `thread_rng`, `getrandom`, `fastrand`);
- no console output or placeholder macros (`println!`, `eprintln!`,
  `print!`, `eprint!`, `dbg!`, `todo!`, `unimplemented!`);
- no global mutable state (`static mut`, `lazy_static`);
- no asset / file-loading concepts (`std::fs`, `AssetLoader`,
  `AssetServer`, `FileReader`, `OpenOptions`);
- no physics / animation / audio / input / plugin / editor / gameplay
  symbols (`Physics`, `RigidBody`, `Collider`, `Animator`,
  `Skeleton`, `Audio`, `SoundSource`, `InputState`, `KeyCode`,
  `MouseButton`, `Gamepad`, `Plugin`, `EditorPanel`, `GameLoop`,
  `rapier`, `winit`, `egui`, `bevy`);
- no `utils`, `helpers`, `common`, or `misc` modules.

`cargo xtask check-architecture` enforces the same set at the
workspace level through the module law's centralized rules.
