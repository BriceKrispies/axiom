# `axiom-text` — Architecture

Axiom's deterministic, backend-neutral **text and typography** capability. This
document explains *why* it is shaped the way it is; the source is the ground
truth for *how*.

## Why `Text` is one unified primitive

There is exactly one developer-facing concept — **`Text`** — and one behavioral
facade, `TextApi`. Screen-space vs world-space, plain vs rich, static vs
animated are all *configurations* of the same object, never separate
`Label`/`Text2D`/`Text3D`/`RichText`/`BitmapText` systems. Fragmenting the
concept would duplicate layout, measurement, caching, and effects across N
half-compatible types; unifying it means every capability (wrapping, hit
testing, effects, world placement) is written once and available everywhere. A
world-space animated rich label is just a `Text` with a world placement, spans,
and an effect.

## Why it is a module, not kernel/layer code

Text is a *composed capability*, not a primitive every layer needs, so it is an
**engine module** (`modules/axiom-text`), not kernel or layer code. It depends
only on the `kernel` (serialization, error identity, binary primitives) and
`math` (vectors/transforms for placement and bounds) layers, and on **no other
module** (`allowed_modules = []`). It exposes neutral data — a `TextPlacement`
and a `GlyphBatch` — and an app (or a legal feature module) translates that into
a renderer contract. This keeps text isolated and recomposable: any backend, any
app, any future renderer consumes the same neutral batch.

The kernel deliberately knows nothing about text or fonts (the Kernel Rules
forbid it), so — like every other module — text defines its **own** error
vocabulary (`TextError`) with deterministic `(variant)` identity, the same
pattern `FigureError`/`MathError` use. The "add text-specific scope and codes"
requirement is met by a module-local error enum, not by widening the closed
`KernelErrorScope`.

## Why font decoding is offline tooling

The runtime module parses **no** font container and rasterizes **no** glyph. TTF,
OTF, WOFF, and WOFF2 decoding, outline rasterization, and atlas packing are
inherently non-deterministic-adjacent, dependency-heavy, and filesystem-bound —
exactly what the engine spine must not contain. So they live in the offline
`tools/axiom-font-import` **tool**, which converts an external font into a
versioned, deterministic `.axfont` asset. The runtime consumes only `.axfont`.
This is the single most important boundary in the design: the fast, branchless,
100%-covered runtime never touches a font parser.

The one *assembly* step — turning primitive glyph coverage bitmaps into a packed,
validated `.axfont` — lives in the module (`CompiledFont::assemble`, with a
deterministic shelf packer), because the format's structure is the module's
responsibility. The tool owns container parsing + rasterization; the module owns
the format. `axiom-font-import` calls `assemble` so there is one source of truth
for the byte layout.

## The `.axfont` format

A versioned, little-endian, bounds-checked binary asset. Layout (see
`compiled_font.rs`):

```
magic "AXFONT\0\0" | u32 version | u64 source_hash
FaceMetrics: family, face (len-prefixed UTF-8) | upem u16 | ascent/descent/line_gap i32
             | weight u16 | slant u8 | replacement_codepoint u32
table<CodepointEntry>  (codepoint u32 -> glyph u32), sorted strictly ascending
table<GlyphMetric>     (glyph, advance, bearing_xy, w, h in design units), sorted
table<KernPair>        (left, right, adjust), sorted by (left,right)
table<SizeLayer>       (pixel_size, table<AtlasPage>, table<GlyphRaster>)
ImportProvenance       (tool_version, padding, atlas_wxh)
```

Design decisions:

- **Metrics in design units, atlas in size layers.** Layout is
  resolution-independent — it scales design-unit metrics by
  `font_size / units_per_em` — while each `SizeLayer` supplies raster coverage at
  one pixel size. The runtime picks the nearest size layer and stretches its
  coverage. A bitmap font has one layer; a multi-size import has several. (This
  build emits one layer per import; multi-size packing is a documented
  extension.)
- **Raw single-channel coverage, not PNG.** The compiled asset is the
  deterministic pixel truth, so the runtime never decodes an image. A backend
  uploads the bytes as an R8 texture.
- **Every table is stored sorted strictly ascending**, which makes lookups a
  binary search *and* makes decode-time validation (duplicate/unsorted →
  `DuplicateGlyph`/`DuplicateCodepoint`) a single `windows(2).all(<)`.
- **Bounds-checked, oversized-count-guarded, deterministic.** No timestamps,
  paths, or hash-map order. `decode` fully validates: magic, version, metrics,
  sortedness, glyph references, a real replacement glyph, and every size layer.

## Default font behaviour

The clean default (`axiom.text("Hello, world")`) must not depend on a host font.
The engine vendors its own: `fallback_font.rs` authors a compact 5×7 bitmap face
(A–Z, 0–9, space, `. , ! : # -`, `a`–`z` aliased to uppercase; a hollow box for
anything else) as source data and compiles it — in pure code — into the *same*
`.axfont` representation. There is one font path; the fallback is not a special
case. Rendering `"Hello, world"` produces `HELLO, WORLD` (the face is caps-only
by design). A richer face is imported offline.

## Layout stages (all branchless)

`layout.rs` is a pipeline of pure data transforms (`lay_out`):

1. **Flatten** spans → per-`char` cells carrying the resolved style *and* the
   span's override (kept so colour can change without re-layout).
2. **Shape** each cell: resolve the glyph through the fallback chain, scale
   design metrics to the font size, and find its atlas rectangle. One glyph per
   Unicode scalar (see *shaping limitations*).
3. **Break lines** greedily: newlines break hard; `Wrap::Word`/`Char` break
   before a token that would overflow the width. `max_lines` truncates.
4. **Position** each line: compute per-line ascent/descent, apply horizontal
   alignment and vertical alignment, place glyphs with kerning, and record
   line + overall bounds. `Overflow::Clip`/`Ellipsis` drop glyphs past the box.

Every stage uses iterator combinators and `bool::then` for conditional
side-effects — there is no `if`/`for`/`match`/`while` in the spine, per the
Branchless Law.

## Style resolution

A fixed four-level cascade, `StyleOverride::apply` repeated:
`TextStyle::default()` → named style → text-level → span-level. Each level is a
sparse `StyleOverride` (every field `Option`); `apply` fills where `Some`. Pure
and order-deterministic.

## Dirty flags (why colour is cheap)

`TextRecord` keeps its resolved layout cached with fine-grained dirty flags
(`content/font/glyph/line/style/placement/effect/batch`). The layout-affecting
subset forces a re-layout; the rest only rebuild the glyph batch. Crucially,
**visual fields (colour, opacity, outline, shadow) are resolved at *batch* time**
from the current base style + the cached span override — so `set_color` and
`set_opacity` never recompute line breaking, exactly as required. Content, font,
size, spacing, line-height, wrap, and width changes *do* invalidate layout.

## Deterministic effects

`effect.rs` represents effects as data (`TextEffect { kind, start_tick,
duration, speed, amplitude, seed }`) evaluated at an explicit integer **tick** —
never wall-clock. Evaluation dispatches through a fixed function table (no
`match`) and yields a per-glyph `GlyphMod` (offset, alpha, visibility) folded in
declaration order. Shake derives its offset from stable integer hashing of
`(seed, glyph, tick)`, so the same state and tick produce byte-identical output.
Effects modify glyph placement/presentation only; they never touch canonical
content.

## Glyph batch output

The runtime product is an ordered, backend-neutral `GlyphBatch`. Each
`GlyphInstance` carries only neutral data: font handle, atlas page + UV
rectangle (in atlas pixels), position, size, source range, fill/outline/shadow
colours, and a stable ordering key (`line << 20 | column`). It holds **no** GPU
handle, WebGPU/WebGL object, canvas context, DOM node, or JS value. An app
normalises the UV by the font's page size and submits it to a renderer.

## Render integration (app-owned translation)

Per the Module Law, `axiom-text` never imports a scene/render/GPU module. An app
reads `TextApi::batch(handle, tick)` and the font's atlas (via the registered
`CompiledFont`) and translates the `GlyphBatch` into its renderer's contract
(e.g. `axiom-render`'s `RenderInput`, or a Canvas2D fill) — the same app-owned
translation pattern the rotating-cube slice uses. The pipeline is proven to
produce *visible* ink by `text_api::tests::hello_world_composites_to_visible_ink`,
which composites the real batch against the real atlas into an ASCII raster of
`HELLO, WORLD`.

## Current shaping limitations (stated plainly)

- **One glyph per Unicode scalar, left-to-right only.** No complex-script shaping
  (Arabic joining, Indic reordering, ligatures, bidi). A future
  `UnsupportedTextShaping` path should record such scripts rather than emit
  broken order; today the engine simply maps each scalar to a glyph, which is
  correct for Latin/mono scripts and safe (never mid-UTF-8) for others but not
  *shaped*.
- **`Align::Justify` approximates as left**, and `Overflow::Ellipsis` behaves as
  `Clip` (no trailing `…` synthesis yet). Both are marked in `layout.rs`.
- **One size layer per imported font.** `--sizes` accepts a list; this build
  packs the first. Multi-size layering is a format-supported extension.
- **WOFF2 is not decompressed by the importer** (brotli + table transforms);
  convert to TTF/OTF/WOFF first.

## The TypeScript SDK surface (design; staged)

`axiom.text()` belongs in the engine SDK (`@axiom/web-engine`), whose strict
branchless + 100%-coverage gate a partial landing would break, so the surface is
specified here and implemented as a follow-up. The intended API (strictly typed,
no `any`):

```ts
const title = axiom.text("ARENA FORGE", {
  position: [640, 80],
  style: { fontFamily: ["Arcade Display", "Axiom Default"], fontSize: 64,
           fontWeight: 800, color: "#ffffff",
           outline: { width: 4, color: "#2b1208" },
           shadow: { offset: [4, 6], blur: 0, color: "#00000080" } },
  layout: { width: 700, align: "center", verticalAlign: "middle",
            wrap: "word", overflow: "clip" },
});
const score = axiom.text([
  { text: "SCORE ", style: { color: "#ffffff" } },
  { text: "1250", style: { color: "#ffd54a", fontWeight: 700 } },
]);
score.setText("SCORE 1300"); score.setVisible(true); score.setPosition(20, 20);
await assets.loadFont("arcade-display", "assets/fonts/arcade-display.axfont");
```

The TS layer parses `"#rrggbbaa"` at the app edge into the engine's `Rgba`,
translates `fontFamily`/`align`/`wrap`/`overflow` strings into the module's enums,
and calls the Rust `TextApi` across the wasm bridge (or a TS mirror of layout).
`loadFont` fetches `.axfont` bytes and calls `register_font`.

## Extension rules

- New style/layout properties: add to `TextStyle`/`StyleOverride`/`LayoutConfig`,
  extend `validate`, and cover the new branch arms.
- New effects: add a fieldless `EffectKind` variant + a table entry in
  `eval_one`; keep evaluation a pure function of the tick.
- New `.axfont` fields: bump `VERSION`, extend `encode`/`decode`/`validate`, and
  keep decode rejecting the old version.
- Never add a font parser, filesystem, browser, wall-clock, or random dependency
  to this module — those belong in `axiom-font-import` or an app.
```
