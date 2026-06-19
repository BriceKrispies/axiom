# Growth — in-browser (WASM) viewer

A full Growth flow in the browser: configure a planet → see its **overworld map**
→ pick a spot → **descend** into a walkable, first-person WebGPU view of the
procedurally generated terrain at that spot. It extends the existing
`axiom-growth` app (an Axiom *leaf* — it depends on no other app) with a
`wasm32`-only presentation arm; the deterministic worldgen core and the native
test suite are untouched.

## The screen flow (state machine in `index.html` + `web.rs`)

- **(A) CONFIG** — an HTML form over the canvas: a **seed** text field, a
  **preset** dropdown (Earthlike / Ocean World / Dry), and a **detail** slider
  (region/site count). **Generate** calls the wasm export
  `generate(seed, preset, sites)`, which runs `Growth::generate(...)` and renders
  the overworld map. The generated planet is stashed in a `thread_local` `WORLD`
  cell between flow steps.
- **(B) OVERWORLD** — an **equirectangular biome+elevation map** of the planet,
  drawn into a 2D `<canvas>` (`#axiom-growth-map`) by sampling
  `sampler::sample_surface` over a lat/long grid and colouring each pixel by
  biome (ocean depth-shaded blue; desert/rainforest/tundra/taiga/grassland on
  land, brightened with elevation). This is option **(b1)** — see "Overworld
  rendering choice" below.
- **(C) SELECT** — a click on the map is converted to a normalised `(u, v)`,
  passed to the wasm export `is_land(u, v)` (pixel → lat/long → unit dir →
  `sample_surface().elevation >= 0`). Ocean clicks are rejected with a hint; a
  land click proceeds.
- **(D) DESCEND** — the wasm export `descend(u, v)` maps the click to a unit
  direction, builds `GameWorldLocalMap::anchored_at(atlas, picked_dir)`, builds
  the terrain mesh around that anchor, and starts the first-person walkable
  windowing loop. A **Back to map** control re-shows the map (the GPU loop is
  single-shot per page load, so a brand-new descent needs a regenerate).

Canvas element ids: first-person **`axiom-growth-canvas`** (`web::CANVAS_ID`);
overworld map **`axiom-growth-map`** (`web::MAP_CANVAS_ID`).

## Ground-follow: the player walks ON the terrain (no floating / clipping)

The engine's `Controller` is a free-fly camera — it integrates horizontal
movement but holds a constant Y, so it clips through hills and floats over
valleys. The viewer instead makes the camera a surface-following character. The
app owns the player state `(x, z, yaw, pitch)` plus `engine_y` (the camera's
current engine-space Y). Each frame `descend`'s loop:

1. Updates `yaw`/`pitch` from keys + mouse deltas.
2. Integrates horizontal movement **exactly as the engine will**: it builds the
   same yaw quaternion `Quat(0, sin(yaw/2), 0, cos(yaw/2))`, rotates the local
   move `(strafe, 0, -forward)`, and adds the `xz` of that to its own `(x, z)` —
   staying in lock-step with the engine's controller.
3. Samples the ground under the new `(x, z)` with `gameworld::sample_height_m`.
   The mesh is recentred by the **true** anchor height (`build_terrain` now
   returns that offset, not 0), so the eye target is
   `sampled - anchor_height + EYE_HEIGHT_M` (1.7 m).
4. Drives the camera with a `FirstPersonInput` whose `move_local` is
   `(strafe, dy, -forward)`, where `dy = desired_y - engine_y`. The engine's
   controller does `translation += yaw.rotate(move_local)`; a +Y yaw rotation
   **preserves the Y component**, so `dy` lands the camera exactly on the surface
   while the horizontal step matches the app's own integration.

This needs no engine change — it uses only the existing
`RunningApp::tick_with_controls(..)` delta-control path. Verified headed on a real
GPU: the eye rises and falls with the terrain as you walk over hills/valleys,
staying ~1.7 m above the surface, with no clip and no float.

## How the terrain mesh reaches the GPU

`build_terrain` samples `gameworld::sample_height_m` over a 257×257 grid (256 m
square at 1 m spacing) centred on the anchor, producing an interleaved
`[x, y, z, nx, ny, nz, r, g, b, a]` vertex stream (10 floats/vertex: heights
recentred toward y≈0; per-vertex normals via central differences; a per-vertex
biome+elevation colour) and a triangle index list (256×256×2 = 131 072
triangles, 393 216 indices). The engine scene is authored with **one
identity-transform renderable** (so the engine emits exactly one draw whose MVP
is the camera view-projection), a first-person `Controller` camera, and a
directional light. The geometry is uploaded directly through
`WindowingApi::run_web(canvas_id, vertices, indices, 1, frame_fn)`; the frame
closure produces the single instance's `[mvp(16), colour(4)]`.

## Overworld rendering choice (b1 vs b2)

Chosen: **(b1) the 2D equirectangular map.** It is the robust path: it sidesteps
the per-vertex-colour blocker entirely (it draws into a 2D canvas, not through
the instanced-cube pipeline), it reads like a real world map (biomes, coastlines,
ice caps), and a pixel maps cleanly to lat/long for picking. Option (b2) — a
rendered 3D globe — is blocked for *colour* by the same per-vertex-colour gap
documented below (a single-colour globe can't show continents), so it was not
pursued.

## How the terrain renders on a cube-only mesh API

The umbrella's `Mesh` enum (`modules/axiom/src/mesh.rs`) only knows
`Mesh::Cube` — there is no API to register an arbitrary triangle mesh as a
`Mesh`. The viewer does **not** need one: the live backend
(`modules/axiom-windowing/src/live_gpu_binding.rs`) uploads ONE shared
vertex/index stream and draws it once per *instance* with a per-instance MVP.
`run_web` takes those vertex/index streams as plain arguments, so the viewer
hands it the terrain geometry and authors a single identity renderable; the
engine's one MVP (`view_projection * identity`) is exactly the transform the
terrain needs. This is a real terrain mesh, not cube columns.

## Per-vertex colour (RESOLVED): biome/elevation-graded terrain

**Requested:** per-vertex colours (biome base, darker low ground → snowy white by
height, blue below 0).

**Status:** done. The live presentation path now carries a per-vertex colour, so
the walkable terrain is coloured by biome + elevation per vertex (no fake).

- Backend layout + WGSL: `modules/axiom-windowing/src/live_gpu_binding.rs`. The
  per-vertex `VertexBufferLayout` is now `position(3) + normal(3) + colour(4)`,
  `array_stride: 40`, with the colour as a `Float32x4` at `shader_location 2`
  (offset 24). The per-instance buffer is unchanged in size (`INSTANCE_STRIDE =
  20 f32`) but its attributes shifted to `shader_location 3..7` (mvp columns +
  instance colour). The shader computes `base = vertex_color * instance_color`,
  so a **white** per-vertex colour reproduces the old per-instance-only look
  exactly (backward compatible), while real per-vertex colours show through when
  the instance colour is white.
- The native offscreen twin used by the agent screenshot path
  (`apps/axiom-doom-browser/src/bin/render.rs`) mirrors the same layout/shader.
- Engine vertex stream: `RunningApp::mesh_vertex_stream`
  (`modules/axiom/src/app.rs`) appends opaque **white** per vertex (10
  floats/vertex), so every existing browser app (doom, netplay, rotating-cube,
  stress-cubes) is visually identical to before.
- Growth terrain: `build_terrain` (`src/web.rs`) samples
  `sampler::sample_surface` at each vertex's unit direction and appends a
  biome+elevation colour (`biome_terrain_color`, linear RGBA), and the terrain
  material is `Color::WHITE` so the per-vertex colours render true. Relief is
  still reinforced by the shader's normal-based diffuse term (correct per-vertex
  normals are computed).

The overworld 2D map path is unchanged (it was always fully coloured in its own
2D canvas).

## Build + serve + screenshot

From the repo root (`C:\dev\axiom`):

```sh
# Build the wasm bundle into web/pkg (Makefile target):
make growth-build
# equivalently, raw:
#   cargo build -p axiom-growth --target wasm32-unknown-unknown --release
#   wasm-bindgen --target web --out-dir apps/axiom-growth/web/pkg \
#     target/wasm32-unknown-unknown/release/axiom_growth.wasm

# Serve it (WebGPU requires an http:// origin, not file://):
make growth          # serves apps/axiom-growth/web at http://localhost:8000
# or pick a port:  make GROWTH_PORT=8137 growth
```

Then open `http://localhost:<port>/` in a WebGPU browser (recent Chrome/Edge, or
Firefox Nightly). Fill the form and **Generate**; the overworld map appears.
Click a **land** spot to descend; on the first-person canvas, click to capture
the mouse, then WASD/arrows to move, mouse to look, Esc to release.

Screenshot via the Playwright controller (must be **headed** for a GPU adapter —
`AXIOM_PW_HEADLESS=0`; headless has no usable GPU):

```sh
AXIOM_PW_HEADLESS=0 uv run scripts/playwright_controller.py goto http://localhost:8137/
uv run scripts/playwright_controller.py screenshot growth_config
# Click Generate, then a land pixel, via eval, then screenshot the play view.
```

## Verification status

- `cargo test -p axiom-growth` (native) — green (109 tests; +3 for the new
  `anchored_at` method); the wasm arm is `#[cfg(target_arch = "wasm32")]` so it
  never compiles natively.
- `cargo xtask check-architecture` — passes (`engine` + `windowing` already in
  `app.toml` `allowed_modules`).
- `cargo build -p axiom-growth --target wasm32-unknown-unknown --release` +
  `make growth-build` — build clean; `wasm-bindgen` emits `web/pkg` with the
  `generate`, `is_land`, and `descend` exports.
- Live browser test (**headed** Chrome, real GPU): the CONFIG form generates a
  planet; the **overworld map** renders as a recognisable equirectangular world
  (blue oceans depth-shaded, green/tan/grey land biomes, ~24% land for an
  Earthlike); an ocean click is rejected and a **land** click descends; the
  first-person view presents green terrain with relief under a sky-blue
  background, and **walking forward (W) follows the ground** — the horizon and
  surface move with the terrain while the eye stays ~1.7 m above it. Zero JS
  errors. (Headless Chromium has no GPU adapter, so the first-person canvas only
  presents headed — the same headless-GPU ceiling as every Axiom browser app; the
  2D overworld map renders in either mode.)
