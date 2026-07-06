# Vertical Slice Audit

> **Structure note (2026-07-06):** the `games/` cartridge tier has since been retired.
> `retro-fps` is now the in-crate `apps/axiom-gallery/src/retro_fps/` demo module (its
> determinism goldens are pinned in `apps/axiom-gallery/slice.toml`, its live harness is
> still `axiom-shot --app retro-fps`). Read `games/retro-fps/…` paths below as
> `apps/axiom-gallery/…`; the slice analysis is otherwise unchanged.

_Repo-wide audit of where Axiom looks architecturally clean locally but fails as an
end-to-end engine pipeline — where a real app can (or cannot) express intent at the
app boundary, flow through existing layers/modules, produce deterministic
intermediate artifacts, and render a visible result through a real backend without
secretly rebuilding engine concepts._

**Date:** 2026-07-04
**Method:** eight parallel subagent investigations (slice inventory, contract
boundaries, app-reimplementation, richness/expressiveness, backend parity, harness
reality, determinism artifacts, architecture-gate blind spots), each reading real
source, cross-checked and spot-verified by the lead.
**Deliverable:** this file only. No engine code was changed by the audit.

---

## 1. Executive summary

Axiom's *structural* laws are green — the layer DAG, module isolation, 100%
coverage, and the branchless spine all hold. But those gates prove **structure, not
semantics**, and beneath them the vertical-slice story has one central fracture:

> **The pipeline Axiom *proves* deterministic is not the pipeline that *renders
> pixels*.**

There are two disjoint rendering paths:

1. **The canonical deterministic slice** — `apps/axiom-demo-rotating-cube`, the
   *only* place the four documented slice modules (`scene → resources → render →
   webgpu`) are composed. It proves byte-equal artifacts at every boundary
   (`SceneSnapshot → ResolvedResources → RenderInput → RenderCommandList →
   GpuSubmission → GpuSubmissionReport`). But it is **headless by construction**:
   `axiom-webgpu` is a *recording-only* backend (`webgpu_api.rs:157-170` captures
   commands into a report and does zero GPU work), so this slice **never produces a
   pixel**.
2. **The real-pixel path** — `tools/axiom-shot` and every gallery/game slice render
   via `axiom-host`'s `FrameOutcome` → `axiom-gpu-backend` (`render_offscreen_rgba`)
   or `axiom-canvas2d-backend`. This path **does not use** `axiom-render`'s
   `RenderCommandList` or `axiom-webgpu`'s `GpuSubmission` at all, and **no
   boundary-determinism proof covers it**.

So the module the vertical-slice doctrine is built around (`axiom-webgpu`) is not on
the path any game renders through, and the path every game renders through is
unproven. `cargo xtask check-architecture` and `cargo test --workspace` are both
green while this holds — the exact "architecture passes while the slice is
semantically fake" failure mode.

The second fracture is **expressiveness**: the engine can push pixels for every
slice but cannot represent a visually-ambitious game *thing* as one coherent engine
object. `Renderable` binds only `mesh + material + visible + shadow`; there is **no
slot for texture, animation state, or gameplay state**, textures live in a *second
id-space* (`ResourceId`) the app must bridge, and `axiom-figure`/`axiom-animation`/
`axiom-physics` are unbound islands. The two most ambitious slices (`soccer_penalty`,
`forest_walk`) therefore **bypass the scene ECS entirely** and ship a parallel engine
app-side (`DioramaObject`, `PenaltyRenderPlan`, `MeshBatch`, three hand-rolled
character rigs). The engine renders the pixels; the *thing* lives in the app.

The good news, and the anchor for every fix: **`games/retro-fps` already uses the
engine object model well** (`spawn((block, Renderable, Player, ContactShadowCaster,
Bounds))`, engine `raycast`/`overlap_box`, engine camera) and has a strong
tick→state determinism proof. It is the slice closest to golden and the right one to
harden first.

**Counts:** Critical 2 · High 4 · Medium 9 · Low 5.

Recommendations do **not** weaken any rule, merge modules, or expose private
internals. They (a) unify the two rendering paths so the proven pipeline is the
rendered pipeline, (b) add an object-binding contract so richer things stop being
app-rebuilt, (c) make canvas2d degrade from one semantic scene instead of diverging,
and (d) add **semantic slice gates** (`slice.toml` + `xtask check-slices`) that the
current structural checks cannot express.

---

## 2. Definition of a healthy Axiom vertical slice

A slice is healthy when **all** hold:

- The app expresses intent/data at the app boundary (spawn objects, feed intents),
  not by hand-authoring backend buffers.
- Data flows through **existing** layers/modules via their facades, and the app glue
  is *minimal translation* between module contracts (the sanctioned
  `scene_to_render_input` shape), never hidden engine logic.
- Every boundary produces a **deterministic, inspectable artifact**: same input →
  byte-equal output, captured and comparable.
- A **visible result renders through a real backend** — and the pipeline that
  renders is the same pipeline that is proven deterministic.
- The app owns **only game-specific gameplay logic**; identity, transform, mesh,
  material, texture, animation, collision-proxy, and render assembly are engine
  concepts bound into one object model, not app-scattered.
- The slice is capturable through a **common harness** and carries a **tick-replay
  proof** and a **backend capability/degradation proof**.

## 3. Definition of a fake or unhealthy vertical slice

A slice is unhealthy when any of the charter's 15 conditions hold. Observed here, in
order of damage: the app reconstructs the engine (scene graph / render plan /
material palette / rig) app-side (#1, #15); the render-facing "slice" is a different
pipeline from the one proven deterministic (#4, #12); the same semantic scene renders
differently across GPU/canvas2d because they consume divergent contracts (#6); a
thing's texture/transform/animation/physics are unrelated app structures re-synced
each frame (#7); the visible feature works only because of an app special case (#8);
the slice unit-tests isolated structs but never proves tick→visible-result (#11); and
the slice has no real capture path or no common harness (#14).

---

## 4. Inventory of runnable slices

(Full 5-table inventory from Subagent 1. `pipe` = stage exists but hidden behind the
`axiom-render-pipeline` facade, never surfaced as a named app-level contract. Only
`axiom-demo-rotating-cube` surfaces the named contract chain.)

### Standalone apps + game

| Slice | manifest | det tick | runtime | scene/sim | resources | RenderInput | RenderCmdList | backend submit | capture | det-replay | parity |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **axiom-demo-rotating-cube** | Y | Y | Y | Y | Y | **Y named** | **Y named** | **Y webgpu (records-only)** | example only | Y golden ✓ | N |
| **retro-fps** (`games/`) | Y | Y | Y | Y | pipe | pipe | ~ bin/render | Y wasm + shot + bins | Y golden ✓ | N |
| axiom-animation-lab | Y | Y | N | ~figure/anim | N | N | N | **SVG only** | N | N |
| axiom-asset-stream-demo | Y | ~ | N | assets | ~ | N | N | N | N | N |
| axiom-game-runtime | Y | Y | Y | engine | pipe | pipe | Y wasm | Playwright | N | N |
| axiom-netcode-demo/sim | Y | Y | ~ | sim | N | N | N | N | ~/Y | N |
| axiom-netplay-ffi | Y | Y | ~ | sim(FFI) | N | N | N | N | Y | N |
| axiom-proc-player | Y | ~ | N | bakes→resources | Y | N | N | **never renders** | Y(struct) | N |
| axiom-proc-playground | Y | N | N | proc | ~ | N | N | N | ~ | N |
| axiom-sim-crucible | Y | Y | ~ | sim-core | N | N | N | N | ~ | N |
| axiom-workspace | Y | Y | Y | dev-console host | N | N | N | hosts gallery | N | N |
| axiom-worldgen-demo | Y | N | N | ~streaming | N | N | N | PNG maps | N | N |

### Gallery demos (`apps/axiom-gallery/src/<demo>`, `<demo>_start` wasm entries)

rotating_cube, stress_cubes, physics_crucible (3-backend `compare` ✓); growth
(agent+visual_target bins ✓); soccer_penalty (shot + agent + 2 visual_targets);
zanzoban, quintet (bespoke Canvas2D 2D games); generia (canvas2d micro-FPS);
forest_walk (**orphaned** — wasm entry + web page but NOT in `gallery.js` DEMOS and
NOT in axiom-shot); netplay; harness; retro_fps (re-hosted).

### Harnesses · visual targets · examples

- **Harnesses (4, no common path):** axiom-shot (native, hardcoded 5-app match:
  `retro-fps`/`showcase`/`nova-roll`/`physics-crucible`/`soccer-penalty`, else →
  showcase); visual-target bin (scene-`manifest.toml` only, growth diorama);
  per-app agent/render bins (growth `shots`, retro-fps `render.rs`, soccer text-only);
  Playwright over the gallery wasm bundle keyed on `gallery.js` DEMOS.
- **Visual targets (3, two harnesses):** `apps/axiom-gallery/visual_targets/
  prologue_postcard_001` (visual-target bin); `visual_targets/soccer_penalty_kick`
  and `.../soccer_penalty_gameplay` (axiom-shot). Same directory convention, two
  different renderers — a reviewer cannot tell which produced the pixels.
- **Examples:** `run_vertical_slice.rs` (full slice driver), `growth_render_maps`,
  `multiplayer_sim`, `generated_micro_fps`, introspection/reflection evidence.

**Determinism-replay present:** rotating-cube (golden), retro-fps (golden),
netplay-ffi, netcode-demo, physics-crucible (sim digest, cross-platform), soccer
(in-memory only), quintet/zanzoban (game-state golden). **Absent:** forest_walk,
generia, harness, stress_cubes, animation-lab, worldgen-demo, proc-*.

---

## 5. Boundary map for each major slice

```
ROTATING-CUBE (proven, headless — the ONLY named-contract chain):
 tick →[axiom-host]→ HostFrameReport →[axiom-frame]→ EngineFrame
   →[axiom-scene]→ SceneSnapshot
   →[axiom-resources]→ ResolvedResources
   ==app glue scene_to_render_input==> RenderInput →[axiom-render]→ RenderCommandList
   ==app glue render_to_gpu_submission==> GpuSubmission →[axiom-webgpu RECORDING]→ Report
   ⟹ NO PIXELS.  Golden bytes at every boundary. tick0≠tick60. replay byte-equal.

REAL-PIXEL PATH (every gallery/game slice + axiom-shot — UNPROVEN, disjoint):
 tick →[axiom-host RunningApp]→ FrameOutcome{mesh_batches, lights, light_view_proj,
        sdf_scene, clear_color}
   ├─ GPU:      GpuBackendApi::render_offscreen_rgba(meshes, materials, lights, sdf,
   │            ambient, retro_32bit)                → pixels
   └─ Canvas2D: frame_packet_from_batches(batches)   → Canvas2dBackendApi.present
                (drops materials, lights, sdf, ambient, retro — see §6/M3)
   ⟹ PIXELS.  No RenderCommandList, no GpuSubmission, no boundary determinism proof.

SOCCER / FOREST (bypass the scene ECS — a parallel app-side engine):
 tick → app SceneBuilder → DioramaObject / MeshBatch (app scene) →
        PenaltyRenderPlan / hand-packed 36-float instances → run_web_multi / respawn-all
   ⟹ PIXELS via app reconstruction. Engine object model unused.
```

The two boxes never meet: `axiom-webgpu`/`RenderCommandList` (proven) and
`axiom-gpu-backend`/`FrameOutcome` (rendered) are different modules with different
shapes. That gap is C1.

---

## 6. Findings by severity

### CRITICAL

#### C1 — The proven pipeline and the rendered pipeline are disjoint; `axiom-webgpu` never renders
- **Files:** `modules/axiom-webgpu/src/webgpu_api.rs:157-170` (recording-only
  `submit`); `apps/axiom-demo-rotating-cube/VERTICAL_SLICE.md:113-152` (headless by
  design); `tools/axiom-shot/src/main.rs:413-475` (`render_gpu`/`render_canvas2d`
  consume `FrameOutcome`, not `RenderCommandList`); `modules/axiom-gpu-backend/src/
  scene_renderer.rs`, `modules/axiom-windowing/src/windowing_api/web.rs:1190-1238`.
- **Slice:** all (the doctrine slice vs every real slice).
- **Current behavior:** the four modules the CLAUDE.md vertical-slice section is
  entirely about (`scene/resources/render/webgpu`) compose only in rotating-cube, and
  that composition ends in a records-only backend that emits no pixels. Every slice
  that renders pixels uses a second path (`FrameOutcome → axiom-gpu-backend/
  canvas2d-backend`) that never constructs a `RenderCommandList` or `GpuSubmission`.
- **Why a slice problem:** (charter #4, #6, #12) the determinism proof proves a
  pipeline no game renders through; the rendering pipeline has no boundary proof. The
  "vertical slice" as documented is semantically fake as a *rendering* proof.
- **Why local tests missed it:** rotating-cube's golden tests pass (they test the
  recording path); each gallery slice's tests pass (they test app state). No test
  asserts the two paths agree, and `check-architecture` proves only the DAG — both
  paths are legal.
- **Smallest structurally correct fix:** give `axiom-webgpu` a live backend arm
  (`BackendKind::Live` per VERTICAL_SLICE.md:143-147) **or** route the real-pixel
  path through `RenderCommandList` → a live `GpuSubmission`, so the proven boundary
  chain is the one that renders. Then extend rotating-cube's golden proof to a
  tolerance-bounded *screenshot* through the same chain. This is a sizable, sign-off
  level structural change — it is the central fix.
- **Placement:** Module (`axiom-webgpu` live arm) + Layer (`axiom-host` surface/
  device capability, already scoped in VERTICAL_SLICE.md:122-139) + Test/Harness.
  **Requires:** code + tests + harness.

#### C2 — The most ambitious slices bypass the scene ECS and ship a parallel engine app-side
- **Files:** `apps/axiom-gallery/src/soccer_penalty/penalty_scene.rs:63`
  (`DioramaObject` — "app-local scene data, not an engine scene node"),
  `penalty_render_plan.rs:176-233` (`PenaltyRenderPlan`/`PenaltyRenderItem`),
  `penalty_materials.rs:20` (`PenaltyMaterialId`), `low_poly_assets.rs:3`
  ("TEMPORARY APP GLUE. Axiom has no soccer/mesh-part asset module"),
  `penalty_render_meshed.rs:260,294` (despawn-all/respawn-all every frame — no
  persistent identity); `apps/axiom-gallery/src/forest_walk/mod.rs:45-221`
  (`MeshBatch`/`Inst`, hand `project_batches`, own first-person controller).
- **Slice:** soccer_penalty, forest_walk (and growth's `visual_target/build.rs`).
- **Current behavior:** these slices do not `spawn` engine objects and flow through
  `SceneSnapshot`; they build an app-local scene model, render plan, material palette,
  and (soccer) three character rigs, then either respawn-all into the engine each
  frame or bypass into `run_web_multi` with hand-packed instance floats.
- **Why a slice problem:** (charter #1, #2, #8, #15) the visible result exists only
  because the app rebuilt the engine. There is no stable engine object identity across
  frames; the slice cannot be re-composed by a different app/backend without carrying
  the app's parallel engine.
- **Why local tests missed it:** apps are coverage- and branchless-exempt, so a
  1000+-line app-side render engine passes every gate (see M7); the app's own tests
  assert its parallel structures, which are internally consistent.
- **Smallest structurally correct fix:** requires H3's object-binding contract first;
  then migrate soccer/forest onto `spawn`-ed engine objects (identity + transform +
  mesh + material + texture + animation-ref + Bounds) and delete the app-local scene/
  render/material models. Large but bounded once H3 lands.
- **Placement:** Module (object contract) + App (delete parallel engine, keep
  gameplay). **Requires:** code + tests.

### HIGH

#### H1 — Soccer carries THREE character rigs; two hand-roll `axiom-figure`/`axiom-animation`
- **Files:** `apps/axiom-gallery/src/soccer_penalty/penalty_goalie_pose.rs:22-466`
  (16-part figure + parent chain + `PoseSampler` clip timeline — a full duplicate of
  figure+animation); `penalty_character.rs:1-16` (second humanoid rig; header claims
  *"Axiom has no character-rig module"* — **factually false**); vs `penalty_kicker.rs:
  12-13` which correctly imports `FigureApi`+`AnimationApi` in the *same crate*.
- **Slice:** soccer_penalty.
- **Current behavior:** one game ships `axiom-figure` (kicker, correct) plus two
  parallel app rigs (goalie, character) with their own parent-transform resolvers and
  clip samplers.
- **Why a slice problem:** (charter #15, #1) an app re-implements the articulated-
  figure + clip-animation engine modules that already exist and are already used
  beside it. Pure drift; the "no module" justification is stale.
- **Why local tests missed it:** the app rigs are internally tested; nothing asserts
  "the app does not re-implement a module."
- **Smallest fix:** migrate goalie + character onto `axiom-figure`+`axiom-animation`;
  delete `penalty_goalie_pose`'s clip system and `penalty_character`'s kit; remove the
  stale comment.
- **Placement:** App→Module reuse. **Requires:** code + tests + doc (comment).

#### H2 — First-person camera+controller verbatim-duplicated between `forest_walk` and `generia`
- **Files:** `apps/axiom-gallery/src/forest_walk/mod.rs:35-282` ↔
  `apps/axiom-gallery/src/generia/mod.rs:68-539` — identical constants (`EYE_HEIGHT_M`,
  `MOVE_SPEED_M`, `TURN_SPEED`, `LOOK_SENS`, `PITCH_LIMIT`), identical `Pose`/`Keys`/
  `Look`, byte-identical `step`, `camera_view_proj`, `install_mouse_look`. Engine
  `ControllerSystem`/`ControllerState` (`scene_storage.rs:273`) + `axiom-input` exist
  and are unused.
- **Slice:** forest_walk, generia.
- **Current behavior:** a complete first-person walker (an engine concept) is copy-
  pasted across two sibling gallery apps.
- **Why a slice problem:** (charter #15, #9) duplicated engine input/camera state
  machine; two copies drift independently.
- **Smallest fix:** one shared first-person controller (feature module or route
  through scene `ControllerSystem` + `axiom-input`); both apps consume it.
- **Placement:** Module/Feature-Module. **Requires:** code + tests.

#### H3 — No object contract binds the facets; texture/animation/gameplay-state have no slot; figure/animation/physics are unbound islands
- **Files:** `modules/axiom-scene/src/renderable.rs:18-20` (`Renderable` = `mesh +
  material + visible + shadow` only; `MaterialRef` u64 is a *different id-space* from
  resources' `ResourceId`); `scene_snapshot.rs:21` (snapshot = `{nodes, cameras,
  lights, renderables, sdf}` — no texture/tag/bounds/animation); `modules/axiom-figure/
  src/figure_api.rs:14,49` ("never touches the animation module — the app drives the
  skeleton"); `modules/axiom-resources/src/material_data.rs:12-18` (`MaterialData` =
  base_color+texture only) vs `modules/axiom-render/src/render_material.rs:28-36`
  (full emissive/roughness/opacity/texture) — the two material shapes disagree.
- **Slice:** all rich slices (soccer, forest, retro-fps, physics-crucible).
- **Current behavior:** a game "thing" (ball, character, enemy, rigid body) cannot be
  one engine object — its texture, animation, and gameplay state live in side
  structures the app hand-syncs every frame; `axiom-figure`/`axiom-animation`/
  `axiom-physics` share no id with a scene node.
- **Why a slice problem:** (charter #7, #10) contracts are too thin to carry a serious
  game object, which forces C2's app-side reconstruction. This is the root cause under
  C2/H1/H2.
- **Why local tests missed it:** each module is 100% covered in isolation; nothing
  tests that a *thing* survives as one object across the pipeline.
- **Smallest fix:** add an object-binding contract — extend `Renderable`/scene columns
  to carry `texture`, an `animation-ref`, and roll `Tag`/`Bounds` into `SceneSnapshot`;
  unify the material id-space (resources `MaterialData` grows to the render catalog, or
  the two are explicitly bridged by one owner); give `axiom-figure`/`axiom-animation`
  a scene-node binding so a posed character is one object. Sizable, sign-off level
  contract work (see §9's minimum-playable-slice for the concrete shape).
- **Placement:** Module (scene/resources/render/figure/animation contracts).
  **Requires:** code + tests + docs.

#### H4 — Ad-hoc 36-float instance-batch packing + camera view-proj hand-rolled across ~5 sub-apps
- **Files:** `forest_walk/mod.rs:188-191`, `generia/mod.rs:353-379`,
  `zanzoban/scene3d.rs:100-110` (`FLOATS_PER_INSTANCE = 36`),
  `growth/visual_target/build.rs:615-624`, `growth/bin/agent.rs:260` — each hand-packs
  `[mvp(16), world(16), colour(4)]` and computes `view_proj.multiply(world)` by hand
  against an untyped layout, feeding `run_web_multi`. Contrast the correct
  `soccer_penalty/web.rs:143` / `physics_crucible/web.rs:140` which use
  `outcome.camera_view_proj()` and let the engine supply meshes.
- **Slice:** forest_walk, generia, zanzoban, growth.
- **Current behavior:** render-command construction done ad-hoc, five times, against
  an undocumented-in-types 36-float contract — the bypass-into-`run_web_multi` apps.
- **Why a slice problem:** (charter #1, #10) apps author backend-specific instance
  buffers because no typed instance-batch primitive exists; the layout contract is
  implicit and drift-prone.
- **Smallest fix:** a typed instance-batch builder in `axiom-render` (or windowing
  input) that owns the 36-float layout; apps append typed instances.
- **Placement:** Layer/Module. **Requires:** code + tests.

### MEDIUM

#### M1 — Single `BASIC_LIT` pipeline is the render ceiling; textures are dangling ids with no payload channel
- **Files:** `modules/axiom-render/src/render_pipeline_kind.rs` (`BASIC_LIT = 1` is
  the only pipeline), `render_api.rs:206` (hardcoded `set_pipeline(BASIC_LIT)`),
  `render_material.rs:35` (`texture_id: u64` with no pixel payload across render→gpu),
  demo `render_to_gpu_submission.rs:164-174` (`GpuCommandArtifact` has no texture
  field — the demo slice silently drops textures/emissive/roughness/opacity).
- **Slice:** all. **Why:** (charter #10) no per-object shader/pipeline/pass selection
  and no texture channel means the render contract can't carry a serious title.
- **Fix:** per-object pipeline id + a texture-binding channel through render→gpu.
  **Placement:** Module. **Requires:** code + tests.

#### M2 — `RenderReport` bakes the wgpu depth convention while also feeding Canvas2D
- **Files:** `modules/axiom-render-pipeline/src/render_pipeline_api.rs:16-21`
  (`GL_TO_WGPU_DEPTH` pre-multiplied into `view_projection`/`light_view_proj`),
  `:518-519` (documented "wgpu-ready") — yet the same report feeds canvas2d; contrast
  `render_api.rs:290-291` (`build_frame_packet` deliberately stays neutral).
- **Slice:** all real-pixel. **Why:** (charter #13, #6) a "neutral" contract with one
  backend's clip convention baked in makes canvas2d a second-class consumer and leaks
  backend-specificity. **Fix:** keep `RenderReport` neutral; apply the depth
  convention in the wgpu consumer. **Placement:** Feature-Module. **Requires:** code +
  tests.

#### M3 — `RenderCapability` is vestigial: 4/6 flags dead, GPU ungated, live canvas never restricted, silent drops
- **Files:** `crates/axiom-host/src/frame_capability.rs:16-32` (6 flags;
  `Sdf`/`AlphaMask`/`DetailInstancing`/`Retro32Bit` gate nothing anywhere),
  `modules/axiom-canvas2d-backend/src/software_rasterizer.rs:271-292` (only Volumetrics
  + PostProcess consulted; SDF runs **un**gated), `canvas2d_backend_api.rs:226-236`
  (textures/shadows reported as `degraded_features` — telemetry, not policy),
  `modules/axiom-gpu-backend/src/scene_renderer.rs:92-149` (PCF shadows, albedo,
  alpha cutout, normal mapping — no capability concept), `web.rs:1481-1486` (live
  canvas keeps default `all()`).
- **Slice:** all backend paths. **Why:** (charter #5, #13) the system meant to let a
  rich scene degrade gracefully barely functions; new GPU features silently no-op on
  canvas2d with no flag forcing an explicit decision — the enum already drifted out of
  sync with reality. **Fix:** make every GPU-only feature a live capability flag with a
  declared degradation (substitute or documented drop); gate both backends. **Placement:**
  Layer (`axiom-host` capability) + Module (backends). **Requires:** code + tests.

#### M4 — Two divergent backend construction paths; parity tests are narrow
- **Files:** `modules/axiom-windowing/src/windowing_api/web.rs:1190-1238` (GPU takes
  `batches` directly; Canvas2D reconstructs a `FramePacket` — not a shared source of
  truth), `tools/axiom-shot/tests/{translucency_parity,draw2d_parity}.rs` (only loose
  centroid agreement + 2D byte parity; **no** texture/shadow/SDF/alpha parity),
  `apps/axiom-workspace/src/runtime_viewport.rs:10-30` (multi-surface "compare" is a
  non-rendering placeholder).
- **Slice:** all backend paths. **Why:** (charter #6) the backends consume different
  shapes and only a translucent-quad centroid is asserted equal; semantic parity is
  unproven for exactly the features that diverge. **Fix:** one shared semantic frame
  contract both backends consume; a parity test per feature class. **Placement:**
  Module + Test/Harness. **Requires:** code + tests.

#### M5 — No common capture harness; orphaned slices with no native pixel path
- **Files:** `tools/axiom-shot/src/main.rs:259-279` (hardcoded 5-app match; else →
  showcase), `apps/axiom-gallery/src/lib.rs:47-58` (`rotating_cubes_app()`/
  `stress_cubes_app()`/`crucible_app()` are callable but unregistered),
  `growth/bin/visual_target.rs:401` vs `axiom-shot/main.rs:540` (triplicated
  `present_request`/`write_png`/`frame_packet`), forest_walk (orphaned — no DEMOS
  entry, no shot registration), `Cargo.toml:115` (`tools/axiom-shot` excluded from the
  workspace).
- **Slice:** rotating-cube, stress-cubes, generia, forest_walk (no native pixel path
  despite exposing an `App` core). **Why:** (charter #14) four fragmented dispatch
  mechanisms, no shared entry point; adding a slice means editing a closed match.
  **Fix:** an app-registry (`fn app_core(name) -> RunningApp`) each demo contributes,
  so axiom-shot renders any registered slice; fold the render bins + visual-target
  renderer into one offscreen-capture path. Immediate win: register the four already-
  buildable `App` cores. **Placement:** Tooling + App (registration). **Requires:**
  code + harness.

#### M6 — Render/GPU determinism chain proven for exactly one slice; soccer/forest lack committed goldens
- **Files:** `apps/axiom-demo-rotating-cube/tests/golden_artifacts.rs` (all six
  boundaries, committed bytes — the only complete proof), `games/retro-fps/tests/
  retro_fps_replay_determinism.rs:99-102` (render+runtime artifacts recorded as
  **empty** `Vec::new()`), soccer tests (in-memory `assert_eq!(build(),build())` only,
  **no** `golden/` dir), forest_walk (no dedicated determinism test).
- **Slice:** all except rotating-cube. **Why:** (charter #11) no other slice proves
  its render command list or GPU submission is deterministic; two slices can't even
  catch cross-commit drift (no golden). **Fix:** once C1 unifies the path, every
  declared slice carries a boundary golden + a negative (perturbed-differs) assertion.
  **Placement:** Test/Harness (enforced by M7's gate). **Requires:** tests.

#### M7 — Architecture gates are structure-only; engine logic hidden in apps is invisible; no slice-semantics gate exists
- **Files:** `crates/xtask/src/main.rs:25` (single subcommand `check-architecture`),
  `crates/xtask/src/coverage_scope.rs:22` (the `apps|games|tools|xtask` ignore),
  `apps/axiom-gallery/src/growth/visual_target/build.rs` (**1189 lines** of neutral-
  render engine math — meshes/instances/matrices/shadow/fog/scatter — that is coverage-
  exempt AND branchless-exempt because it lives under `apps/`: uses `match` L168,
  `if let` L120, `for` L397+), `golden_artifacts.rs:29-44` (missing golden is silently
  re-blessed; `AXIOM_REGOLD=1` rewrites).
- **Slice:** all. **Why:** (charter #12) the exemption boundary is drawn by directory,
  but "is this engine logic?" is a property of the code; the code that turns a scene
  into pixels is the least-gated in the repo, and nothing requires a slice to prove
  determinism, a reference-hash, golden integrity, or harness registration. **Fix:**
  the `slice.toml` + `xtask check-slices` gate in §12, and extract `build.rs` into a
  feature module so the spine gates reclaim it. **Placement:** Tooling + Module.
  **Requires:** code + tests.

#### M8 — `Tag` is stored in scene but dropped from `SceneSnapshot`
- **Files:** `modules/axiom-scene/src/tag.rs:13` (semantic "what is this" kind, used
  by perception raycasts), `scene_snapshot.rs:41-79` (snapshot never reads it — 0 tag
  mentions). `games/retro-fps/src/lib.rs:272` re-derives enemy-vs-wall kind app-side
  (`enemy_index_of`) instead of using `Tag`.
- **Slice:** retro-fps, perception consumers. **Why:** (charter #7) perception and
  render see different scenes; the game that most needs semantic kind re-implements it.
  **Fix:** roll `Tag` into `SceneSnapshot`/`RenderObject`. **Placement:** Module.
  **Requires:** code + tests.

#### M9 — Material shape disagreement between resources and render tiers
- **Files:** `modules/axiom-resources/src/material_data.rs:12-18` (base_color +
  optional texture) vs `modules/axiom-render/src/render_material.rs:28-36` (emissive/
  roughness/opacity/texture). The resource tier cannot express what the render tier
  accepts, so full materials are only reachable via the pipeline's
  `frame_add_lit_material`, not the resource table.
- **Slice:** all. **Why:** (charter #10) the resolved-resources contract is thinner
  than the render material, forcing the pipeline to author materials outside resources.
  **Fix:** grow `MaterialData` to the render catalog (one material contract).
  **Placement:** Module. **Requires:** code + tests.

### LOW

- **L1 — Stale comment.** `penalty_character.rs:1-16` asserts "Axiom has no
  character-rig module" — false (`axiom-figure` exists, used by the kicker). Fix with
  H1. Documentation.
- **L2 — Duplicated scalar math.** `lerp`/`smoothstep` re-implemented ~6× in
  `growth/{vista,visual_target/build,visual_target/scatter}.rs` while `axiom-math`
  exposes `lerp`. Module reuse. Code.
- **L3 — SVG stand-in counted as capture.** `axiom-animation-lab` emits SVG, not real
  pixels; its figure content is only really captured via soccer's axiom-shot path.
  Register a posed-figure scene in axiom-shot. Harness.
- **L4 — Camera computed two ways.** `zanzoban/scene3d.rs:67` flags its
  `view_projection` as a "native-test fallback" distinct from the live path — a smell
  that live and test compute the camera differently. Fold into H4's camera builder.
  Code.
- **L5 — Harness outside all gates.** `Cargo.toml:115` excludes `tools/axiom-shot`
  from the workspace, so the capture tool is untested by `cargo test --workspace`.
  Re-include it (it's tooling, coverage-exempt anyway) so its parity tests run in CI.
  Tooling.

---

## 7. Golden vertical slice contract

Every serious Axiom slice must prove **all** of these artifacts, captured through a
real harness (not unit structs, not SVG, not mocks):

1. **Deterministic input/intents** — a fixed intent track; same track → same run.
2. **Deterministic runtime step** — `HostFrameReport` byte-equal per tick.
3. **Deterministic scene/sim snapshot** — `SceneSnapshot` byte-equal per tick.
4. **Deterministic resolved resources** — `ResolvedResources` byte-equal.
5. **Deterministic render input** — `RenderInput` byte-equal.
6. **Deterministic render command list** — `RenderCommandList` byte-equal.
7. **Deterministic backend submission report** — the live `GpuSubmission`/report
   byte-equal (C1 makes this the *rendered* path, not a records-only stand-in).
8. **Deterministic or tolerance-bounded screenshot** — real pixels from the harness;
   Canvas2D byte-exact, GPU within a pinned tolerance, hashed in the slice manifest.
9. **Tick replay proof** — a fixed scenario run twice is byte-equal at every boundary,
   AND a *perturbed* run differs (no vacuous `assert_eq!(x, x)`; the
   `retro_fps_replay_determinism.rs::a_diverging_input_track_is_detected` pattern).
10. **Backend capability/degradation proof** — the slice declares its required
    capabilities and a test asserts each backend either provides them or degrades per
    the declared policy (§10).
11. **No app-owned engine system unless explicitly justified** — the slice owns only
    gameplay logic; identity/transform/mesh/material/texture/animation/collision/render
    assembly come from engine modules. Any exception carries a written justification
    checked by the gate.

Today **only `axiom-demo-rotating-cube` proves 1–7 and 9** (and never 8/10). No slice
proves all eleven.

---

## 8. Minimum playable game slice (the next real target beyond the rotating cube)

**Harden `games/retro-fps` into the first slice that proves all eleven golden
artifacts through the real-pixel path.** It is chosen because it already: uses the
engine object model (`spawn((block, Renderable, Player, ContactShadowCaster,
Bounds))`), uses engine `raycast`/`overlap_box`/camera, and has a strong tick→state
golden. The concrete target — a "DOOM room" slice:

- **A player** (identity + transform + camera + input intents) driven by
  `axiom-input` + scene `ControllerSystem` (fix L3-style pose read-back so the app
  stops mirroring).
- **≥2 enemies** as single engine objects binding identity + transform + mesh +
  **textured** material + **an animation-ref** (a bob/hit clip via
  `axiom-figure`/`axiom-animation`) + `Bounds` collision proxy + **gameplay state on
  the entity** (`DynamicComponents` health/liveness, not an app `Enemy` struct) +
  `Tag` = Enemy.
- **Walls** as objects with `Tag` = Wall (so `enemy_index_of` scanning dies).
- **Render** through the unified C1 path so `RenderCommandList` → live `GpuSubmission`
  → pixels is the *proven* chain, captured by axiom-shot on both GPU and Canvas2D.
- **Proof:** the full §7 contract, including a tolerance-bounded screenshot and a
  capability/degradation assertion (textured enemy on GPU; declared texture-drop on
  Canvas2D).

Delivering this proves richer engine concepts than the cube (identity, textured
materials, animation binding, semantic tags, collision, gameplay state, backend
degradation) on a real game, and becomes the template every later slice copies.

---

## 9. Backend fallback policy (canvas2d degrades from one semantic scene, not a second ceiling)

1. **One semantic frame, many backends.** Both GPU and Canvas2D consume the *same*
   semantic frame contract (fixes M4's divergent construction). No app builds a
   different scene per backend; no backend takes a private data shape.
2. **Every GPU-visible feature is a declared capability.** The `RenderCapability`
   enum must cover *every* feature any backend implements (textures, alpha cutout,
   normal maps, PCF shadows, SDF, retro profile, volumetrics, post-process) — fixing
   M3's four dead flags. A new GPU feature that adds no capability flag fails the gate.
3. **Each capability declares a degradation.** For every capability a backend lacks,
   the policy names either a cheaper substitute (e.g. PCF shadows → planar contact
   shadow) or an explicit, reported drop (`degraded_features`). Silent no-op is
   forbidden.
4. **Both backends are gated by the same profile.** GPU is no longer unconditionally
   full; the live Canvas2D is no longer unconditionally `all()`. The profile is set on
   both from the frame's declared requirements.
5. **Canvas2D never sets the ceiling.** The semantic scene always carries full
   richness; Canvas2D drops or substitutes per policy. Nothing lowers the GPU path to
   Canvas2D's level.
6. **Parity is asserted per capability** (M4): a test per feature class proves GPU and
   Canvas2D render the same semantic scene up to the declared degradation.

---

## 10. Harness requirements (which apps must be wired into which harness)

- **A single common capture path (M5).** axiom-shot gains an app-registry so any
  slice exposing a `RunningApp` core is renderable by name; the growth/soccer/retro
  render bins and the visual-target scene renderer collapse into one offscreen-capture
  routine (dedup `present_request`/`write_png`/`frame_packet`). `tools/axiom-shot`
  re-enters the workspace (L5).
- **Must be registered in axiom-shot (native pixel path):** `rotating-cube`,
  `stress-cubes`, `generia`, `forest_walk` (all expose a buildable `App` core today and
  have no native capture) — one-line registrations each; plus `axiom-demo-rotating-cube`
  (render a real frame, not only struct goldens), `axiom-animation-lab` (a posed-figure
  scene instead of SVG), `axiom-proc-player` (render the baked room it currently
  discards).
- **Legitimately Playwright-only:** `netplay`, `zanzoban`, `quintet`, `harness`
  (network/2D-canvas/overlay — no 3D `App` core); keep, but pin a Playwright screenshot
  route per demo.
- **Every visual target names its harness.** `visual_targets/*` manifests record which
  renderer produced the pixels (soccer→axiom-shot, prologue→visual-target), so the two
  `visual_targets/` roots stop being ambiguous.

---

## 11. Semantic gates to add (prove end-to-end behavior; no existing rule weakened)

Introduce a `slice.toml` manifest per renderable app/game and one `xtask check-slices`
subcommand, wired into `check-architecture`'s workspace test so it runs in CI:

1. **`check-slices` (new xtask subcommand).** For each `slice.toml`, assert:
   - a **determinism test** exists that runs a fixed scenario twice byte-equal AND a
     perturbed run differs (kills vacuous goldens);
   - each declared **golden `.bin`** exists and matches its `slice.toml`-recorded
     SHA-256 (fixes M7's trust-on-first-use / `AXIOM_REGOLD` silent-reblessing — the
     checker cannot be silenced by the env var);
   - a **reference screenshot** exists with a recorded hash, produced by the real
     harness (Canvas2D byte-exact, GPU tolerance-bounded); manifest and reference move
     together or the test fails (kills stale/mocked screenshots);
   - the app's **`harness_entry` symbol exists** (reuse `check.rs:420`'s
     `find_public_export` scan) and the app is **registered in axiom-shot's registry**
     (fixes M5's "runnable but un-harnessable").
2. **`check-slice-placement` (extend the class-aware check).** Flag an `apps/` source
   file that defines mesh/instance/matrix-producing `pub` functions and is a large pure
   data-transform with no module-facade call — i.e. engine logic hiding in an app (M7,
   C2). The honest resolution is extraction into a feature module, which pulls it back
   into the coverage+branchless spine.
3. **`check-object-binding` (lint or xtask).** Flag an `apps/` type that re-declares a
   scene/figure/rig/material system (`DioramaObject`, `PenaltyRenderPlan`, a parent-
   transform resolver) — steering C2/H1/H2 fixes and preventing regrowth.

These add assertions only in the currently-ungated app/slice seam; they do not touch
the Layer/Module/Coverage/Branchless laws.

---

## 12. Do not change — rules that must remain strict

- **Layer Law / DAG.** No layer imports outside its `depends_on`; the kernel stays a
  root. Unifying the render path (C1) must not add an illegal edge.
- **Module isolation.** Engine modules never import each other; only apps and
  feature-modules compose. The object contract (H3) lives in a module/feature-module,
  not by making one engine module import another.
- **Apps are leaves; no engine code depends on an app.** The fixes pull logic *out of*
  apps into modules — never the reverse. Do not make a module depend on a private app
  internal.
- **No merging modules to dodge a boundary.** `scene`/`resources`/`render`/`webgpu`
  stay separate; C1 unifies the *path*, not the modules.
- **100% spine coverage + branchless spine.** Extracting app render logic into modules
  (M7) means it must arrive fully covered and branchless — that is the point, not a
  reason to widen the ignore list.
- **Determinism invariants.** No hidden nondeterminism, no ambient wall-clock, no
  unseeded randomness, no unstable iteration order in deterministic artifacts. The new
  slice goldens depend on these.
- **No browser/platform APIs inward** (host/windowing allowlist only); **no junk-drawer
  modules** (`utils`/`helpers`/`common`/`misc`). A shared controller/camera/instance-
  builder (H2/H4) must be a named capability module, not a grab-bag.

---

## Validation

`cargo test --workspace` and `cargo xtask check-architecture` were run after writing
this report; results are in the final response accompanying this file. The audit made
**no code changes** other than creating this file, so it cannot have introduced a new
failure; any failure reported is pre-existing.
