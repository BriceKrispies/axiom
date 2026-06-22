# Browser render fallback: WebGL2 and the Canvas-2D software path

**Status:** Part 1 (WebGL2 fallback) is **implemented and browser-verified**.
Part 2 (Canvas 2D software rasterizer) remains a scoping note — not built.
**Why this exists:** the browser render path was WebGPU-only. `navigator.gpu`
is still absent or disabled for a meaningful slice of browsers and locked-down
environments, so we need a fallback that keeps the cube/scene visible. This note
records exactly what each fallback costs, because the two common readings of
"fall back to canvas" are wildly different in effort and visual fidelity.

## The distinction that drives everything

A browser `<canvas>` can hand back several different drawing contexts. "Falling
back to canvas" can mean either of these, and they are not close:

- **WebGL2** (`getContext("webgl2")`) — a real, hardware-accelerated GPU API, the
  universally-supported predecessor of WebGPU. Same triangles, same shaders
  (cross-compiled), same depth buffering, **essentially identical fidelity**.
- **Canvas 2D** (`getContext("2d")`) — a software 2D painting API (`fillRect`,
  `drawImage`, `putImageData`). Rendering a 3D scene through it means writing a
  **CPU software 3D rasterizer** from scratch: vertex transform, triangle
  rasterization, depth test, perspective-correct interpolation, texture sampling,
  and per-light shading, all on the CPU. Large, slow, and lossy.

**If the goal is "keep roughly the same fidelity," WebGL2 is the answer and
Canvas 2D is the wrong tool.** Canvas 2D is only worth it as a last-resort
"there is no GPU at all" path, and even then it cannot match the current shadowed
output without enormous effort (see Part 2).

## Current rendering architecture (the facts a fix builds on)

The browser draw path, innermost-first:

| Concern | Crate / module | Key file |
|---|---|---|
| Backend-neutral render commands | `modules/axiom-render` | `render_command_list.rs`, `render_input.rs` |
| WebGPU submission shape (recording) | `modules/axiom-webgpu` | `gpu_submission.rs` |
| **Real GPU device + swap-chain** | `modules/axiom-gpu-backend` | `live_gpu_binding.rs` |
| **The one shared renderer (shaders, pipelines, draw loop)** | `modules/axiom-gpu-backend` | `scene_renderer.rs` |
| Deterministic run loop + canvas lookup | `modules/axiom-windowing` | `windowing_api.rs` |
| Backend-neutral host contracts | `crates/axiom-host` | `host_presentation_request.rs`, `host_surface_handle.rs` |

Important structural facts:

- **`SceneRenderer` (`modules/axiom-gpu-backend/src/scene_renderer.rs`) is the
  single definition of how a frame is drawn.** Both the live browser arm
  (`live_gpu_binding.rs`) and the native off-screen screenshot arm
  (`offscreen`, behind the `offscreen` feature) drive the same `SceneRenderer`,
  so there is no second pipeline to keep in sync. Whatever a fallback does, it
  should *not* fork this into a hand-synced copy.
- **The GPU arms are already outside the engine gates.** `scene_renderer.rs` /
  `live_gpu_binding.rs` compile only on `wasm32` (live) or under the native
  `offscreen` feature (screenshot). The default native build, the **coverage
  gate**, and the **branchless lint** never see them — which is why those files
  are full of `for` loops and `if let` today. **Backend-selection logic added to
  the wasm32 arm is therefore not subject to the branchless/coverage laws.** It
  is platform-arm code, verified through the browser, exactly like the existing
  WebGPU path.
- **`windowing` is a feature module** (`modules/axiom-windowing/module.toml`,
  `kind = "feature-module"`) that already composes one module, `gpu-backend`,
  via its nameable `GpuBackendApi` facade. Composing a *second* backend module is
  the sanctioned feature-module pattern (add it to `allowed_modules`).
- **Platform APIs are allow-listed.** Only the `host` layer and the `windowing` +
  `gpu-backend` modules may touch `web_sys` / `wasm_bindgen` / `wgpu` etc. — see
  `PLATFORM_FACING_MODULES` in `crates/xtask/src/hygiene.rs`. Any new
  platform-facing module must be added there (a deliberate amendment, not a
  default).

### What "same fidelity" means today (the bar to match)

`scene_renderer.rs` currently produces, per frame:

- instanced triangle meshes (per-mesh vertex/index buffers, per-instance
  MVP + world matrix + colour),
- per-vertex + per-instance colour, multiplied with a sampled **albedo texture**
  (sRGB, nearest, repeat),
- lighting = ambient (`base * 0.12`) + up to 16 lights: directional, and point
  lights with distance attenuation,
- a **directional shadow-map depth pre-pass** rendered from the light's POV into
  a 2048² `Depth32Float` map, sampled in the main pass with a **comparison
  sampler + 3×3 PCF**,
- depth buffering (`Depth32Float`, `depth_compare = Less`).

Any fallback that claims "same fidelity" has to reproduce all of the above —
the shadow pass is the hard part.

---

# Part 1 — WebGL2 fallback (recommended)

### Why it is cheap here

We render through **`wgpu` 25**, and `wgpu` ships a **WebGL2 (GLES3) backend**.
It cross-compiles our WGSL to GLSL ES automatically (via naga), so the existing
`SceneRenderer`, both shaders (`SCENE_WGSL`, `SHADOW_WGSL`), the instancing, the
depth buffer, and the shadow pass **run unchanged** — no rendering rewrite.

The codebase was visibly pre-shaped for this:

- `live_gpu_binding.rs` already requests
  **`wgpu::Limits::downlevel_webgl2_defaults()`** (line 63) — it constrains
  itself to what WebGL2 supports.
- Instances are packed with **per-batch byte offsets**, never `firstInstance`
  (`scene_renderer.rs::record`) — and `firstInstance` is precisely the feature
  WebGL2 lacks.
- Lighting is a **uniform buffer**, not a storage buffer (WebGL2 has no storage
  buffers).
- Shadows use a **depth texture + comparison sampler** — both supported by
  WebGL2 / GLES3.

### Steps

1. **Enable the WebGL backend in `wgpu`.**
   `modules/axiom-gpu-backend/Cargo.toml`, the wasm32 target table (line ~44–48)
   currently has `wgpu = "25"` with no features → only the WebGPU backend is
   compiled in. Change to enable both backends, e.g.:
   ```toml
   wgpu = { version = "25", features = ["webgl"] }
   ```
   (`webgpu` is on by default; `webgl` is additive. Confirm the exact default
   feature set for the pinned 25.x — we want both `BROWSER_WEBGPU` and `GL`
   available at runtime.)

2. **Make backend selection fall back — probe the *device*, not just the adapter.**
   `modules/axiom-gpu-backend/src/live_gpu_binding.rs::initialize` now probes
   WebGPU on a `Backends::BROWSER_WEBGPU` instance with **no canvas**
   (`compatible_surface: None`), requesting both an adapter **and** a device. Only
   a live device commits to WebGPU; otherwise it builds a `Backends::GL` instance
   and presents through WebGL2. The chosen backend is logged
   (`axiom: render backend = …`) for browser/Playwright confirmation.

   **Why probe the device, not just the adapter (verification caught this):** a
   browser can hand back a WebGPU *adapter* whose *device* creation then fails
   ("Device failed at creation" — exactly what this dev sandbox does). A browser
   canvas hosts exactly one context type and it **cannot be reclaimed** once
   `getContext("webgpu")` has been called, so the backend must be fully validated
   *before* the surface is created. Committing the canvas on adapter-presence
   alone strands the app on a dead backend (black canvas) with no path back to
   WebGL2. The probe uses no canvas, so a failed WebGPU probe leaves the canvas
   free for WebGL2.

3. **Loosen the JS capability gate.**
   Each browser app's `web/index.html` hard-checked `navigator.gpu` and bailed
   when it was absent. They now compute
   `hasRenderBackend = ("gpu" in navigator) || <canvas can getContext("webgl2")>`
   and only show the unsupported message when **neither** is present. Applied to
   all five gating apps: `axiom-demo-rotating-cube-browser`,
   `axiom-stress-cubes-browser`, `axiom-doom-browser`, `axiom-netplay-browser`,
   and `axiom-growth` (plus the `gallery/index.html` copy). This is app-level JS,
   outside the engine gates.

4. **Verify in a real browser.** ✅ Done via the Playwright controller
   (`scripts/playwright_controller.py`). This dev sandbox's headless Chromium
   exposes a WebGPU adapter but fails device creation, so it exercised the
   fallback for free: the console logged `render backend = Gl` and the rotating-
   cube scene rendered in full — textured sphere + cubes, lit ground plane, and
   the directional **shadow-map pass** (soft contact shadows under the cubes). A
   browser with working WebGPU logs `render backend = BrowserWebGpu` and is
   unaffected.

### Caveats / things to check

- **Shadow pass on WebGL2.** ✅ Verified working — depth-texture sampling +
  comparison samplers run on WebGL2, and the shadow-map pass rendered correctly
  in the fallback screenshot. Still the most likely place for a *subtle* visual
  delta (PCF filtering, depth-bias) if a future change touches it; screenshot-
  compare both backends when it does.
- **sRGB surface format.** ✅ A WebGL2 surface offered an sRGB format in testing;
  the `unwrap_or(caps.formats[0])` fallback guards the no-sRGB case either way.
- **Feature-set drift.** WebGL2 covers the *current* feature set. Future
  additions — compute shaders, storage buffers, multiple render targets beyond
  WebGL2's limits — will have no WebGL2 equivalent. When we add such a feature,
  it must either stay WebGPU-only (degrade gracefully on WebGL2) or get a WebGL2
  path. Note it in this file when it happens.
- **No new module, no architecture amendment.** This entire change lives inside
  the already-platform-blessed `gpu-backend` module plus app-level JS. The
  deterministic core, the data contracts, `windowing`, and all gates are
  untouched.

### Effort — actual

Small, as predicted: a Cargo feature flip (`webgl` + `console`), the
device-probe + fallback in `initialize` (~50 lines incl. the `request_render_device`
helper), the `hasRenderBackend` JS gate across five apps, and Playwright
verification. No new crate, no gated-spine tests (the arm is coverage-exempt),
no law amendments.

---

# Part 2 — Canvas 2D software rasterizer (only if a no-GPU target demands it)

This is the path for environments with **neither WebGPU nor WebGL2** (rare:
hardened browsers, GPU blocklists, some headless/embedded webviews). It is a
genuine software-renderer project, and it **cannot cheaply match** the current
shadowed fidelity. Treat it as a separate, large initiative — do not start it
unless a concrete target requires it.

### Architectural placement

- **New engine module `modules/axiom-canvas2d-backend`** (`kind =
  "engine-module"`, `allowed_modules = []`), mirroring `gpu-backend`'s shape: a
  native-clean facade (no-op present + surface size, 100% covered) and a
  `#[cfg(target_arch = "wasm32")]` live arm holding the real
  `CanvasRenderingContext2d` + the software framebuffer. Native default build
  stays browser-free, so the coverage/branchless gates only see the facade.
- **It consumes the same host contract** (`HostPresentationRequest`) and the same
  plain-data per-frame inputs `gpu-backend` already takes — meshes
  `(id, 12-float verts, indices)`, materials `(id, w, h, RGBA8)`, light tuples
  `(kind, vec3, vec3, intensity)`, instance batches
  `(mesh_id, mat_id, [mvp+world+colour] floats, count)`, clear colour. **No
  module-to-module crossing**; it speaks the same neutral vocabulary the app/
  windowing already produce.
- **Hygiene allow-list:** add `"canvas2d-backend"` to `PLATFORM_FACING_MODULES`
  in `crates/xtask/src/hygiene.rs` (deliberate amendment). It will reference
  `web_sys` (`CanvasRenderingContext2d`, `ImageData`) + `wasm_bindgen`.
- **`web-sys` features:** `CanvasRenderingContext2d`, `ImageData`,
  `HtmlCanvasElement`.

### The renderer work (the actual cost)

A CPU rasterizer that reproduces `scene_renderer.rs` needs, per frame:

1. **Framebuffer.** Allocate an `width*height` RGBA8 colour buffer and an
   `f32`/`u32` depth buffer.
2. **Vertex stage.** For every instance in every batch, transform each vertex by
   the per-instance MVP (clip space) and by the world matrix (world position +
   world normal). This is the `vs` entry of `SCENE_WGSL` done in Rust.
3. **Clipping + viewport.** Perspective divide, near-plane clip (at minimum),
   map NDC → pixel coordinates.
4. **Triangle rasterization.** Edge-function / barycentric scan-convert each
   triangle with **perspective-correct interpolation** of normal, uv, colour, and
   world position. Per-pixel **depth test** against the depth buffer
   (`Less`), matching `Depth32Float` semantics closely enough to avoid z-fighting
   differences.
5. **Texture sampling.** Nearest sample of the material's RGBA8 albedo with
   **repeat** wrap and **sRGB→linear** decode (the GPU texture is
   `Rgba8UnormSrgb`; to match output you must decode on sample and re-encode on
   store).
6. **Fragment shading.** Reproduce the `fs` entry: ambient `base*0.12` + per-light
   diffuse with `max(dot(N,L),0)`; directional vs point branch (point =
   normalize(pos−world) + `1/(1+0.09d+0.032d²)` attenuation). Multiply albedo ×
   vertex colour × instance colour.
7. **Present.** Pack the colour buffer into an `ImageData` and `putImageData` to
   the 2D context (or blit via an offscreen ImageBitmap for speed).

### Fidelity gaps you will hit (be honest about these)

- **Shadows are the wall.** Matching the directional shadow-map + 3×3 PCF means
  rendering the scene **twice** on the CPU (a light-POV depth pass into a 2048²
  software depth buffer, then a comparison+PCF lookup per lit fragment). This
  roughly doubles an already-expensive rasterizer and is the single biggest cost.
  A realistic v1 ships **without shadows** (flat-lit) and is visibly different
  from the WebGPU/WebGL2 output — call that out explicitly rather than claiming
  parity.
- **Performance.** Software rasterization of instanced meshes at 60fps in WASM is
  a real engineering problem (per-pixel inner loops, no SIMD by default).
  Expect to need a resolution cap, tile/bounding-box rasterization, backface
  cull, and possibly `wasm-simd`. It will be far slower than either GPU path.
- **sRGB / filtering exactness.** Byte-exact match to the GPU is unlikely;
  perceptual closeness is the realistic target.

### Selection + verification

- **Backend selection** lives in the wasm32 arm (gates-exempt): try WebGPU →
  WebGL2 (Part 1) → Canvas 2D. `windowing` (feature module) lists both
  `gpu-backend` and `canvas2d-backend` in `allowed_modules` and constructs
  whichever facade the capability probe selected. Both facades expose the same
  `initialize` / `render_frame` / `replace_geometry` shape so the run loop calls
  them uniformly.
- **JS capability probe** in the app: prefer `navigator.gpu`; else
  `getContext("webgl2")`; else `getContext("2d")`; pass the choice to WASM (query
  param or init arg).
- **Coverage:** the software rasterizer's *math* (vertex transform, edge
  functions, barycentric interpolation, sRGB decode, shading) is pure and should
  live in native-testable, fully-covered functions in the module's deterministic
  core — only the `CanvasRenderingContext2d` binding is wasm-gated. This is how a
  Canvas 2D backend earns real test coverage instead of hiding behind the
  platform-arm exemption: keep the rasterizer pure and the platform glue thin.
- **Verify** via Playwright screenshot-compare against the WebGPU reference
  (with the documented shadow/perf caveats).

### Effort

Large. A new module, a from-scratch perspective-correct rasterizer with depth +
texture + multi-light shading, a perf pass to make it usable, and (for true
parity) a software shadow-map pass. Plan it as its own milestone with its own
fidelity bar — and seriously consider whether WebGL2 (Part 1) already covers the
real target audience first.

---

## Recommendation

1. **Part 1 (WebGL2) — done.** Small, structurally clean, confined to one module
   + app JS, fidelity preserved (verified incl. shadows), and the codebase was
   pre-shaped for it. Near-universal browser support now.
2. **Defer Part 2 (Canvas 2D)** until a concrete no-GPU target appears. When it
   does, scope it as a standalone module + rasterizer milestone with an explicit,
   reduced fidelity bar (likely shadow-less v1), not as a quick "fallback."

## Open questions / follow-ups

- ✅ `wgpu` 25.x `webgl` feature is additive — both backends coexist; the GL
  backend (`glow`) compiles in alongside WebGPU.
- Whether to expose the selected backend through the deterministic host contract
  (e.g. a `HostDeviceProfile` variant) so apps/telemetry can observe it, vs.
  keeping selection inside the wasm arm + console log (current).
- Do any current browser apps rely on WebGPU-only behaviour (timestamp queries,
  etc.) that would silently degrade on WebGL2? None known for the current feature
  set (lit/textured/shadowed instanced draws all run on WebGL2), but re-audit
  when adding compute/storage-buffer features.
- The other four browser apps (`stress-cubes`, `doom`, `netplay`, `growth`) got
  the same JS gate change but were only verified to *build*, not screenshot-
  tested on the WebGL2 path — worth a pass if any is a shipping target.
