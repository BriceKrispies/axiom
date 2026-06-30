import assert from "node:assert/strict";
import { test } from "node:test";

import { UNBOUND_UI } from "./ui-binding.ts";
import type { Rgba } from "./vocabulary.ts";

const WHITE: Rgba = [1, 1, 1, 1];

test("the inert UNBOUND_UI surface makes every draw a safe no-op", () => {
  assert.doesNotThrow(() => {
    UNBOUND_UI.uiBeginFrame({ height: 1, width: 1 }, { x: 0, y: 0 }, false);
    UNBOUND_UI.uiRect({ height: 1, width: 1, x: 0, y: 0 }, { fill: WHITE });
    UNBOUND_UI.uiText("hp", { color: WHITE, font: { family: "monospace", size: 1 }, pos: { x: 0, y: 0 } });
    UNBOUND_UI.uiSprite(0, { pos: { x: 0, y: 0 } });
  });
});

test("the inert UNBOUND_UI read-back verbs return neutral total values", () => {
  assert.equal(UNBOUND_UI.uiButton({ height: 1, width: 1, x: 0, y: 0 }, "ok", { fill: WHITE }), false);
  assert.deepEqual(UNBOUND_UI.uiViewport(), { height: 0, width: 0 });
  assert.deepEqual([...UNBOUND_UI.uiDrawList()], []);
  assert.deepEqual(UNBOUND_UI.uiSolveLayout({ height: 1, width: 1 }, [1, 2]), []);
});
