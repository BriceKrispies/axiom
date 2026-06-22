# Canvas 2D backend — implementation plan

**Status:** plan only. No code written yet.
**Requirement (accepted, not up for debate):** Axiom must support Canvas 2D as a
first-class last-resort browser fallback, forced through the *same* render
structure as WebGPU/WebGL2. Goal is not identical pixels — it is that a game
authored/tested against the normal render path stays **recognizable, playable,
deterministic, and structurally identical** when the backend is Canvas 2D.

Target pipeline:

```text
Game / app
  -> SceneSnapshot -> ResolvedResources -> RenderInput -> RenderCommandList
  -> shared backend-neutral FRAME PACKET            (NEW: host::FramePacket)
  -> WebGPU/WebGL2 backend  OR  Canvas2D backend     (uniform consumer)
  -> FrameSubmissionReport                           (NEW: uniform report)
```

---

## Current render path audit

### The runtime browser path, concretely

| Step | File / function | Shape produced |
|---|---|---|
| App entry (wasm) | `modules/axiom/src/app.rs` → `App::run()` (`#[cfg(target_arch="wasm32")]`) | builds `RunningApp`, calls `windowing.run_web_multi(surface_id, meshes, materials, max_instances, frame_fn)` |
| Per-frame closure | `app.rs` `App::run` closure | returns `([f32;4] clear, Vec<(u32,[f32;3],[f32;3],f32)> lights, [f32;16] light_vp, Vec<(u64,u64,Vec<f32>,u32)> batches)` |
| Frame compute | `app.rs` → `RunningApp::tick(tick) -> FrameOutcome` | builds a `RenderFrame`, calls `pipeline.submit(&frame,&scene,&webgpu) -> RenderReport`, extracts per-draw tuples into `DrawData` |
| Pipeline compose | `modules/axiom-render-pipeline/src/render_pipeline_api.rs` → `RenderPipelineApi::submit()` | builds `RenderInput`; calls `render.build_command_list(&input) -> RenderCommandList`; **walks the list** to (a) record a `GpuSubmission` (proof only) and (b) extract `Vec<(Mat4 world,[f32;4] color,u64 mesh_id,u64 material_id)>` + lights + light_vp into `RenderReport` |
| Command model | `modules/axiom-render/src/render_command.rs` | tagged `RenderCommand` (KIND_CLEAR_FRAME=1, SET_CAMERA=2, SET_PIPELINE=3, SET_MESH=4, SET_MATERIAL=5, DRAW_INDEXED=6); `DrawIndexed` payload = `(u32 index_count, Mat4 world)` |
| Batch packing | `modules/axiom/src/frame_outcome.rs` → `FrameOutcome::mesh_batches()` | groups `DrawData` by `(mesh_id,material_id)`, packs 36 floats/instance (mvp16+world16+color4) → `Vec<(u64,u64,Vec<f32>,u32)>` |
| Windowing | `modules/axiom-windowing/src/windowing_api.rs` → `run_web_multi()` | calls `GpuBackendApi::initialize(canvas,&meshes,&materials,max)` then per frame `present_frame(clear,&lights,light_vp,&batches)` |
| GPU live | `modules/axiom-gpu-backend/src/gpu_backend_api.rs::present_frame` → `live_gpu_binding.rs::render_frame` → `scene_renderer.rs::record` | unpacks batches into `wgpu` instance buffer + `draw_indexed` |

### The exact translation point — and why there is no clean one

`RenderCommandList` is **built and then immediately discarded** inside
`RenderPipelineApi::submit`. Two things are derived from walking it:

1. a `GpuSubmission` (the `axiom-webgpu` *recording* path) used **only** for
   deterministic proof/tests — it does **not** present;
2. a `RenderReport` of loose tuples `(Mat4, [f32;4], u64, u64)`.

The **live GPU input** (instance batches) is then assembled *outside* the render
layer — in `axiom` (`RunningApp::tick` → `FrameOutcome::mesh_batches`) — from
those tuples. So today's "translation" is smeared across `render-pipeline`
(list → tuples) and the `axiom` umbrella (tuples → `DrawData` → batches), and the
live backend consumes neither `RenderCommandList` nor any named contract — just
anonymous `Vec<(u64,u64,Vec<f32>,u32)>`.

**Smallest refactor to create one clean point:** introduce a single named,
backend-neutral **`FramePacket`** produced *by `axiom-render` from its own
`RenderCommandList`*, and make **both** backends consume that one type. This
deletes the bespoke tuple extraction and the `mesh_batches` packing (the GPU
backend does its own instance-batching from the packet). See next section.

---

## Backend-neutral frame contract

**Placement decision (committed, single choice): the frame packet lives in the
`host` layer** (`crates/axiom-host`). Rationale, not options:

- Backends are **modules** (`gpu-backend`, the new `canvas2d-backend`) and the
  Module Law forbids a module depending on another module — so the packet **may
  not** live in `axiom-render`. It must live in a **layer** both backend modules
  can depend on.
- `gpu-backend`'s only layer dep is already `host`. `host`'s `lib.rs` already
  declares a section for *"presentation-boundary data types future browser/WASM
  adapters … must be able to name."* A neutral frame-presentation payload is
  exactly that. `frame`/`math` are not presentation contracts; `host` is.
- The packet uses **primitive types only** (`[f32;16]`, `[f32;4]`, `[f32;3]`,
  `u32`, `u64`, `bool`) — no `Mat4`, so `host` needs no new `math` dep, and the
  contract is maximally neutral.

### New `host` types (all fields private, public accessors + public constructor, matching existing host style)

`crates/axiom-host/src/frame_packet.rs`:

```text
FrameViewport   { width: u32, height: u32 }
FrameCamera     { view: [f32;16], projection: [f32;16], view_proj: [f32;16] }   // column-major
FrameLight      { kind: u32, vec: [f32;3], color: [f32;3], intensity: f32 }     // kind 0=dir,1=point
FrameDrawItem   { object_id: u64, mesh_id: u64, material_id: u64,
                  world: [f32;16], mvp: [f32;16], color: [f32;4] }
FrameFeatureSet { uses_textures: bool, uses_shadows: bool,
                  directional_lights: u32, point_lights: u32 }                  // fallback metadata
FramePacket     { frame_index: u64, tick: u64,
                  viewport: FrameViewport,
                  clear_color: [f32;4],
                  camera: Option<FrameCamera>,
                  draws: Vec<FrameDrawItem>,        // deterministic RenderCommandList order
                  lights: Vec<FrameLight>,
                  light_view_proj: [f32;16],
                  features: FrameFeatureSet }
```

Rules the contract enforces:

- **Must NOT contain** `wgpu`, `web_sys`, `js_sys`, `wasm_bindgen`, WebGPU/WebGL,
  DOM, or Canvas types (host hygiene + the curated-export test already guard this).
- `draws` are **only the visible objects**, in command-list order (invisible
  objects emit no `DrawIndexed`, so absence encodes "mesh visibility").
- `object_id` = stable render/entity id, carried so Canvas can preserve object
  identity and hit-test. This requires a small `axiom-render` change (below).

### Derivation (in `axiom-render`, from `RenderCommandList`)

`modules/axiom-render/src/render_api.rs`:

```text
RenderApi::build_frame_packet(&self, input: &RenderInput, tick: u64,
                              light_view_proj: [f32;16]) -> host::FramePacket
```

Implementation walks `self.build_command_list(input)` once:
`ClearFrame` → `clear_color`; `SetCamera` → `FrameCamera` (`view_proj = projection*view`);
threads current mesh/material; each `DrawIndexed` → one `FrameDrawItem`
(`mvp = view_proj * world`, color resolved from the current `RenderMaterial`).
`features` computed from `input`. The packet is *literally* derived from the list.

Required small `axiom-render` changes:
- `RenderObject` gains `id: u64`; `RenderApi::add_input_object(...)` takes it.
- `DrawIndexed` payload gains `object_id: u64` so the list fully describes the
  packet (derivation is provably from the list, not from side state).
- `axiom-render/module.toml`: add `host` to `allowed_layers`; `Cargo.toml`: add
  `axiom-host` dep. Genuine use = `build_frame_packet` returns `host::FramePacket`.
- `axiom-render-pipeline/module.toml`: add `host` to `allowed_layers`;
  `submit()` returns `host::FramePacket` instead of the tuple `RenderReport`.

---

## Canvas 2D module placement

New **engine module** `modules/axiom-canvas2d-backend/`:

```text
modules/axiom-canvas2d-backend/
  module.toml
  Cargo.toml
  src/
    lib.rs                 # single facade: Canvas2dBackendApi (+ no other pub item)
    canvas2d_backend_api.rs# facade impl; native-clean + wasm delegation
    projection.rs          # pure: clip/NDC/screen transforms
    viewport.rs            # pure: NDC->pixel (y-flip)
    triangle.rs            # pure: indexed mesh -> screen triangles
    bounds.rs              # pure: screen AABB
    depth_sort.rs          # pure: painter's-order key + stable sort
    color_resolve.rs       # pure: fallback colour resolution
    wireframe.rs           # pure: triangle -> outline ops
    emit.rs                # pure: FramePacket -> (Vec<Canvas2dOp>, FrameSubmissionReport)
    hit_test.rs            # pure: (x,y) -> Option<object_id>
    canvas_op.rs           # pure neutral IR: Canvas2dOp enum
    live_canvas_binding.rs # #[cfg(target_arch="wasm32")] ONLY: ctx interpreter
  tests/
    architecture.rs        # facade-is-one, no browser API in pure core
```

`module.toml`:

```toml
[module]
name = "canvas2d-backend"
crate_name = "axiom-canvas2d-backend"
kind = "engine-module"
allowed_layers = ["host"]       # names host::FramePacket / FrameSubmissionReport
allowed_modules = []            # isolated: NO scene/resources/render/game imports
introduced_capabilities = ["canvas2d-fallback-presentation"]
```

`Cargo.toml`: native deps = `axiom-host` only. wasm32 target table:
`wasm-bindgen`, `web-sys = { features = ["HtmlCanvasElement","CanvasRenderingContext2d","Window","Document"] }`.
No `wgpu`. Pure core compiles on native (no platform deps).

### Architecture-checker / hygiene change (exact)

`crates/xtask/src/hygiene.rs`:

```rust
const PLATFORM_FACING_MODULES: &[&str] = &["windowing", "gpu-backend", "canvas2d-backend"];
```

(One-line, deliberate amendment — Module Law #9.) Nothing else: the pure core
must contain **zero** browser-API needles; only `live_canvas_binding.rs` (wasm32)
references `web_sys`/`canvas`. `tests/architecture.rs` asserts the needles appear
only in the wasm-gated file.

---

## Canvas fallback fidelity policy

**v1 preserves (must):** object identity (`object_id`), transforms (`world`/`mvp`),
camera framing (`camera`), draw-ordering rules (packet order + painter's depth
sort), clear color, mesh visibility (absence), playable silhouettes (filled
projected triangles), approximate depth ordering (per-object/triangle key),
hit-test compatibility (`hit_test`), deterministic frame/tick reporting,
structured degradation reporting.

**v1 degrades / drops (allowed):** exact pixels, PBR/material parity,
**shadow-map (no shadows in v1)**, exact texture filtering (textured materials →
flat fallback colour from material base colour; **no perspective-correct texture
sampling**), GPU performance, postprocessing, exact lighting parity.

**v1 renderer behaviour, in order:** clear canvas to `clear_color` → for each
`FrameDrawItem` in order: look up mesh (init table), project every vertex by
`mvp`, build screen triangles → compute a painter's depth key → resolve fallback
colour → emit filled triangles (optional wireframe overlay via build flag) →
stable-sort all triangles back-to-front (packet order breaks ties) → emit ops.
Unknown mesh/material → skip + count. Textured material → flat colour + count.

### `FrameSubmissionReport` (uniform across all backends, in `host`)

`crates/axiom-host/src/frame_submission_report.rs`:

```text
BackendKind  = WebGpu | WebGl2 | Canvas2d
FrameFeature = Textures | Shadows | PointLightAttenuation
             | PerspectiveCorrectTexture | MultiLight | Postprocess
FrameSubmissionReport {
  backend: BackendKind,
  frame_index: u64,
  tick: u64,
  submitted_draws: u32,
  skipped_draws: u32,            // unknown mesh/material
  degraded_materials: u32,       // textured -> flat colour
  unsupported_features: u32,     // = degraded_features.len()
  degraded_features: Vec<FrameFeature>,
}
```

GPU backends return this with empty degradation; Canvas fills it. This is the
single observable result type the app/windowing/telemetry read, and the anchor of
the cross-backend comparison test.

---

## Pure software renderer core

All native-testable, no browser APIs. Each piece + its **behavioural** tests
(no "does not panic" tests):

- **viewport transform** (`viewport.rs`): `ndc_to_screen(ndc:[f32;3], vp) -> [f32;2]`
  with y-flip. Tests: ndc(-1,-1)→(0,h); ndc(1,1)→(w,0); ndc(0,0)→(w/2,h/2);
  non-square viewport maps x and y independently (hand-computed pixels).
- **clip/NDC projection** (`projection.rs`): `project(mvp,pos)->[f32;4]`;
  `perspective_divide([f32;4])->Option<[f32;3]>` (None when `w<=eps`, i.e. behind
  near plane). Tests: identity mvp maps unit cube corners to themselves; a vertex
  with w≤0 returns None (culled); a known perspective matrix maps a known point to
  a hand-computed ndc.
- **screen-space triangle conversion** (`triangle.rs`):
  `screen_tris(positions,indices,mvp,vp,object_id,color)->Vec<ScreenTri>`.
  Tests: a 4-vertex/2-triangle quad yields exactly 2 tris with expected vertex
  order; a triangle fully behind the near plane yields 0 tris; winding order
  preserved.
- **bounding-box** (`bounds.rs`): `aabb2(pts)->Aabb2`. Tests: exact min/max of a
  known triangle; a tri partially offscreen still yields true bounds (used to cull
  fully-offscreen tris) — assert a fully-offscreen tri is flagged out-of-viewport.
- **depth sort key** (`depth_sort.rs`): `key(tri)->f32` (mean clip-space depth);
  `painter_order(keys)->Vec<usize>` stable back-to-front. Tests: three tris with
  z = {0.1,0.9,0.5} order to indices [1,2,0] (far→near); two equal-z tris keep
  input order (determinism); reversing input with equal keys is NOT reordered.
- **colour fallback resolution** (`color_resolve.rs`):
  `resolve(material_base:[f32;4], draw_color:[f32;4], textured:bool)->([f32;4],bool)`
  returning (colour, degraded). Tests: untextured → base×draw, degraded=false;
  textured → base×draw flat, degraded=true; unknown material id → debug magenta
  `[1,0,1,1]`, degraded=true; channels clamp to [0,1].
- **optional wireframe** (`wireframe.rs`): `outline(tri)->[Canvas2dOp;1]` (closed
  3-point polyline). Tests: produces a polyline with 4 points (closed), colour =
  configured wire colour.
- **deterministic draw command emission** (`emit.rs`):
  `emit(packet, resources, opts)->(Vec<Canvas2dOp>, FrameSubmissionReport)`.
  Tests: a 2-object packet emits `[Clear, FillTriangle…]` in a **golden** exact
  vector; replaying the same packet yields a byte-equal op vector and equal
  report; an object whose `mesh_id` is absent increments `skipped_draws` and emits
  no fill for it; a textured material increments `degraded_materials` and adds
  `FrameFeature::Textures`; a packet with a directional light adds
  `FrameFeature::Shadows` (shadows dropped in v1); report `frame_index`/`tick`
  echo the packet.
- **hit-test** (`hit_test.rs`): `hit_test(frame_geometry, x, y)->Option<u64>`
  topmost (nearest) object whose triangle contains the point. Tests: point inside
  a single front object returns its id; with two overlapping objects the nearer
  `object_id` wins; a point in empty space returns None; a point exactly on a
  shared edge resolves deterministically (documented tie rule).

`Canvas2dOp` (`canvas_op.rs`): `Clear{rgba:[f32;4]}`, `FillTriangle{pts:[[f32;2];3], rgba:[f32;4]}`,
`Polyline{pts:Vec<[f32;2]>, rgba:[f32;4], width:f32}`. Pure data; no browser types.

---

## WASM Canvas binding

`modules/axiom-canvas2d-backend/src/live_canvas_binding.rs`
(`#[cfg(target_arch="wasm32")]` only) — **as thin as possible**, owns:

- `HtmlCanvasElement` + `CanvasRenderingContext2d` (`canvas.get_context("2d")`),
- canvas width/height,
- the init resource tables (`meshes`, materials' fallback colours),
- `present(packet) -> FrameSubmissionReport`: calls **pure** `emit::emit(...)`,
  then *interprets* the returned `Vec<Canvas2dOp>` against the 2D context
  (`Clear`→`fill_rect`; `FillTriangle`→`begin_path`/`move_to`/`line_to`/
  `close_path`/`fill`; `Polyline`→`stroke`). **No projection/sort/colour logic
  here** — it is a dumb op interpreter so 100% of rendering logic stays native-
  testable.

`Canvas2dBackendApi` facade (`canvas2d_backend_api.rs`): native build = clean
facade that still runs `emit::emit` (returning the report, drawing nothing) so
native tests exercise the full renderer; wasm build delegates to
`live_canvas_binding`. Methods mirror `GpuBackendApi`:
`initialize(canvas, meshes, materials, max_instances)` and
`present_packet(&FramePacket) -> FrameSubmissionReport`.

---

## Backend selection

Runtime order: **WebGPU → WebGL2 → Canvas2D → unsupported**.

Lives in **`axiom-windowing`** (the feature module that already owns canvas lookup
+ run loop). Add `canvas2d-backend` to `windowing/module.toml` `allowed_modules`.

Selection, in the wasm32 arm (gates-exempt), **without poisoning the canvas**:

1. **Probe GPU without touching the real canvas.** Extend `gpu-backend` with
   `GpuBackendApi::probe_backend() -> Option<BackendKind>`:
   - WebGPU: `navigator.gpu` adapter **and** device (no canvas) — the lesson
     already in `live_gpu_binding::initialize`.
   - WebGL2: build a **throwaway** `document.createElement("canvas")`, attempt a
     `Backends::GL` adapter+device on it, discard it. (This *moves* today's GL
     probe off the real canvas — today it creates the GL surface on the real
     canvas, which would poison a later 2D fallback. This refactor is mandatory.)
2. `windowing` selects: `probe_backend()` is `Some` → `GpuBackendApi::initialize`
   on the **real** canvas (it re-acquires the confirmed backend); `None` →
   `Canvas2dBackendApi::initialize` on the pristine real canvas; if even
   `getContext("2d")` is null → surface "unsupported".
3. Uniform call shape: a wasm-arm enum

   ```rust
   enum SelectedBackend { Gpu(GpuBackendApi), Canvas(Canvas2dBackendApi) }
   ```

   with `present_packet(&FramePacket) -> FrameSubmissionReport` dispatched by
   `match`. The run loop builds a `FramePacket` from `frame_fn` and hands it to
   the selected backend uniformly.
4. **Test override:** `windowing` accepts an optional `BackendPreference`
   (`Auto | ForceWebgl2 | ForceCanvas2d`) so Playwright can force Canvas2D even
   where GPU works. Plumbed from the app (`?backend=` query param).

`run_web_multi`'s `frame_fn` return type changes from the 4-tuple to
`host::FramePacket`; `windowing` no longer knows about instance batches (the GPU
backend derives those from the packet internally).

---

## App/browser JS gate changes

Each app's gate must accept **WebGPU OR WebGL2 OR Canvas2D**, only showing
"unsupported" when all three are absent (Canvas2D is effectively always present,
so the gate ceases to block normal browsers). Extend the existing
`hasRenderBackend` (added for the WebGL2 work) with a `getContext("2d")` arm, and
optionally read a `?backend=` override to pass into the app.

Entrypoints to update (all `web/index.html`):
- `apps/axiom-demo-rotating-cube-browser/web/index.html`
- `apps/axiom-stress-cubes-browser/web/index.html`
- `apps/axiom-retro-fps-browser/web/index.html`
- `apps/axiom-netplay-browser/web/index.html`
- `apps/axiom-growth/web/index.html`
- `gallery/index.html` (copy text only)

---

## Tests and validation

The test contract is **structural equality + playability**, never pixel equality.

- **Architecture / hygiene** — `cargo xtask check-architecture`: new module
  classifies; `canvas2d-backend` in `PLATFORM_FACING_MODULES`; pure core has no
  browser needles; `host` curated-export test updated for the new types;
  `real_repo_layers_pass` / `real_repo_class_aware_check_passes` green.
- **Native unit tests for the pure core** — every piece above, behavioural.
- **Golden structural test: `RenderInput` → `FramePacket`** — in
  `axiom-render/tests`: a fixed `RenderInput` yields an exact `FramePacket`
  (draw order, `object_id`s, `mesh_id`/`material_id`, matrices); and a test
  asserting `packet.draws.len()` == number of `DrawIndexed` commands in
  `build_command_list`, with 1:1 id correspondence (proves "derived from
  `RenderCommandList`").
- **Canvas degradation report tests** — `emit` populates `skipped_draws`,
  `degraded_materials`, `degraded_features`, `backend = Canvas2d` correctly for
  crafted packets.
- **Cross-backend comparison test** — one `RenderInput` → `build_frame_packet` →
  feed (a) `gpu-backend`'s packet→instance-batch adapter and (b) `canvas2d`'s
  `emit`; assert both reference the **same draw set**: equal `object_id` multiset,
  equal `(mesh_id,material_id)` per draw, equal count, equal order. Structural,
  not pixels.
- **wasm build smoke** — `cargo check -p axiom-canvas2d-backend --target wasm32-unknown-unknown`
  and `cargo build -p axiom-demo-rotating-cube-browser --target wasm32-unknown-unknown`.
- **Playwright screenshot smoke** — serve the demo, force Canvas2D via
  `?backend=canvas2d`, screenshot; assert canvas is non-blank and the cube/sphere
  silhouettes are present (bounding-box/colour heuristic), console logs
  `render backend = Canvas2d`.

Coverage: the pure core is engine-spine → **100%** native coverage required; the
`live_canvas_binding` wasm arm is coverage-exempt (cfg-gated), exactly like
`gpu-backend`'s live arm.

---

## Implementation milestones

### Milestone 1 — shared backend-neutral frame packet
- **Create:** `crates/axiom-host/src/frame_packet.rs` (`FramePacket`,
  `FrameDrawItem`, `FrameCamera`, `FrameLight`, `FrameViewport`,
  `FrameFeatureSet`); export from `host/src/lib.rs`.
- **Change:** `axiom-render` — `RenderObject.id`, `DrawIndexed.object_id`,
  `RenderApi::add_input_object`, `RenderApi::build_frame_packet`; add `host` to
  `render/module.toml` + `Cargo.toml`. `axiom-render-pipeline::submit` returns
  `FramePacket` (+ `host` dep). `axiom` (`RunningApp::tick`/`FrameOutcome`) and
  `gpu-backend` (`present_packet` deriving batches internally) + `windowing`
  (`frame_fn` returns `FramePacket`) threaded through so the **existing GPU path
  consumes the packet** (delete `mesh_batches`/tuple extraction).
- **Tests:** host packet construction/accessors; `RenderInput→FramePacket` golden
  + 1:1-with-`DrawIndexed`; gpu `packet→batches` golden equals old `mesh_batches`
  output; `host` curated-export test updated.
- **Validation:** `scripts/coverage.ps1`; `cargo xtask check-architecture`;
  `cargo build -p axiom-demo-rotating-cube-browser --target wasm32-unknown-unknown`.
- **Acceptance:** GPU path renders identically (Playwright screenshot unchanged);
  one named `host::FramePacket` is the sole GPU input; 100% coverage; arch green.

### Milestone 2 — Canvas2D module skeleton + allow-list
- **Create:** `modules/axiom-canvas2d-backend/` with `module.toml`, `Cargo.toml`,
  `lib.rs` (facade `Canvas2dBackendApi` only), stub `canvas2d_backend_api.rs`,
  `canvas_op.rs`, empty `tests/architecture.rs`.
- **Change:** `crates/xtask/src/hygiene.rs` `PLATFORM_FACING_MODULES` += `"canvas2d-backend"`.
- **Tests:** facade-is-one; no browser needle in pure files; module classifies.
- **Validation:** `cargo xtask check-architecture`; `cargo test -p xtask`;
  `cargo check -p axiom-canvas2d-backend`.
- **Acceptance:** module builds native + wasm; arch/hygiene green; facade exposes
  exactly `Canvas2dBackendApi`.

### Milestone 3 — pure projection/silhouette renderer core
- **Create:** `projection.rs`, `viewport.rs`, `triangle.rs`, `bounds.rs`,
  `depth_sort.rs`, `color_resolve.rs`, `wireframe.rs`, `emit.rs`, `hit_test.rs`.
- **Purpose:** `FramePacket` (+ init resources) → `Vec<Canvas2dOp>` +
  `FrameSubmissionReport`, fully native.
- **Tests:** the full behavioural suite in "Pure software renderer core" above.
- **Validation:** `scripts/coverage.ps1` (100% on the module); `cargo test -p axiom-canvas2d-backend`.
- **Acceptance:** golden op vectors deterministic; report counts correct; 100%
  coverage; zero browser APIs in these files.

### Milestone 4 — wasm32 Canvas 2D binding
- **Create:** `live_canvas_binding.rs` (`cfg(wasm32)`); wire
  `Canvas2dBackendApi::{initialize,present_packet}` to it.
- **Purpose:** acquire `CanvasRenderingContext2d`; interpret `Canvas2dOp`s; present.
- **Tests:** native facade test (runs `emit`, returns report, draws nothing);
  wasm build check. (Binding itself is coverage-exempt.)
- **Validation:** `cargo check -p axiom-canvas2d-backend --target wasm32-unknown-unknown`.
- **Acceptance:** wasm compiles; binding contains only op-interpretation, no
  render math.

### Milestone 5 — backend selection integration
- **Change:** `gpu-backend` — `probe_backend()`; move GL probe to a throwaway
  canvas (no poisoning). `windowing` — `canvas2d-backend` in `allowed_modules`;
  `SelectedBackend` enum; `BackendPreference`; `frame_fn` returns `FramePacket`;
  selection order WebGPU→WebGL2→Canvas2D→unsupported.
- **Tests:** native selection-logic tests for the preference/decision function
  (pure, gates-exempt parts kept thin); arch green with new allowed_modules.
- **Validation:** `cargo xtask check-architecture`;
  `cargo build -p axiom-demo-rotating-cube-browser --target wasm32-unknown-unknown`.
- **Acceptance:** with GPU available → `WebGpu`/`WebGl2`; GPU forced off →
  `Canvas2d` on a non-poisoned canvas; real canvas never acquired before commit.

### Milestone 6 — app JS gates + browser smoke tests
- **Change:** the 6 entrypoints listed; add Canvas2D arm + `?backend=` override.
- **Tests:** Playwright: default path unchanged; `?backend=canvas2d` renders
  recognizable silhouettes; console logs `render backend = Canvas2d`.
- **Validation:** `make demo-build`; `uv run scripts/playwright_controller.py …`.
- **Acceptance:** no browser is rejected when only Canvas2D exists; forced-canvas
  screenshot shows the scene's silhouettes.

### Milestone 7 — degradation reporting + documentation
- **Create:** `crates/axiom-host/src/frame_submission_report.rs`
  (`FrameSubmissionReport`, `BackendKind`, `FrameFeature`); export from host.
- **Change:** both backends return `FrameSubmissionReport`; surface it through
  windowing/app for telemetry. Write `modules/axiom-canvas2d-backend/ARCHITECTURE.md`
  and update `docs/render-fallback.md`.
- **Tests:** report-field tests per backend; comparison test asserting equal draw
  set across GPU and Canvas reports.
- **Validation:** `scripts/coverage.ps1`; `cargo xtask check-architecture`.
- **Acceptance:** uniform report observable; degraded features enumerated; docs
  describe the policy and the v1 degradation list.

### Milestone 8 — optional later quality upgrades (NOT v1)
- Affine/perspective-correct textured triangles; simple Lambert shading from
  `lights`; cheap planar/contact shadow approximation; per-triangle clipping
  against the near plane; SIMD raster. Each behind its own milestone + feature
  flag, never lowering the gate.

---

## Non-goals (must NOT be built in v1)

- Full CPU shadow maps / shadow parity.
- Full perspective-correct textured rasterizer (textures → flat fallback colour).
- PBR / real material parity.
- Postprocessing.
- Skeletal-animation special cases.
- **Any** scene/resources/render/game imports inside the Canvas backend
  (`allowed_modules = []`).
- Game-specific fallback hacks.
- DOM/browser/`web_sys`/`wgpu` types in the neutral contracts (`FramePacket`,
  `FrameSubmissionReport`) or in the Canvas pure core.

---

## Final recommended first PR

**PR #1 = Milestone 1: the shared backend-neutral frame packet, with the existing
GPU path migrated onto it.**

Scope:
1. Add `host::FramePacket` (+ `FrameDrawItem`, `FrameCamera`, `FrameLight`,
   `FrameViewport`, `FrameFeatureSet`) — primitive-only, browser-free.
2. `axiom-render`: `RenderObject.id`, `DrawIndexed.object_id`,
   `RenderApi::build_frame_packet(&RenderInput, tick, light_vp) -> FramePacket`
   (derived by walking `build_command_list`); add `host` dep.
3. Migrate the **existing** GPU live path to consume `FramePacket`:
   `render-pipeline::submit` → `FramePacket`; `gpu-backend` gains
   `present_packet` that derives instance batches internally; delete
   `FrameOutcome::mesh_batches` and the tuple `RenderReport`; `windowing`
   `frame_fn` returns `FramePacket`.

Proof tests in the PR:
- `RenderInput → FramePacket` golden, and `draws` ↔ `DrawIndexed` 1:1
  (proves derivation from `RenderCommandList`).
- `gpu-backend` `packet → batches` equals the previous `mesh_batches` output
  (proves the existing GPU path consumes the packet with no behaviour change).
- A consumer-shaped unit test standing in for Canvas (packet → a trivial
  draw-set summary) proving the **planned** Canvas path can consume the identical
  artifact.

Acceptance: Playwright screenshot of the rotating-cube demo is unchanged
(WebGPU/WebGL2), 100% coverage holds, `cargo xtask check-architecture` green. This
lands the spine that Milestones 2–7 build the Canvas backend on, with zero net
behaviour change to shipping backends.
