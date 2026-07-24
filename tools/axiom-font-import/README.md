# `axiom-font-import` — the offline font compiler

Converts an external font into a deterministic, versioned **`.axfont`** runtime
asset that the engine's `axiom-text` module consumes. This is the *only* place in
the repo that parses a font container or rasterizes a glyph — the runtime never
does. It is repo tooling, outside the engine dependency graph.

## Supported source formats

| Format | Support |
|--------|---------|
| `.ttf` (TrueType) | ✅ parsed + rasterized (via `fontdue`) |
| `.otf` (OpenType/CFF) | ✅ parsed + rasterized (via `fontdue`) |
| `.woff` (WOFF 1.0) | ✅ zlib-decompressed to sfnt (via `flate2`), then as OTF/TTF |
| `.woff2` (WOFF 2.0) | ❌ not decompressed here (brotli + table transforms). Convert to TTF/OTF/WOFF first. The tool detects it and exits non-zero with a clear message. |

## Command

```sh
axiom-font-import import \
  --input  assets/source/MyFont.woff2 \
  --output assets/fonts/my-font.axfont \
  --sizes  32 \
  --ranges U+0020-007E,U+00A0-00FF
```

### Flags

| Flag | Default | Meaning |
|------|---------|---------|
| `--input` | *(required)* | source font path (`.ttf/.otf/.woff`) |
| `--output` | *(required)* | `.axfont` output path |
| `--sizes` | *(required)* | comma-separated pixel sizes; **this build packs the first** (multi-size is a documented extension) |
| `--ranges` | `U+0020-007E` | Unicode ranges to include (see below) |
| `--family` | input file stem | family name the asset advertises |
| `--atlas-width` / `--atlas-height` | `512` | target atlas page bounds |
| `--padding` | `1` | pixels between packed glyphs |
| `--replacement` | `U+FFFD` | replacement codepoint (a box is synthesised if the font lacks it) |
| `--overwrite` | off | allow overwriting an existing output |

### Unicode range syntax

Comma-separated items, each `U+XXXX` (single), `U+XXXX-YYYY`, or
`U+XXXX-U+YYYY` (inclusive). Hex is case-insensitive; blanks are ignored.
Example: `--ranges U+0020-007E,U+00A0-00FF,U+2022`.

### Charset files

Not yet a flag; pass explicit `--ranges`. A `--charset <file>` reading literal
characters is a planned addition that lowers to the same codepoint list.

## Deterministic output promise

The same source bytes and arguments produce **byte-identical** `.axfont` output
on every run: glyphs are packed by ascending codepoint with a stable shelf
packer, all tables are sorted before serialization, and the asset contains no
timestamps, source paths, host metadata, or hash-map order. The provenance block
records only the deterministic knobs (padding, atlas size). After writing, the
tool **verifies** by decoding its own output through the runtime decoder and
checking every glyph references valid atlas data; a mismatch is a non-zero exit.
Output is written atomically (temp file + rename).

## Regenerating the engine default font

The engine's default face is code-authored in
`modules/axiom-text/src/fallback_font.rs` (an OS-independent 5×7 bitmap) and needs
no import. To ship a *richer* default from a properly-licensed source font,
import it and register it as an additional face in the app that wants it — commit
the source license alongside the generated `.axfont`. Do **not** commit an
unknown/unlicensed font.

## Asset integration

The output `.axfont` is loaded at runtime through the asset-facing API:

```rust
let handle = text_api.register_font(&std::fs::read("assets/fonts/my-font.axfont")?)?;
// select it by family, or by the returned handle:
text_api.set_fonts(text, vec![handle])?;
```

(In the TS SDK the equivalent is `await assets.loadFont("my-font", "…/my-font.axfont")`.)

## Failure diagnostics & exit codes

Non-zero exit on: invalid/unreadable font, unsupported container (WOFF2/unknown),
malformed `--ranges`, empty codepoint set, atlas packing overflow, invalid
metrics, verification mismatch, or output-write failure. Diagnostics go to
`stderr` in a stable `axiom-font-import: error: …` form.
