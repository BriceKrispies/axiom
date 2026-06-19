# Gap analysis — what Axiom is missing to build the target product

The core document. Pairs every target subsystem ([`target-product.md`](target-product.md)) against the Axiom baseline ([`axiom-baseline.md`](axiom-baseline.md)), states the **gap**, the **correct placement** under the Layer/Module laws, and the **hard risks**. "Placement" is a recommendation, not a decree — but it is chosen to respect Axiom's dependency rules.

## How to read the matrix
- **Status**: ❌ absent · 🟡 partial (something to build on) · ✅ present (rare here).
- **Placement**: `kernel` / `layer:<name>` / `module:<name>` (engine) / `feature-module:<name>` / `app`. Recall: engine **layers** are the branchless, 100%-covered spine; **apps** are exempt from both gates. That exemption is the single biggest lever in the whole port (see Risk R1).

---

## 1. Determinism primitives

| Need | Status | Gap | Placement |
|------|--------|-----|-----------|
| Seeded integer RNG | ✅ | `DeterministicRng` (splitmix64) exists | — |
| **Float / distribution sampling** | ❌ | worldgen needs `f32` in range, uniform/normal, unit-vector-on-sphere | **kernel** (extend `DeterministicRng`) or `layer:math` helper |
| Fixed-tick clock, replay records | ✅ | `SimulationClock`, `RuntimeStepRecord` | — |
| Stable iteration / serialization | ✅ | `BTreeMap`, `Reflect`, `SchemaVersion` | — |

The float-RNG gap is small but blocking — nearly every generation stage needs it. Add it at the lowest correct layer (kernel RNG) so all of worldgen shares one deterministic source.

## 2. Procedural-generation math

| Need | Status | Gap | Placement |
|------|--------|-----|-----------|
| **Value/Perlin/Simplex noise + FBM, domain warp** | ❌ | nothing exists; needed for elevation detail, moisture, chunk `detail_noise` | **layer:noise** (new, depends on `math` + kernel RNG) |
| **Spherical / geodesic math** (lat/long, great-circle, unit-dir frames) | ❌ | the planet substrate is entirely spherical | **layer:math** (extend) or **layer:geo** (new) |
| **Icosphere construction + subdivision** | ❌ | the overworld topology *is* an icosphere; Growth quantises region count by subdivision level | **module:planet-topology** (engine module over `math`/`geo`) |
| Vec/Mat/Quat/AABB/Frustum | ✅ | present in `math` | — |

These are reusable, determinism-critical primitives → they belong **down in layers/modules**, where the branchless + 100%-coverage discipline actually pays off (noise/geo functions are pure and table-/arithmetic-shaped, which suits the branchless style well).

## 3. Overworld generation pipeline

| Need | Status | Gap | Placement |
|------|--------|-----|-----------|
| Data-driven **stage list** (pipeline as data) | ❌ | Axiom has ordered *systems* (`RuntimeScheduler`) but no notion of a moddable, named stage pipeline | `feature-module:worldgen` defines a stage registry; **stage order as data** (see §10 moddability) |
| Tectonic plates (spherical Voronoi + BFS labelling) | ❌ | needs `region_neighbours` graph + BFS | `module:planet-topology` (graph) + `feature-module:worldgen` (stages) |
| Elevation (plate-boundary distance fields + FBM) | ❌ | needs noise + BFS distance | `feature-module:worldgen` |
| **Stream-power erosion** (iterative, priority-flood) | ❌ | the single heaviest stage in Growth (~35% of gen time) | `feature-module:worldgen` — **see Risk R3 (no threads, branchless loops)** |
| Land-fraction fit, moisture BFS, rivers (downflow/flow/carve) | ❌ | all new | `feature-module:worldgen` |
| Climate: wind field → moisture advection → rain shadow | ❌ | later-phase in Growth too | `feature-module:worldgen` (optional stages) |
| **Atlas build + region graph (CSR neighbours)** | ❌ | the durable output the game reads | `module:planet-atlas` (the queryable result), built by `worldgen` |

Worldgen is a **composition** of topology + noise + graph ops → it is a **feature module** (the only module kind allowed to compose other modules), or an app-level pass first (Risk R1). The *output* atlas is a clean isolated module.

## 4. Surface atlas + query API

| Need | Status | Gap | Placement |
|------|--------|-----|-----------|
| `PlanetSurfaceAtlas` (per-region plate/elev/moist/pos + CSR) | ❌ | new | **module:planet-atlas** |
| `locate_region(unit_dir)` with **spatial index** (not O(R)) | ❌ | Growth learned this the hard way (hierarchical icosphere lookup) | `module:planet-atlas` |
| `sample_surface` → small struct (+ derived temperature/biome) | ❌ | new | `module:planet-atlas` |
| Session ownership of the atlas | 🟡 | could be an ECS resource/component, but ECS has no "resource" concept — only component columns | decide: atlas as a singleton component vs app-owned value |

## 5. Game-world streaming

| Need | Status | Gap | Placement |
|------|--------|-----|-----------|
| `GameWorldLocalMap` (chunk ↔ unit_dir tangent frame) | ❌ | new | `module:game-world` |
| Atlas-seeded **chunk generator** + per-chunk pipeline | ❌ | new (`sample_macro→base_height→detail_noise→build_height_grid`) | `feature-module:game-world` (composes atlas + noise) |
| Continuous (IDW) macro sampling, coherent detail, shared seams | ❌ | Growth's SC-E8 gate; pure functions, testable | `module:game-world` |
| **Chunk store** + focus-radius load + **sim-side unload** | ❌ | spatial set keyed by chunk coord; ECS has no spatial index | `module:game-world` (its own store) |
| **Dynamic chunk meshes streamed in/out at runtime** | 🟡→❌ | `resources` can `register_mesh` arbitrary geometry, but **no proven dynamic per-frame vertex-buffer upload/unload path** — demos bake at startup | **Risk R2** — needs a resources/webgpu streaming path |
| Player edits (dig) mutate cells + emit diffs; edits persist | ❌ | new; mutation + a diff queue (runtime has FIFO queues to model this) | `module:game-world` + app intent wiring |
| Chunk-edit save/load | ❌ | `Reflect`/`BinaryWriter` make this tractable | later; `module:game-world` |

Streaming is where Axiom's "bake geometry at startup" assumption (proven only by RetroFps/stress-cubes) meets the target's "constantly upload/free chunk meshes." This is the **#1 engine-capability gap**, not just a gameplay gap.

## 6. Gameplay layers (all downstream, mostly app-owned)

| Need | Status | Gap | Placement |
|------|--------|-----|-----------|
| Player avatar + play camera + first-person input | 🟡 | `scene` has an FPS controller primitive; RetroFps proves the loop; **input capture is app `web.rs`, not engine** | `app` (+ optional `feature-module:input` later) |
| Interaction ray / picking | 🟡 | `math::Ray` + intersect exist; no picking service against world cells | `app` / `module:game-world` |
| Dig/terraform intents → cell mutation → diff | ❌ | new; model intents on `RuntimeCommandQueue` | `app` → `module:game-world` |
| Sim-owned inventory | ❌ | new; ECS component columns fit | `app` ECS storage, later a module |
| Survival needs/threats, place/build | ❌ | new gameplay systems | `app` (ECS `WorldSystem`s) |
| Guardrailed emergence (bias defs steer gen/spawn) | ❌ | new; data-driven weights | `feature-module:worldgen`/`game-world` + data |
| Spirit influence + possession + **sim-time gate** | ❌ | new; time-gate maps cleanly onto `SimulationClock` advance control | `app` / gameplay module |
| Ecology (template species → regional pops → local spawn) | ❌ | new; ECS + atlas | `app` / `feature-module:ecology` |
| Presentation: cel materials, lighting, biome tint, foliage LOD | ❌ | render is **basic-lit diffuse only**; no custom shaders/materials/PBR/normal maps | `feature-module:render-*` (extend render; feature modules may depend on `render`) |

## 7. Platform / app-framework gaps

| Need | Status | Gap | Placement |
|------|--------|-----|-----------|
| **Async / long world-gen** without freezing the frame | ❌ | Growth uses a worker thread; **Axiom bans `thread::spawn` in the spine**, single-threaded deterministic | **Risk R3** — worldgen must be **cooperative/amortised across ticks** (a stage-stepped job), or run in a `tool`/offline. |
| Input system (keyboard/mouse → actions, rebinding) | ❌ | apps poll raw keys in `web.rs` | `app` first; promote to `feature-module:input` once stable |
| Save/load world + session | ❌ | primitives exist (`Reflect`, `BinaryWriter`, ECS serialize) | `app`/module; format versioned |
| Asset pipeline (meshes/textures/defs from files) | ❌ | only built-in primitives + RGBA8; **no file import** | `tool` + `app`; Axiom is wasm-first so asset delivery differs from Godot `res://` |
| Moddable **data defs** (pipelines, presets, biomes, profiles) | ❌ | Growth is heavily XML-def-driven; Axiom has no def-database concept | `feature-module:defs` or app-loaded data; keep "stage order = data" (see §10) |

## 8. Determinism & QA (an alignment, not a gap)

This is the one area where Axiom is **ahead** of the target. Growth had to *add* determinism hashes, topology-ring validation, and worldgen bench gates as CI after the fact. Axiom enforces determinism, 100% coverage, and structural laws **from day one**. Map Growth's `worldgen_bench` gates (land-fraction, inward-tri, seam delta, determinism hash) directly onto Axiom workspace tests — they become first-class, not bolt-ons. Treat this as a reason the port is *worth* doing here.

## 9. Rendering scale & terrain specifics

- ✅ Instancing backend proven (~200k cubes) — encouraging for many chunk draws.
- ❌ **No frustum culling in the render path** (app must pre-filter; `math::Frustum` exists to do it).
- ❌ **No terrain material/shader** (normals/lighting for height-field terrain, biome splat/tint) — only basic-lit. Needs a render feature-module extension.
- ❌ One draw per object in the command model; terrain LOD/large draw counts need batching strategy (instancing helps for props, less for unique chunk meshes).

## 10. Moddability / data-driven pipeline

Growth's defining trait is **content + assembly as data** (pipeline stage order, presets, biomes, scene/profile bindings are XML, overridable by packs). Axiom has **no def-database / profile concept** and is code-composition-first (apps wire facades). To preserve the target's moddability:
- Represent pipeline **stage order as data** (a list the `worldgen`/`game-world` feature module reads), not a hardcoded system order. Each stage = a registered pure transform keyed by a stable id.
- A `feature-module:defs` (or app-loaded config) parses pipeline/preset/biome data. **Note the branchless constraint**: parsing/`match`-heavy code is awkward branchless — defs parsing may be cleanest in an **app or tool** initially, with only the resolved, validated tables handed to modules.

---

## Hard risks (read these before planning)

**R1 — The spine gates make "port the simulator into the engine" the expensive path.**
Growth's `sim_core` is ordinary branchy, multi-threaded C++. Putting the equivalent into Axiom **layers/modules** means it must be **branchless and 100%-covered** — a large, sustained tax on tens of thousands of lines of generation/erosion/streaming logic. **Mitigation (central recommendation):** build the world/gameplay in an **app** first (apps are exempt from both gates), prove it, then **graduate** only the stable, genuinely-reusable primitives (noise, geo, icosphere, atlas query) down into modules/layers — paying the branchless/coverage tax only on code that has earned permanence. This is also how Axiom's own slice grew.

**R2 — Dynamic streamed meshes are unproven in Axiom.**
The target constantly creates/frees chunk meshes and edits their vertices (digging). Axiom's render path is demonstrated with geometry **baked at startup** (RetroFps/stress-cubes vary transforms/instances, not vertex buffers). A real per-frame **upload/free** path through `resources`→`webgpu`→`windowing` (live wgpu buffers) must be designed and proven early — it is a prerequisite for *any* visible streamed terrain, before gameplay matters. Build a "single dynamic chunk mesh that regenerates each second" spike first.

**R3 — No threads; long worldgen must be cooperative.**
`thread::spawn` is banned in the spine and Axiom is single-threaded-deterministic. Growth's worldgen runs seconds-to-minutes on a worker thread with progress/cancel. On Axiom, worldgen must be a **tick-amortised job** (process N stages/regions per `step`, surface progress, support cancel) or an **offline tool** that bakes an atlas the app loads. Decide this early; it shapes the whole gen architecture. (The determinism upside: a tick-stepped job is trivially replayable.)

**R4 — WASM-first vs the target's desktop assumptions.**
Axiom's live presentation is the browser (`wasm32` + wgpu). The target (from Growth) assumes a desktop Godot context with large worlds, big RAM, threads, and `res://` assets. Browser memory/threading/asset-delivery constraints are real for an "earth-scale" sim. Confirm the deployment target: native wgpu host, or browser with bounded world size. The RetroFps app shows the browser path *works*; it does not show it at planetary scale.

**R5 — No queries / no spatial index in ECS.**
ECS is ordered `BTreeMap` columns with manual filtering and no spatial structure. Chunk lookup, "entities near player," and region location all need spatial indices the engine doesn't provide. Expect to build spatial indexing inside `module:game-world`/`planet-atlas` rather than getting it from ECS. Whether ordered-`BTreeMap` ECS performs at the entity counts an ecology sim wants is **UNCLEAR** and should be benchmarked before committing gameplay-scale entity counts to it.

**R6 — Moddability model mismatch.**
The target's value prop includes data-driven, pack-overridable pipelines/presets/biomes. Axiom is compile-time composition with no def system, and branchless rules make data *parsing* awkward in the spine. Reproducing Growth-level moddability is real new surface (a defs/loader story), not free.

## Net assessment
The foundation is real and the **determinism/QA alignment is a genuine asset** — but the substrate the target is "about" (planetary worldgen, atlas, streaming, terrain rendering with dynamic meshes) is **~80% greenfield**, and Axiom's laws make the engine-spine path costly. The realistic plan is app-first, graduate-the-primitives, and prove dynamic-mesh streaming + a cooperative gen job before any gameplay. See [`roadmap.md`](roadmap.md).
