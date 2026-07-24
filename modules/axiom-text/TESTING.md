# `axiom-text` — Testing

The module ships at 100% intent: every public behaviour has a direct test, and
every branch arm of the deterministic spine is exercised. Run:

```sh
cargo test -p axiom-text                       # unit + integration tests
cargo run -p xtask -- check-architecture       # layer/module law + one-facade
bash scripts/coverage.sh                        # 100% region/line/function gate
cargo dylint --all -- --all-targets            # branchless + no-unwrap + rulebook
```

## Compiled-font (`.axfont`) parser tests

In `compiled_font.rs`, `face_metrics.rs`, `size_layer.rs`, `atlas_page.rs`,
`glyph_raster.rs`, `font_table.rs`:

- valid decode + encode/decode **round trip** (via the fallback font);
- **deterministic bytes** (re-encode is byte-identical);
- invalid magic → `MalformedFont`; unsupported version → `UnsupportedFontVersion`;
- truncated sections (anywhere) → `MalformedFont`;
- oversized table counts rejected before reading the body;
- duplicate / unsorted codepoints → `DuplicateCodepoint`;
- duplicate / unsorted glyphs & kerning → `DuplicateGlyph`;
- codepoint pointing at a missing glyph → `MissingGlyph`;
- missing replacement glyph → `MissingReplacementGlyph`;
- invalid face metrics → `InvalidFontMetrics`;
- bad atlas dimensions / out-of-page rasters → `InvalidAtlasDimensions` /
  `InvalidAtlasPage`;
- non-UTF-8 family/face name → `InvalidFontMetadataUtf8`.

## Builder + importer determinism

- `font_builder.rs`: `assemble` validates, is byte-identical on repeat, packs a
  shelf atlas that wraps to new rows, reports `AtlasPackingOverflow`, and rejects
  a missing replacement glyph.
- `tools/axiom-font-import` (`cargo test -p axiom-font-import`): Unicode range
  parsing (single/range/`U+` on both ends/dedup/malformed/inverted), container
  sniffing, WOFF2 rejection, the synthetic box glyph, and CLI parsing.
- **Real-font determinism** (manual, non-committed — no font is vendored):
  `axiom-font-import import --input <system.ttf> --output a.axfont …` twice
  yields byte-identical output, and the tool's built-in verify decodes the
  result. Proven against Consolas (191 codepoints, 32px).

## Layout golden cases

In `layout.rs`, `laid_out_text.rs`, and `text_api.rs` tests: empty text, one
line, multiple lines (`\n`), word wrap, char wrap, no-wrap, alignment
(left/center/right), vertical alignment, `max_lines`, overflow clip, tabs,
letter/word spacing, kerning, mixed spans, fallback/replacement glyphs,
deterministic glyph ordering, and deterministic bounds.

## Hit testing

`hit_test.rs`: point-in-glyph, nearest-char snapping (far-left → first char,
far-right → last), glyph→source mapping, and empty-text safety.

## Cache invalidation

`text_record.rs`: content and width dirty layout; colour, opacity, placement, and
effects do **not**; a fresh record is fully dirty; char counting.

## Effect determinism

`effect.rs`: reveal progresses with tick, shake is byte-identical for the same
`(state, tick)` and bounded by amplitude, every kind evaluates and validates,
composition order (fade+reveal), and different ticks differ.

## Serialization / replay round trips

`text_snapshot.rs` + `text_api.rs`: identical `(state, tick)` produce a
byte-identical `TextSnapshot`; a different tick changes it; a reveal/shake effect
changes it across ticks. The snapshot serializes placement + per-glyph
position/size/colour deterministically.

## Facade behaviour

`text_api.rs` covers the whole `TextApi`: create plain/rich, remove (+ stale
handle), set text/style/color/opacity/layout/placement/visible/effects/fonts,
measure without an object, bounds/line-metrics/glyph-bounds/hit, snapshot, batch
with the glyph cap, capacity + content-length caps, register/unregister a custom
font (+ `FontStillReferenced`), malformed-font rejection, and update-mode.

## Real-render validation

`text_api::tests::hello_world_composites_to_visible_ink` composites the **real**
glyph batch against the **real** atlas into an ASCII raster and asserts legible
ink — a concrete proof the pipeline produces *visible* text, not just data. The
integration test `tests/load_font.rs` proves the public asset path: assemble →
encode → `register_font` → select by family → render.

## Architecture checks

`cargo run -p xtask -- check-architecture` proves: `axiom-text` classifies as an
engine module, imports only `kernel` + `math`, depends on no other module/app/
tool, exposes exactly one facade (`TextApi`) plus its `ids` vocabulary, and
contains no browser/filesystem/console/placeholder-macro references. The
`no_unwrap_in_engine` and `engine_no_branching` dylints enforce no `unwrap` and no
control-flow branching in non-test code.
