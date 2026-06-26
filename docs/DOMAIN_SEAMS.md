# Axiom Domain Seams

> Read-only architectural seam report. Produced by seven parallel discovery
> sub-agents (Workspace Cartographer, Dependency/Import Auditor, Domain Language
> Miner, Data Contract Hunter, Determinism/Side-Effect Auditor, Validation Seam
> Finder, Future Capability Scout). No production code was modified. Every claim
> below is backed by a cited path/symbol in the appendix.
>
> Verification at time of writing: `cargo xtask check-architecture` → **PASS**
> (exit 0). `cargo test --workspace` → see "Verification" at the end.

## Executive Summary

The most important architectural fact about Axiom today is that **its seams are
already cut, manifested, and mechanically enforced.** This is not a codebase
asking "where are the boundaries?" — it is one that has already drawn 14 layers,
21 modules (16 engine + 5 feature), 14 apps, and a tool/support tier, and pins
all of it with `layer.toml`/`module.toml`/`app.toml` manifests plus a checker, a
branchless dylint, and a 100% coverage gate. The discovery work here is therefore
mostly **confirmation and pressure-detection**, not greenfield seam-finding.

The seams that matter most, in priority order:

1. **The deterministic spine (kernel → runtime → math → host → frame → ecs →
   introspect)** is the load-bearing seam. It is clean, acyclic, and the single
   thing every other seam leans on. Protect it above all.
2. **The vertical-slice data-contract chain (SceneSnapshot → ResolvedResources →
   RenderInput → RenderCommandList → GpuSubmission)** is the engine's defining
   producer/consumer seam. It is stable and correctly app-glued today.
3. **The procedural-generation branch (space → entropy → proc → proc-validate,
   plus terrain/biome/placement domain modules composed by levelgen/worldsave)**
   is a second, parallel spine that is real and growing.
4. **The platform/side-effect boundary** — browser/GPU APIs confined to `host` +
   `windowing`/`gpu-backend`/`canvas2d-backend`/`debug-overlay` — is the seam that
   keeps the spine deterministic. It is enforced by source scans.
5. **Two emerging pressure seams** that are *not yet* domains: a missing **noise**
   layer and missing **spherical/geo math**, both blocking the Growth worldgen
   roadmap; and the **dynamic-mesh-streaming** capability (`game-world` module),
   which is blocked on an un-proven per-frame upload path.

The biggest risk is not a missing seam — it is **over-eager generalization**: app
glue (`scene_to_render_input`, `render_command_list_to_gpu_submission`) and
parallel vocabularies (lifecycle states, command/event queues) that *look* like
they want a shared abstraction but have fewer than three real consumers.

## Current Architecture Reality

What the repository actually is today (not what it aspires to be):

- **A WASM-first Rust engine with a real, enforced layer graph.** 14 crates under
  `crates/` carry `layer.toml`. The chain is **not** linear — it is a DAG with one
  ordered backbone (`kernel → runtime → {math, host} → frame → ecs → introspect`)
  and several **root-adjacent layers** that depend only on the kernel (`crypto`,
  `interface`, `space`) or on a short proc chain (`entropy` on `space`; `proc` on
  `space`+`entropy`; `proc-validate` on `proc`; `layout` on `host`).
- **A module tier with a sanctioned composition sub-tier.** 16 engine modules
  (`allowed_modules = []`, genuinely isolated) and 5 feature modules
  (`kind = "feature-module"`) that may compose a declared module set:
  `render-pipeline` (scene+render+webgpu), `windowing`
  (gpu-backend+canvas2d-backend+recording), `levelgen` (terrain+biome+placement),
  `worldsave` (levelgen), and the `axiom` umbrella prelude.
- **Apps are true leaves.** 14 apps under `apps/`, each with `app.toml`; nothing in
  the engine graph imports an app. They own all cross-module translation glue.
- **The spine is deterministic by construction and proven by replay.** No
  wall-clock, no ambient RNG, no `static mut`/`lazy_static`, no `HashMap`
  iteration in spine code — all banned by per-crate `tests/architecture.rs`.
  Nondeterminism enters only as *data* at the `host` boundary
  (`HostFrameInput::elapsed_nanos`, viewport size, lifecycle signals).
- **The spine is also branchless and 100% covered**, both enforced as hard gates
  (`engine_no_branching` dylint at baseline 0; `cargo-llvm-cov` at 100%
  regions/lines/functions), with apps and tooling deliberately scoped out.
- **Tooling and a TS SDK ring the engine without being part of it.** `xtask`,
  `tools/*` (profile-runner, proc-fuzz/inspect, dev-reload, relay/server,
  axiom-shot, the dylint rulebook), `e2e/`, `scripts/`, and
  `packages/axiom-client` (its own native gate stack) all sit outside the runtime
  dependency graph.

In short: Axiom is **structurally mature and lightly populated** — the framework
of laws is heavier and more complete than the gameplay/render content sitting on
top of it. That is the intended order (laws first), and the discovery confirms it.

## Confirmed Domain Seams

### Seam: Deterministic Kernel
- **Classification:** `Kernel`
- **Current location:** `crates/axiom-kernel/`
- **Evidence:** `layer.toml` `depends_on = []`; 41 reverse-deps (every layer/module
  imports it); `ARCHITECTURE.md`; curated `lib.rs` export set enforced by
  `tests/architecture.rs::lib_exports_are_curated_set`.
- **Owned concepts:** `FixedStep`/`Tick`/`FrameIndex`/`SimulationClock`,
  `HandleId`/`EntityId`/`MessageId`, `KernelError`/`KernelResult`,
  `BinaryReader`/`BinaryWriter`, `LogRecord`/`TelemetryMetric`/sinks,
  `DeterministicRng`, `StableHash`, dimensioned scalars (`Meters`/`Radians`/`Ratio`).
- **Inputs:** none (root). **Outputs:** the primitives above, via `KernelApi`.
- **Allowed dependencies:** none (Axiom-internal). **Forbidden:** every layer,
  module, app, and all rendering/physics/animation/asset/input/audio/scene/browser
  concepts.
- **Facade/contract:** `KernelApi` + curated value-type exports.
- **Tests enforcing it:** `crates/axiom-kernel/tests/architecture.rs` (no
  browser/JS, no wall-clock, no RNG, no console, no placeholder macros, no global
  mutable state, curated exports); `tests/facade.rs` (cross-crate facade reach).
- **Tests missing before growth:** none for current scope; any new primitive must
  ship with its own curated-export assertion.
- **Risk if ignored:** kernel bloat is the one failure the whole architecture is
  built to prevent; an "exciting" primitive landing here poisons all 41 consumers.

### Seam: Deterministic Runtime Stepping
- **Classification:** `Layer`
- **Current location:** `crates/axiom-runtime/`
- **Evidence:** `layer.toml` `depends_on = ["kernel"]`; 12 reverse-deps;
  `tests/integration.rs::full_lifecycle_and_deterministic_replay` proves
  byte-identical replay.
- **Owned concepts:** `Runtime`, `RuntimeScheduler`, `RuntimeContext`,
  `RuntimeStep`/`RuntimeStepRecord`, `RuntimeCommandQueue`/`RuntimeEventQueue`,
  `RuntimeState`.
- **Inputs:** kernel ticks/results. **Outputs:** deterministic step records, FIFO
  command/event drain, per-step log/telemetry.
- **Allowed dependencies:** `kernel`. **Forbidden:** math, host, anything above.
- **Facade/contract:** `RuntimeApi` / `Runtime`; `RuntimeStep` is a nameable cross-
  boundary contract.
- **Tests enforcing it:** `tests/integration.rs` (replay, FIFO ordering, failure
  propagation, per-step sinks); `tests/architecture.rs`.
- **Tests missing before growth:** a cooperative-job test once worldgen needs
  amortized work across ticks (Risk R3 — see Candidate Seams).
- **Risk if ignored:** runtime is the determinism anchor; a hidden timestamp or
  scheduler tie-breaker silently breaks replay everywhere downstream.

### Seam: Geometry & Math
- **Classification:** `Layer`
- **Current location:** `crates/axiom-math/`
- **Evidence:** `layer.toml` `depends_on = ["kernel", "runtime"]`; 13 reverse-deps;
  `tests/architecture.rs` bans time/RNG/HashMap.
- **Owned concepts:** `Vec2/3/4`, `Quat`, `Mat4`, `Transform`, `Aabb`, `Frustum`,
  `Plane`, `Ray`, `Sphere`, `Scalar`/`Epsilon`/`ApproxEq`.
- **Inputs:** kernel scalars; runtime telemetry (the declared, genuinely-used
  edge — math emits a `TelemetryMetric`). **Outputs:** pure deterministic geometry.
- **Allowed dependencies:** `kernel`, `runtime`. **Forbidden:** host and above.
- **Facade/contract:** `MathApi` + value types.
- **Tests enforcing it:** `crates/axiom-math/tests/architecture.rs`.
- **Tests missing before growth:** spherical/geodesic tests *if* geo math lands
  here rather than as a peer layer (see Layer Candidates).
- **Risk if ignored:** math is the largest remaining branchless-rewrite target
  (`docs/unbranching.md`); casual edits here can break the Branchless Law gate.

### Seam: Host / Platform Boundary
- **Classification:** `Layer`
- **Current location:** `crates/axiom-host/`
- **Evidence:** `layer.toml` `depends_on = ["kernel", "runtime"]`; 14 reverse-deps;
  it is the *only* layer permitted browser-adjacent vocabulary
  (`PLATFORM_FACING_LAYERS` in `crates/xtask/src/hygiene.rs`).
- **Owned concepts:** `HostStepDriver`, `HostFrameInput`/`HostFrameReport`,
  `HostViewport`/`Orientation`/`HostSafeAreaInsets`/`Pixels`,
  `HostPresentationRequest`/`HostPresentationTarget`/`HostSurfaceHandle`,
  `HostFramePacket`, lifecycle/device-capability facts.
- **Inputs:** kernel + runtime, plus *external nondeterminism as validated data*.
  **Outputs:** deterministic presentation/stepping contracts.
- **Allowed dependencies:** `kernel`, `runtime`. **Forbidden:** real GPU/surface
  objects (those bind out-of-band in adapter modules).
- **Facade/contract:** `HostApi`; `HostFramePacket` is the backend-neutral packet.
- **Tests enforcing it:** `crates/axiom-host/tests/architecture.rs` (bans web_sys/
  js_sys/wasm_bindgen/wgpu/RAF/performance.now/std::time); `tests/manifest.rs`.
- **Tests missing before growth:** golden input/output bytes if a future layer
  owns real I/O at this boundary.
- **Risk if ignored:** this is *the* determinism firewall; a real surface object or
  ambient time read here collapses the pure-spine guarantee.

### Seam: Engine Frame Contract
- **Classification:** `Layer`
- **Current location:** `crates/axiom-frame/`
- **Evidence:** `layer.toml` `depends_on = ["kernel", "runtime", "host"]`;
  `TESTING.md` documents `repeated_builder_use_with_identical_input_is_deterministic`.
- **Owned concepts:** `EngineFrame`, `FrameBuilder`, `FrameContext`,
  `FrameCommandQueue`, `FrameStepSummary`, `FrameViewport`, `FrameLifecycleState`,
  `FrameDiagnostics`.
- **Inputs:** host reports + runtime step records. **Outputs:** the canonical
  immutable per-frame result every higher consumer reads.
- **Allowed dependencies:** `kernel`, `runtime`, `host`. **Forbidden:** ecs, scene,
  render.
- **Facade/contract:** `FrameApi` / `EngineFrame`.
- **Tests enforcing it:** `tests/manifest.rs`, `tests/architecture.rs`, frame
  determinism tests.
- **Risk if ignored:** `EngineFrame` is the merge point of host+runtime; vocabulary
  drift here (see "Command/Frame/Step overload" below) is where ambiguity starts.

### Seam: ECS World Model
- **Classification:** `Layer`
- **Current location:** `crates/axiom-ecs/`
- **Evidence:** `layer.toml` `depends_on = ["kernel", "frame"]`; consumed by
  `axiom-scene` and `axiom-introspect`; `EntityHandle` is the sanctioned
  exception to single-facade (a returned id newtype).
- **Owned concepts:** `World<S>`, `EntityRegistry`/`EntityHandle`, `ComponentColumn`/
  `ColumnSet`, `CommandBuffer`, `Query`, `WorldSystem`/`SchedulePhase`, `ReplayLog`.
- **Inputs:** kernel ids, frame context. **Outputs:** a generic, deterministic
  entity-component store (BTreeMap-backed for ordered iteration).
- **Allowed dependencies:** `kernel`, `frame`. **Forbidden:** scene/gameplay
  concepts (those live in `axiom-scene`).
- **Facade/contract:** `EcsApi` + `EntityHandle`.
- **Tests enforcing it:** `tests/architecture.rs`; in-crate world tests.
- **Tests missing before growth:** an entity-count scalability benchmark before
  ecology commits 1000+ entities (`gap-analysis.md` flags this as UNCLEAR).
- **Risk if ignored:** ECS is the substrate scene/sim build on; the deferred
  `Resources<T>` / generic component-insert seams (`PHASE_1_DEFERRED.md`) must not
  be hacked in with `Any`/`downcast`.

### Seam: Frame Introspection / Observability
- **Classification:** `Layer`
- **Current location:** `crates/axiom-introspect/`
- **Evidence:** `layer.toml` `depends_on = ["kernel", "frame", "ecs"]`; the
  north-star "agent-interrogable engine" direction (memory: introspection layer).
- **Owned concepts:** `IntrospectApi`, `FrameHistory`, `FrameReport`/`SystemReport`/
  `WorldReport`/`MetricReport`.
- **Inputs:** frame + ecs state. **Outputs:** bounded per-frame replay history and
  structured reports.
- **Allowed dependencies:** `kernel`, `frame`, `ecs` (math/host/runtime are
  *dev-only* fixture deps — correctly not runtime edges).
- **Facade/contract:** `IntrospectApi`.
- **Risk if ignored:** if observation logic migrates *into* ecs/scene it stops being
  a clean read-only seam; keep it strictly downstream.

### Seam: Content Addressing → Entropy → Proc → Validation (Procedural Spine)
- **Classification:** `Layer` (four layers, one seam family)
- **Current location:** `crates/axiom-space/`, `axiom-entropy/`, `axiom-proc/`,
  `axiom-proc-validate/`
- **Evidence:** `space depends_on [kernel]`; `entropy depends_on [kernel, space]`;
  `proc depends_on [kernel, space, entropy]`; `proc-validate depends_on [kernel,
  proc]`; 11 reverse-deps on `space`.
- **Owned concepts:** `Address` (domain-free key-path), `EntropyStream` (address/
  version-seeded), `Recipe`/`Artifact`/`ProcTrace`/`Evaluation`, `ValidationReport`.
- **Inputs:** kernel RNG/hash; addresses. **Outputs:** deterministic, replayable
  generation primitives consumed by terrain/biome/placement.
- **Allowed dependencies:** as declared above. **Forbidden:** scene/render/gameplay.
- **Facade/contract:** `SpaceApi`, `EntropyApi`, `ProcApi`, `ProcValidateApi`.
- **Tests enforcing it:** entropy reproducibility/collision tests
  (`entropy_api.rs`), proc evaluation tests, `tools/axiom-proc-fuzz`.
- **Risk if ignored:** this is a *second spine*; treating it as ad-hoc helpers for
  one game would re-introduce the soup the layering prevents.

### Seam: Scene Graph (vertical-slice producer #1)
- **Classification:** `Module` (engine module)
- **Current location:** `modules/axiom-scene/`
- **Evidence:** `module.toml` `allowed_layers = [kernel, runtime, math, frame, ecs]`,
  `allowed_modules = []`; ECS-native (memory: scene-is-ecs-native);
  `SceneSnapshot::from_scene` byte-equality/order tests.
- **Owned concepts:** `Scene` (=`World<SceneStorage>`), `SceneNodeId`, `Camera`/
  `Light`/`Renderable`/`MaterialRef`/`MeshRef`/`ProcAnim`/`Spin`, systems
  (`TransformPropagation`, `SpinSystem`, `ProcAnimSystem`, `PlayerMoveSystem`),
  and the **`SceneSnapshot`** contract.
- **Inputs:** layer APIs. **Outputs:** `SceneSnapshot` (ascending-node-id ordered)
  via `SceneApi::advance()`.
- **Allowed dependencies:** its allowed layers only. **Forbidden:** any other
  module (esp. render/resources).
- **Facade/contract:** `SceneApi` (+ `ids` vocabulary).
- **Tests enforcing it:** `tests/architecture.rs`; snapshot determinism tests.
- **Risk if ignored:** scene is the richest module; if it ever imports render
  ("just to build commands") the slice's re-composability dies.

### Seam: Resource Resolution (vertical-slice producer #2)
- **Classification:** `Module` (engine module)
- **Current location:** `modules/axiom-resources/`
- **Evidence:** `module.toml` `allowed_layers = [kernel, runtime, math, frame]`,
  `allowed_modules = []`; explicitly must *not* know node ids/transforms (CLAUDE.md
  slice rule #5).
- **Owned concepts:** CPU `ResourceTable`, `MeshData`/`MaterialData`/`TextureData`,
  and the **`ResolvedResources`** contract (sorted by `ResourceId`).
- **Inputs:** resource descriptions. **Outputs:** `ResolvedResources` via
  `ResourcesApi::resolve()`.
- **Facade/contract:** `ResourcesApi`.
- **Risk if ignored:** must stay scene-blind; a `SceneNodeId` leaking in here would
  fuse two modules that the architecture insists stay independent.

### Seam: Render Command Construction (vertical-slice transformer)
- **Classification:** `Module` (engine module)
- **Current location:** `modules/axiom-render/`
- **Evidence:** `module.toml` `allowed_layers = [kernel, runtime, math, frame,
  host]`, `allowed_modules = []`; CLAUDE.md slice rule #4 (render must not import
  scene).
- **Owned concepts:** **`RenderInput`** (scene-independent), **`RenderCommandList`**
  (tagged `RenderCommand` with `KIND_*` constants), `build_frame_packet`.
- **Inputs:** neutral matrices/meshes/lights via the `RenderInput` builder.
  **Outputs:** deterministic `RenderCommandList` + `HostFramePacket`.
- **Facade/contract:** `RenderApi`.
- **Risk if ignored:** render taking scene data directly is the single most likely
  "convenient" violation; the tag-struct command format must stay enum-free
  (branchless) and order-deterministic.

### Seam: GPU Submission Boundary (vertical-slice sink)
- **Classification:** `Module` (engine module)
- **Current location:** `modules/axiom-webgpu/`
- **Evidence:** `module.toml` `allowed_layers = [kernel, runtime, math, host,
  frame]`, `allowed_modules = []`; CLAUDE.md slice rule #6 (webgpu does not import
  render *yet*).
- **Owned concepts:** **`GpuSubmission`** (mutable `GpuCommand` sequence incl.
  `KIND_PRESENT`), **`GpuSubmissionReport`** (per-kind counters + status).
- **Inputs:** app-built submission. **Outputs:** deterministic submission report;
  real presentation delegated to `gpu-backend`/`canvas2d-backend`.
- **Facade/contract:** `WebGpuApi`.
- **Risk if ignored:** the "webgpu may later consume render as a backend adapter"
  decision is explicitly *not granted today*; granting it casually couples the sink
  to the transformer.

### Seam: Live Presentation Backends + Windowing Loop (platform side-effect seam)
- **Classification:** `Module` (engine `gpu-backend`/`canvas2d-backend`/`recording`
  + feature `windowing`)
- **Current location:** `modules/axiom-gpu-backend/`, `axiom-canvas2d-backend/`,
  `axiom-recording/`, `axiom-windowing/`
- **Evidence:** `PLATFORM_FACING_MODULES = [windowing, gpu-backend,
  canvas2d-backend, debug-overlay]` in `hygiene.rs`; live binding behind
  `#[cfg(target_arch = "wasm32")]`; memory: live-render-is-instanced-cube.
- **Owned concepts:** `LiveGpuBinding`, canvas2d raster, RAF tick loop, pointer
  capture (BTreeMap-ordered), `RecordingApi` opaque-byte capture, frame scrubber.
- **Inputs:** `HostPresentationRequest`/`HostFramePacket` (validated host data).
  **Outputs:** real pixels (wasm32) or deterministic recording (native).
- **Allowed dependencies:** `windowing` composes the three engine modules; backends
  depend only on `host`.
- **Facade/contract:** backend facades + `WindowingApi`.
- **Tests enforcing it:** `e2e/test_smoke.py` (render-actually-painted proof),
  Playwright controller, native fallback compiles branchlessly.
- **Risk if ignored:** this is the only place browser APIs are legal; one stray
  `web_sys` call elsewhere fails the hygiene scan — keep it confined.

### Seam: Render Pipeline Composition
- **Classification:** `Module` (feature module)
- **Current location:** `modules/axiom-render-pipeline/`
- **Evidence:** `module.toml` `kind = "feature-module"`,
  `allowed_modules = [scene, render, webgpu]`; memory: feature-module-tier.
- **Owned concepts:** the scene→render→GPU composition that an app would otherwise
  hand-wire.
- **Inputs:** the three composed module facades. **Outputs:** a ready pipeline the
  browser app consumes.
- **Allowed dependencies:** `math` layer + its three listed modules. **Forbidden:**
  any unlisted module, any app.
- **Risk if ignored:** the headless app does *not* yet use it (memory) — divergence
  between "pipeline as feature module" and "pipeline as app glue" is a live tension
  (see Pressure Points).

### Seam: Procedural-Domain Composition (levelgen / worldsave)
- **Classification:** `Module` (feature modules) over engine domain modules
- **Current location:** `modules/axiom-levelgen/`, `axiom-worldsave/`, over
  `axiom-terrain/`, `axiom-biome/`, `axiom-placement/`
- **Evidence:** `levelgen allowed_modules = [terrain, biome, placement]`;
  `worldsave allowed_modules = [levelgen]`; memory: procgen-build.
- **Owned concepts:** world recipe composition; save = seed + version + address +
  cell-override deltas (regenerates byte-identically).
- **Risk if ignored:** worldgen's stage list must be **data, not hardcoded calls**
  (Risk R6) from day one; retrofitting moddability is expensive.

### Seam: Deterministic-Lockstep Netcode vs Server-Authoritative Stack
- **Classification:** `Module` (two distinct module families — do not conflate)
- **Current location:** `modules/axiom-netcode/` (+ `axiom-crypto` layer) vs
  `modules/axiom-net-protocol/` + `axiom-client-core/` (+ TS SDK)
- **Evidence:** memory: two-multiplayer-stacks; `netcode allowed_layers =
  [kernel, crypto]`; `lockstep_convergence.rs` (adversarial transport, state-hash
  equality).
- **Owned concepts:** lockstep input timeline + state-hash reconciliation
  (deterministic) vs intent/snapshot wire contract + client connection state
  machine (server-authoritative).
- **Risk if ignored:** these are *two architectures*; a shared "net" abstraction
  would be a junk drawer fusing incompatible models.

## Candidate Seams

### Candidate: Noise Generation
- **Why a seam:** `docs/growth-port/roadmap.md` Phase 1 names `layer:noise`
  (Perlin/Simplex/FBM + domain warp, seeded, branchless); every worldgen stage
  needs it.
- **Why not confirmed:** no code exists yet; no consumer compiles against it.
- **Evidence to confirm:** the worldgen feature module + ≥1 domain module
  genuinely importing a `NoiseApi`.
- **Probably belongs:** a `Layer` (`axiom-noise`, `depends_on [kernel, math,
  entropy]`) — pure deterministic arithmetic, broadly shared.
- **Don't build yet:** a generic "procedural texture/material" surface on top of it.

### Candidate: Spherical / Geodesic Math
- **Why a seam:** `roadmap.md` Phase 1 (lat/long, great-circle, tangent frames,
  unit-dir ↔ region); Growth's planet substrate is "entirely spherical."
- **Why not confirmed:** unbuilt; unclear whether it extends `axiom-math` or is a
  peer.
- **Evidence to confirm:** a second consumer beyond Growth needing the same
  great-circle/tangent-frame ops.
- **Probably belongs:** extend `axiom-math` *unless* it grows domain meaning, then a
  `geo` peer layer (`depends_on [kernel, math]`).
- **Don't build yet:** a full GIS/projection suite — only what worldgen consumes.

### Candidate: Game-World Streaming / Chunks
- **Why a seam:** `roadmap.md` Phase 3 (`module:game-world`: per-chunk generator,
  chunk store, focus-radius load/unload, seam coherence); `gap-analysis.md` calls
  streaming the #1 engine-capability gap (Risk R2).
- **Why not confirmed:** no proven per-frame dynamic vertex-buffer upload/unload
  path in the browser (`axiom-resources` only bakes at startup).
- **Evidence to confirm:** a Phase-0 dynamic-mesh-streaming spike that paints.
- **Probably belongs:** an engine `Module` (`allowed_modules = []`, `depends_on
  [kernel, space, terrain]`) owning a spatial index ECS lacks.
- **Don't build yet:** the chunk store before the upload path is proven; don't sneak
  per-frame GPU calls into app code.

### Candidate: Unified Lifecycle State
- **Why a seam:** `RuntimeState`, `HostLifecycleState`, `FrameLifecycleState` are
  three parallel enums modeling the same init/ready/running/finished idea.
- **Why not confirmed:** they live at different layers with different granularity; a
  shared trait could be ceremonial coupling rather than real reuse.
- **Evidence to confirm:** a concrete consumer that must treat all three uniformly.
- **Probably belongs:** a kernel `Lifecycle` trait *only if* ≥2 layers genuinely
  need the polymorphism; otherwise leave them distinct.
- **Don't build yet:** a `Lifecycle` abstraction "for tidiness."

### Candidate: Unified Command/Event Dispatch
- **Why a seam:** `RuntimeCommandQueue`/`RuntimeEventQueue`, `FrameCommandQueue`,
  `InterfaceInputEvent` all repeat a deterministic-FIFO pattern.
- **Why not confirmed:** each queue carries layer-specific payloads; a generic
  `Queue<T>` would add a kernel type with no behavioral need beyond `VecDeque`.
- **Evidence to confirm:** a third+ queue needing identical ordering/audit
  semantics that justify a shared contract.
- **Probably belongs:** stay as-is; the shared primitive is already `VecDeque` in
  the kernel.
- **Don't build yet:** a `Dispatcher` framework.

### Candidate: Player Interaction / Picking
- **Why a seam:** `roadmap.md` Phase 3; `axiom-input` module exists (touch/pointer)
  but interaction-ray + picking + command queue are still app-level.
- **Why not confirmed:** only proven inside individual apps (doom, roomed-puzzle).
- **Evidence to confirm:** ≥3 apps sharing the same interaction shape.
- **Probably belongs:** start `App`, graduate to a `Module` once stable.
- **Don't build yet:** a `feature-module:input` before the loop is proven.

## False Seams and Junk Drawers

1. **"A presentation-backend trait unifying GPU + Canvas2D."** Explicitly rejected
   in `docs/canvas2d-backend-plan.md`. Two isolated modules with a *uniform data
   contract* (`HostFramePacket` in, `GpuSubmissionReport`/raster out) is the correct
   shape; a shared public trait would force two modules to name one type, violating
   module isolation. **Not a domain.**

2. **A generic "net" abstraction over both multiplayer stacks.** Deterministic
   lockstep (`axiom-netcode`) and server-authoritative intent/snapshot
   (`net-protocol` + `client-core`) are *different architectures* (memory:
   two-multiplayer-stacks). Merging them is a junk drawer. **Keep separate.**

3. **`scene_to_render_input` / `render_command_list_to_gpu_submission` as a reusable
   engine primitive.** These are app translation glue
   (`apps/axiom-demo-rotating-cube/src/`). They have **one** real consumer (the
   demo). Promoting them now violates the "three real consumers" rule. **Stays app-
   owned.**

4. **A shared `Lifecycle` / `Queue<T>` / `StepReport` abstraction.** The vocabulary
   overlap (lifecycle states, command queues, step records) is real but the *reuse
   pressure is not* — each lives at a different layer with a different payload.
   Generalizing now manufactures ceremonial coupling. **Candidate at best.**

5. **A "render trait" for custom shaders.** Custom visuals belong in future isolated
   `feature-module:render-*` compositions, never a polymorphic trait inside the core
   render module (`roadmap.md` Phase 5). **Not a core-render concern.**

6. **Moddable data-definition "framework."** Growth is XML-def-heavy, but parsing is
   match-heavy and awkward under the Branchless Law; it belongs in a **tool/app**
   reading data, not a spine module — unless ≥3 apps reuse the exact def system.
   **Premature framework.**

7. **`SceneNodeId` as a distinct identity from `EntityHandle`.** Scene is ECS-native;
   `SceneNodeId` shadowing `EntityHandle` is duplicated identity vocabulary, not a
   new domain. It is a *naming* question inside scene, not a seam. **Belongs inside
   the scene module.**

## Layer Candidates

Only seams that deserve to be ordered engine layers. (Both below are *future* —
neither should be built before its first genuine consumer compiles.)

### Proposed layer: `noise`
- **Proposed index:** root-adjacent; no fixed integer (the graph is a DAG, not a
  line). Sits beside `entropy`/`proc`.
- **Previous layer it adapts:** `entropy` (seeded streams) + `math` (vectors).
- **Lower-layer capability consumed:** `EntropyStream` for seeding; `Vec2/3` +
  scalars for sample coordinates.
- **Higher-level capability introduced:** `NoiseApi` — deterministic, branchless
  coherent-noise fields (value/Perlin/Simplex/FBM, domain warp).
- **Why not a module:** every worldgen domain module + future game-world streaming
  shares it; a broadly-shared pure primitive with no single owning module is a
  layer, not a module (CLAUDE.md "broadly-shared primitive" rule).
- **Minimal facade:** `NoiseApi` returning sampled scalar fields keyed by seed +
  coordinate.
- **Required architecture tests:** `tests/architecture.rs` (no time/RNG/HashMap),
  reproducibility golden (same seed+coord → same value across platforms),
  branchless-lint pass, 100% coverage.

### Proposed layer: `geo` (only if it grows domain meaning; else extend `math`)
- **Proposed index:** beside `math`; `depends_on [kernel, math]`.
- **Previous layer it adapts:** `math` (Vec3, Quat, Mat4).
- **Lower-layer capability consumed:** unit-vector and matrix ops.
- **Higher-level capability introduced:** `GeoApi` — lat/long ↔ unit-direction,
  great-circle distance, tangent frames, region mapping.
- **Why not a module:** pure geometric transforms shared by worldgen *and* game-
  world *and* any future planet renderer — broadly-shared primitive.
- **Minimal facade:** `GeoApi` value transforms only (no I/O, no domain state).
- **Required architecture tests:** determinism goldens for great-circle/tangent
  frames; branchless + coverage gates.

> Do **not** propose ceremonial layers: a unified-lifecycle layer, a queue layer, a
> "viewport authority" layer, or a "diagnostics" layer would each be a tiny
> ceremonial wrapper around existing primitives — rejected.

## Module Candidates

Only seams that deserve isolated feature/engine modules. (Future; build after the
gating spike, never before.)

### Proposed module: `game-world` (engine module)
- **Allowed layers:** `kernel`, `space`, `terrain` (+ `noise`/`geo` once they
  exist).
- **Forbidden modules:** all (`allowed_modules = []`) — it is an isolated engine
  module; apps/feature-modules compose it.
- **Single public facade:** `GameWorldApi`.
- **Data contracts exposed:** chunk descriptor, chunk-mesh diff, focus-radius
  load/unload events — all plain primitive data.
- **App glue required:** the app translates chunk-mesh diffs into
  `axiom-resources` streaming uploads and `RenderInput` (the same glue tier as the
  vertical slice).
- **Required `module.toml` capabilities:** `chunk-store`, `spatial-index`,
  `chunk-generator`.
- **Required architecture tests:** deterministic chunk regeneration goldens; seam-
  coherence test (adjacent chunks agree at the boundary); branchless + 100%
  coverage; `tests/architecture.rs`.
- **Blocked by:** Risk R2 dynamic-mesh-upload spike must pass first.

### Proposed module: `worldgen` (feature module)
- **Allowed layers:** `kernel`, `space` (+ `noise`/`geo`).
- **Allowed modules:** `terrain`, `biome`, `placement` (the same set `levelgen`
  already lists — `worldgen` may end up being `levelgen` grown up rather than a new
  module; confirm before duplicating).
- **Single public facade:** `WorldgenApi`.
- **Data contracts exposed:** a serializable, versioned **stage list** + per-stage
  artifacts (tectonics → elevation → erosion → moisture → rivers).
- **App glue required:** the app drives stage execution per-tick (cooperative job)
  and feeds results to `game-world`.
- **Required `module.toml` capabilities:** `world-recipe`, `stage-pipeline`.
- **Required architecture tests:** stage-list-as-data round-trip (Risk R6),
  deterministic full-world golden, branchless + coverage.

> No module-to-module dependencies are proposed: `game-world` is isolated;
> `worldgen` composes only the three procgen domain modules it is permitted, exactly
> as `levelgen` does today.

## App-Owned Glue

All composition logic that must stay in apps (it names cross-module contracts that
are un-nameable from a third crate, so only an app — reading one facade, calling the
next — can bridge them).

1. **`scene_to_render_input` (SceneSnapshot + ResolvedResources → RenderInput).**
   - **Producer:** `axiom-scene` (`SceneSnapshot`) + `axiom-resources`
     (`ResolvedResources`). **Consumer:** `axiom-render` (`RenderInput` builder).
   - **Translation:** maps first camera to view/projection, emits lights per a
     *demo-specific* policy (all directional), resolves mesh/material refs to render
     indices, skips unresolved renderables.
   - **Why it must not move into either module:** scene must not import render
     (slice rule #4) and resources must stay scene-blind (rule #5); the policy
     ("all lights directional") is app-specific, not engine truth. Lives in
     `apps/axiom-demo-rotating-cube/src/scene_to_render_input.rs`.

2. **`render_command_list_to_gpu_submission` (RenderCommandList → GpuSubmission).**
   - **Producer:** `axiom-render`. **Consumer:** `axiom-webgpu`.
   - **Translation:** maps each `RenderCommand` kind to its `GpuCommand` counterpart,
     appends a trailing `Present`.
   - **Why it must not move:** `axiom-webgpu` is explicitly *not* allowed to consume
     `axiom-render` yet (slice rule #6); the bridge is the app's job. Lives in
     `apps/axiom-demo-rotating-cube/src/render_to_gpu_submission.rs`.

3. **`run_vertical_slice` orchestration (the un-nameable plumbing).**
   - **Producer/consumer:** all five slice modules in sequence.
   - **Translation:** holds the type-inferred locals for contracts that *cannot be
     named* outside their owning crate (memory: app-glue-uses-inlined-plumbing); it
     cannot be factored into helpers because helper signatures would need to name
     those types.
   - **Why it must not move:** it is the definition of an app — the only tier that
     may read one module facade and feed the next. Lives in
     `apps/axiom-demo-rotating-cube/src/vertical_slice.rs`.

4. **Per-app input/interaction wiring** (doom, roomed-puzzle, quintet, growth).
   - **Producer:** `axiom-input` / raw host pointer+key facts. **Consumer:** scene
     `PlayerCommand`/`ControllerCommand`.
   - **Why it must not move:** the mapping from raw input to gameplay intent is
     game-specific until ≥3 apps share it (then it graduates to a module).

## Tooling and Harness Domains

Seams that belong outside runtime engine code (none may be imported by any layer/
module/app):

1. **`xtask` (architecture checker).** *Inspects:* every `layer.toml`/`module.toml`/
   `app.toml`, cargo metadata, source text (hygiene scans in `hygiene.rs`). *Must not
   be imported by:* anything. *Validates:* Layer Law, Module Law, hygiene, coverage-
   scope drift, the whole DAG.

2. **`tools/lints/` (dylint rulebook).** *Inspects:* HIR/DefId of all spine crates.
   *Validates:* `engine_no_branching` (Branchless Law, baseline 0),
   `engine_genuine_dependency`, `test_without_assertion`. *Must not be imported by:*
   any runtime crate.

3. **`tools/axiom-shot` (headless renderer).** *Inspects:* any app via the real
   `scene_renderer`, GPU or canvas2d backend, self-driven camera. *Validates:* GPU
   verification of proc-driven rendering, golden artifacts. *Not in the workspace*
   (memory: headless-screenshot-tool).

4. **`tools/axiom-profile-runner` (CPU profiler).** *Inspects:* per-phase engine
   timing. *Note:* bounds/culling phases are placeholders — no engine system owns
   them yet (memory: cpu-profile-runner-tool), itself a future-seam signal.

5. **`tools/axiom-proc-fuzz` / `axiom-proc-inspect`.** *Inspects:* the procgen layers
   (space/entropy/proc + domain modules). *Validates:* generator determinism under
   fuzzing.

6. **`tools/axiom-dev-reload`, `axiom-netcode-relay`, `axiom-netplay-server`.**
   *Inspects:* nothing engine-internal; serve/relay runtime. *Validates:* live
   multiplayer/dev loops.

7. **`e2e/` + `scripts/playwright_controller.py` (browser verification).**
   *Inspects:* the packaged wasm apps in a real Chromium. *Validates:* the `wasm32`
   presentation arm the native gate cannot reach (canvas actually painted, no page
   errors). Outside the engine graph.

8. **`scripts/coverage.{ps1,sh}` + `package_app.py` + `ts-gate`.** Coverage gate,
   wasm2js packaging (memory: wasm2js-packaging), and the TS SDK native gate stack
   (memory: ts-sdk-gates) — all tooling, none load-bearing at runtime.

9. **`crates/axiom-zones` (support crate).** Build-time zone-marker proc-macros
   (`#[sim]`/`#[hot_path]`/`#[strict]`/`#[supervisor]`/`#[escape_hatch]`). Classified
   `PackageClass::Support`; every crate may depend on it; it depends on nothing
   engine; coverage-exempt. The one sanctioned escape from layer ordering.

## Boundary Violations or Pressure Points

No *violations* were found — `cargo xtask check-architecture` passes. The following
are *pressure points* (the architecture straining, not breaking):

1. **Render pipeline composition is duplicated as both a feature module and app
   glue.**
   - **File/symbol:** `modules/axiom-render-pipeline/` vs
     `apps/axiom-demo-rotating-cube/src/{scene_to_render_input,vertical_slice}.rs`.
   - **Pressured rule:** "apps translate between module contracts" vs "a feature
     module may compose modules." The browser app uses the pipeline feature module;
     the demo app re-implements the same scene→render→GPU bridge by hand (memory:
     feature-module-tier — "headless app not yet" using the pipeline).
   - **Likely root cause:** the feature module post-dates the demo's hand glue.
   - **Recommended fix:** once `render-pipeline` proves out, migrate the demo's glue
     onto it (or consciously keep the demo as the *minimal hand-wired reference* and
     document why). **Defer** — not breaking anything; decide deliberately.

2. **`SceneNodeId` shadows `EntityHandle` despite scene being ECS-native.**
   - **File/symbol:** `modules/axiom-scene/` (`SceneNodeId`) vs `axiom-ecs`
     (`EntityHandle`).
   - **Pressured rule:** single-identity / no-duplicate-vocabulary.
   - **Root cause:** scene predates its ECS rewrite; the id newtype was not collapsed
     to a transparent projection.
   - **Recommended fix:** make `SceneNodeId` a transparent newtype/alias over
     `EntityHandle` (a naming cleanup inside scene, not a new seam). **Defer**, low
     risk.

3. **`webgpu` ↔ `render` backend-adapter decision is explicitly suspended.**
   - **File/symbol:** `modules/axiom-webgpu/ARCHITECTURE.md` (slice rule #6).
   - **Pressured rule:** module isolation vs the temptation to let the sink consume
     the transformer directly.
   - **Root cause:** the clean boundary is currently the app's `GpuSubmission` shape;
     a future decision may permit `webgpu` to consume `render`.
   - **Recommended fix:** keep the app bridge until a *second* backend or consumer
     justifies the adapter; do not grant it casually. **Defer.**

4. **Dynamic mesh streaming has no home and no proven path (Risk R2).**
   - **File/symbol:** `axiom-resources` (startup-bake only); `gap-analysis.md`.
   - **Pressured rule:** "fix at the lowest correct layer" — streaming touches
     resources (lifecycle), a future `game-world` module, and the app upload glue.
   - **Root cause:** no per-frame vertex-buffer upload/unload exists in the browser.
   - **Recommended fix:** run the Phase-0 spike *before* designing the module; extend
     `axiom-resources` for a streaming lifecycle at that layer, not in app code.
     **Defer** until the spike, but do not implement casually meanwhile.

5. **Cooperative generation cannot thread (Risk R3).**
   - **File/symbol:** `axiom-proc` (determinism + no `thread::spawn`).
   - **Pressured rule:** determinism + Branchless Law vs a heavy erosion stage.
   - **Root cause:** the proc layer is single-threaded by design; long stages must
     amortize across ticks via `RuntimeCommandQueue`, expressed as data.
   - **Recommended fix:** prove a cooperative-job pattern on `axiom-runtime` before
     worldgen's heavy stages land. **Defer** to the Phase-0 spike.

6. **`axiom-math` is the largest remaining branchless-rewrite surface.**
   - **File/symbol:** `crates/axiom-math/` per `docs/unbranching.md`.
   - **Pressured rule:** Branchless Law (baseline 0 across the whole spine).
   - **Root cause:** math has the most arithmetic-with-conditionals to convert.
   - **Recommended fix:** continue the documented unbranching recipes; treat any new
     math branch as a gate failure. **Ongoing**, already tracked.

## Proposed Final Domain Map

```text
Kernel:
  - axiom-kernel  (deterministic time/IDs/results/serialization/observability/RNG/scalars)

Layers (DAG, not a strict line — "consumes" lists genuine depends_on):
  kernel:
    owns:     FixedStep, Tick, HandleId/EntityId, KernelError, Binary{Reader,Writer},
              LogRecord/TelemetryMetric, DeterministicRng, StableHash, Meters/Radians/Ratio
    consumes: (root)
    produces: KernelApi
  runtime:
    owns:     Runtime, RuntimeScheduler, RuntimeStep(Record), command/event queues
    consumes: kernel
    produces: deterministic stepping + step records
  math:
    owns:     Vec/Quat/Mat4/Transform, Aabb/Frustum/Plane/Ray/Sphere
    consumes: kernel, runtime
    produces: MathApi (pure geometry)
  host:
    owns:     HostStepDriver, HostViewport/Pixels/insets, HostPresentation*, HostFramePacket
    consumes: kernel, runtime
    produces: HostApi (platform boundary; nondeterminism-as-data)
  frame:
    owns:     EngineFrame, FrameBuilder, FrameContext, FrameCommandQueue
    consumes: kernel, runtime, host
    produces: FrameApi (canonical per-frame contract)
  ecs:
    owns:     World, EntityHandle, ComponentColumn, CommandBuffer, Query, WorldSystem
    consumes: kernel, frame
    produces: EcsApi (generic deterministic world)
  introspect:
    owns:     FrameHistory, FrameReport/SystemReport/WorldReport
    consumes: kernel, frame, ecs
    produces: IntrospectApi (observability)
  crypto:
    owns:     SigningKey/VerifyingKey/Signature
    consumes: kernel
    produces: signed-message authentication
  interface:
    owns:     PanelId, InterfaceDrawList, CommandTable
    consumes: kernel
    produces: InterfaceApi (generic windowing primitives)
  layout:
    owns:     LayoutTree/LayoutResult/LayoutRect, flex/constraint solve()
    consumes: kernel, host
    produces: responsive layout
  space:
    owns:     Address (content addressing)
    consumes: kernel
    produces: SpaceApi
  entropy:
    owns:     EntropyStream
    consumes: kernel, space
    produces: EntropyApi (keyed deterministic randomness)
  proc:
    owns:     Recipe/Artifact/ProcTrace/Evaluation
    consumes: kernel, space, entropy
    produces: ProcApi (recipe DAG evaluation)
  proc-validate:
    owns:     ValidationReport, bounded repair
    consumes: kernel, proc
    produces: ProcValidateApi
  [future] noise:   consumes kernel, math, entropy → NoiseApi (coherent noise)
  [future] geo:     consumes kernel, math → GeoApi (spherical) — or extend math

Modules (engine = isolated; feature = composes listed modules):
  scene (engine):
    owns: Scene/SceneNodeId/components/systems; consumes kernel,runtime,math,frame,ecs
    produces: SceneApi + SceneSnapshot
  resources (engine):
    owns: ResourceTable/Mesh/Material/Texture; consumes kernel,runtime,math,frame
    produces: ResourcesApi + ResolvedResources
  render (engine):
    owns: RenderInput/RenderCommandList(KIND_*)/FramePacket; consumes kernel,runtime,math,frame,host
    produces: RenderApi
  webgpu (engine):
    owns: GpuSubmission/GpuSubmissionReport; consumes kernel,runtime,math,host,frame
    produces: WebGpuApi
  gpu-backend / canvas2d-backend (engine, platform-facing):
    owns: live wgpu / canvas2d presentation (wasm32); consumes host
  recording (engine):
    owns: opaque-byte frame capture + scrub; consumes kernel
  input (engine):
    owns: TouchControls/ControlFrame; consumes kernel, math
  debug-overlay (engine, platform-facing):
    owns: browser overlay/console; consumes interface
  netcode (engine):
    owns: lockstep timeline + state-hash reconcile; consumes kernel, crypto
  net-protocol / client-core (engine):
    owns: wire contract / client state machine; consumes kernel
  sim-core (engine):
    owns: Fact/Relation/Process/Effect/CausalJournal; consumes kernel, ecs
  terrain / biome / placement (engine, procgen domains):
    owns: heightfield / classification / scatter; consumes kernel,space,entropy(,proc)
  render-pipeline (feature):
    composes: scene, render, webgpu; consumes math → scene→GPU pipeline
  windowing (feature, platform-facing):
    composes: gpu-backend, canvas2d-backend, recording; consumes kernel, host
  levelgen (feature):
    composes: terrain, biome, placement; consumes kernel, space → world recipe
  worldsave (feature):
    composes: levelgen; consumes kernel, space → seed+delta save
  axiom (feature, umbrella):
    composes: scene, resources, render-pipeline, webgpu, windowing → prelude
  [future] game-world (engine): chunk store/streaming; consumes kernel,space,terrain(,noise,geo)
  [future] worldgen (feature):  staged pipeline; composes terrain,biome,placement(,noise,geo)

Apps (leaves, own all cross-module glue):
  axiom-demo-rotating-cube(+browser), axiom-doom-browser, axiom-stress-cubes-browser,
  axiom-netcode-demo, axiom-netcode-sim, axiom-netplay-browser, axiom-netplay-ffi,
  axiom-growth, axiom-roomed-puzzle, axiom-quintet, axiom-sim-crucible,
  axiom-browser-dev-harness, axiom-proc-playground

Tools (outside the engine graph):
  xtask (architecture checker), tools/lints (dylint rulebook), axiom-shot,
  axiom-profile-runner, axiom-proc-fuzz, axiom-proc-inspect, axiom-dev-reload,
  axiom-netcode-relay, axiom-netplay-server
  + axiom-zones (Support crate), scripts/, e2e/, packages/axiom-client (TS SDK)

Tests/Harnesses:
  per-crate tests/architecture.rs (law scans), tests/manifest.rs (layer contracts),
  replay_determinism.rs + golden_state/artifacts (byte-equal proof),
  lockstep_convergence.rs, required_behaviors.rs (app gameplay),
  zones/markers.rs, xtask checker/class_checker tests, e2e/test_smoke.py
```

## Recommended Next Refactor Sequence

> These are sequenced *architecture* changes, not implementations. Each is small,
> structural, and gated. Do them in order; stop at the first that fails its gate.

1. **Decide the render-pipeline-vs-demo-glue duplication.**
   - **Goal:** one canonical scene→render→GPU composition; either migrate the demo
     onto `axiom-render-pipeline` or document the demo as the deliberate minimal
     hand-wired reference.
   - **Files likely touched:** `apps/axiom-demo-rotating-cube/src/{vertical_slice,
     scene_to_render_input,render_to_gpu_submission}.rs`, possibly
     `modules/axiom-render-pipeline/`.
   - **Validation command:** `cargo test --workspace`
   - **Stop condition:** demo tests + slice determinism goldens still pass and there
     is exactly one documented composition story.

2. **Collapse `SceneNodeId` into a transparent projection of `EntityHandle`.**
   - **Goal:** remove duplicated identity vocabulary inside `axiom-scene`.
   - **Files likely touched:** `modules/axiom-scene/src/*` (ids), snapshot accessors.
   - **Validation command:** `cargo xtask check-architecture`
   - **Stop condition:** checker passes (single-facade + `ids` exemption intact) and
     scene snapshot tests are byte-stable.

3. **Run the Risk-R2 dynamic-mesh-streaming spike (in an app, not a module yet).**
   - **Goal:** prove a per-frame vertex-buffer upload/unload path paints in-browser.
   - **Files likely touched:** a throwaway app under `apps/`, `e2e/test_smoke.py`.
   - **Validation command:** `cargo test --workspace`
   - **Stop condition:** the spike app renders a streamed mesh that changes per
     frame; no engine module created until it does.

4. **Introduce the `noise` layer (only once a consumer compiles against it).**
   - **Goal:** add `crates/axiom-noise` (`depends_on [kernel, math, entropy]`) with
     `NoiseApi`, reproducibility goldens, branchless + 100% coverage.
   - **Files likely touched:** `crates/axiom-noise/{Cargo.toml,layer.toml,src,tests}`,
     root `Cargo.toml`.
   - **Validation command:** `cargo xtask check-architecture`
   - **Stop condition:** checker reports the new layer satisfies the Layer Law and a
     genuine consumer (worldgen/domain module) references `NoiseApi`.

5. **Promote worldgen staging to a feature module with stage-list-as-data (Risk R6).**
   - **Goal:** a `worldgen` feature module composing terrain/biome/placement with a
     serializable, versioned stage pipeline (no hardcoded stage calls).
   - **Files likely touched:** `modules/axiom-worldgen/{module.toml,src,tests}` (or
     grow `axiom-levelgen`), app glue under `apps/axiom-growth`.
   - **Validation command:** `cargo test --workspace`
   - **Stop condition:** full-world determinism golden passes and the stage list
     round-trips through serialization.

## Sub-Agent Evidence Appendix

### Workspace Map (Sub-agent 1)

14 layers (`crates/*/layer.toml`): kernel (root), runtime, math, host, frame, ecs,
introspect (ordered backbone); crypto, interface, layout, space, entropy, proc,
proc-validate (root-adjacent / procgen branch). 16 engine modules: scene, resources,
render, webgpu, gpu-backend, canvas2d-backend, netcode, net-protocol, client-core,
sim-core, recording, input, debug-overlay, biome, terrain, placement. 5 feature
modules: render-pipeline (scene+render+webgpu), windowing
(gpu-backend+canvas2d-backend+recording), levelgen (terrain+biome+placement),
worldsave (levelgen), axiom (umbrella). 14 apps under `apps/`, all leaves. Tools:
axiom-dev-reload, axiom-netcode-relay, axiom-netplay-server, axiom-proc-fuzz,
axiom-proc-inspect, axiom-profile-runner, axiom-shot (out of workspace), lints/.
Support: `axiom-zones` (proc-macros, no layer.toml, all may depend, coverage-exempt).
Repo tooling: `xtask` (no layer.toml). **No accidental domains, no junk drawers, no
hidden composition roots outside apps/.**

### Dependency Seams (Sub-agent 2)

Fan-in hubs (clean, intended): kernel (41), host (14), math (13), runtime (12),
space (11). Fan-out: composition roots only — `axiom-demo-rotating-cube` (11),
`axiom` umbrella (11), `axiom-scene` (7, all appropriate). `axiom-introspect` reaches
math/host/runtime only as **dev-deps** (test fixtures), not runtime edges. Layer DAG
acyclic; `cargo xtask check-architecture` → **OK**. Feature modules are the *only*
composition tier; all 16 engine modules have `allowed_modules = []`. Every module
exports exactly one facade (`SceneApi`/`RenderApi`/`WebGpuApi`/`EcsApi`/...) plus the
`ids` exemption. Apps have zero incoming engine edges. **Zero violations; preserve
the feature-module tier and the engine-module isolation as the scaling points.**

### Domain Vocabulary Seams (Sub-agent 3)

Clean vocabularies: kernel, runtime, math, frame, ecs, host. **Overloaded terms:**
`Frame` (`Tick` vs `EngineFrame`), `Step` (`RuntimeStep` vs `FrameStepSummary`),
`Command` (`RuntimeCommand`/`FrameCommand`/`PlayerCommand`/`ParsedCommand`), `State`
(`RuntimeState`/`HostLifecycleState`/`FrameLifecycleState`), `Report`/`Snapshot`/
`Queue`/`Handle`. **Repeated concepts implying *possible* shared layers (all
candidate, none confirmed):** unified lifecycle, unified command/event dispatch,
viewport authority, backend-submission contract, node/entity identity unification.
**App-glue vocabulary that must NOT generalize:** PlayerCommand/Spin/ProcAnim,
RenderCommand KIND_* tagging, biome thresholds, client-core/net-protocol split,
recording-vs-live backends, InterfaceDrawList/ParsedCommand. Top concrete cleanup:
make `SceneNodeId` a transparent `EntityHandle` projection.

### Data Contract Seams (Sub-agent 4)

Major contracts + owners: `SceneSnapshot` (scene), `ResolvedResources` (resources),
`RenderInput`/`RenderCommandList` (render), `GpuSubmission`/`GpuSubmissionReport`
(webgpu), `RuntimeStep` (runtime), `HostFramePacket` (host). The five slice contracts
are **un-nameable outside their crate** — reachable only through facades, which is
*why* glue must live in the app. Glue today: `scene_to_render_input.rs`,
`render_to_gpu_submission.rs`, `vertical_slice.rs` — all in
`apps/axiom-demo-rotating-cube/`, all correct per architecture. Zero cross-imports
between the four core slice modules. All slice contracts are stable enough to be
module boundaries; `RuntimeStep`/`FramePacket` are stable but their downstream
surface binding is still evolving. **No glue crosses 3 module boundaries; none lives
inside a module; none names an un-nameable type in a signature.**

### Determinism and Side-Effect Seams (Sub-agent 5)

Pure deterministic spine: kernel, runtime, math, host(boundary), frame, ecs,
introspect. Enforced bans (per-crate `tests/architecture.rs`): no `std::time`/
`Instant::now`/`SystemTime`/`chrono`, no `rand`/`thread_rng`, no `static mut`/
`lazy_static`/`OnceCell`, no `HashMap` iteration (BTreeMap/Vec/VecDeque only), no
`web_sys`/`js_sys`/`wasm_bindgen`/`wgpu`/RAF/`performance.now` outside host+the four
platform-facing modules, no `println!`/`dbg!`/`todo!`. Nondeterminism enters only as
**validated data** at host (`HostFrameInput::elapsed_nanos`, viewport, lifecycle) and
as **seeded** RNG (`DeterministicRng`, `EntropyApi`). Proven by replay:
`runtime/tests/integration.rs` (byte-identical), `recording` compare-with first-
divergence, frame determinism, `netcode` lockstep convergence. Side-effect boundaries
confined to `windowing`/`gpu-backend`/`canvas2d-backend` (wasm32-gated). **No hidden
nondeterminism found.**

### Validation Seams (Sub-agent 6)

Hard-law tests: per-crate `tests/architecture.rs` (token scans), `xtask` checker +
`checker_tests.rs`/`class_checker_tests.rs` (Layer/Module Law fixtures), coverage gate
(`scripts/coverage.*`, 100% regions/lines/functions), `engine_no_branching` dylint
(Branchless Law, baseline 0), hygiene scans (`hygiene.rs`). Runtime-contract tests:
`replay_determinism.rs`, `golden_state.rs`/`golden_artifacts.rs` (cross-commit byte
pinning, `AXIOM_REGOLD=1` to re-capture), `lockstep_convergence.rs`, `manifest.rs`,
`integration.rs`. Harnesses that are tooling (not runtime law): `xtask` checker tests,
`e2e/test_smoke.py`, `playwright_controller.py`, `axiom-shot`, `zones/markers.rs`. **No
validation code leaks into engine crates** (all in `tests/`, `xtask`, `tools/`,
`scripts/`, `e2e/`). Missing tests to gate future seams: module-composition law for
new feature modules, deterministic-I/O golden if a layer ever owns I/O, capability
versioning negotiation, cross-layer serialization round-trips. **Rule: write the
enforcing test first, then the seam.**

### Future Capability Seams (Sub-agent 7)

Emerging (evidence: `docs/growth-port/roadmap.md`, `gap-analysis.md`,
`canvas2d-backend-plan.md`, `unbranching.md`, PHASE_*_DEFERRED.md): **noise**
(→ Layer), **spherical/geo math** (→ Layer or math extension), **worldgen**
(→ feature module, stage-list-as-data from day one — Risk R6), **game-world streaming**
(→ engine module, blocked on Risk R2 dynamic-mesh upload), **player interaction**
(App → graduate to module), **ecology** (future feature module, blocked on ECS entity-
count benchmark), **render extensions** (feature-module family, never a trait).
Premature abstractions to avoid: presentation-backend trait (rejected), generic "net",
typed `Resources<T>` (deferred — no `Any`/downcast), generic component insert/remove
(deferred — ColumnSet redesign), moddable-defs framework (tool/app first). Hard risks
tracked: R2 (streaming) and R3 (cooperative gen — must amortize via
`RuntimeCommandQueue`, never `thread::spawn`) block Phase 3; R6 (stage-order-as-data)
must be baked from the start. **No architecture violations hidden in current plans.**

---

## Verification

- `cargo xtask check-architecture` → **PASS** (exit 0): "OK: all layers satisfy the
  Axiom Layer Law." Layers checked: crypto, ecs, entropy, frame, host, interface,
  introspect, kernel, layout, math, proc, proc-validate, runtime, space.
- `cargo test --workspace` → **PASS** (exit 0): every suite reported `test result:
  ok`, zero failures across the workspace (unit, integration, architecture, replay,
  golden, behavior, and doc tests).
