# `ai/textures.js` → `apps/shmup/src/ai/textures.rs`

Source: `C:/dev/Claude-of-Duty/src/ai/textures.js:1-951` (951 lines).

Files written:

| file | what |
|---|---|
| `apps/shmup/src/ai/textures.rs` | the port |
| `apps/shmup/tests/ai_textures_port.rs` | the golden test |
| `apps/shmup/tests/ai_textures/capture.mjs` | the capture, run under Node 24 |
| `apps/shmup/tests/ai_textures/golden.json` | 485 KB, byte-reproducible |

`mod.rs` line the orchestrator must add (I did not touch it):

```
apps/shmup/src/ai/mod.rs:    pub mod textures;
```

`ai/mod.rs`'s doc table lists `textures.js` under "deliberately not in this
slice" — that paragraph now needs editing too, but it is a shared file so I left
it alone.

## Answer to the coordinator's `soldier.rs` question

**`CLOTH_TILE` is a `const`, exactly as `soldier.rs` assumes.** The source is
`export const CLOTH_TILE = 0.78;` (`textures.js:228`) — a literal, not computed,
not baked. The port is `pub const CLOTH_TILE: f64 = 0.78;`. No call-site fix
needed.

Other symbols `soldier.js` might reach for from this module, all also plain
consts: `KIT_CAL` (`f64`), `CLOTH_BUDGET` (`ClothBudget`), `RIM` (`RimParams`),
`CAMO_ARID` / `CAMO_WOODLAND` / `CAMO_URBAN` / `CAMO` (`CamoConfig`s),
`GLASS` (`GlassParams`).

## What is ported

Everything arithmetic — `textures.js:26-813`:

- `TileNoise` (`h` / `n2` / `fbm` / `ridge`) and its 4096-entry table build.
- `srgb`, `smooth`, `mix`, `mix3`, `cellDist`, `ridgeLine`, `lum3`.
- `bake` and `bakeDetail` — the full CPU bake including the Sobel and the 8-bit
  quantisation.
- `CAMO`, `CLOTH_TILE`, `CLOTH_BUDGET`, `KIT_CAL`, `RIM`, `budgetFor`.
- `garmentRelief`, `camoTexel`, `measureCamo`, `applyBudget`, `makeCamoSampler`.
- `SoldierMaterials`'s constructor: all nine bakes (camo × N, nylon, plate,
  skin, polymer, steel, rubber) plus the two detail tiles, and `camoStats`.

## What is NOT ported, and why

| source | why |
|---|---|
| `dataTexture()` (`98-108`) | `THREE.DataTexture`. The *pixels* are ported; the wrapper's settings are carried as data (`TextureData.srgb`/`.anisotropy`) or as associated consts (`WRAP_REPEAT`, `GENERATE_MIPMAPS`, `MIN_FILTER`, `MAG_FILTER`). |
| `SoldierMaterials.get` (`827-855`) | `THREE.MeshStandardMaterial` construction + a string material cache key. No CPU form. |
| `_attachShader` (`867-923`) | GLSL spliced into Three's fragment shader via `onBeforeCompile`, plus `customProgramCacheKey`. **No oracle** — see below. |
| `glass()` (`926-943`) | Another `MeshStandardMaterial`. Its constants are ported as `GLASS`. |
| `dispose()` (`945-950`) | GPU resource release. |
| `this.bakeMs` (`537`, `807`) | `performance.now()` — wall-clock. Deliberately dropped; it is nondeterministic and the determinism rules forbid it. |

### No `CanvasRenderingContext2D` anywhere

Worth stating explicitly because the brief asked: this file never touches a
canvas. Every pixel is computed arithmetically into a `Uint8Array`. The bake is
therefore a **genuine oracle** end to end — the capture script imports the
original module, constructs a real `SoldierMaterials`, and reads
`texture.image.data` back out. Three.js is only a passive wrapper around a
buffer the CPU already filled.

### The GLSL that has no oracle

`_attachShader` injects three fragment-shader fragments (the rim term, the
detail roughness delta, the detail normal blend). Shader source held in a JS
string never runs anywhere but a browser GPU, so there is nothing to call.

**I did not transcribe it into executable Rust.** A sibling slice reportedly hit
two bugs that appeared *identically* in a port and in the "independent" JS
transcription meant to check it, because one reading produced both. There is no
way to defend against that with one pair of eyes, so I did not create the second
transcription at all. Instead:

- the three GLSL bodies are copied **verbatim** into the doc comments on
  `rim_uniform` and `DetailBlend` (verified character-for-character against the
  source text by script), for the render/`gpu-backend` workstream that will
  eventually write the WGSL;
- only the *uniform values* those shaders consume are ported as data and pinned:
  `rim_uniform(scale) = [RIM.strength * scale, RIM.edge, RIM.power, 0]`, and
  `DetailBlend`'s `?? 8 / ?? 0.7 / ?? 0.2` defaults.

Nothing in the port executes a reading of that GLSL, so nothing can be silently
wrong in it.

## Why this does not reuse `materials/`

I checked `materials/noise.rs`, `materials/bake.rs`, `materials/masks.rs` and
`materials/surfaces/` first. **There is nothing to share**, and forcing a share
would have changed the numbers:

- `materials/noise.rs` ports `glsl/noise.js` — sin-free Dave-Hoskins hashes
  evaluated analytically in a shader (`ow_fbm`, `ow_worley`, …). `TileNoise` is
  a JS-side value noise over a 4096-entry table drawn from the `Rng` stream.
  Same English description, entirely different algorithm and numbers.
- `materials/bake.rs` bakes at texel **centres** (`(x + 0.5) / size`) into `f32`
  textures with a `relief / world_size` Sobel scaled by `0.125` and a naive
  `normalize3`. `ai/textures.js` bakes at texel **corners** (`x / size`) into
  8-bit `Uint8Array`s with a `normal_scale * 0.17` Sobel and V8's `Math.hypot`.

They are two independent procedural-texture systems in the same game. The
module doc says so at the top so the next agent does not re-litigate it. I did
not touch anything under `src/materials/`.

## Traps checked, by name

**`Float32Array` / `Uint8Array` storage width** — three hits, all real:

1. `TileNoise.tab` (`textures.js:32`) is a `Float32Array`. Every `rng.float()`
   is rounded to `f32` on store and read back rounded. Ported as `Vec<f32>`
   widened to `f64` per read. (Aside worth knowing: `Rng::float`'s max draw
   `(2^32-1)/2^32` rounds **up to exactly `1.0f32`**, so `n2` returns `[0, 1]`
   inclusive, not `[0, 1)`.) `perm` is `Uint16Array` → `Vec<u16>`.
2. Both bakes' height scratch (`textures.js:119`, `178`) and `bakeDetail`'s
   roughness scratch (`179`) are `Float32Array`. The Sobel reads the **rounded**
   heights. Ported as `Vec<f32>`. An all-`f64` port moves normal-map texels by
   whole 8-bit steps.
3. The three output maps are `Uint8Array`, so every channel rounds on store —
   see the next trap.

**`Uint8Array` is not a clamp and not `as u8`** — ECMAScript `ToUint8` truncates
toward zero then takes the result **modulo 256**; `Uint8ClampedArray` (which the
source does *not* use) is the clamping one, and Rust's `as u8` **saturates**.
`-1.5` stores as `255`, not `0`. Ported as `to_u8`, verified against Node:
`[254.999, -0.4, -1.5, 255.0, 256.7, NaN]` → `[254, 0, 255, 255, 0, 0]`, and
pinned by `to_u8_matches_the_uint8array_store`. (Every write site here is
provably in `[0, 255]` by construction, so the wrap never fires — but the wrap
is the semantics, and `as u8` is a different function.)

**`Math.hypot` is not `sqrt(x*x + y*y + z*z)`** — this one bites hard. V8
normalises by the largest magnitude and **Kahan-compensates** the sum of
squares. Measured under Node 24 over 200 000 random `(x, y, 1)` triples of the
shape the Sobel actually produces: the naive form disagreed with `Math.hypot` on
**50 738 (25 %)**; the transcribed V8 algorithm on **0**. A quarter of every
normal map would have been wrong. Ported as `math_hypot3`; the golden carries
both `hypot` and `naive` per case so the test asserts the port matches one and
genuinely differs from the other.

**`sign` is not `signum`** — no `Math.sign` and no zero-sign dependence anywhere
in this file. Not applicable.

**Float arithmetic is not associative** — nothing was tidied. Verified two ways
by script rather than by eye:

1. the ordered multiset of numeric literals in every ported block matches the
   source's exactly, with the only differences being the `gain = 0.5` /
   `oct` default arguments Rust must pass explicitly (JS defaults them) and
   const-hoisted initialisers;
2. an identifier-stripped operator skeleton of each generator body matches,
   with differences only where JS and Rust *must* differ (`Math.max(a,b)` →
   `a.max(b)`, `a ? b : c` → `if a {b} else {c}`, comma-declarations split).

Every division in the source maps one-to-one to a division in the port (checked
by grep): **no reciprocal-multiplies, no re-associated multiply chains.**

**An enum used as a table index is order-dependent** — `CAMO`'s declaration
order (`arid`, `woodland`, `urban`) and `SoldierMaterials.sets`' insertion order
are both pinned. `sets` is a `Vec<(String, BakedSet)>`, not a map, so insertion
order is structural. The 64 px golden deliberately passes `camo: ['urban',
'arid']` — reversed — so a port that sorted or hard-coded the pattern order
fails.

**Dead computation is still part of the source** — nothing dropped. Notable
carry-overs: `TileNoise::FBM_DEFAULT_OCT` / `RIDGE_DEFAULT_OCT` (default
arguments no call site in the file uses), `CamoSampler::with_budget` (the
`B = budgetFor(cfg)` parameter no call site passes), and
`Texel::MEASURE_INIT` (`measureCamo`'s scratch initialiser, which has
*different* defaults from `bake`'s reset and is never observable because
`camoTexel` overwrites every field).

**`Math.floor` on negatives / the `((ix % p) + p) % p` wrap** — `garmentRelief`
samples `nz.ridge(u + 3.1, v - 2.2, 52, 2)`, so negative lattice coordinates are
real. Transcribed literally (JS `%` and Rust `%` are the same truncated
remainder) rather than collapsed to `rem_euclid`, and pinned with negative
`hSamples`.

**A matching count is not proof** — so the 16 px golden is a *complete* pixel
dump, not a sample: every byte of every map.

## Divergences, all documented at the site

1. **`smooth` uses `f64::clamp`, not `.max(0.0).min(1.0)`.** JS
   `Math.min(1, Math.max(0, t))` propagates `NaN`; Rust's `max`/`min` swallow it.
   `f64::clamp` propagates it. Unreachable (no call site passes `e0 == e1`; the
   narrowest `ridgeLine` width is `0.009`), but the version that cannot diverge
   is free.
2. **`Math.max(1e-6, x)` written as `x.max(1e-6)`** in `apply_budget` — argument
   order flipped, identical for non-`NaN`.
3. **`detailNylon` calls `cell_dist` where the source inlines its body**
   (`textures.js:794-795`). The two expressions are character-for-character the
   same; commented at the site.
4. **`bake`'s `fn(u, v, out, x, y)` keeps `x`/`y`** even though no generator
   reads them (`_x`, `_y` in every closure).
5. **`SoldierMaterials` returns `Vec<(String, …)>` rather than JS objects**, to
   keep insertion order structural. `set()` / `detail()` / `camo_stat()` return
   `Option` where the source's `get` throws.
6. **`makeCamoSampler`'s closure-with-a-property becomes `CamoSampler`** — a
   struct with a `sample()` method and a public `src_mean` field.
7. **`opts.size ?? 512` etc. live in `SoldierOpts::default()`.** Rust has no
   default arguments.

## What is pinned, and at what tolerance

Golden built by `node capture.mjs > golden.json`, importing the original module.
Byte-reproducible (verified by two runs + `cmp`).

**Exact `f64` equality** — everything built only from `+ - * /`, comparisons and
`sqrt`, which are IEEE-754 deterministic:

- `TileNoise.tab` / `perm` entries at 14 indices **plus whole-table sums** (so a
  single wrong draw anywhere is caught);
- `_h` at 14 lattice points including negatives and wraps;
- `n2` at 88 points, `fbm` at 248 (every `(period, oct, gain)` triple the file
  uses, plus the defaults), `ridge` at 64;
- `smooth` / `mix` / `mix3` / `cellDist` / `ridgeLine` / `lum3` tables;
- `budgetFor` for all three patterns plus the two `undefined` forms;
- `math_hypot3` (69 cases) and `to_u8` (18 cases).

**Exact `u8` equality** — every baked pixel:

- **`{ size: 16 }`, defaults otherwise**: a *complete* dump of all 8 sets × 3
  maps + 2 detail tiles = 26 624 bytes, byte for byte, plus per-channel
  min/max/sum/mean and an FNV-1a hash per map. This also exercises
  `anisotropy ?? 8` and `camo ?? ['arid','woodland']`.
- **`{ size: 64, anisotropy: 4, camo: ['urban','arid'] }`**: an 8×8 texel grid
  per map plus whole-image per-channel min/max/sum/mean and the FNV-1a hash.

  *Caveat, stated honestly:* the bake reaches `pow` (via `srgb`) and `sin`,
  which are not bit-guaranteed across V8 and Rust's libm. Truncation to 8 bits
  hides a sub-ULP difference unless a channel lands within ~1e-15 of an integer
  boundary. If integration produces a single channel off by exactly 1, that is
  what happened — not a port bug. Everything upstream of the quantisation is
  pinned at float precision separately, which is where a real bug would show.

**`1e-12` relative** (this port's established transcendental figure) — the
float-precision paths that reach `sin`/`pow`:

- `camoTexel` (all 7 out-fields) at 8 UVs × 3 patterns. Its `out.h` **is**
  `garmentRelief(nz, u, v)` (`textures.js:480-481`), so the private relief
  function gets a genuine oracle;
- `measureCamo` at `n = 96` (the default) and `n = 12`, all four fields;
- `CamoSampler.src_mean` and the remapped texels — the delta between those and
  the raw ones is `applyBudget`, so it is pinned without being transcribed;
- `camoStats` (mean/sd/min/max/was) for both bakes.

## Not pinned

- **The default 512 px bake.** Nine 512×512 tiles of this noise is minutes of
  debug-build CPU. `SoldierOpts::default()` is pinned as data and the pipeline
  is pinned at 16 px and 64 px.
- **`dsize = min(512, size)`'s `min` branch** — needs `size > 512`, which needs
  the expensive bake above. `DETAIL_MAX_SIZE` is pinned as a constant.
- **The seven module-private one-liners are copied, not imported**, in the
  capture script (they have no export). That is a JS→JS copy of 1-3 lines each,
  clearly labelled in the script header, and every one of them is *also* pinned
  transitively through `camoTexel` / `makeCamoSampler` / the full bakes, which
  are genuine.

## Things a future reader should know

- **Urban camo does not hit its stated budget.** `CAMO.urban.budget` is `0.083`
  but the measured post-bake mean is `0.0956` (arid: target `0.104`, measured
  `0.1037`). That is not a port bug — `applyBudget` clamps every texel into
  `[0.040, 0.152]` *after* a `1.5×` contrast stretch, and the darkest pattern
  loses the most to the bottom clamp, pulling the mean up. It is in the golden
  and pinned; the source's own `selftest.mjs` audit would report the same.
- **One texel-flipping risk, flagged in the code.** Both detail tiles decide the
  weave phase with `Math.sin((tu + tv) * PI) > 0`. At texels where `tu + tv` is
  an exact integer that is `sin(k·PI)` ≈ `1e-14`, and its *sign* selects between
  two different height expressions. Both engines round the argument identically
  and their `sin` error is orders of magnitude below `1e-14`, so the branch
  agrees — but it is the one place in this file where a libm difference could
  flip a texel rather than nudge it.
- **`bake`/`bake_detail` take `&mut BakeFn<'_>`** (a `dyn FnMut`) because the
  camo generator mutates captured statistics accumulators. Every call site
  annotates its closure's parameters, which is what makes the higher-ranked
  `&mut Texel` lifetime infer cleanly. If integration hits a closure-inference
  error there, that is the shape to preserve.

## Compliance

Not built, not tested, not committed, nothing staged. No `mod.rs` / `lib.rs` /
`Cargo.toml` / `app.toml` touched. Nothing under `src/materials/` touched.
`C:/dev/Claude-of-Duty` untouched (read only). `serde_json` with
`arbitrary_precision` is already a dev-dependency, so the test needs no manifest
change.
