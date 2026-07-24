/*
 * text.test.ts — `node --test` coverage for the pure text authoring layer in
 * text.ts. Exercises colour parsing (all hex forms + tuple + invalid), the style
 * cascade, the immutable fluent updates, and layout (monospace metrics, explicit
 * newlines, word wrap, alignment, visibility). No DOM — everything is a pure value.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { axiom, text, type TextGlyph } from "./text.ts";

const EPS = 1e-6;
const close = (actual: number, expected: number, msg: string): void => {
  assert.ok(Math.abs(actual - expected) <= EPS, `${msg}: expected ${expected}, got ${actual}`);
};
// fontSize 16 → advance 16 * 0.6 = 9.6, line height 16 * 1.2 = 19.2.
const ADVANCE = 9.6;
const LINE_H = 19.2;
const chars = (glyphs: readonly TextGlyph[]): string => glyphs.map((glyph): string => glyph.char).join("");

test("default plain text lays out at the origin, opaque white", () => {
  const glyphs = text("HI").glyphs();
  assert.equal(glyphs.length, 2);
  assert.equal(chars(glyphs), "HI");
  close(glyphs[0]!.x, 0, "first x");
  close(glyphs[1]!.x, ADVANCE, "second x");
  close(glyphs[0]!.y, 0, "y");
  close(glyphs[0]!.height, 16, "height = fontSize");
  assert.deepEqual(glyphs[0]!.color, [1, 1, 1, 1]);
});

test("spaces advance the pen but emit no glyph", () => {
  const glyphs = text("A B").glyphs();
  assert.equal(chars(glyphs), "AB");
  close(glyphs[1]!.x, ADVANCE * 2, "second letter after the space gap");
});

test("measure reports width, height, and line count", () => {
  const bounds = text("HI").measure();
  close(bounds.width, ADVANCE * 2, "width");
  close(bounds.height, LINE_H, "height");
  assert.equal(bounds.lineCount, 1);
});

test("explicit newlines split into lines", () => {
  const laid = text("A\nB");
  assert.equal(laid.measure().lineCount, 2);
  const glyphs = laid.glyphs();
  assert.equal(glyphs[1]!.line, 1);
  close(glyphs[1]!.y, LINE_H, "second line y");
});

test("empty string yields no glyphs but one line of default height", () => {
  const bounds = text("").measure();
  assert.equal(text("").glyphs().length, 0);
  assert.equal(bounds.lineCount, 1);
  close(bounds.height, LINE_H, "empty line height");
});

test("hex colours: #rgb, #rrggbb, #rrggbbaa, and an Rgba tuple", () => {
  const red = text("A", { style: { color: "#f00" } }).glyphs()[0]!.color;
  assert.deepEqual(red, [1, 0, 0, 1]);
  const green = text("A", { style: { color: "#00ff00" } }).glyphs()[0]!.color;
  assert.deepEqual(green, [0, 1, 0, 1]);
  const half = text("A", { style: { color: "#0000ff80" } }).glyphs()[0]!.color;
  close(half[3], 128 / 255, "alpha from #..80");
  const tuple = text("A", { style: { color: [0.5, 0.5, 0.5, 1] } }).glyphs()[0]!.color;
  assert.deepEqual(tuple, [0.5, 0.5, 0.5, 1]);
});

test("an invalid hex channel sanitises to zero", () => {
  const { color } = text("A", { style: { color: "#zz" } }).glyphs()[0]!;
  close(color[0], 0, "non-finite channel → 0");
});

test("rich spans carry per-span style; unset spans inherit", () => {
  const glyphs = text([{ text: "A" }, { text: "B", style: { color: "#0000ff" } }]).glyphs();
  assert.deepEqual(glyphs[0]!.color, [1, 1, 1, 1]);
  assert.deepEqual(glyphs[1]!.color, [0, 0, 1, 1]);
});

test("the style cascade layers text-level under span-level", () => {
  const glyphs = text([{ text: "A" }, { text: "B", style: { fontSize: 32 } }], {
    style: { fontSize: 24, weight: 700, italic: true, letterSpacing: 2, lineHeight: 2 },
  }).glyphs();
  close(glyphs[0]!.fontSize, 24, "text-level font size");
  close(glyphs[1]!.fontSize, 32, "span overrides font size");
  // letterSpacing widens the advance: 24 * 0.6 + 2 = 16.4.
  close(glyphs[1]!.x, 24 * 0.6 + 2, "letter spacing folds into advance");
});

test("word wrap breaks a long line at the width", () => {
  assert.equal(text("AAAA BBBB").measure().lineCount, 1);
  const wrapped = text("AAAA BBBB", { layout: { width: 60, wrap: "word" } }).measure();
  assert.ok(wrapped.lineCount > 1, "narrow box wraps");
});

test("alignment shifts the line within the box", () => {
  const left = text("AB", { layout: { width: 200, align: "left" } }).glyphs()[0]!.x;
  const centre = text("AB", { layout: { width: 200, align: "center" } }).glyphs()[0]!.x;
  const right = text("AB", { layout: { width: 200, align: "right" } }).glyphs()[0]!.x;
  close(left, 0, "left flush");
  assert.ok(centre > left, "centre indents");
  assert.ok(right > centre, "right indents most");
});

test("fluent updates return new immutable values", () => {
  const base = text("0", { position: [10, 20] });
  assert.deepEqual(base.position, [10, 20]);
  assert.equal(chars(base.setText("100").glyphs()), "100");
  const recoloured = base.setStyle({ color: "#000000" }).glyphs()[0]!.color;
  assert.deepEqual(recoloured, [0, 0, 0, 1]);
  const moved = base.setPosition(5, 6);
  assert.deepEqual(moved.position, [5, 6]);
  close(moved.glyphs()[0]!.x, 5, "position offsets glyphs");
  assert.equal(base.setVisible(false).glyphs().length, 0);
  assert.equal(base.setVisible(false).visible, false);
  const aligned = base.setLayout({ width: 100, align: "right" });
  assert.ok(aligned.glyphs()[0]!.x > 0, "re-layout applies alignment");
});

test("space and world placement are carried through", () => {
  const world = text("A", { space: "world" });
  assert.equal(world.space, "world");
  assert.equal(text("A").space, "screen");
});

test("the `axiom` namespace exposes the same authoring entry", () => {
  assert.equal(chars(axiom.text("HI").glyphs()), "HI");
});
