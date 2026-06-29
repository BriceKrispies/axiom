# SPEC-04 — 2D surface (shapes / text / sprites / gradients / particles)

> Status: Landed (with deferrals — see below)
> Landed (2026-06-28): `axiom-draw2d::Draw2dApi` builds the full neutral draw-list — `rect`/`circle`/`ellipse`/`line`/`path`/`sprite`/`text`+`measure_text`/`linear`+`radial` gradients + the camera/transform stack + the `(layer, submission)` stable sort — and `Draw2dList`/`Draw2dCommand` are **host-owned** (`crates/axiom-host`, the 2D peer of `FramePacket`). The software backend (`axiom-canvas2d-backend::draw2d_raster`) rasterizes **filled rect + sprite with src-over alpha compositing**.
> Deferred (documented in `draw2d_raster.rs`): backend rasterization of circle/ellipse/line/path/gradient(paint) fills/stroke/text glyph-runs (their `KIND_*` commands are recognised and skipped, never mis-rasterized), and exact rotated rasterization. **Particles (§10.1) and render-targets (§10.3) are deferred** at the facade pending a kernel `Seconds` dimensioned scalar. Live browser raster is browser-proven (sandbox cannot run browser WebGPU).
> Contract: §10 (incl. §10.1 particles, §10.2 flip-book, §10.3 render targets)   Vocabulary: Text/glyph/emoji, 2D shapes, gradients, alpha blending, layer/z-order, glow/shadow, particle system, 2D transform stack, sprite/image draw, off-screen baked layer, DPR/responsive   Determinism: presentation

## 1. Summary

This is the single largest missing surface in the engine. **9 of 11 games**
need a 2D drawing surface — to paint shapes, text/score HUDs, sprites,
backgrounds, gradients, glow, and particle bursts — and **today none of it
exists**. The contract's `Frame` 2D interface (§10) is entirely unbacked: there
is no author-facing 2D draw at all, no text rendering anywhere in the tree, no
2D shape/gradient/particle/transform primitives, no per-draw layer/z ordering,
and no alpha blending (the GPU backend is hardcoded `REPLACE`).

The two existing "2D backends" (`axiom-canvas2d-backend`, `axiom-gpu-backend`)
are **3D-scene → 2D-framebuffer rasterizers** — they consume a 3D `FramePacket`
(camera matrices + meshes), not a 2D draw-list. The author has no way to say
"draw a red circle at (x,y) on layer 3."

This spec closes that gap with a new engine module that owns a **neutral,
ordered 2D draw-list as data** — the 2D peer of `axiom-render`'s
`RenderCommandList` — plus extensions to the two backend arms that rasterize it.
It is **presentation** class throughout: immediate-mode, called from `onRender`,
never authoritative, and (critically) **its particle simulation runs on the
presentation clock and never re-enters sim** (§17.5).

## 2. Current state (verified)

- **No author-facing 2D surface exists.** Nothing in `crates/` or `modules/`
  exposes `rect`/`circle`/`text`/`sprite`/`camera2D`. The contract `Frame` (§10)
  has zero backing.
- **The "2D" backends rasterize 3D scenes, not 2D draw-lists.** Both
  `axiom-canvas2d-backend` and `axiom-gpu-backend` consume
  `axiom_host::FramePacket` — a camera-matrix + mesh + world-transform packet
  (`crates/axiom-host/src/frame_packet.rs`) derived from a
  `RenderCommandList`. `axiom-canvas2d-backend` is a software triangle
  rasterizer (`software_rasterizer.rs`, `raster_triangle.rs`, depth buffer,
  planar shadows); `axiom-gpu-backend` is a `wgpu` scene renderer
  (`scene_renderer.rs`). Neither has a 2D primitive.
- **No text rendering anywhere.** No glyph atlas, no font handling, no
  `measureText`. The vocabulary's `n=11` text gap is real and total.
- **No 2D shapes / gradients / particles / transform stack.** None of these
  primitives exist in any form.
- **Blend is hardcoded `REPLACE`** in the GPU backend
  (`scene_renderer.rs:643`, `upscale.rs:134` both `wgpu::BlendState::REPLACE`).
  There is **no alpha blending** — opaque overwrite only. The 2D surface's
  `alpha`/`shadow`/gradients are unimplementable without a real blend path.
- **Draw order = submit order.** Both backends present commands in list order
  with depth sorting on the *3D* z, not a 2D `layer`. There is **no layer/z
  reorder** for 2D draws — confirming the contract's `layer` field must be
  carried explicitly and the draw-list must be sorted by it before raster.
- **Sprite = textured `Plane` mesh only.** The only texture path is
  `RenderCommand::set_material(material_id, material_texture_id)` sampling a
  whole texture; there is **no atlas sub-rect / UV source-rect / anchor / tint /
  flip / pose-swap**. `SpriteOpts.source` (§10) has no backing.
- **Render-to-texture exists only internally.** `axiom-gpu-backend` has
  `render_offscreen_rgba` (`gpu_backend_api.rs:181`, behind the non-wasm
  `offscreen` feature, → `offscreen::render_to_rgba`) used for headless
  screenshots. It is **not exposed to authors** and has no 2D `drawTo` form.
- **Responsive resize exists; DPR does not.** `axiom-windowing` resizes to the
  physical surface size (`FrameViewport`), but nothing reads
  `devicePixelRatio` — text and shapes would be sub-pixel-blurry on HiDPI.

## 3. Placement

**New engine module `axiom-draw2d`** (`modules/axiom-draw2d/`, `module.toml`,
`kind = "engine-module"`, `allowed_modules = []`), exposing **one** facade
`Draw2dApi`. Two existing modules are **extended**: `axiom-canvas2d-backend` and
`axiom-gpu-backend`.

```text
Draw2dApi (axiom-draw2d)  ──builds──▶  Draw2dList  (neutral, ordered, DATA)
                                            │
       app/runtime (SPEC-00) translates the list into a backend submission
                                            │
              ┌─────────────────────────────┴─────────────────────────────┐
   axiom-canvas2d-backend (software)                       axiom-gpu-backend (wgpu)
        rasterizes Draw2dList                                  rasterizes Draw2dList
```

**Why a new module, under the Module Law.** This is an *isolated capability*:
"compile author 2D-draw calls into a neutral, ordered, hashable 2D command
stream." It is the exact 2D peer of `axiom-render`, which owns `RenderInput` →
`RenderCommandList` for 3D. `axiom-render` does **not** grow a 2D arm — its
contract is camera-matrix/mesh/material 3D; a 2D shape/text/sprite/gradient
draw-list is a different data shape with a different consumer, and folding it in
would make `axiom-render` a two-headed module. So `axiom-draw2d` is its own
isolated engine module: `allowed_modules = []`, it imports no other module, and
it produces only data.

**Why the rasterization lives in the backends, not here.** Turning a draw-list
into pixels is a *platform* concern — software raster on canvas, `wgpu` on GPU —
exactly the role `axiom-canvas2d-backend` and `axiom-gpu-backend` already own
for 3D `FramePacket`s. They are the two sanctioned platform-facing modules
(Module Law #9). `axiom-draw2d` itself is a **pure, native-testable,
fully-covered, branchless core** with no platform code: it builds the list and
sorts it by layer; it rasterizes nothing.

**Why the app does the wiring.** `axiom-draw2d` and the backend modules never
import one another (Module Law: engine modules are isolated). The app/runtime
(SPEC-00, `apps/axiom-game-runtime`) reads `Draw2dApi`'s list out and submits it
to whichever backend is active — the single legal home for cross-module
translation, identical to how the app already feeds `FramePacket` to a backend.

`allowed_layers` for `axiom-draw2d`: `kernel` (ids, `Ratio`/`Radians`),
`math` (`Vec2`/`Mat3` for the 2D transform stack), and `host` only if a 2D draw
artifact must be a host-nameable boundary type the backends can name (mirrors
`axiom-render` depending on `host` for `FramePacket`). The neutral `Draw2dList`
is the cross-module contract; whether it is host-owned or draw2d-owned is the
one placement question to settle in §9.

**Particles** (§10.1) live **inside `axiom-draw2d`** for now (it owns the
draw-list they emit into and the presentation clock they advance on), behind the
same facade — see §9 for the sibling-module alternative.

## 4. API surface

### 4.1 Native Rust facade — `Draw2dApi` (presentation-class, branchless, 100% covered)

A single facade that accumulates a frame's draws into an ordered list, then
yields the neutral, layer-sorted contract. Shape mirrors `RenderApi`: typed
builders in, opaque `KIND_*`-tagged commands out, branchless `as_*` accessors
for the consumer.

```rust
impl Draw2dApi {
    pub fn new() -> Self;

    // Camera + transform stack (2D). Pure Mat3 composition; no branch.
    pub fn set_camera2d(&mut self, center: Vec2, zoom: Ratio);
    pub fn push_transform(&mut self, m: Mat3) -> TransformDepth;
    pub fn pop_transform(&mut self, depth: TransformDepth);

    // Shapes — each carries a resolved Common (layer, alpha, shadow) + FillStroke.
    pub fn rect(&mut self, r: Rect, style: Fill2d, common: Common2d);
    pub fn circle(&mut self, center: Vec2, radius: Meters, style: Fill2d, common: Common2d);
    pub fn ellipse(&mut self, center: Vec2, rx: Meters, ry: Meters, rotation: Radians,
                   style: Fill2d, common: Common2d);
    pub fn line(&mut self, a: Vec2, b: Vec2, color: Rgba, width: Meters, common: Common2d);
    pub fn path(&mut self, points: &[Vec2], style: Fill2d, common: Common2d, closed: bool);

    // Sprites — source sub-rect, anchor, tint, flip ride on the command.
    pub fn sprite(&mut self, texture: TextureId, opts: SpriteDraw2d, common: Common2d);

    // Text — value is a glyph-index run resolved against a FontHandle (see §9).
    pub fn text(&mut self, run: GlyphRun, opts: TextDraw2d, common: Common2d);
    pub fn measure_text(&self, run: &GlyphRun, font: FontHandle) -> TextMetrics;

    // Paints — registered once, referenced by handle from Fill2d::fill.
    pub fn linear_gradient(&mut self, from: Vec2, to: Vec2, stops: &[GradientStop]) -> PaintId;
    pub fn radial_gradient(&mut self, center: Vec2, radius: Meters, stops: &[GradientStop]) -> PaintId;

    // Particles (§10.1) — config registered; emit queues a burst; advance steps
    // the live particles on the PRESENTATION dt and appends their draws.
    pub fn create_emitter(&mut self, config: EmitterConfig) -> EmitterId;
    pub fn emit(&mut self, id: EmitterId, at: Vec2, direction: Vec2);
    pub fn advance_particles(&mut self, presentation_dt: Seconds);

    // Render targets (§10.3) — a named off-screen list reused as a texture.
    pub fn create_render_target(&mut self, width: u32, height: u32) -> RenderTargetId;
    pub fn begin_target(&mut self, target: RenderTargetId);   // subsequent draws route here
    pub fn end_target(&mut self);
    pub fn target_texture(&self, target: RenderTargetId) -> TextureId;

    // Finalize: stable-sort by (layer, submit-order) and expose the neutral list.
    pub fn finish(&mut self) -> Draw2dList;
}
```

`finish` performs the **explicit layer sort** the backends do not: a *stable*
sort by `(layer, submission_index)` so equal layers keep call order (contract
§10: "by `layer` … then call order"). This is the structural fix for the
verified "draw order = submit order, no reorder" gap — the ordering is resolved
in the neutral core, once, not left to each backend.

### 4.2 TS authoring projection (the contract, §10 verbatim)

Projected through SPEC-00's `Frame` interface, exactly the contract signatures:

```ts
interface Frame {
  camera2D(view: { center: Vec2; zoom: number }): void;

  rect(r: Rect, style: FillStroke & Common): void;
  circle(center: Vec2, radius: number, style: FillStroke & Common): void;
  ellipse(center: Vec2, rx: number, ry: number, rotation: number, style: FillStroke & Common): void;
  line(a: Vec2, b: Vec2, style: { color: Rgba; width: number } & Common): void;
  path(points: Vec2[], style: FillStroke & Common & { closed?: boolean }): void;

  sprite(texture: TextureId, opts: SpriteOpts): void;

  text(value: string, opts: TextOpts): void;
  measureText(value: string, font: FontSpec): { width: number; height: number };

  linearGradient(from: Vec2, to: Vec2, stops: GradientStop[]): Paint;
  radialGradient(center: Vec2, radius: number, stops: GradientStop[]): Paint;
}

interface Common  { layer?: number; alpha?: number; shadow?: { color: Rgba; blur: number } }
interface FillStroke { fill?: Rgba | Paint; stroke?: Rgba; strokeWidth?: number }
type GradientStop = { offset: number; color: Rgba };
type Paint = Handle;

interface SpriteOpts extends Common {
  pos: Vec2; rotation?: number; scale?: Vec2; anchor?: Vec2;
  tint?: Rgba; flipX?: boolean; flipY?: boolean; source?: Rect;
}
interface TextOpts extends Common { pos: Vec2; font: FontSpec; color: Rgba; align?: "left"|"center"|"right" }
type FontSpec = { family: string; size: number; weight?: number };

function loadTexture(url: string): TextureId;     // fetch in the app; handle stable for life
function loadFont(url: string): FontSpec;

// §10.1 particles
interface EmitterConfig {
  count: number; lifetime: [Seconds, Seconds]; speed: [number, number];
  spread: number; gravity?: Vec2; size: [number, number];
  colorStart: Rgba; colorEnd: Rgba; layer?: number;
}
function createEmitter(config: EmitterConfig): EmitterId;
function emit(id: EmitterId, at: Vec2, direction?: Vec2): void;

// §10.2 flip-book — PURE sampler, no state; lives in the core, trivially covered.
interface SpriteAnimation { frames: Rect[]; fps: number }
function sampleAnimation(anim: SpriteAnimation, elapsed: Seconds, loop?: boolean): Rect;

// §10.3 render targets
function createRenderTarget(width: number, height: number): RenderTargetId;
function drawTo(target: RenderTargetId, draw: (frame: Frame) => void): void;
function targetTexture(target: RenderTargetId): TextureId;
```

`loadTexture`/`loadFont` resolve their bytes **in the app** (fetch/`web_sys`),
then register a handle; `axiom-draw2d` only ever names the resolved handle —
no I/O crosses into the module (the same fetch-in-the-app rule as `axiom-assets`).

## 5. Data contracts

The neutral, ordered draw-list that crosses the module → app → backend boundary
— primitives only, no GPU/DOM/font/scene types, hashable for golden tests.

- **`Draw2dList`** — the frame's commands after the layer sort, plus the
  per-frame paint table, the resolved camera2D, and the render-target table.
  Indexed accessors + `KIND_*` codes (peer of `RenderCommandList`).
- **`Draw2dCommand`** — one tagged, branchless command (peer of
  `RenderCommand`): `KIND_RECT`, `KIND_CIRCLE`, `KIND_ELLIPSE`, `KIND_LINE`,
  `KIND_PATH`, `KIND_SPRITE`, `KIND_TEXT_GLYPHS`, `KIND_PARTICLE_QUAD`. Every
  command carries its **resolved** `layer: i32`, `alpha: Ratio`, optional
  shadow, transform (a baked `Mat3` from the stack), and fill/stroke (color or
  `PaintId`). Nothing un-resolved (no defaults, no `Option` flow) reaches a
  backend.
- **`Common2d`** — `{ layer, alpha, shadow }`, resolved (defaults applied at the
  facade): `layer` is the **explicit z-order** the backends must honor.
- **`Fill2d` / `SpriteDraw2d` / `TextDraw2d`** — the resolved per-draw style.
  `SpriteDraw2d` carries `source: Rect` (atlas/flip-book sub-rect), `anchor`,
  `tint`, `flip_x`, `flip_y` — the verified-missing sprite fields.
- **`PaintTable`** — registered linear/radial gradients keyed by `PaintId`; a
  command's fill references a paint by id, never inlines stops.
- **`GlyphRun` / `TextMetrics` / `FontHandle`** — text as a resolved run of glyph
  indices + advances against a baked font (see §9 for where the atlas lives).
  This keeps `axiom-draw2d` free of font rasterization: it traffics glyph
  indices + metrics, a backend (or a baked atlas asset) turns them into pixels.
- **`ParticleField`** — the live particle set advanced on the presentation
  clock; its per-frame output is appended as `KIND_PARTICLE_QUAD` commands. The
  field state is **presentation-only** and never serialized into sim.
- **`RenderTargetId` / target sub-lists** — a render target is a *named nested
  `Draw2dList`*; `target_texture` yields a `TextureId` the backend binds. Pure
  data; the backend owns the actual off-screen surface.

These are the new author-facing vocabulary nouns; per Module Law #8 they may be
re-exported from `lib.rs` as the facade's id/value vocabulary, with all behavior
behind `Draw2dApi`.

**Backend extensions (the alpha-blend root fix).** Both backends gain a
`Draw2dList` consumer alongside their `FramePacket` consumer. The GPU backend's
hardcoded `BlendState::REPLACE` is replaced by a **blend mode selected per
draw** from the command's resolved `alpha`/shadow — straight alpha for the
common case, additive for glow. This is the lowest correct layer for the fix:
"no alpha blending" is a backend-pipeline defect, so it is fixed in the backend,
not papered over above. The canvas backend composites with its 2D ops directly.

## 6. Determinism — presentation-excluded (§17.5)

**The entire surface is presentation class. None of its output may ever be read
back into a `sim`-class API** (contract §0.1, §17.5; README "presentation").
Concretely:

- `onRender` is the **only** caller. The 2D facade is never invoked from
  `onFixedUpdate`; nothing it produces is authoritative.
- **Particles are simulated on the PRESENTATION clock.** `advance_particles`
  takes the *real* presentation `dt` (frame-delta), not the fixed tick. Particle
  position/lifetime/color are visual only. They **never re-enter sim** — a
  particle cannot kill an entity, set a flag, or be queried by sim code. This is
  the single sharpest determinism rule in this spec: a particle that affected
  gameplay would be a §17 break, so particles are physically confined to the
  presentation module and read no sim state back.
- The §10.2 flip-book sampler is a **pure function** of `(anim, elapsed)` — same
  inputs, same `Rect`, every time — so it is trivially deterministic *as a
  function* even though `elapsed` is presentation time.
- **Within the module, the core is still deterministic and fully covered.** The
  list build, the layer sort, gradient registration, transform composition, and
  particle stepping are pure given their inputs — so two runs of the same
  `onRender` calls + same presentation-`dt` sequence produce a byte-identical
  `Draw2dList` (the property the render golden test relies on). Presentation
  exclusion is about *not feeding sim*, not about internal sloppiness.
- The module reads **no wall-clock** itself: the app passes presentation `dt`
  in. The only nondeterminism (true frame timing) is owned by the app, on the
  presentation side of the boundary, exactly where §17.5 puts it.

## 7. Acceptance / proof

- **`axiom-draw2d`: 100% covered, branchless** (engine module — both laws
  apply). Every shape/sprite/text/gradient/particle/render-target path exercised;
  every `as_*` accessor's Some and None arm hit (the `RenderCommand` test
  pattern).
- **Layer-sort golden.** Submit draws out of layer order across several layers
  with ties; assert `finish()` yields them stably sorted by `(layer,
  submit-order)`. This is the regression test for the verified "no reorder" gap.
- **Determinism-as-function.** The same sequence of facade calls + the same
  presentation-`dt` stream produces a byte-identical `Draw2dList` hash on a
  second run (particles included). Replaying the same `dt` partition is
  chunk-stable for the flip-book sampler.
- **Presentation-exclusion proof.** A test asserts the module exposes **no** API
  that returns particle/draw state into a sim-readable form — there is no
  read-back path. (Structural: the facade has no getter a sim could call.)
- **Backend alpha-blend proof.** Extend each backend's tests: a layer-2 draw with
  `alpha < 1` over a layer-1 fill composites (not overwrites) — the explicit
  regression for the hardcoded `REPLACE`. Verified end-to-end on the wasm arm via
  the Playwright controller / `axiom-shot` (both backends), since the live blend
  path is platform code outside the coverage gate.
- **TS projection.** `@axiom/game`'s `Frame` 2D methods: tsgo + Oxlint (branch
  ban) + 100% TS coverage; a headless test draws each primitive and asserts the
  marshalled command stream matches the native `Draw2dList`.

## 8. Dependencies & order

- **Depends on SPEC-00** (the `Frame` interface and the wasm boundary that
  carries `Draw2dList` to the app, and the app-side backend submission). Cannot
  project to TS before SPEC-00 lands.
- **Depends on the alpha-blend backend fix** before `alpha`/shadow/gradients are
  visually correct — but the neutral list and its golden tests land independent
  of the live blend path.
- **Uses** `axiom-math` (`Vec2`/`Mat3`) and kernel ids/quantities.
- **Lands after** SPEC-01/02/03 (the sim core) in the contract build order
  (§18: 2D surface is step 4 of the presentation block), but is otherwise
  independent of the other presentation specs.
- **Depended on by** essentially every 2D game app and by SPEC-09 (UI/HUD
  overlay, which draws its widgets through this surface's screen layer).

## 9. Open questions

- **Text rendering strategy (the hardest open question).** Three candidates,
  each putting the glyph pixels in a different place:
  1. **Baked bitmap glyph atlas as an `axiom-assets` asset.** Fonts are
     rasterized offline (by a tool) into an atlas texture + metrics table;
     `axiom-draw2d` emits `KIND_TEXT_GLYPHS` referencing atlas sub-rects —
     **identical to sprite draws**, so it needs *no* new backend code and stays
     fully deterministic and platform-free. Cost: no live font sizing; emoji and
     arbitrary `family` need pre-baking; HiDPI needs multiple bakes.
  2. **SDF (signed-distance-field) atlas.** One baked atlas scales crisply to any
     size in a backend shader. Best quality/size tradeoff; cost is a real
     GPU-backend shader path and a canvas-backend fallback.
  3. **System-font rasterization in a platform arm.** Canvas `fillText` /
     browser font stack. Trivial in the browser, but it is *non-deterministic
     across platforms*, pushes text into platform-only code (untestable in the
     native core), and breaks `measureText` reproducibility.
  **Leaning (1) for the first cut**: a baked atlas asset keeps `axiom-draw2d`
  glyph-index-only and reuses the sprite path; promote to SDF (2) when scaling
  quality demands it. (3) is rejected for the determinism-of-`measureText` break.
  Where fonts load (`loadFont`) and where the atlas is owned (`axiom-assets` vs a
  draw2d-local table) rides on this choice.
- **Particles: in `axiom-draw2d` or a sibling module?** Placed here for now
  because particles *only* produce 2D draws and share the presentation clock and
  paint table. If a 3D particle need appears (SPEC-11), or if particle
  *simulation* grows beyond "emit + integrate + fade," extract a sibling
  presentation module (`axiom-particles`) that emits into the draw-list — but
  that is a second consumer proving the primitive, not a default. Until then,
  a separate module would be a ceremonial split.
- **`Draw2dList` ownership: host layer or draw2d module?** `axiom-render` puts
  its cross-backend artifact (`FramePacket`) in the `host` layer so both backends
  can name it. The 2D list has the same two consumers. Settle whether
  `Draw2dList` is a host-layer type (backends depend only on `host`, as today) or
  a draw2d-owned type the app re-marshals — the former matches the existing
  `FramePacket` pattern and is the likely answer.
- **DPR / responsive.** Resize exists; `devicePixelRatio` does not. The neutral
  list is DPR-agnostic (world/screen units); the app scales the backend's target
  size by DPR before submission. Confirm DPR is an app/host concern (SPEC-12),
  not a field on `Draw2dList` — text crispness depends on the backend rasterizing
  at device resolution, which is a backend+app responsibility, not the contract's.
- **Screen-space layer (§13 HUD).** The contract routes screen-targeted draws to
  a screen layer that bypasses `camera2D`. Confirm this is a reserved `layer`
  band / a `space: World | Screen` flag on `Common2d`, resolved in the core so a
  backend never decides projection.
