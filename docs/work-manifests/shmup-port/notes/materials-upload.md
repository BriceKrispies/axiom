# materials/upload — baking the nineteen generators and binding the result

Slice: item 1 of `10-convergence-plan.md`, app tier (`apps/shmup`).

Files written:

- `apps/shmup/src/materials/upload.rs` — the bake's write side.
- `apps/shmup/tests/materials_upload/capture.mjs` + `golden.json` — the oracle.
- `apps/shmup/tests/materials_upload_port.rs` — 9 tests, green.
- `apps/shmup/src/materials/system.rs` — `TextureSet::bake_at`, which `bake`
  now delegates to (bit-identical at the library's own size).
- `apps/shmup/src/materials/mod.rs` — `pub mod upload;`.
- `apps/shmup/src/scene/app.rs` — the bake runs at install, one albedo texture
  per library name, bound per batch.

## The finding that shaped the slice: a CPU bake cannot be a runtime path

The plan assumed the bake was the easy half. It is not. Measured natively,
`--release`, on this machine:

| what | time |
|---|---|
| one 512² surface (asphalt, 3 output passes) | 16.6 s |
| all nineteen at 512² + the two shared maps | **232 s** |
| all nineteen at their authored sizes (mostly 1024²) | **~930 s** |

That is ~15.5 µs per `owSurface` evaluation, and the library's own resolutions
need 57 million of them (three output passes × ~19 Mtexel). The source does the
same work in ~1.3 s.

The cause is structural, not a missing optimisation. `ow_hash22` is the classic
`fract(sin(dot(…)))` GLSL hash: one instruction on a GPU, an `f64::sin` on a
CPU, and a single surface evaluation makes hundreds of them. Worley alone is
nine cells × two hashes; a four-octave FBM is another thirty-two. No CPU rewrite
closes a gap that is really 1024²-way parallelism. Even the two legitimate wins
available — evaluating the surface once per texel instead of three times, and
dropping to `f32` — total maybe 5x against a 700x shortfall.

**The fix is the source's own: bake on the GPU.** `bake.rs`'s module doc already
specifies it (WGSL emission of `owSurface`, a half-float scratch height target,
the Sobel as a fragment shader, and its own `sobel` written to be line-for-line
portable). Alternatively — and more in the spirit of `axiom-surface` — the
generators could *be* a surface program and never bake at all, evaluated
per-pixel in the material shader. Either is an engine-scale change and neither
is this slice.

So `upload.rs` splits in two:

- `bake_library(quality, size_cap, names)` — the faithful full bake, all four
  map kinds. This is what a GPU bake would replace, and what the golden pins.
- `bake_albedo_maps(names, size_cap)` — albedo only (one evaluation per texel),
  size-capped. `RUNTIME_BAKE_SIZE = 64`, which costs ~1.2 s native and is the
  part that fits in a page load. Its doc carries the measurements above so the
  next agent to raise it knows the curve is quadratic.

64² over a 2 m tile is 3 cm per texel — coarse, but it is the real generator's
colour field, and the material shader's macro, weathering and cavity layers are
per-pixel and cost nothing, so they carry the high frequencies on top of it.

## What is real now, and what is not

**Real, visible today:** every level batch whose palette key names a library
surface binds a baked albedo (the generator's own sRGB colour field, height in
alpha) through `Material::with_custom_texture`, sampled anisotropically. The
street is textured rather than nineteen flat palette colours.

One correctness change rides with it. `level::key_albedo` falls back to a
neutral mid grey `0xb0b0b0` for a palette entry with no `tint`, standing in for
"the material's own generator owns the colour" — its own words. Now that the
generator's colour is actually uploaded, that stand-in has to go, or every
untinted surface is darkened by exactly the amount the stand-in was invented to
supply. `scene::app::textured_base_color` makes an untinted key white, which is
what the source constructs the material with (`index.js:199`, `color:
0xffffff`). A *tinted* key keeps its tint, because the source multiplies too.

**Produced but unbindable:** the normal map, the ORM+height map, the shared
detail tile and the macro field. All four are computed by `bake_library` and
packed for the exact bindings the backend declares — and there is no way to
hand them to the engine. See below.

## The engine contract change this needs

Today:

- `axiom_host::MaterialTexture { material_id, width, height, pixels, sampling }`
  is the **only** backend-neutral per-material pixel carrier, and it carries
  albedo alone.
- Normal maps travel beside it as a bare `&[(u64, u32, u32, Vec<u8>)]`, on the
  offscreen path only. `live_gpu_binding.rs:391` passes `&[]` — *the live
  browser arm has no normal-map lane at all*.
- `RunningApp::material_textures()` (`modules/axiom/src/app/resources.rs:71`)
  builds the `MaterialTexture` list, and `Material` carries one texture id
  (`custom_texture: u64`).
- `scene_renderer.rs:1113-1155` therefore binds neutral 1x1s at 4, 5 and 6.

**The smallest honest extension**, at the lowest correct layer:

1. **`crates/axiom-host/src/material_texture.rs`** — `MaterialTexture` gains
   four optional map payloads beside its albedo: `normal`, `orm_height`,
   `detail`, `macro_field`, each an `Option<MapPixels { width, height, pixels }>`
   with builder setters and accessors. This is the lowest correct layer by the
   type's own module doc: "it is why this lives in the `host` layer: it is the
   one place `axiom`, `axiom-windowing`, `axiom-gpu-backend` and
   `axiom-canvas2d-backend` can all name a type." It also **collapses the
   parallel `normals: &[(u64, u32, u32, Vec<u8>)]` slice into the carrier it
   should always have been** — the same "a fifth positional field on that tuple
   would have been unreadable" argument the doc already makes, and the reason
   the live arm's normal maps are `&[]` today is precisely that the second
   slice never got plumbed.

2. **`modules/axiom/src/material.rs`** — `Material` gains four `u64` ids
   (`normal_texture`, `orm_texture`, `detail_texture`, `macro_texture`) with
   `with_*` builders, resolved through the **existing** `custom_textures` store
   that `RunningApp::add_texture_data` already fills. No new store, no new
   registration API, `Material` stays `Copy`.

3. **`modules/axiom/src/app/resources.rs`** — `material_textures()` resolves
   those four ids into the new `MaterialTexture` fields.

4. **`modules/axiom-gpu-backend/src/scene_renderer.rs`** — `SceneRenderer::new`
   drops its separate `normals` parameter and reads all five maps off
   `MaterialTexture`, keeping today's neutral 1x1 for any map a material did not
   author. `live_gpu_binding.rs` and `offscreen.rs` then shrink rather than
   grow, and **the live browser arm gets normal maps for free** — it already
   passes `materials`.

That is one new value type, four fields on two existing types, one signature
that gets shorter, and no new lane.

### A second, smaller gap the packing exposed

Binding 5 (`material_detail_tex`) is documented `(normal.rgb, height.a)` — four
channels for what the source samples through **two** textures and **five**
scalars: `detailNormal.xyz`, `detailAlbedo.r` (the micro albedo/roughness) and
`detailAlbedo.a` (the micro height). `compose.rs` reads `d_tex.r` for the
source's `(dTex.r - 0.5) * 1.25` micro-albedo term, but with the documented
packing `d_tex.r` is the normal's *x*, which on a near-flat detail normal is
~0.5 — so that term contributes ~nothing and half the micro layer is dead even
once the map is bound.

Two ways out, both in `modules/axiom-gpu-backend`:

- Pack `(normal.xy, micro_albedo, height)` and reconstruct
  `z = sqrt(1 - x² - y²)` in the shader. Four channels suffice; the detail
  normal is tangent-space with `z > 0` by construction, so nothing is lost. One
  line in `compose.rs`, no new binding.
- Or add binding 7 for the second detail texture, matching the source exactly.

The first is smaller and I would take it, but it is a shader change and belongs
to whoever owns `material_shader/`. `upload.rs` writes the packing the backend
*currently* documents and records the divergence rather than guessing.

## The golden

`capture.mjs` is a **real oracle**: it imports the original `MaterialSystem` and
runs it under Node against the same stub `WebGLRenderer`
`../materials_system/capture.mjs` uses, intercepting
`TextureForge.prototype.build` (not the instance — `sys._forge` does not exist
until `_tryBuild` runs inside `init`, and `buildDetail`/`buildMacro` are called
in that same breath) to record the *complete* definition behind every bake:
key, size, seed, `worldSize`, `relief`, `tintA`, `tintB`, `param`, and the three
output flags. Twenty-one builds at each of `ultra` and `low`: the two shared
maps first, then nineteen materials in `LIBRARY` order.

`../materials_system/golden.json` already pins the bake *list* (`qualityBakes`);
this one pins the bake *inputs*, which that one does not carry (no `worldSize`,
no `relief`, no tints, no `param`) and which are exactly what the CPU bake
reads.

**What the oracle cannot reach** is the texels: the source writes them with a
real GPU rasteriser into a `THREE.WebGLRenderTarget` and Node has no GL context
— the "GLSL in a JS string has no oracle" case from `07-fanout-brief.md`, one
step along. Both halves of that gap are covered elsewhere:

- the float values are the ported `owSurface` bodies and the Sobel, pinned by
  `materials_surfaces_*_port.rs`, `materials_noise_port.rs` and `bake.rs`'s own
  analytic Sobel tests;
- the float→byte write is not a choice. `THREE.WebGLRenderTarget` defaults to
  `UnsignedByteType`, so it is the OpenGL ES 3.0 fixed-point conversion for an
  8-bit UNORM colour attachment: `round(clamp(f, 0, 1) * 255)`.
  `quantize_matches_the_unorm8_write_rule` pins that expression against an
  independently-written reference over 2053 values including both ends, the
  exact half, and NaN.

Tolerance: keys, sizes, counts, orders and flags exact; `worldSize`, `relief`,
`seed`, tints and `param` at 1e-6 relative, the figure
`materials_system_port.rs` establishes for `LIBRARY` values that pass through
the port's `f32` storage.

## `TextureSet::bake_at`

`bake()` now delegates to `bake_at(self.size, true, true)`, which is
bit-identical. The two knobs `bake_at` adds — the size and each output pass —
are the only two that change what a CPU bake *costs*, and `upload.rs` is its
only caller. The source has no such variant, which is noted at the site.

## Not done

- The three non-albedo maps are unbound (contract change above).
- The bake resolution is 64², not 1024² (GPU bake above).
- `SurfaceIn.view_distance`, `front_facing`, the vertex mask lane and
  `SurfaceOut.ao` are item 2 of the convergence plan, untouched here.

## Verified

`cargo test -p axiom-shmup --release`: **1388 passed, 1 failed** — the failure is
`world_system_port::interior_furnishing_is_the_single_upstream_blocker`, an
untracked test file from a concurrent agent's world/interiors slice, unrelated
to this one. All 9 `materials_upload_port` tests pass.

Browser (`localhost_servers restart shmup` → Playwright, WebGPU, 1280x720):
console clean, and the street is visibly textured — grain on the plaster and
brick, streaking on the timber awnings, mottled patches and gravel scatter on
the road — against the flat palette colours it had before. Screenshot:
`scripts/.playwright-controller/screenshots/bake-textured-street-163822.png`.

Two hours of that run were spent waiting on concurrent agents: `fx/system.rs`
and then `axiom-gpu-backend`'s wasm-only `live_gpu_binding.rs` (an in-flight
G-buffer change) were parked non-compiling, and the wasm bundle cannot be built
past either.
