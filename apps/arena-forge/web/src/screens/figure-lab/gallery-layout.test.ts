import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../../sim/content/bundle.ts";
import { buildCatalog, queryCatalog } from "./catalog.ts";
import { CAPTION_H, HEADER_H, clampIcon, layoutGallery } from "./gallery-layout.ts";

const content = loadDefaultContent();
const catalog = buildCatalog(content);
const all = queryCatalog(catalog, { group: "all", search: "", sort: "tribe" });

test("every entry gets exactly one cell, in order", () => {
  const layout = layoutGallery(all, 800, 100, "tribe");
  assert.equal(layout.cells.length, all.length);
  assert.deepEqual(
    layout.cells.map((c) => c.index),
    all.map((_, i) => i),
  );
});

test("columns follow the available width and the icon size", () => {
  const wide = layoutGallery(all, 800, 100, "name");
  const narrow = layoutGallery(all, 360, 100, "name");
  const big = layoutGallery(all, 800, 240, "name");
  assert.ok(wide.columns > narrow.columns, "narrower fits fewer columns");
  assert.ok(wide.columns > big.columns, "bigger icons fit fewer columns");
  assert.ok(narrow.columns >= 1, "never fewer than one column");
});

test("a one-column layout still lays out every entry without overlap", () => {
  const layout = layoutGallery(all, 60, 260, "name");
  assert.equal(layout.columns, 1);
  assert.equal(layout.cells.length, all.length);
  const ys = layout.cells.map((c) => c.y);
  for (let i = 1; i < ys.length; i += 1) {
    assert.ok((ys[i] as number) > (ys[i - 1] as number), "each row advances");
  }
});

test("the tribe sort emits one header per tribe, counting its members", () => {
  const layout = layoutGallery(all, 800, 100, "tribe");
  const labels = layout.headers.map((hh) => hh.label);
  assert.deepEqual(labels, ["Ironbound", "Emberkin", "Bloomtide", "Echowisp", "Neutral"]);
  assert.equal(
    layout.headers.reduce((n, hh) => n + hh.count, 0),
    all.length,
    "headers account for every entry",
  );
  assert.deepEqual(
    layout.headers.map((hh) => hh.count),
    [8, 8, 8, 8, 4],
  );
});

test("a section always starts on a fresh row below its header", () => {
  const layout = layoutGallery(all, 800, 100, "tribe");
  for (const header of layout.headers) {
    const first = layout.cells.find((c) => c.y >= header.y + HEADER_H);
    assert.ok(first !== undefined, "a section has cells");
    assert.equal(first?.x, 8, "the first cell of a section is at the left pad");
  }
});

test("unsectioned sorts emit no headers", () => {
  for (const sort of ["name", "tier", "attack", "health"] as const) {
    assert.equal(layoutGallery(all, 800, 100, sort).headers.length, 0, sort);
  }
});

test("content height covers the last row plus its caption", () => {
  const layout = layoutGallery(all, 800, 100, "tribe");
  const lowest = layout.cells.reduce((m, c) => Math.max(m, c.y + c.size), 0);
  assert.ok(layout.contentHeight >= lowest + CAPTION_H, "the last caption is inside the content");
});

test("an empty gallery has no cells and no headers", () => {
  const layout = layoutGallery([], 800, 100, "tribe");
  assert.deepEqual(layout.cells, []);
  assert.deepEqual(layout.headers, []);
});

test("icon size is clamped to the resize range", () => {
  assert.equal(clampIcon(10), 44);
  assert.equal(clampIcon(1000), 260);
  assert.equal(clampIcon(120), 120);
});
