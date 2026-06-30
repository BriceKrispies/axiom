/*
 * Out-of-gate INTEGRATION proof for SPEC-04 §7's last bullet: "@axiom/game's
 * Frame 2D methods ... a headless test draws each primitive and asserts the
 * marshalled command stream matches the native Draw2dList." It drives the REAL
 * `axiom-game-runtime` wasm in Node (no browser): the SDK's `Frame` surface
 * forwards every draw across the wasm boundary to the native `axiom-draw2d`
 * builder, and `frame.finish()` returns the flat, layer-sorted, self-describing
 * `[kind, layer, submission, len, …geometry]` stream `draw2d_finish` emits.
 *
 * This is NOT part of the @axiom/game coverage gate (the node:test unit suite
 * never loads wasm). It is an app-tier integration test — see README.md for how
 * to (re)build the `pkg/` bindings and run it.
 *
 * What it proves:
 *   1. Layer-sort golden — draws submitted out of layer order come back stably
 *      sorted by (layer, submission), including a within-layer tie.
 *   2. Geometry + colour marshalling — each primitive's flat payload matches the
 *      native per-kind layout (RECT/CIRCLE/LINE), with the SDK's `[r,g,b,a]`
 *      colours round-tripping through the boundary's packed `0xRRGGBBAA`.
 *   3. Determinism — the same Frame draws produce a byte-identical command stream.
 */

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";

import { bindNative, makeFrame } from "../../../packages/axiom-game/src/index.ts";
import { hostFromWasm } from "../../../packages/axiom-game/src/wasm-host.ts";

const require = createRequire(import.meta.url);
const { WasmGame } = require("./pkg/axiom_game_runtime.js");

const FIXED_STEP_NANOS = BigInt(Math.round(1_000_000_000 / 60));
const MAX_STEPS = 8;

// The native command kind codes (apps/axiom-game-runtime/src/draw2d.rs).
const KIND_RECT = 1;
const KIND_CIRCLE = 2;
const KIND_LINE = 4;

// The colours the SDK packs and the native list re-emits (`0xRRGGBBAA`).
const RED = 0xff00_00ff;
const BLUE = 0x0000_ffff;
const GREEN = 0x00ff_00ff;
const YELLOW = 0xffff_00ff;
const WHITE = 0xffff_ffff;

/**
 * Bind a fresh wasm host and draw a fixed scene OUT of layer order, returning the
 * finished flat command stream. Submission order is rectA(layer 2), circle(0),
 * line(1), rectB(0) — so layer 0 carries a circle→rectB tie that must keep its
 * submission order after the sort.
 */
const drawScene = () => {
  const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS);
  bindNative(hostFromWasm(game));
  const frame = makeFrame(0);
  frame.rect(
    { height: 40, width: 30, x: 10, y: 20 },
    { alpha: 1, fill: [1, 0, 0, 1], layer: 2, stroke: [0, 0, 1, 1], strokeWidth: 3 },
  );
  frame.circle({ x: 5, y: 6 }, 7, { alpha: 0.5, fill: [0, 1, 0, 1], layer: 0 });
  frame.line({ x: 1, y: 2 }, { x: 3, y: 4 }, { alpha: 1, color: [1, 1, 0, 1], layer: 1, width: 2 });
  frame.rect({ height: 1, width: 1, x: 0, y: 0 }, { alpha: 1, fill: [1, 1, 1, 1], layer: 0 });
  return frame.finish();
};

/**
 * Decode the self-describing `[kind, layer, submission, len, …payload]` stream
 * into `(kind, layer, payload)` records by advancing 4 + len past each command.
 */
const records = (list) => {
  const out = [];
  let i = 0;
  while (i < list.length) {
    const kind = list[i];
    const layer = list[i + 1];
    const len = list[i + 3];
    out.push({ kind, layer, payload: list.slice(i + 4, i + 4 + len) });
    i += 4 + len;
  }
  return out;
};

test("Frame 2D draws marshal to the native layer-sorted Draw2dList", () => {
  const recs = records(drawScene());

  // (1) Layer-sort golden: circle(0), rectB(0), line(1), rectA(2) — the layer-0
  // circle→rectB tie keeps its submission order, then layer 1, then layer 2.
  assert.deepEqual(
    recs.map((r) => ({ kind: r.kind, layer: r.layer })),
    [
      { kind: KIND_CIRCLE, layer: 0 },
      { kind: KIND_RECT, layer: 0 },
      { kind: KIND_LINE, layer: 1 },
      { kind: KIND_RECT, layer: 2 },
    ],
  );

  // (2) Geometry + colour marshalling, per the native per-kind payload layout.
  // CIRCLE: [cx, cy, radius, fillRGBA, strokeRGBA, strokeWidth, alpha]; the
  // omitted stroke packs transparent/zero, and alpha 0.5 rides through.
  assert.deepEqual(recs[0].payload, [5, 6, 7, GREEN, 0, 0, 0.5]);
  // RECT (the white layer-0 tie): [minX, minY, w, h, fillRGBA, strokeRGBA, strokeWidth, alpha].
  assert.deepEqual(recs[1].payload, [0, 0, 1, 1, WHITE, 0, 0, 1]);
  // LINE: [aX, aY, bX, bY, colorRGBA, width, alpha]; a line carries no fill/stroke.
  assert.deepEqual(recs[2].payload, [1, 2, 3, 4, YELLOW, 2, 1]);
  // RECT (the red/blue layer-2 draw): fill + stroke + strokeWidth all present.
  assert.deepEqual(recs[3].payload, [10, 20, 30, 40, RED, BLUE, 3, 1]);

  // (3) Determinism: the same Frame draws yield a byte-identical command stream.
  assert.deepEqual(drawScene(), drawScene());
});
