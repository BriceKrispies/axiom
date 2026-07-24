/*
 * glyph-font.test.ts — `node --test` coverage for the built-in 5×7 bitmap font
 * geometry in glyph-font.ts. Verifies run-length collapse (single strokes, edge
 * flush, multi-run rows, blank rows), case-folding, the unknown-glyph fallback,
 * and whole-string layout (column widths + per-glyph offsets).
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import {
  GLYPH_GAP,
  GLYPH_H,
  GLYPH_W,
  glyphRuns,
  textColumns,
  textRuns,
  type CellRun,
} from "./glyph-font.ts";

const hasRun = (runs: readonly CellRun[], row: number, col: number, len: number): boolean =>
  runs.some((run): boolean => run.row === row && run.col === col && run.len === len);

test("cell dimensions are 5×7 with a one-column gap", () => {
  assert.equal(GLYPH_W, 5);
  assert.equal(GLYPH_H, 7);
  assert.equal(GLYPH_GAP, 1);
});

test("a glyph collapses each row's lit cells into horizontal runs", () => {
  const runs = glyphRuns("A");
  // row 0 ".###." → one 3-cell run starting at col 1.
  assert.ok(hasRun(runs, 0, 1, 3), "top bar run");
  // row 3 "#####" → a full 5-cell run flushed at the right edge by the sentinel.
  assert.ok(hasRun(runs, 3, 0, 5), "full-width bar, edge-flushed");
  // row 1 "#...#" → two separate 1-cell runs.
  assert.ok(hasRun(runs, 1, 0, 1), "left leg");
  assert.ok(hasRun(runs, 1, 4, 1), "right leg");
});

test("a row of alternating cells yields several runs", () => {
  // "M" row 1 is "##.##" → runs at col 0 (len 2) and col 3 (len 2).
  const runs = glyphRuns("M");
  assert.ok(hasRun(runs, 1, 0, 2), "left pair");
  assert.ok(hasRun(runs, 1, 3, 2), "right pair");
});

test("lowercase is folded to uppercase", () => {
  assert.deepEqual(glyphRuns("a"), glyphRuns("A"));
});

test("space has no lit cells", () => {
  assert.equal(glyphRuns(" ").length, 0);
});

test("an unknown glyph falls back to a hollow box", () => {
  const runs = glyphRuns("~");
  assert.ok(hasRun(runs, 0, 0, 5), "solid top");
  assert.ok(hasRun(runs, 6, 0, 5), "solid bottom");
  assert.ok(hasRun(runs, 1, 0, 1), "left wall");
  assert.ok(hasRun(runs, 1, 4, 1), "right wall");
});

test("textColumns sums glyph widths and inter-glyph gaps", () => {
  assert.equal(textColumns(""), 0);
  assert.equal(textColumns("A"), GLYPH_W);
  assert.equal(textColumns("AB"), GLYPH_W * 2 + GLYPH_GAP);
});

test("textRuns offsets each glyph's runs by its column origin", () => {
  assert.deepEqual(textRuns(""), []);
  const runs = textRuns("AB");
  // "B" starts at column GLYPH_W + GLYPH_GAP = 6; its top row "####." is col 6, len 4.
  assert.ok(hasRun(runs, 0, GLYPH_W + GLYPH_GAP, 4), "second glyph offset");
  // The first glyph's strokes still sit at their un-offset columns.
  assert.ok(hasRun(runs, 0, 1, 3), "first glyph unmoved");
});
