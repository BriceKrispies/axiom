# `materials/index.js` → `apps/shmup/src/materials/system.rs`

The materials **facade**: the bake cache, the material cache, the name/alias
resolution in front of them, the parameter merge behind them, and the
scratch-release idle timer. 353 source lines.

| | |
|---|---|
| source | `C:/dev/Claude-of-Duty/src/materials/index.js:1-353` |
| module | `apps/shmup/src/materials/system.rs` |
| test | `apps/shmup/tests/materials_system_port.rs` |
| golden | `apps/shmup/tests/materials_system/golden.json` (203 KB) |
| capture | `apps/shmup/tests/materials_system/capture.mjs` |

The nineteen generators, the noise library, the mask bake and the CPU bake
pipeline were already ported and golden-verified. This slice is only what sits
on top of them.

## The golden is a real oracle, not a transcription

`capture.mjs` **imports and runs the original `MaterialSystem`**, together with
the real `LIBRARY`, `DEFAULT_PARAMS` and the world `PALETTE`, under Node.

That is possible because `TextureForge` — the only thing in the file that needs
a browser — touches exactly five things on a `WebGLRenderer`:
`capabilities.getMaxAnisotropy()`, `extensions.has()`, `getRenderTarget()`,
`setRenderTarget()`, `render()`, plus the `autoClear` flag. A twelve-line stub
covers all of them. Every render target is still a real
`THREE.WebGLRenderTarget`; only the draw is a no-op, and the facade never reads
a texel, so nothing the cache layer does is affected.

`performance.now()` is pinned to `0`. Both `console.info` lines in the file
(`index.js:91`, `index.js:152`) are gated on elapsed milliseconds, so freezing
the clock makes the log transcript a function of the source alone. The capture
is byte-reproducible; re-running it produces an identical file.

**One thing is transcribed**, and only because it is unreachable: the merged
`threeProps` map (`index.js:193-195`). It is a local inside `get()`, it is
never stored on the material, and three.js silently absorbs an assignment that
equals a property's default (`specularIntensity: 1` on glass), so it cannot be
recovered by diffing the finished material either. The capture re-types the two
spreads and evaluates them over the *imported* `LIBRARY`, so only the merge
itself — three lines — is second-hand. The driven half is also captured
(`applied`: a property-by-property diff of the finished material against a bare
one from the same constructor), and it is what pins `applyProps`'s
sRGB→linear Color conversion.

Instrumentation observes rather than predicts: hits and misses are read off
`_sets.size`/`_materials.size` deltas and a wrapper counting
`TextureForge.build` calls, never by re-deriving a key (which would both
duplicate the code under test and perturb `_missing`, the once-per-name gate on
the unknown-surface warning).

## The cache is the whole file, and it is two different keys

```text
_sets      bakeKey = `${key}|${size}|${seed}|${tintA}|${tintB}|${param.join('_')}`
_materials matKey  = `${key}|${stableKey(opts)}`
```

The bake key names only what changes texels; the material key names every
option. The measured consequence, captured by driving the real
`world/palette.js` through the real system in order: **the level's 46 palette
entries — five plasters, four woods, three fabrics, all differing only in
tint/scale/weather — are 46 distinct materials over exactly 19 bakes.** Get the
key wrong in either direction and the engine silently bakes more (slow) or
fewer (wrong) textures than the original.

Both keys are strings assembled by JavaScript coercion, so the port reproduces
the coercion rather than approximating it:

- **`js_number`** is ECMAScript `Number::toString`, not Rust `Display`. They
  disagree at both ends of the range (`1e-7` vs `0.0000001`, `1e+21` vs
  twenty-two digits) and on negative zero (`0` vs `-0`). The shortest
  round-trip digits come from Rust's `LowerExp`, which uses the same algorithm
  JavaScript does, and are then re-laid-out by the spec's five cases. Pinned
  against the captured keys, including a `1e-7` and a `1e21` case put into the
  capture specifically to exercise the exponential arms.
- A hex colour is a JS `Number`, so `tint: 0xcfc0a4` keys as `tint=13615268`,
  in **decimal**. Same for `tintA`/`tintB` in the bake key.
- `stableKey` sorts the **top level only**, then `JSON.stringify`s each value.
  A nested object is stringified in **insertion order**, so
  `three: { opacity, envMapIntensity }` and `three: { envMapIntensity,
  opacity }` are two different materials. `MaterialOpts` and `OptValue::Obj`
  are therefore insertion-ordered `Vec<(String, _)>`, and only `stable_key`
  sorts. `nested_option_order_is_part_of_the_identity` pins it on its own.
- `JSON.stringify(undefined)` returns the JS value `undefined`, which the
  template literal coerces to the literal text `undefined` — the one place a
  key reads as an unquoted word. `OptValue::Undefined` is a distinct variant
  from `Null` for this reason *and* two others: a key present with value
  `undefined` still overrides the merged default (the object spread copies
  it), and it is still nullish for `??`.
- A defaulted value is still a distinct entry: `{ vertexMasks: false }` and
  `{}` are two materials.

### Order-preserving goldens

The capture emits every `opts` bag twice: once as plain JSON (readable) and
once as an `optsEnc` tagged tree (`{t:'obj', v:[[k, …]]}`). `serde_json`
without the `preserve_order` feature parses an object into a `BTreeMap` and
**sorts the keys** — which would destroy exactly the property being tested. The
Rust test decodes `optsEnc`, never `opts`.

## Source defects, ported and pinned

1. **`worldSize` and `relief` are not in the bake key.** `_bakeKey` names five
   fields; the def passes seven. Both missing ones are real inputs to the bake
   (`relief / worldSize` is the Sobel's slope), so
   `getTextureSet('concrete', { bake: { worldSize: 9 } })` after the default
   bake exists hands back the *cached* set, built at 2.5 m — the override is
   silently dropped. Captured (`override-worldSize`, `override-relief`) and
   pinned by `worldsize_and_relief_overrides_are_silently_dropped`.
2. **The "built without textures" warning is unreachable.** `index.js:214`'s
   `else if (!this._warned)` can only fire when there is no texture set, and
   the only thing that produces no texture set — `_tryBuild` failing — set
   `_warned` on the line above. The golden confirms the message never appears.
3. **`medium` quality bakes at `high`'s resolution.** `_size` scales by the
   preset and then snaps to the nearest power of two;
   `round(log2(768)) == 10`, so every 1024 base and every 512 base comes back
   unchanged. Only `low` (0.5, an exact halving) reduces anything. The golden
   captures all nineteen bake sizes for all three scalars and
   `medium_quality_bakes_at_high_quality_sizes` asserts medium == ultra and
   low != ultra.
4. **`mat.transparent = false` is inert.** `index.js:213` writes `false` over a
   material whose constructor already defaulted it to `false`. What survives is
   whatever `applyProps` copies out of `threeProps`. Ported as the outcome,
   with the dead line named at `ThreeProps::transparent`.
5. **The GPU program cache leaks uniforms between bakes — NOT ported.**
   `TextureForge._material(key, glsl)` caches one `ShaderMaterial` per surface
   key, and `build` assigns `uTintA`/`uTintB`/`uParam` only `if (def.tintA)`
   etc. So a second bake of the same surface *without* a tint reuses the
   previous bake's tint. This is reachable (`{ bake: { param: undefined } }` on
   concrete keys as a new set but would sample with the leaked `[1,0,0,0]`) and
   it is a genuine defect, but it is stateful GPU-program plumbing with no
   analogue in a stateless CPU bake: `TextureSet::bake` passes every uniform
   explicitly, every time. Recorded here rather than reproduced.

## Divergences, and why

**Texels are baked on demand, not at cache-fill time.** `TextureForge.build`
is four full-screen GPU draws with no readback; the port's `bake::bake` is a
CPU loop over `size²` texels, and the library's nineteen 1024²/512² surfaces
are roughly 15 million noise-stack evaluations. So `get_texture_set` caches the
*descriptor* — every input `build` consumes, resolved exactly as the source
resolves it, under exactly the source's cache key — and `TextureSet::bake()`
runs the pixels when a caller wants them. Cache identity, bake count and bake
order are unaffected; only *when* the arithmetic happens moves. `SharedMaps`
gets the same treatment for `buildDetail`/`buildMacro`.

**The renderer is a capability record, not a renderer.** `_renderer()` reads an
injected renderer, else `ctx.peek('render')?.renderer`. There is no render
subsystem in this crate yet, so only the injected arm exists — as
`RendererCaps`, carrying the single number `TextureForge` reads
(`getMaxAnisotropy()`). `MaterialSystem::set_renderer` is the seam for when a
render subsystem lands. The no-renderer path is fully exercised (deferral
warning, null texture set, textureless material, no uniform block, `tune` and
`update` as no-ops).

**`extendMaterial` is out of slice.** `shader.js` is a separate port (890 lines
that become WGSL in the GPU backend). What this facade owns of it is the
mutable subset `tune` and `setGroundLevel` write to — `owTile`, `owTintCol`,
`owParallaxP`, `owGroundY`, `owNormalAmp`, `owWeatherP` — which is
`LiveUniforms`, captured from the *real* `extendMaterial` running in Node and
pinned. The rest of the uniform block and the whole `#define` set land with
`shader.js`.

**`applyProps` has no counterpart.** It is a THREE-specific guard (assigning a
hex over a `THREE.Color` property replaces the object and produces a black
material, so colour-valued props must go through `.set()`). There is no THREE
material here; the facade's output is the merged `ThreeProps` map as data, and
whoever binds it to a renderer owns the guard. The one behaviour `applyProps`
adds — the sRGB→linear decode — is pinned by
`colour_valued_three_props_decode_to_the_captured_linear_value`, which decodes
the raw hex with the same `hex_to_linear_tint` the tint uniform uses and
compares against the captured linear triple at `1e-12`.

**`buildDebugBoard`'s geometry is not ported.** `SphereGeometry(radius, 64,
48)`, `BoxGeometry(0.92, 0.92, 0.14, 8, 8, 2)` and the `bakeMasks(panel, {wear:
1, grime: 0.9})` over it are Three.js scene-graph construction. The placement
arithmetic and the two material requests per cell — which is the part that
exercises this file, and which turns 19 surfaces into 38 materials over 19
bakes — are ported and pinned, driven from the real `system.debugBoard()`.

**`tune(material, …)` takes a cache key.** The system owns the entries; the
source's `material` argument is a handle into exactly this map.

**Warnings go into a `Vec<String>`, not a `println!`.** Testable, and the
transcript is compared step by step against the captured `console.warn` order.

## Two gaps in the already-ported library, and the one-line fixes

Both are in `apps/shmup/src/materials/mod.rs`, which this slice was not
permitted to edit. Each is named once, loudly, in `system.rs`, with a test
guarding it.

1. **`ThreeOptions` has no `transparent` field**, so `glass`'s
   `three.transparent: true` (`library.js:376`) cannot be represented.
   Compensated by `MISSING_LIBRARY_THREE`, a one-entry table.
   Fix: add `pub transparent: Option<bool>` to `ThreeOptions` and
   `transparent: Some(true)` to the `glass` entry, then delete the constant —
   `only_glass_needs_a_three_compensation` fails the moment the field exists.
   `ThreeOptions` also carries a `double_sided` field no entry uses and the
   source has no key for; `side` is the real one.
2. **`BakeParams::param` is `[f32; 4]`, not `Option<[f32; 4]>`**, so "declared
   `[0,0,0,0]`" and "not declared" are indistinguishable — and they key
   differently (`0_0_0_0` vs the empty string). Only two of the nineteen
   entries declare a `param`, and neither declares all-zeros, so the ambiguity
   is resolvable by name: `LIBRARY_HAS_PARAM`. Fix: make the field an `Option`.
3. **`MatParams`/`BakeParams` store `f32` where the source is `f64`.** JS
   numbers are doubles throughout; `0.085` stored as `f32` and widened is
   `0.085000000894069671875`. This is the one place the test cannot assert
   exact equality: `LIBRARY`-sourced scalars use a **1e-6 relative** tolerance
   (`f32` carries ~1.2e-7 relative precision, so that is an order of margin and
   still catches a wrong value), while everything sourced from
   `DEFAULT_PARAMS` is compared **bit-exactly** by
   `default_params_are_bit_exact`. Every key, count, size, string and boolean
   is exact. Fix: `f64` fields on both structs.

None of the three changes any cache key — sizes, seeds and tints are integers,
and the two `param` arrays are small integers.

### And one duplication to collapse later

`system.rs` carries a private `js_round` that is character-for-character
`crate::jsmath::round` (a sibling slice of this same wave, arrived at
independently — its golden is what caught the naive `floor(x + 0.5)` form).
`materials/masks.rs` carries a **third** copy, still in the naive form. Once
`jsmath` is declared in `lib.rs`, both should call it. `system.rs` writes it
out rather than importing so this slice does not depend on a module that is
not yet wired; `masks.rs` was out of bounds for this slice, and its naive
version is a latent bug (harmless at its current call sites, which never see a
sub-`0.5` input).

## Traps checked by name

- **`Float32Array`** — `grep` over `index.js`, `library.js` and `shader.js`'s
  `DEFAULT_PARAMS`: no hit. Nothing in this slice stores through a typed array.
  The `f32` issue above is the *inverse* — an f32 narrowing this port
  introduced upstream, not one the source has — and it is stated with its
  tolerance rather than hidden.
- **An enum used as a table index is order-dependent.** Avoided outright.
  `sample_surface` dispatches on the generator's **name** (`&'static str`), not
  on an enum ordinal, and `uv_mode` is a `String` compared for equality —
  exactly as the source compares `p.uvMode === 'mesh'` — rather than an enum,
  because it is never indexed and an unrecognised value must fall through to
  planar. `every_library_entry_bakes` proves all nineteen generator names
  resolve, and `tints_and_params_reach_the_generator` proves the tint and
  `uParam` plumbing actually reaches the two generators that read them (a
  dropped `uTintA` would make metal_painted's tinted and untinted bakes
  identical, and a dropped `uParam` would make `concrete` and `concrete_floor`
  identical — the test asserts both differ).
- **Float arithmetic is not associative.** `_size`'s
  `Math.max(128, Math.round((base * q) / 128) * 128)` is transcribed with its
  grouping intact. `js_round` reproduces `Math.round` — ties toward
  `+Infinity`, unlike `f64::round` — and does **not** use the obvious
  `floor(x + 0.5)`, which is wrong for `0.49999999999999994` (a sibling
  slice's golden caught exactly that). `Math.log2` is the one transcendental: it is only
  ever evaluated at multiples of 128, whose `log2` is never within 0.05 of a
  half-integer, so a last-ULP difference between V8's and libm's `log2` cannot
  change the rounded result.
- **A matching count is not proof.** The palette script does not assert "19
  bakes"; it asserts, per call, which key was produced, whether each cache hit,
  and the running total — 46 steps of it. Likewise the cache script's 23 steps
  and the idle timer's 53 records.
- **Your comparator can be the bug.** The hit/miss instrumentation is a cache
  *size* delta on both sides, and the bake key for a `get` is recovered by an
  immediately-following `getTextureSet` that is a guaranteed hit — so the probe
  cannot itself bake, grow a cache, reset `_idle` (the reset sits after the
  early return) or emit a warning (`_missing` already holds the name).

## What is pinned, and at what tolerance

| | tolerance |
|---|---|
| every cache key, hit/miss flag, bake count, cache size, warning string | exact |
| `_size` over 18 bases × 5 quality names | exact |
| anisotropy clamp over 7 renderer/preset pairs | exact |
| `resolveName` / `_resolve` / `surfaceOf` over 37 names + the warn-once gate | exact |
| `stableKey` over 21 option shapes, `_bakeKey` over 12 | exact |
| `DEFAULT_PARAMS`, all 30 fields | bit-exact |
| `LIBRARY`-sourced scalars in the resolved params for all 19 entries | 1e-6 relative (`f32` storage, above) |
| `owTintCol` / `sheenColor` linear decode | 1e-12 (one `powf`) |
| `debugBoard` placement | 1e-12 |
| idle timer: 53 records of `_idle`, `_scratchFreed`, scratch target set | 1e-12 |

## Wiring the orchestrator must add

```text
apps/shmup/src/materials/mod.rs:  pub mod system;
```

Nothing else — no `Cargo.toml` change, no `lib.rs` change. `MaterialSystem`
implements `Subsystem` (`id = "materials"`, `deps = ["render"]`,
`phases = [Update]`) so it registers like any other.
