/*
 * branding.test.ts — the brand vocabulary's invariants: the geometry font's
 * cell math and run coalescing (glyphs.ts), the brand value's validation +
 * color helpers (brand.ts), and the surface-welding label builder's fit, weld,
 * and basis behavior (label.ts).
 */

import assert from "node:assert/strict";
import test from "node:test";

import { QUAT_IDENTITY, v3 } from "../stage/vectors.ts";
import { GLYPH_GAP, GLYPH_H, GLYPH_W, STROKE_THICK, textColumns, textStrokes } from "./glyphs.ts";
import { brandIssues, brandMaterials, DEFAULT_BRAND, hexToRgb, readBrand, rgbToHex } from "./brand.ts";
import { stampText } from "./label.ts";

// ── glyphs (the Helvetica-idiom stroke font) ──────────────────────────────────

test("textColumns counts glyph widths plus inter-glyph gaps, and empty is zero", () => {
  assert.equal(textColumns(""), 0);
  assert.equal(textColumns("A"), GLYPH_W);
  assert.equal(textColumns("ABC"), 3 * GLYPH_W + 2 * GLYPH_GAP);
});

test("a glyph is drawn from stroke centerlines that stay inside its cell box", () => {
  // 'I' is a single vertical stroke — one segment, near-vertical, spanning most
  // of the cap height and sitting on the glyph's centerline.
  const strokes = textStrokes("I");
  assert.equal(strokes.length, 1, "I is one stroke, not a stack of pixel runs");
  const stem = strokes[0];
  assert.ok(stem !== undefined);
  assert.ok(Math.abs(Math.abs(stem.angle) - Math.PI / 2) < 1e-9, "the stem is vertical");
  assert.ok(stem.len > GLYPH_H * 0.7, "the stem spans most of the cap height");
  // Every glyph's stroke CENTERS stay inside the [0,GLYPH_W]×[0,GLYPH_H] cell, so
  // no letter spills into its neighbour's slot.
  for (const ch of "ACMESW0") {
    for (const s of textStrokes(ch)) {
      assert.ok(s.cx >= 0 && s.cx <= GLYPH_W, `${ch} stroke center stays within the cell width`);
      assert.ok(s.cy >= 0 && s.cy <= GLYPH_H, `${ch} stroke center stays within the cell height`);
    }
  }
});

test("diagonals are real slanted strokes, not axis-aligned staircases", () => {
  // 'A' has two diagonal legs (neither horizontal nor vertical) plus a crossbar.
  const angles = textStrokes("A").map((s) => Math.abs(s.angle));
  const diagonal = angles.filter((a) => a > 0.2 && Math.abs(a - Math.PI / 2) > 0.2);
  assert.ok(diagonal.length >= 2, "A carries at least two genuine diagonal legs");
});

test("lowercase is uppercased for the font", () => {
  assert.deepEqual(textStrokes("a"), textStrokes("A"));
});

test("a space is blank (no strokes) but still advances a glyph width", () => {
  assert.deepEqual(textStrokes(" "), []);
  assert.equal(textColumns("A B"), 3 * GLYPH_W + 2 * GLYPH_GAP);
});

test("an unknown glyph falls back to a visible box rather than vanishing", () => {
  // '█' is not in the font; the fallback is a box outline (four strokes), so an
  // unknown char reads as a placeholder instead of silently disappearing.
  assert.ok(textStrokes("█").length > 0);
});

test("textStrokes offsets each glyph's strokes by its advance origin", () => {
  // The second glyph's strokes all sit past one glyph width + gap.
  const strokes = textStrokes("II");
  assert.equal(strokes.length, 2, "two stems for two I's");
  const second = strokes.filter((s) => s.cx >= GLYPH_W + GLYPH_GAP);
  assert.equal(second.length, 1, "the second I is advanced past the first");
  assert.ok(strokes.every((s) => s.cx >= 0 && s.cx <= textColumns("II")));
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
  // height == GLYPH_H → cell == 1; 'I' is a single vertical stroke → one box.
  assert.equal(boxes.length, 1);
  assert.ok(boxes.every((b) => b.material === "M"));
  // The stroke centers all sit within +/- half the word width of the origin x.
  const halfWidth = textColumns("I") / 2;
  assert.ok(boxes.every((b) => Math.abs(b.transform.position.x - FRAME.origin.x) <= halfWidth + 1e-9));
  // The stem box is a tall thin bar: its long axis (scale.x) spans most of the cap.
  assert.ok(boxes.some((b) => b.transform.scale.x > GLYPH_H * 0.7));
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
  // 'E' carries off-center strokes (top/mid/bottom bars), so this exercises the
  // offset scaling, not just the stroke-weight scaling.
  const unit = stampText("k", "E", FRAME, STYLE);
  const scaled = stampText("k", "E", { ...FRAME, basis: v3(1, 2, 1) }, STYLE);
  assert.ok(unit.length > 0 && unit.length === scaled.length);
  unit.forEach((box, i) => {
    const big = scaled[i];
    assert.ok(big !== undefined);
    assert.ok(Math.abs(big.transform.scale.y - box.transform.scale.y * 2) < 1e-9, "stroke weight doubles");
    assert.ok(Math.abs(big.transform.position.y - box.transform.position.y * 2) < 1e-9, "offset doubles");
  });
});

test("a long name shrinks uniformly to fit maxWidth instead of overflowing", () => {
  const tight = { ...STYLE, maxWidth: 30 };
  const long = stampText("k", "IIIIIIIIII", FRAME, tight);
  // The stem centers span the block; their horizontal spread must fit maxWidth.
  const xs = long.map((b) => b.transform.position.x);
  const spread = Math.max(...xs) - Math.min(...xs);
  assert.ok(spread <= tight.maxWidth + 1e-6, `stem spread ${spread} <= ${tight.maxWidth}`);
  // Shrinking is uniform: the stroke weight (scale.y) is now below the unshrunk
  // weight (STROKE_THICK at cell == 1), proving the whole cell scaled down.
  assert.ok((long[0]?.transform.scale.y ?? Infinity) < STROKE_THICK, "the cell shrank below its unshrunk size");
});
