import assert from "node:assert/strict";
import { test } from "node:test";

import { makeUi } from "./ui.ts";
import { bindNative } from "./host-binding.ts";
import { FakeHost } from "./fake-host.testkit.ts";
import type { Rgba } from "./vocabulary.ts";

const WHITE: Rgba = [1, 1, 1, 1];
const BLACK: Rgba = [0, 0, 0, 1];

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
  ]);
  assert.equal(activated, true);
  assert.deepEqual(ui.viewport(), { height: 240, width: 320 });
  assert.deepEqual([...ui.drawList()], [1, 2, 3]);
});

test("button with no style supplies the default transparent-fill style (the orElse default)", () => {
  const host = new FakeHost();
  bindNative(host);
  host.uiButtonReturn = false;
  const ui = makeUi();

  const defaulted = ui.button({ height: 20, width: 40, x: 0, y: 0 }, "x");

  assert.equal(defaulted, false);
  assert.deepEqual(host.uiButtons, [
    { bounds: { height: 20, width: 40, x: 0, y: 0 }, label: "x", style: { fill: [0, 0, 0, 0] } },
  ]);
});
