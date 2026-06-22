# axiom-canvas2d-backend — architecture

Canvas2D is Axiom's **last-resort browser fallback**: when neither WebGPU nor
WebGL2 is available, the scene is rendered on the CPU by a **software z-buffer
rasterizer** into a small RGBA framebuffer, which is then blitted to a
`CanvasRenderingContext2d` with `putImageData`. **Canvas 2D is the blit target,
not the renderer** — there is no high-level Canvas path/region drawing.

## It is an intentional low-resolution software renderer, not a pixel match

Canvas2D is **allowed to drift visually** from the GPU backends — a low internal
resolution (320×180 by default, nearest-neighbour upscaled), flat-shaded
triangles, no shadows, no PBR, no texture sampling, no post-processing. That
drift is a deliberate *presentation policy*, not a bug, and it lives **only** at
this backend's presentation seam. Canvas2D is **not** a separate game path, scene
path, resource path, or render path.

## The shared render spine is preserved

```
Game/app → SceneSnapshot → ResolvedResources → RenderInput → RenderCommandList
        → host::FramePacket → GPU backend OR Canvas2D backend
        → host::FrameSubmissionReport
```

Canvas2D consumes the **same `host::FramePacket`** the GPU backend consumes, and
returns the **same `host::FrameSubmissionReport`** (carrying the neutral
`host::FrameRasterStats`). It imports **no** scene/game/resource module and never
reaches around `FramePacket` for richer data. Its only inputs are:

- the neutral `FramePacket` (per frame),
- a backend-owned resource table (`MeshCache`) initialised from the same neutral
  `(mesh_id, vertices, indices)` upload the GPU backend gets,
- backend-owned visual policy (`canvas_policy.rs`, `low_poly_raster_options.rs`).

Browser APIs (`web_sys`, `CanvasRenderingContext2d`, `ImageData`, …) appear
**only** in the wasm-gated `live_canvas_binding.rs`; the entire rasterizer is
pure, native-testable Rust at 100% coverage (a `tests/architecture.rs` test
enforces the isolation).

## The software rasterizer pipeline (pure, native-tested)

```
FramePacket
  └─ frame_packet_raster::convert ─────────────────────────┐
       project each draw's triangles through draw.mvp        │  RasterTriangle[]
       (perspective divide → NDC → framebuffer pixels),      │  (screen-space,
       resolve flat colour (mesh vertex colour × draw        │   flat-shaded,
       colour), cull near-plane-invalid triangles,           │   object_id kept)
       coverage-preserving terrain LOD                       │
  └─ software_rasterizer::rasterize_packet ─────────────────┘
       clear colour + depth buffers; for each triangle:
         edge-function coverage, barycentric depth interp,
         per-pixel depth test (smaller = nearer), flat fill,
         optional depth fog; optional debug overlay
  └─ SoftwareFramebuffer (RGBA8 bytes)
  └─ live_canvas_binding::blit  (wasm only)
       ImageData::new_with_u8_clamped_array_and_sh + putImageData,
       canvas backing store = 320×180, CSS-scaled with image-rendering: pixelated
```

### Components

- `software_framebuffer.rs` — `SoftwareFramebuffer`: RGBA8 colour buffer
  (`clear`, `set_pixel`, `into_rgba_bytes`). Top-left origin, the `ImageData`
  layout, so the blit needs no conversion.
- `depth_buffer.rs` — `DepthBuffer`: f32 per-pixel depth. **Convention: smaller =
  nearer**; clears to `+∞` (far); `test_and_write` passes on strict `<`, so a
  nearer fragment overwrites a farther one, a farther never overwrites a nearer,
  and on exactly-equal depth the earlier fragment wins (deterministic regardless
  of draw order).
- `raster_vertex.rs` / `raster_triangle.rs` — projected screen-space vertices and
  flat-shaded triangles (`object_id` carried through for picking/telemetry).
- `frame_packet_raster.rs` — `FramePacket → RasterTriangle[]`: projection, flat
  colour resolution, near-plane cull, terrain LOD, conversion stats.
- `software_rasterizer.rs` — the edge-function rasterizer + depth test + fog +
  overlays; returns the finished bytes and per-frame stats.
- `low_poly_raster_options.rs` — `LowPolyRasterOptions`: framebuffer size, depth
  fog (near/far), debug overlay, terrain LOD cap, frame pixel budget. Built from
  a `CanvasQualityPreset` via `from_preset`.
- `canvas_policy.rs` — `CanvasVisualProfile` (`LowPolyFramebuffer`),
  `CanvasQualityPreset` (`UltraLow 160×90` | `Low 240×135` | `Medium 320×180` |
  `High 426×240`), `CanvasDebugOverlay` (`None` | `TriangleEdges` | `DepthBuffer`
  | `Bounds`), `CanvasFallbackImportance` + `classify` (terrain/critical
  detection).

## Performance & quality tiers

The renderer is built for speed at low internal resolution:

- **Quality tiers.** `CanvasQualityPreset` resolves to a framebuffer size. The
  forced fallback defaults to **Low (240×135)**; the platform arm reads
  `?quality=ultralow|low|medium|high` (windowing) and a future
  dynamic-resolution policy can step tiers from measured frame time — the
  documented seam in `low_poly_raster_options.rs` (deterministic core stays
  timer-free; only the wasm arm reads a clock).
- **Pre-raster culling** (`frame_packet_raster`). Triangles are dropped before
  the pixel loop: invalid-projection (near plane), degenerate (≈0 area),
  off-screen (bbox outside the framebuffer), and sub-pixel (below
  `MIN_TRIANGLE_AREA`) for **non-critical** draws (critical coverage is exempt).
- **Single-pass conversion.** Projection + area + cull happen in one fold into a
  single pre-reserved candidates `Vec` (was five intermediate `Vec`s); `retain`
  and `sort`/`truncate` run in place. This removed the dominant per-frame
  allocation churn.
- **Scanline hot loop** (`software_rasterizer`). Per row the covered x-span is
  computed (one divide per edge) and only that span is iterated, stepping
  barycentrics + depth incrementally (no per-pixel division, no per-row edge
  re-eval). Colour bytes are precomputed once per triangle; the conditional
  depth/colour write is a branchless index-select into the preallocated
  `&mut [u8]`/`&mut [f32]` (no closures, no temporaries).
- **Frame pixel budget.** A cumulative estimated-cost budget; once exceeded,
  **Decorative** draws are skipped (`skipped_decorative_draws`,
  `budget_exhausted`). GameplayObject and CriticalCoverage are never skipped.
- **Fog is a post-pass**, not per candidate pixel (one pass over the finished
  framebuffer), so it adds nothing to the hot loop when off (the default).

The remaining cost on a heavy scene (e.g. growth at ≈173k projected triangles)
is dominated by **projecting every triangle each frame** — resolution-independent
and bounded by the source mesh's triangle count. Reducing it further safely
(without violating "terrain coverage must not disappear") is a **screen-space /
distance LOD** that keeps a representative triangle per region rather than
dropping the smallest — a documented next step, not done in this pass.

## Near-plane handling (v1)

A triangle with any vertex at/behind the near plane (clip `w ≤ ε`) is **culled**
and counted (`skipped_invalid_projection_triangles`). This is the deterministic
v1 rule — no near-plane clipping yet — and it guarantees invalid projections
never produce NaN pixels. (Full near-plane clipping is the next fidelity step.)

## Terrain: coverage-preserving LOD, never holes

Terrain (large screen coverage ⇒ `CriticalCoverage`) is **never skipped as a
draw** — `critical_coverage_skipped` is the invariant and is zero in every
healthy frame. When a terrain draw exceeds the per-draw triangle cap
(`max_triangles_per_terrain_draw`, default 200 000), it is decimated by **keeping
the largest-area triangles and dropping the smallest**. At 320×180 the dropped
triangles are sub-pixel — invisible, and covered by their neighbours through the
z-buffer — so coverage is preserved with **no holes**. (Dropping *every Nth*
triangle, the earlier approach, punched gaps in the foreground and is gone.) The
cap sits comfortably above the count of triangles that can be visible at 320×180,
so normal terrain keeps every visible triangle; the cap only bites pathological
draws, and `terrain_triangles_decimated` reports when it does.

## Depth cues (`CanvasDepthCueProfile`)

On top of the flat-shaded z-buffer image the LowPolyFramebuffer profile layers a
set of **cheap, deterministic, subtle** Canvas-only depth cues that make the
scene read as 3D space. They are pure presentation policy
(`canvas_depth_cue_profile.rs` config, `canvas_depth_cue.rs` per-triangle math,
`canvas_post_pass.rs` per-pixel/per-object passes) — never game logic, never a
scene/resource import. The default profile is deliberately gentle.

The cues split by where they run, in this composed order:

1–2. **Base + draw colour** — resolved in conversion (`mesh colour × draw colour`).
3. **Fake directional lighting** (per triangle, baked into the flat colour). The
   model-space face normal (cross of two model edges already read for projection)
   is rotated into world space by the draw's `world` upper-3×3 — a *real* face
   normal, not a screen-space approximation (which degenerates to flat under the
   pixel-vs-NDC unit mismatch). `brightness = ambient + max(N·L, 0)·diffuse`,
   optionally banded, clamped. This is the strongest object cue (a flat cube
   becomes 3D; terrain slopes vary in shade).
4. **Height/elevation tint** (per triangle) — mix toward a low/high elevation
   colour by the triangle's world-Y within the draw's Y extent.
5. **Distance detail/colour falloff** (per triangle) — gently desaturate toward
   luminance by depth (far = slightly less saturated). CriticalCoverage is never
   *dropped* for distance — this is colour only.
6. **Depth fog** (per pixel, `apply_fog`) — mix toward the frame clear colour by
   final depth. NDC z is non-linear (visible depth clusters high), so fog starts
   **late** (`near 0.85`) and **gentle** (`strength 0.35`): only the far horizon
   recedes, near/mid terrain keeps its colour (no wash).
7. **Vertical colour grade** (per pixel, `apply_vertical_grade`) — a faint
   lower-screen darkening anchor by screen y.
8. **Contact shadows + outlines** (per important object,
   `apply_contact_shadows` / `apply_outlines`) — for **GameplayObject**-coverage
   draws only (never terrain/critical): a dark flattened ellipse anchored at the
   object's screen base (so it reads as a ground shadow), and a depth-weighted
   bbox silhouette (near objects stronger than far). Derived from the object's
   projected screen bbox + mean depth; cheap, deterministic, clipped.

Alpha is preserved by every cue. All ranges are clamped and NaN-safe. The cues
add a few ms over the bare rasterizer.

### Horizon silhouette — a documented seam (disabled)

`enable_horizon_silhouette` ships **off**: deriving a clean far-terrain
silhouette band needs neutral *far-terrain band* data the `FramePacket` does not
carry (it carries triangles + depth, not a per-column terrain-top horizon).
Adding it cleanly later means either a neutral horizon hint on the packet or a
backend per-column terrain-top scan; until then the profile knob + the
`horizon_silhouette_drawn` report field exist as the seam, and the fog already
recedes the far horizon.

## Debug overlays (opt-in, `None` by default)

- `None` — the shipping solid-filled look.
- `TriangleEdges` — per-pixel barycentric wireframe (depth-tested, so occluded
  edges hide).
- `DepthBuffer` — grayscale visualization of the z-buffer (nearer = brighter).
- `Bounds` — each triangle's screen bounding-box border.

## Reporting

`host::FrameSubmissionReport` carries the backend (`Canvas2d`), frame identity,
`submitted_draws`/`skipped_draws`, the `critical_coverage_skipped` invariant,
degraded materials/features, and the neutral `host::FrameRasterStats` (a
public-field telemetry DTO): framebuffer size, `projected_draws`,
`projected_triangles`, `culled_triangles`, `rasterized_triangles`,
`skipped_degenerate_triangles`, `skipped_invalid_projection_triangles`,
`candidate_pixels`, `depth_tested/written/rejected_pixels`,
`terrain_draws_preserved`, `terrain_triangles_decimated`, `rasterized_objects`,
`skipped_decorative_draws`, `budget_exhausted`, and the grouped
`host::FrameDepthCueStats` (`lit_triangles`, `height_tinted_triangles`,
`distance_falloff_applied_triangles`, `depth_fog_applied_pixels`,
`vertical_grade_applied_pixels`, `contact_shadows_drawn`, `contact_shadow_pixels`,
`outlined_objects`, `outline_pixels`, `horizon_silhouette_drawn`,
`depth_cue_profile_name`). A non-rasterizing hardware backend reports
`FrameRasterStats::ZERO`. No Canvas/browser types leak into `host`.

Per-frame **timings** (`projection`/`raster`/`blit` ms) are deliberately *not*
on the host report — they require a wall clock the deterministic core may not
read. The wasm facade logs them (with the counters) to the console via
`performance.now()`; native is a no-op, so the tested path stays deterministic
and timer-free.

## Non-goals (this profile)

CPU **shadow maps**, full texture sampling, PBR/material parity, SSAO,
post-processing, near-plane clipping (v1 culls), and a separate scene path are
explicitly out of scope. (Contact *shadow blobs* are a cheap ground-anchor cue,
not real shadow casting; fake per-triangle lighting is not a lighting system.)

## Tuning

`canvas_policy.rs`: `CanvasQualityPreset` dimensions (the resolution tiers); the
coverage fractions that bound terrain/critical classification.
`low_poly_raster_options.rs`: the default tier (`CanvasQualityPreset::Low`),
`DEFAULT_MAX_TERRAIN_TRIANGLES` (LOD cap), `DEFAULT_PIXEL_BUDGET`.
`canvas_depth_cue_profile.rs`: every depth-cue knob (fog range/strength, light
direction + strengths + banding, height tint colours/strength, contact-shadow
alpha/radius, outline alphas, falloff range, vertical-grade strength) — the
`low_poly_framebuffer()` default is the shipping subtle look.
`frame_packet_raster.rs`: `AREA_EPS` (degeneracy), `MIN_TRIANGLE_AREA` (sub-pixel
cull for non-critical draws). `software_rasterizer.rs`: `EDGE_EPS` (wireframe
overlay thickness).
