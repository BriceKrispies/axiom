# Terrain streaming stutter — diagnosis & options

**Status:** Diagnosed, not yet fixed (no bandaid applied). 2026-06-19.

## Symptom
A significant, isolated FPS hitch when the player walks far enough that the local terrain "zone" re-centers.

## Diagnosis (measured, not guessed)
The browser viewer renders the game world as **one finite terrain mesh window** (`AREA_HALF_M = 160` → a 321×321-vertex, ~320 m square). When the player crosses `RECENTER_THRESHOLD_M` (~80 m), the per-frame closure **regenerates the entire window synchronously in that one frame** (`build_terrain` → 103,041 × `sample_height_m`, then per-vertex normals, then a full GPU vertex/index buffer realloc + upload).

Two independent measurements:

| Measurement | Result |
|---|---|
| Native bench (`examples/bench_stream.rs`, release) — field eval only | **271 ms** for the 321×321 window (2,632 ns/sample) ≈ **16 dropped frames @60fps** |
| Same, the newly-exposed leading strip only (1 chunk slide) | 14 ms (≈0.9 frames) |
| Same, a single 16 m chunk | 0.73 ms |
| In-browser frame probe (JS RAF dt) while walking across an edge | a single **300 ms** frame; all other frames <40 ms |

The native 271 ms and the browser 300 ms agree (the ~30 ms gap is normals + buffer realloc/upload).

## Root cause (three compounding problems)
1. **All-at-once.** The whole window is rebuilt in one synchronous frame.
2. **Redundant.** Re-centering by one chunk only *exposes a thin strip*, yet the full window is regenerated — **19× more work than necessary** (271 ms vs 14 ms).
3. **Expensive per sample.** Each `sample_height_m` is ~2.6 µs because every vertex evaluates many noise layers (a mountainousness mask + a 5-octave **domain-warped** ridged-mountain field + a hill FBM + a fine FBM) plus the macro atlas IDW. Normals double the height reads.

The viewer also **bypasses the simulator's own `ChunkStore`** (which already implements per-chunk `request`/`unload` + diffs, per the audit's GW-E9 streaming model) and reinvented streaming as "one big mesh, full regen." That divergence is what created the all-at-once cost.

## Options (with honest trade-offs)

### 1. Raymarching / mesh-free implicit terrain (the user's idea)
Don't build a mesh. Terrain *is* `height(x,z)`; render by casting a ray per pixel and marching to the surface (like `examples/render_maps.rs`, or shadertoy planets), in a full-screen **fragment shader**.
- **Pros:** the zone/edge/regen concept disappears entirely — *the whole stutter category is gone*; walk forever; bounded memory; automatic distance LOD.
- **Cons / why it fights this project:**
  - The height function must run **per pixel × per march step in WGSL**. The noise layers port; the **macro atlas IDW** (region graph + per-region elevation) must be baked into a texture/buffer for the shader — the hard part.
  - **Determinism / single-source-of-truth break.** Growth's core bet (audit) is deterministic worldgen in Rust. Raymarching re-implements `height()` in WGSL → two sources of truth that diverge on GPU vs CPU float math. Worse for a *game*: the player **walks on / collides with the Rust terrain** but **sees the WGSL terrain** — they would mismatch. Fine for a flythrough; wrong for a walkable, editable world.
  - It's a **new GPU rendering path** that bypasses Axiom's instanced-mesh + ECS model and doesn't compose cleanly with mesh entities (player, trees, structures, dig edits) — you'd depth-composite raymarched terrain with rasterized meshes.
  - Heavy per-pixel cost (960×600 × tens of steps × many noise evals) demands an aggressively cheap shader.
  - **Verdict:** elegant and truly zone-free, but the biggest architectural departure and it breaks the render/collision single-source-of-truth. Reserve for a pure-terrain "globe/flythrough" mode, not the walkable game world.

### 2. Geometry clipmaps / GPU vertex displacement (AAA-standard)
Upload ONE fixed nested-grid mesh once (centered on camera); the **vertex shader displaces Y** by sampling a **height texture**. The texture is updated **toroidally** — only the newly-exposed edge texels are written as you move (the cheap 14 ms strip, or a compute pass), never the whole thing.
- **Pros:** no mesh rebuild ever → no geometry stutter; incremental texture update is the strip not the window; natural LOD rings; **composes with the existing mesh pipeline**; the texture can be filled by the **CPU `sample_height_m`** → determinism stays in Rust (one source of truth).
- **Cons:** needs vertex-shader height-texture sampling + a toroidal texture-update path in `axiom-windowing` (engine work, but an *extension* of the mesh pipeline, not a new path); normals from the heightmap.
- **Verdict:** the principled long-term answer for visual scale/LOD; moderate engine work.

### 3. Chunked incremental meshing (aligned with the existing `ChunkStore`)
Many small chunk meshes (16–32 m). Moving generates only the **few new leading-edge chunks** (0.7 ms each) and drops trailing ones — never the whole window. This is exactly the simulator's existing `ChunkStore` streaming (GW-E9), which the viewer currently bypasses.
- **Pros:** per-event cost tiny (a few ms, trivially spread across frames); **determinism stays in Rust**; matches Growth's *intended* architecture (reuse `ChunkStore` + diffs); conceptually simple.
- **Cons:** needs the engine to render **multiple distinct meshes** (or a pooled/growing buffer), not today's single-mesh `run_web` (`replace_geometry` replaces one mesh). Engine work, but bounded.
- **Verdict:** smallest per-event cost and least conceptual novelty; the most "fix it where it belongs" option (use the ChunkStore that already exists).

### 4. Amortize the full-window regen across frames — **the bandaid (avoid)**
Spread the 271 ms over N frames or prefetch ahead. Reduces the visible spike but keeps the redundant full regen. Explicitly *not* the fix the user wants.

## Recommendation
Primary: **(3) chunked incremental meshing, aligned with the simulator's `ChunkStore`** — it removes the stutter at the root (per-event work drops from 271 ms to a few ms), keeps the deterministic height function as the single source of truth in Rust (so render == collision), and stops the viewer from diverging from Growth's own streaming design. Upgrade path: **(2) clipmaps** when terrain view distance / LOD becomes the priority.

**Raymarching (1)** is the right tool for a non-interactive globe/terrain *showcase*, but for a walkable, editable, ECS-based game world it breaks render/collision parity and Axiom's mesh model — so not the primary terrain renderer here.

Independently of which is chosen, two field-level wins help every option: (a) **stop the 19× redundancy** (only generate what's newly exposed), and (b) **make `sample_height_m` cheaper** — cache the macro/atlas term per region instead of per vertex, drop octaves with distance, and use analytic derivatives for normals instead of extra height samples.

## Repro
- `cargo run -p axiom-growth --example bench_stream --release` — prints the regen cost table above.
- In-browser: inject a RAF dt probe, walk across an edge, read the max frame ms (a single ~300 ms spike).
