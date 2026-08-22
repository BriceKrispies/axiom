# `weapons/materials.js` → `apps/shmup/src/weapons/materials.rs`

Source: `C:/dev/Claude-of-Duty/src/weapons/materials.js`, 1,215 lines, **all of
it ported**. Nothing was skipped, nothing deferred.

## Files

| path | what |
|---|---|
| `apps/shmup/src/weapons/materials.rs` | the port |
| `apps/shmup/tests/weapons_materials_port.rs` | 25 tests, all reading the golden |
| `apps/shmup/tests/weapons_materials/capture.mjs` | the capture, importing the ORIGINAL JS |
| `apps/shmup/tests/weapons_materials/golden.json` | 109 KB, byte-reproducible (md5 `3c22a35c3799ce08f8c62dc562de80e4`) |

Wiring the orchestrator must add: `apps/shmup/src/weapons/mod.rs: pub mod materials;`
(alphabetically it goes between `hands` and `mathx`). Nothing else — no
`Cargo.toml` change, no new dependency; the test uses the `serde_json` already
in `[dev-dependencies]`.

## This slice had a real oracle, and it was used

Unlike every GLSL slice on this port, `weapons/materials.js` is plain
JavaScript that runs under Node. `capture.mjs` therefore **imports the original
module** — plus the original `src/materials/library.js` and
`src/materials/shader.js` for the merge halves, and real three r180 for the
material classes — and reads back what they actually produce. There is no hand
transcription anywhere in the capture and no expected value in the Rust that
was reasoned out rather than measured.

That mattered twice (see "Source defects" below): both defects were found
*because* the golden disagreed with a plausible reading of the source, not
because anyone spotted them by eye.

## What the golden pins — everything, not a sample

* `ENV_OCCLUSION`, and `MATERIAL_KEYS` in order.
* All **15** table rows: the library surface each names, the `BASE` spread
  (`uvMode`, `localSpace`, `vertexMasks`, `weather`, `macro`, `aoStrength`),
  and all twelve per-entry override fields.
* Both merges the material system performs on each row —
  `{...LIBRARY[lib].bake, ...opts.bake}` and
  `{...LIBRARY[lib].three, ...opts.three}` (`src/materials/index.js:129,193`).
* The full `{...DEFAULT_PARAMS, ...LIBRARY[lib].mat, ...opts}` merge, used as a
  cross-check that this module declares the *right* fields (the override wins
  the spread, so every declared field must reappear unchanged in `p`).
* All **256** sRGB byte decodes through three's `SRGBToLinear`.
* All **13** owned-material variants (7 constructors, several at non-default
  arguments) with **25** properties each, colours already decoded to linear.
* All **13** cache-key strings, in `Map` insertion order.
* The `get()` dispatch for **23** keys (5 special + 15 table + 3 unknown),
  with and without a materials subsystem.
* All **18** `_fallback` rows.
* All **16,384** bytes of `_rimRamp`, plus its eight texture-state fields.

Tolerances: **exact** for everything built from literals, integers or
`+ - * /` — the table, both merges, the cache keys, the rim ramp's `u8`s, and
`glass`'s derived `ior`. **1e-13 absolute** for anything through `powf(2.4)`,
i.e. the sRGB decodes. That figure is justified in the test: it is two orders
under the 1.08e-11 error it must catch (see below) and four orders over a
one-ULP `pow` disagreement. Every decode in fact compares bit-exactly on this
toolchain; the tolerance exists only so a different libm cannot produce a false
failure.

## The named trap: enum/table ordering — **they agree**

The brief asked for this explicitly, so, explicitly:

`src/materials/library.js`'s 19 keys, in declaration order:

```
concrete, concrete_floor, brick, plaster, tile, asphalt, sand, dirt, gravel,
metal_rust, metal_painted, metal_brushed, corrugated, wood, fabric, burlap,
foliage, rubber, glass
```

`crate::materials::LIBRARY`'s 19 entries, in array order: **identical, name for
name and position for position.** The alias table matches too (12 aliases, same
targets). This is captured as `libraryKeys` in the golden and asserted by
`the_ported_library_ordering_matches_the_source`, so a future reorder of either
side fails rather than silently repointing recipes.

The port additionally refuses to depend on that agreement: `WeaponMaterial`
stores its library surface as the library's own `&'static str` and resolves it
through a name lookup (`library_entry`, a faithful port of
`MaterialSystem._resolve` including the alias hop and the `concrete` fallback),
never as an index. The three surfaces the weapon table names are `rubber` (7
entries), `metal_brushed` (5) and `fabric` (3).

One near-miss worth recording, because it is exactly the shape that bit the
audio recipes: `WEAPON_MATERIALS` has a key **`steel`**, and the library's
alias table *also* has `steel -> metal_brushed`. They agree here by luck of
authoring, not by construction — the weapon key `steel` names
`'metal_brushed'` directly. And `WEAPON_MATERIALS` has a key `rubber` that
names the library surface `rubber`, while `get('glass')` never reaches the
library's `glass` surface at all. Any port that "helpfully" resolved a weapon
key as a library name would produce three wrong materials and one crash.

## The other named traps

* **`Float32Array`** — grepped. The source file contains **none**. It contains
  one `Uint8Array` (the rim ramp), whose eight-bit quantisation *is* part of
  the algorithm and is reproduced byte for byte.
* **`Math.hypot`** — one occurrence, `_rimRamp:1083`. Ported as `f64::hypot`,
  never the expanded form. Honest caveat recorded in the test: within the
  ramp's own input range the two forms agree *to the byte* (the closest any
  `a * 255` comes to a `.5` rounding boundary is 9.6e-4, thirteen orders clear
  of an ULP), so the golden cannot distinguish them. The test therefore names
  the trap and demonstrates the difference on the guaranteed case rather than
  pretending to cover it.
* **Float arithmetic is not associative** — the biggest finding in the slice,
  below.
* **Dead computation** — two instances, below.
* **`sign` / Euler order / matrix order** — not applicable; this file has no
  geometry and no rotations.

## Finding: three's sRGB decode is NOT the library's `owSRGB`

Every hex colour in this file (`wearColor`, `grimeColor`, `sheenColor`, and
every owned material's `color`/`specularColor`) is decoded to the linear
working space by `new THREE.Color(hex)` before it reaches a uniform. Three's
decode is:

```js
c < 0.04045 ? c * 0.0773993808 : Math.pow(c * 0.9478672986 + 0.0521327014, 2.4)
```

The library's own GLSL `owSRGB` — already ported as
`crate::materials::noise::ow_srgb`, and already the obvious thing to reach for
— is the same transform written the other way:

```glsl
c > 0.04045 ? pow((c + 0.055) / 1.055, 2.4) : c / 12.92
```

**Algebraically identical, numerically not.** Measured over all 256 byte
values: **254 of 256 differ**, by up to **1.08e-11**. The port therefore
declares its own `srgb_to_linear` with three's grouping and a comment saying
why it is not the existing function, and the test
`srgb_decode_is_threes_grouping_not_the_glsl_owsrgb` asserts the port sits on
three's side of the gap at the byte where the gap is widest — a check a
tolerance alone could not make.

The branch condition also differs (`<` vs `>`), so the two disagree about
`c == 0.04045` exactly; no `n / 255` hits it, so that half is moot.

### Cross-slice: this may affect `materials/surfaces/metal.rs`

`crate::materials::surfaces::metal::hex_to_linear_tint` decodes
`LIBRARY.bake.tint_a` through `ow_srgb`. But the call site that fills that
uniform is `src/materials/index.js:145`:
`tintA: new THREE.Color(bake.tintA)` — i.e. **three's** decode, not the
shader's. If that is right, `metal_painted`'s baked `uTintA` is off by ~1e-11
per channel in the port. Not my file and not my golden (that slice's golden is
a hand transcription of the GLSL, which cannot see the discrepancy), so I have
not touched it — flagging it for the orchestrator. It is almost certainly
below the visible threshold; it is recorded because it is the same defect class
and the same one-line fix.

## Source defects, ported as-is and pinned by name

### 1. `glass(tint = 0x3b6e8c)` — the parameter is dead

`materials.js:1007-1065`. `tint` reaches the cache key
(`` `glass:${tint}` ``) and **nothing else**; the material's colour is the
literal `0x121c22` whatever is passed. So `glass(0xff0000)` allocates a second,
byte-identical `MeshPhysicalMaterial` under a second cache key — pure waste,
and a live footgun for anyone who thinks they can retint the optic.

Pinned by `glass_tint_argument_is_dead_source_quirk`, which asserts the two
materials are field-for-field equal *except* the cache key.

### 2. `glass`'s `ior: 1.52` is dead too — and this one changes a number

The same constructor literal sets `ior: 1.52` and then, three lines later,
`reflectivity: 0.55`. Three defines `reflectivity` on `MeshPhysicalMaterial`
as an accessor whose **setter writes `ior`**:

```js
set: function (reflectivity) { this.ior = (1 + 0.4 * reflectivity) / (1 - 0.4 * reflectivity); }
```
(`three/src/materials/MeshPhysicalMaterial.js:146-157`)

`Material.setValues` applies the literal's keys in insertion order, so the
shipped optic glass has **ior = 1.5641025641025641**, not 1.52. Read back off a
real three r180 instance in the golden, which is how it was found — a port that
transcribed the literal faithfully would have shipped 1.52 and been wrong.

The port stores the value the material actually carries, with the expression
transcribed literally (`(1.0 + 0.4 * 0.55) / (1.0 - 0.4 * 0.55)` — grouping
preserved, non-associativity), and keeps the authored 1.52 as
`GLASS_AUTHORED_IOR` so the defect is visible rather than silently dropped.
Pinned by `glass_authored_ior_is_clobbered_by_reflectivity_source_quirk`.

Note this is a **visible** defect, not a rounding one: 1.52 → 1.564 is a ~2.9%
shift in the index of refraction, which moves F0 from 0.0426 to 0.0477 (+12%)
on the one surface in the game the player presses their eye against.

### 3. `_fallback` misclassifies `steel_soot` and `copper`

`materials.js:916-921`. The metal test is
`key === 'steel' || 'steel_bright' || 'steel_black' || 'brass' || 'copper'`:

* **`steel_soot` is missing from it**, so a sooted muzzle brake falls back as a
  dielectric at `0x2a2b2e` / roughness 0.72 / metalness 0. (Arguably correct
  by accident — the shipped material *is* nearly dielectric, metalness 0.12 —
  but it is not what the list intends.)
* **`copper` is in it**, but the colour ternary only special-cases `brass`, so
  copper falls back to steel's `0x3a3d42` grey rather than to anything
  copper-coloured.

Fallbacks only fire in the standalone harness, so neither is a shipping bug;
both are pinned by `fallback_misclassifies_soot_and_copper_source_quirk` so
they cannot be quietly "fixed" into a divergence.

### 4. `opticTube`'s comment is stale (not a code defect)

`materials.js:946` says "0x272a2c is 0.0205 linear — the middle of the band";
the colour it sets is `0x1d2023` (0.0123-0.0168 linear). Code transcribed as
written, comment recorded as stale in the port's doc.

## Divergences from the source shape, and why

* **`f64` throughout, narrowing only at the `BakeParams` seam.** JS numbers are
  `f64` and none of this data passes through a `Float32Array` on the way to the
  merge, so the table is `f64`. `resolved_bake` returns
  `crate::materials::BakeParams`, which is `f32` — the width
  `crate::materials::bake::BakeDef` consumes and therefore the width these
  numbers actually reach the generator at. The test narrows the golden's `f64`
  for that comparison rather than widening the port's `f32`; doing it the other
  way round fails on a port that is exactly right, which is the "your
  comparator can be the bug" trap, and it did bite once during this slice.
* **`ThreeOverride` / `MergedThree` rather than reusing
  `crate::materials::ThreeOptions`.** Two reasons, both faithfulness, not
  taste: `ThreeOptions` has **no `metalness` field** (which `steel_soot` sets,
  and which is the entire point of treating soot as a dielectric powder), and
  it is `f32` where this table is `f64`. It also carries a dozen fields no
  weapon entry touches. `BakeParams`, `LibraryEntry`, `LIBRARY` and `ALIASES`
  *are* reused directly.
* **`tint` is `[f64; 3]`, not a hex.** `crate::materials::MatParams::tint` is
  `Option<u32>`, which cannot represent these: they are
  `new THREE.Color(r, g, b)` — linear floats passed straight through, three's
  rgb constructor doing no colour-space conversion — and `brass` is
  `(2.3, 1.58, 0.74)`, well outside a hex triplet.
* **`WeaponMaterials` is a set of free functions, not a struct with a cache.**
  The class's whole state is a `Map` cache plus two dispose lists; the cache is
  a THREE-instance-identity optimisation with no bearing on the values, and
  disposal is a GPU-resource concern this port has no equivalent of.
  `material_request(key, has_library)` is the pure part of `get()`, and its
  five-key special dispatch and the two properties `get()` applies to a
  library material (`shadowSide = FrontSide`, `envMapIntensity = ENV_OCCLUSION`)
  are all pinned against the real class in the golden — captured by driving the
  real `WeaponMaterials` with a fake library that records every
  `(name, opts)` pair it is handed.
* **Cache keys are a `CacheKey` enum with a `to_key()`, not raw strings.** The
  keys are observable (`glass:3894924` is *decimal*, `lensRing:1` has no
  `.0`), so `to_key()` reimplements the JS template-literal number formatting
  and the golden pins all 13.

## What is NOT in this module, on purpose

`{...DEFAULT_PARAMS, ...}` — the third merge. `DEFAULT_PARAMS` belongs to
`src/materials/shader.js`, which is a separate slice (the runtime material
shader, destined for hand-written WGSL in `gpu-backend`). The overrides this
table declares win that spread outright, so nothing is lost by leaving the
defaults where they live; the golden captures the merged result anyway and the
test uses it as a cross-check that this module declares the right fields.

Two smaller things the merge exposes, for whoever ports `materials/index.js`:

* `MaterialSystem._size()` quantises `bake.size` by the quality preset before
  the bake. At quality 1 it is the identity on 512 and 1024, so the golden's
  `mergedBake` is pre-quantisation and correct as-is; a `low`/`medium` preset
  would halve or three-quarter it.
* The ported `LIBRARY` materialises `bake.param` as `[0.0; 4]` where the
  JavaScript library entry has **no `param` key at all** for `rubber`,
  `metal_brushed` and `fabric`. `index.js:147` reads
  `bake.param ? new Vector4().fromArray(bake.param) : undefined`, so absent and
  all-zero are *not* the same thing — absent leaves the uniform unset. Not this
  slice's file; noted for the `materials/index.js` agent.

## Verification actually performed

The fan-out brief forbids building in the shared target directory, and that was
respected — no `cargo` command was run in `C:\dev\axiom` and nothing was
committed or staged. The port was instead type-checked and executed in an
isolated scratchpad crate with its own `CARGO_TARGET_DIR`, built from
`weapons/materials.rs` verbatim plus a copy of the real `materials/mod.rs`
(its `Surface` import stubbed). Result: **clean compile, no warnings, all 25
tests pass against the committed `golden.json`.** The golden was regenerated
twice and is byte-identical.

That is a stronger signal than "it looks right", but it is not the integration
pass: the scratchpad crate stubs `materials::{noise, bake, masks, surfaces}`,
so a name collision with a sibling slice landing in the same wave would still
show up only when the real crate is built.
