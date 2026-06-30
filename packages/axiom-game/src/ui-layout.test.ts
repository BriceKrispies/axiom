import assert from "node:assert/strict";
import { test } from "node:test";

import { solveLayout } from "./ui-layout.ts";
import { bindNative } from "./host-binding.ts";
import { FakeHost } from "./fake-host.testkit.ts";

test("solveLayout flattens a node tree, feeds the solver the flat table, and reshapes the rects", () => {
  const host = new FakeHost();
  bindNative(host);
  // The fake returns three stacked [x, y, w, h] rects, one per node, in input order.
  host.uiSolveLayoutReturn = [0, 0, 200, 100, 0, 0, 100, 100, 100, 0, 100, 100];

  const rects = solveLayout(
    {
      children: [
        // explicit basis exercises the present `orElse(node.basis, …)` path;
        // omitted direction exercises the "row" default.
        { basis: 50, grow: 1, id: "left" },
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
      -1, 0, 0, 0, 0, 0, 0, // root: a root (parent -1), direction row (0), basis/grow default
      0, 0, 0, 0, 0, 50, 1, // left: parent 0, direction row (default), basis 50, grow 1
      0, 1, 0, 0, 0, 0, 1, // right: parent 0, direction column (1), basis default, grow 1
    ],
  );
});

test("solveLayout solves a lone leaf root (no children) into a single keyed rect", () => {
  const host = new FakeHost();
  bindNative(host);
  host.uiSolveLayoutReturn = [3, 4, 30, 40];

  const rects = solveLayout({ id: "solo" }, { height: 40, width: 30, x: 0, y: 0 });

  assert.deepEqual(rects, { solo: { height: 40, width: 30, x: 3, y: 4 } });
  // A leaf root: parent -1, defaulted direction/basis/grow all 0.
  assert.deepEqual([...host.uiSolveLayoutCalls[0]!.nodes], [-1, 0, 0, 0, 0, 0, 0]);
});
