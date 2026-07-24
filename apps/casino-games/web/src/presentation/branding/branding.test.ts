/*
 * branding.test.ts — the brand vocabulary's invariants: the geometry font's
 * cell math and run coalescing (glyphs.ts), the brand value's validation +
 * color helpers (brand.ts), and the surface-welding label builder's fit, weld,
 * and basis behavior (label.ts).
 */

import assert from "node:assert/strict";
import test from "node:test";

import { QUAT_IDENTITY, v3 } from "../stage/vectors.ts";
import { GLYPH_GAP, GLYPH_H, GLYPH_W, glyphRuns, textColumns, textRuns } from "./glyphs.ts";
import { brandIssues, brandMaterials, DEFAULT_BRAND, hexToRgb, readBrand, rgbToHex } from "./brand.ts";
import { stampText } from "./label.ts";

// ── glyphs ──────────────────────────────────────────────────────────────────

test("textColumns counts glyph widths plus inter-glyph gaps, and empty is zero", () => {
  assert.equal(textColumns(""), 0);
  assert.equal(textColumns("A"), GLYPH_W);
  assert.equal(textColumns("ABC"), 3 * GLYPH_W + 2 * GLYPH_GAP);
});

test("a solid glyph row coalesces into ONE run, not five touching cells", () => {
  // 'I' has a full top row (#####) and a full bottom row; the middle five rows
  // are a single centered cell each — so 2 full-width runs + 5 single runs.
  const runs = glyphRuns("I");
  const full = runs.filter((r) => r.len === GLYPH_W);
  const single = runs.filter((r) => r.len === 1);
  assert.equal(full.length, 2, "top and bottom bars are one run each");
  assert.equal(single.length, GLYPH_H - 2, "the stem is one cell per middle row");
  assert.deepEqual(
    full.map((r) => r.row).sort((a, b) => a - b),
    [0, GLYPH_H - 1],
  );
});

test("lowercase is uppercased for the font", () => {
  assert.deepEqual(glyphRuns("a"), glyphRuns("A"));
});

test("a space is blank (no runs) but still advances a glyph width", () => {
  assert.deepEqual(glyphRuns(" "), []);
  assert.equal(textColumns("A B"), 3 * GLYPH_W + 2 * GLYPH_GAP);
});

test("an unknown glyph falls back to a visible box rather than vanishing", () => {
  // '#' is not in the font; the fallback is a filled 5x7 outline box, so it must
  // produce lit runs (otherwise an unknown char would silently disappear).
  assert.ok(glyphRuns("█").length > 0);
});

test("textRuns offsets each glyph's runs by its column origin", () => {
  // The second glyph's runs all start at column >= one glyph width + gap.
  const runs = textRuns("II");
  const secondGlyph = runs.filter((r) => r.col >= GLYPH_W + GLYPH_GAP);
  assert.ok(secondGlyph.length > 0, "the second glyph contributes runs past the first");
  assert.ok(runs.every((r) => r.col >= 0 && r.col < textColumns("II")));
});

// ── brand value + validation ──────────────────────────────────────────────────

test("the default brand validates clean", () => {
  assert.deepEqual(brandIssues(DEFAULT_BRAND, "b"), []);
});

test("brandIssues flags a non-object, an empty name, and out-of-range colors", () => {
  assert.deepEqual(brandIssues(null, "b"), [{ message: "brand must be an object", path: "b" }]);
  const bad = brandIssues({ ink: [0, 0, 0], name: "   ", onPrimary: [1, 1, 1], primary: [2, 0, 0] }, "b");
  assert.ok(bad.some((i) => i.path === "b.name"));
  assert.ok(bad.some((i) => i.path === "b.primary"));
  assert.ok(bad.every((i) => i.path !== "b.ink" && i.path !== "b.onPrimary"));
});

test("brandIssues flags a missing / malformed color triple", () => {
  const bad = brandIssues({ ink: [0, 0], name: "X", onPrimary: [1, 1, 1], primary: [0.5, 0.5, 0.5] }, "b");
  assert.deepEqual(bad, [{ message: "brand.ink must be an [r, g, b] triple in [0, 1]", path: "b.ink" }]);
});

test("readBrand extracts a valid brand and rejects everything else", () => {
  assert.deepEqual(readBrand({ brand: DEFAULT_BRAND }), DEFAULT_BRAND);
  assert.equal(readBrand({ brand: { name: "" } }), null);
  assert.equal(readBrand({}), null);
  assert.equal(readBrand(42), null);
});

test("brandMaterials derives the branded palette from the brand colors", () => {
  const mats = brandMaterials(DEFAULT_BRAND);
  for (const key of ["BrandPrimary", "BrandInk", "BrandLetter", "BrandLetterOnPrimary", "BrandPost"]) {
    assert.ok(key in mats, `${key} exists`);
  }
  assert.deepEqual(mats.BrandPrimary?.baseColor, [...DEFAULT_BRAND.primary, 1]);
  assert.deepEqual(mats.BrandLetterOnPrimary?.baseColor, [...DEFAULT_BRAND.onPrimary, 1]);
});

test("rgb <-> hex round-trips, and hexToRgb rejects a non-hex string", () => {
  assert.equal(rgbToHex([1, 0, 0]), "#ff0000");
  assert.equal(rgbToHex([0, 0, 0]), "#000000");
  assert.deepEqual(hexToRgb("#ff0000"), [1, 0, 0]);
  assert.deepEqual(hexToRgb("00ff00"), [0, 1, 0]);
  assert.equal(hexToRgb("not-a-color"), null);
});

// ── the label builder ──────────────────────────────────────────────────────────

const FRAME = { basis: v3(1, 1, 1), center: v3(0, 0, 0), orient: QUAT_IDENTITY, origin: v3(10, 0, 0) } as const;
const STYLE = { depth: 1, height: GLYPH_H, lift: 0, material: "M", maxWidth: 1000 } as const;

test("empty and whitespace-only text stamp nothing", () => {
  assert.deepEqual(stampText("k", "", FRAME, STYLE), []);
  assert.deepEqual(stampText("k", "   ", FRAME, STYLE), []);
});

test("stamped lettering stays within the block width and carries the material", () => {
  const boxes = stampText("k", "I", FRAME, STYLE);
  // height == GLYPH_H → cell == 1; 'I' has 7 runs.
  assert.equal(boxes.length, 7);
  assert.ok(boxes.every((b) => b.material === "M"));
  // All within +/- half the word width (cols/2 * cell) of the origin x.
  const halfWidth = textColumns("I") / 2;
  assert.ok(boxes.every((b) => Math.abs(b.transform.position.x - FRAME.origin.x) <= halfWidth + 1e-9));
  // The top bar is one 5-wide run.
  assert.ok(boxes.some((b) => Math.abs(b.transform.scale.x - GLYPH_W) < 1e-9));
});

test("the lettering is WELDED to the frame origin — moving the origin translates every box", () => {
  const a = stampText("k", "ACME", FRAME, STYLE);
  const b = stampText("k", "ACME", { ...FRAME, origin: v3(30, 0, 0) }, STYLE);
  assert.equal(a.length, b.length);
  a.forEach((box, i) => {
    const other = b[i];
    assert.ok(other !== undefined);
    assert.ok(Math.abs((other.transform.position.x - box.transform.position.x) - 20) < 1e-9);
  });
});

test("the basis scale stretches both offsets and box sizes (the chest's squash/grow)", () => {
  const unit = stampText("k", "I", FRAME, STYLE);
  const scaled = stampText("k", "I", { ...FRAME, basis: v3(1, 2, 1) }, STYLE);
  unit.forEach((box, i) => {
    const big = scaled[i];
    assert.ok(big !== undefined);
    assert.ok(Math.abs(big.transform.scale.y - box.transform.scale.y * 2) < 1e-9, "box height doubles");
    assert.ok(Math.abs(big.transform.position.y - box.transform.position.y * 2) < 1e-9, "offset doubles");
  });
});

test("a long name shrinks uniformly to fit maxWidth instead of overflowing", () => {
  const tight = { ...STYLE, maxWidth: 30 };
  const long = stampText("k", "IIIIIIIIII", FRAME, tight);
  // The stamped extent must not exceed maxWidth.
  const xs = long.flatMap((b) => [b.transform.position.x - b.transform.scale.x / 2, b.transform.position.x + b.transform.scale.x / 2]);
  const span = Math.max(...xs) - Math.min(...xs);
  assert.ok(span <= tight.maxWidth + 1e-6, `span ${span} <= ${tight.maxWidth}`);
  // And shrinking is uniform: the cell (box height) is now below the unshrunk height.
  assert.ok((long[0]?.transform.scale.y ?? Infinity) < GLYPH_H, "cells shrank below the target height");
});
