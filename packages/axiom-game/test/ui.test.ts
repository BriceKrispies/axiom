import assert from "node:assert/strict";
import { test } from "node:test";

import { bindNative, boundHost } from "../src/host-binding.ts";
import { makeUi } from "../src/ui.ts";
import { solveLayout } from "../src/ui-layout.ts";
import type { Rgba } from "../src/vocabulary.ts";
import { FakeHost } from "./fake-host.ts";

const WHITE: Rgba = [1, 1, 1, 1];
const BLACK: Rgba = [0, 0, 0, 1];

// Runs FIRST, before any bindNative in this file, so boundHost() is the inert
// UNBOUND_UI: every UI draw is a safe no-op and every read a neutral value.
// (node:test isolates each file in its own process.)
test("the unbound UI surface is inert until a host is bound", () => {
  const inert = boundHost();
  assert.doesNotThrow(() => {
    inert.uiBeginFrame({ height: 1, width: 1 }, { x: 0, y: 0 }, false);
    inert.uiRect({ height: 1, width: 1, x: 0, y: 0 }, { fill: WHITE });
    inert.uiText("hp", { color: WHITE, size: 1, x: 0, y: 0 });
    inert.uiSprite(0, { height: 1, width: 1, x: 0, y: 0 });
  });
  assert.equal(inert.uiButton({ height: 1, width: 1, x: 0, y: 0 }, "ok", { fill: WHITE }), false);
  assert.deepEqual(inert.uiViewport(), { height: 0, width: 0 });
  assert.deepEqual([...inert.uiDrawList()], []);
  assert.deepEqual(inert.uiSolveLayout({ height: 1, width: 1 }, [1, 2]), []);
});

test("makeUi forwards every verb to the bound host", () => {
  const host = new FakeHost();
  bindNative(host);
  host.uiButtonReturn = true;
  host.uiViewportReturn = { height: 240, width: 320 };
  host.uiDrawListReturn = Uint8Array.from([1, 2, 3]);
  const ui = makeUi();

  ui.beginFrame({ height: 240, width: 320 }, { x: 5, y: 6 }, true);
  ui.rect({ height: 4, width: 3, x: 1, y: 2 }, { fill: WHITE, stroke: BLACK, strokeWidth: 2 });
  ui.text("score", { color: WHITE, size: 12, x: 8, y: 8 });
  ui.sprite(7, { height: 16, width: 16, x: 10, y: 10 });
  const activated = ui.button({ height: 20, width: 40, x: 100, y: 50 }, "ok", { fill: WHITE });
  // No style → the default transparent-fill button style (covers the orElse default).
  const defaulted = ui.button({ height: 20, width: 40, x: 0, y: 0 }, "x");

  assert.deepEqual(host.uiBeginFrames, [
    { pointer: { x: 5, y: 6 }, pressed: true, viewport: { height: 240, width: 320 } },
  ]);
  assert.deepEqual(host.uiRects, [
    { bounds: { height: 4, width: 3, x: 1, y: 2 }, style: { fill: WHITE, stroke: BLACK, strokeWidth: 2 } },
  ]);
  assert.deepEqual(host.uiTexts, [{ opts: { color: WHITE, size: 12, x: 8, y: 8 }, value: "score" }]);
  assert.deepEqual(host.uiSprites, [{ bounds: { height: 16, width: 16, x: 10, y: 10 }, texture: 7 }]);
  assert.deepEqual(host.uiButtons, [
    { bounds: { height: 20, width: 40, x: 100, y: 50 }, label: "ok", style: { fill: WHITE } },
    { bounds: { height: 20, width: 40, x: 0, y: 0 }, label: "x", style: { fill: [0, 0, 0, 0] } },
  ]);
  assert.equal(activated, true);
  assert.equal(defaulted, true);
  assert.deepEqual(ui.viewport(), { height: 240, width: 320 });
  assert.deepEqual([...ui.drawList()], [1, 2, 3]);
});

test("solveLayout translates a node tree and reshapes the solved rects", () => {
  const host = new FakeHost();
  bindNative(host);
  // The fake returns three stacked [x, y, w, h] rects, one per node, in input order.
  host.uiSolveLayoutReturn = [0, 0, 200, 100, 0, 0, 100, 100, 100, 0, 100, 100];
  const rects = solveLayout(
    {
      children: [
        { grow: 1, id: "left" },
        { direction: "column", grow: 1, id: "right" },
      ],
      direction: "row",
      id: "root",
    },
    { height: 100, width: 200, x: 0, y: 0 },
  );
  assert.deepEqual(rects.root, { height: 100, width: 200, x: 0, y: 0 });
  assert.deepEqual(rects.left, { height: 100, width: 100, x: 0, y: 0 });
  assert.deepEqual(rects.right, { height: 100, width: 100, x: 100, y: 0 });

  // The flat node table the solver received: 3 nodes × 7 columns
  // [parent, directionIdx, justify, align, gap, basis, grow]; the viewport drops its x/y.
  assert.equal(host.uiSolveLayoutCalls.length, 1);
  const call = host.uiSolveLayoutCalls[0]!;
  assert.deepEqual(call.viewport, { height: 100, width: 200 });
  assert.deepEqual(
    [...call.nodes],
    [
      -1, 0, 0, 0, 0, 0, 0, // root: a root (parent -1), direction row (0)
      0, 0, 0, 0, 0, 0, 1, // left: parent 0, direction row (default), grow 1
      0, 1, 0, 0, 0, 0, 1, // right: parent 0, direction column (1), grow 1
    ],
  );
});
