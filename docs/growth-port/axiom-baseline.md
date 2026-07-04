# Axiom capability baseline (today)

What Axiom genuinely provides right now, per crate/module, with real symbol names. This is the honest starting line for the gap analysis. Verified by reading source on 2026-06-19; file paths are given so claims are checkable.

Legend: ✅ exists and works · 🟡 partial / demo-only · ❌ absent.

## Layers (`crates/`) — the deterministic spine

### `axiom-kernel`
- ✅ Deterministic time: `FixedStep` (integer nanos), `SimulationClock` (advances only by explicit `advance`/`advance_by`), `Tick`, `FrameIndex`.
- ✅ Identity: `EntityId` (u64), `HandleId`, `AssetId`, `ResourceId`, `MessageId`, `LayerId` — monotonic, ordered.
- ✅ Deterministic RNG: `DeterministicRng::seeded(u64)` — **splitmix64**; `next_u64`, `next_bounded` (Lemire), `next_bool_in_thousand`. **❌ no float sampling** (`next_f32`/range/normal) — caller must convert.
- ✅ Serialization: `BinaryReader`/`BinaryWriter` (little-endian), `SchemaVersion`, `TypeSchema`, `Reflect` trait (compile-time `const SCHEMA`).
- ✅ Dimensioned scalars: `Meters`, `Radians`, `Ratio`; float finiteness validation.
- ✅ Logging/telemetry sinks (no ad-hoc printing).
- ❌ No noise, no spherical math, no fixed-point. (Kernel deliberately excludes these.)
- File: `crates/axiom-kernel/src/lib.rs` (curated export list enforced by `tests/architecture.rs`).

### `axiom-runtime`
- ✅ `Runtime` + `RuntimeScheduler`: explicitly **ordered** systems (`(HandleId, name, order: i32)`), lifecycle state machine (`Created→Initialized→Running⇄Paused→Stopped`), FIFO `RuntimeCommandQueue`/`RuntimeEventQueue`, `RuntimeStepRecord` (replay audit).
- ❌ No parallel execution, no dynamic (runtime) system registration, no system dependency graph (explicit order only).
- File: `crates/axiom-runtime/src/runtime.rs` (`#[sim]` zone).

### `axiom-math`
- ✅ `Vec2/3/4`, `Quat` (mul/conjugate/inverse/slerp/look_rotation), `Mat4` (perspective, look_at, inverse, determinant), `Transform`.
- ✅ Geometry: `Aabb`, `Sphere`, `Ray` (intersect aabb/sphere), `Plane`, `Frustum` (sphere/aabb tests). Basic culling math present.
- ❌ **No noise (Perlin/Simplex/Worley/FBM). No spherical/geodesic/icosphere math. No fixed-point. No matrix T/R/S decomposition. No splines.**
- File: `crates/axiom-math/src/lib.rs` (single `MathApi` facade).

### `axiom-ecs`
- ✅ `World<S>` generic over a user storage struct; sparse-per-type `ComponentColumn<T>` (`BTreeMap<EntityId,T>`, ordered iteration); `WorldSystem<S>` trait; `Startup`/`Update` phases; world serialize/deserialize + `describe()` reflection; frame-gated `advance`.
- ❌ **No queries/filters** (systems get raw storage; filter manually), **no spatial index**, **no change detection**, **no hierarchical transforms** (user components), static system set, **no entity spawn mid-frame** (spawn upfront / at boundaries).
- Scale: ordered `BTreeMap` storage — correct and deterministic, not cache-optimised for very large hot iteration.
- File: `crates/axiom-ecs/src/world.rs`.

### `axiom-frame`, `axiom-host`, `axiom-introspect`, `axiom-zones`
- ✅ `axiom-frame`: per-frame immutable `EngineFrame`, `FrameContext`, timing validation, lifecycle gating (skipped frames skip system phases).
- ✅ `axiom-host`: deterministic frame-pulse boundary — `HostFrameInput`/`HostFrameReport`, `HostStepDriver` (only caller of `Runtime::step`), `HostViewport`, lifecycle signals, a **deterministic** presentation-request boundary (`HostPresentationRequest`/`HostSurfaceHandle`, no real GPU object). ❌ **No input mapping** (only coarse lifecycle).
- ✅ `axiom-introspect`: bounded frame history, serializable `FrameReport`, `snapshot_bytes()` for external agents.
- ✅ `axiom-zones`: `#[sim]`/`#[hot_path]`/`#[strict]`/`#[supervisor]`/`#[escape_hatch]` greppable markers driving the dylint rulebook.

## Modules (`modules/`) — isolated capabilities

### `axiom-scene`
- ✅ `SceneApi`: scene graph with monotonic `SceneNodeId`, `set_parent` (cycle/self/missing checks), local/world transforms with deterministic propagation, `remove_node`, perspective camera, **directional + point lights**, renderables (opaque `MeshRef`/`MaterialRef` u64 + per-node visibility flag), a **first-person `add_controller`** (yaw + local-frame move) and `controller_command`. Emits `SceneSnapshot` (nodes/cameras/lights/renderables).
- ❌ No frustum/occlusion culling, no terrain/chunk/LOD concept.
- File: `modules/axiom-scene/src/lib.rs`.

### `axiom-resources`
- ✅ `ResourcesApi::register_mesh(name, &[(pos,normal,uv,color)], &[u32])` — **arbitrary triangle mesh**, procedural-friendly. `register_cube_mesh`. `Vertex{position,normal,uv,color}`. `basic_lit_material` (base colour). RGBA8 textures. `resolve()` → immutable `ResolvedResources`.
- 🟡 Meshes are registered then snapshotted per frame; **no streaming/fence-based dynamic vertex-buffer re-upload** path is exercised — demos bake geometry at startup and vary transforms/instances, not vertices.
- ❌ No LOD/mipmaps, no normal maps/PBR/splatmaps, no custom vertex layouts, no asset-file import (glTF/textures).
- File: `modules/axiom-resources/src/lib.rs`.

### `axiom-render`, `axiom-webgpu`, `axiom-render-pipeline`, `axiom-windowing`
- ✅ Render path: `RenderInput` (camera/meshes/materials/lights/objects) → branchless `RenderCommandList` (`CLEAR/SET_CAMERA/SET_PIPELINE/SET_MESH/SET_MATERIAL/DRAW_INDEXED`) → `GpuSubmission` → `GpuSubmissionReport`. Deterministic **recording** backend always; **live `wgpu`** arm compiled for `wasm32` in `axiom-windowing` (real browser presentation works — see retro FPS).
- ✅ `axiom-render-pipeline` (feature module) composes scene+resources+render+webgpu into a per-frame `submit(...) → RenderReport` (incl. `GL_TO_WGPU` depth remap).
- ✅ Lighting: directional + point, **basic-lit diffuse only**. ✅ Instancing **backend** proven at scale (stress-cubes ≈ up to 200k cubes).
- ❌ No frustum culling in the render module (app must pre-filter), one draw per object in the command model, no terrain/PBR/normal-mapped shaders, no shadows.
- ✅ `axiom-windowing`: surface config, `run_web` RAF loop, real `wgpu`/`web-sys` binding on wasm. ❌ Input is captured **in the app's `web.rs`**, not the engine.

### `axiom` (top-level facade, feature module)
- ✅ App framework: `App::new().window(..).add_plugins(DefaultPlugins).setup(closure)` → `RunningApp`; `RunningApp::tick(tick) → FrameOutcome` (headless) / `run()` (wasm RAF). `prelude` exposes `Transform`, `Mesh`, `Material`, `Camera`, `DirectionalLight`, `Controller`/`FirstPersonInput`, `Player`/`PlayerInput`, `Renderable`, `Spin`, `SceneCommands` (spawn/with_child), `Assets<Mesh>`/`Assets<Material>`.
- ❌ No implicit game loop/`main`, no input system, no save/load, no asset pipeline, no gameplay systems — all app-owned by design.

### `axiom-netcode` / `axiom-net-protocol` / `axiom-client-core`
- ✅ Deterministic **signed lockstep**: per-peer input timeline, readiness gate, Ed25519-signed + schema-versioned wire codec, BLAKE3 per-tick state-hash reconciliation, adversarial-network test harness.
- ❌ No rollback, no client-side prediction, no interpolation/reconnect (desync halts). Server-authoritative netplay demo feeds snapshots from a real Axiom host.

## Apps (`apps/`) — evidence of game-readiness
- ✅ **`axiom-retro-fps-browser`**: a genuinely playable FPS in the browser — keyboard/mouse → deterministic `RetroFpsGame::step(intent)` → engine `tick_with_controls` → render; real **raycasting** (hitscan), grid **collision** with wall-sliding, simple enemy AI, health/score/respawn, live hot-reload of a `level.axiom` doc. **Proves input→deterministic-sim→render works end to end.**
  - But: geometry is **cubes/instances only** — no procedural mesh generation, no dynamic vertex buffers; the level is hand-authored, not generated.
- ✅ `axiom-stress-cubes-browser` (instancing scale), `axiom-demo-rotating-cube[-browser]` (the canonical vertical slice), netplay/netcode demos.

## Build / run / verify
- Native: `cargo test --workspace` (apps build headless; sims/tests run natively). WASM: `wasm-pack build --target web apps/<app>`; served statically (+ `axiom-dev-reload` for hot reload). Browser verification via `scripts/playwright_controller.py` (Chromium, console, screenshots).

## Hard invariants that shape everything built here
- **Branchless spine**: zero `if/match/for/while/loop/&&/||/?` in non-test code of any **layer or module** (`engine_no_branching` dylint, baseline 0). Apps and tooling are exempt.
- **100% coverage** on every layer and module (regions/lines/functions). Apps and tooling are exempt.
- **Layer Law / Module Law**: layers form a DAG and use only lower layers; engine modules never import other modules (feature modules may compose a declared set); only apps/feature-modules glue contracts together; one public facade per module.
- **No `thread::spawn`** in the spine (`engine_no_thread_spawn`), no `static mut`, no transmute/uninit, no wildcard imports, no recursion, no large files/fns/structs, module docs required, no `utils/helpers/common/misc`.
- **Determinism**: no wall-clock, seeded RNG only, stable iteration order, replayable.
