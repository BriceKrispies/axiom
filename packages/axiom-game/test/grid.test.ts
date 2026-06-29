import assert from "node:assert/strict";
import { test } from "node:test";

import {
  createGrid,
  gridDistanceField,
  gridPath,
  gridReachable,
  stepToward,
  tileSpace,
} from "../src/grid.ts";
import { bindNative } from "../src/host-binding.ts";
import { FakeHost } from "./fake-host.ts";

test("createGrid fills every cell and reports its dimensions", () => {
  const grid = createGrid(3, 2, 7);
  assert.equal(grid.cols, 3);
  assert.equal(grid.rows, 2);
  assert.equal(grid.get(0, 0), 7);
  assert.equal(grid.get(2, 1), 7);
});

test("idx is row-major and inBounds gates the grid rectangle", () => {
  const grid = createGrid(3, 2, 0);
  assert.equal(grid.idx(2, 1), 5);
  assert.equal(grid.inBounds(2, 1), true);
  assert.equal(grid.inBounds(3, 1), false);
  assert.equal(grid.inBounds(0, 2), false);
  assert.equal(grid.inBounds(-1, 0), false);
});

test("set writes a cell; get returns the default outside the grid", () => {
  const grid = createGrid(3, 2, 0);
  grid.set(1, 0, 5);
  assert.equal(grid.get(1, 0), 5);
  // Out-of-bounds read returns the default cell, never a wrapped/aliased cell:
  // idx(-1, 1) = 2 is a valid array slot, but inBounds is false so get is the default.
  assert.equal(grid.get(-1, 1), 0);
  assert.equal(grid.get(99, 99), 0);
});

test("an out-of-bounds set is a clean no-op", () => {
  const grid = createGrid(2, 2, 0);
  grid.set(-1, 0, 9);
  grid.set(5, 5, 9);
  assert.deepEqual([grid.get(0, 0), grid.get(1, 0), grid.get(0, 1), grid.get(1, 1)], [0, 0, 0, 0]);
});

test("fill overwrites every cell and clone is an independent copy", () => {
  const grid = createGrid(2, 1, 0);
  grid.fill(4);
  const copy = grid.clone();
  copy.set(0, 0, 9);
  assert.equal(grid.get(0, 0), 4);
  assert.equal(copy.get(0, 0), 9);
  assert.equal(copy.get(1, 0), 4);
});

test("forEach visits every cell in row-major order with its coordinates", () => {
  const grid = createGrid(2, 2, 0);
  grid.set(1, 0, 1);
  grid.set(0, 1, 2);
  const visited: [number, number, number][] = [];
  // eslint-disable-next-line unicorn/no-array-for-each -- exercising Grid's own forEach API, not Array.prototype.forEach
  grid.forEach((value, x, y) => {
    visited.push([value, x, y]);
  });
  assert.deepEqual(visited, [
    [0, 0, 0],
    [1, 1, 0],
    [2, 0, 1],
    [0, 1, 1],
  ]);
});

test("gridPath feeds the native core a passability mask and forwards its cells", () => {
  const host = new FakeHost();
  host.gridPathReturn = [
    { x: 0, y: 0 },
    { x: 1, y: 0 },
    { x: 1, y: 1 },
  ];
  bindNative(host);
  const grid = createGrid(2, 2, 0);
  grid.set(0, 1, 1); // blocked cell
  const path = gridPath(grid, { goal: { x: 1, y: 1 }, start: { x: 0, y: 0 } }, (value) => value === 0);
  assert.deepEqual(path, host.gridPathReturn);
  const call = host.gridPathCalls[0]!;
  assert.deepEqual(call.field, { cols: 2, passable: [true, true, false, true], rows: 2 });
  assert.deepEqual(call.start, { x: 0, y: 0 });
  assert.deepEqual(call.goal, { x: 1, y: 1 });
});

test("gridPath returns the empty value the core reports for an unreachable goal", () => {
  const host = new FakeHost();
  host.gridPathReturn = undefined;
  bindNative(host);
  const grid = createGrid(2, 2, 0);
  assert.equal(gridPath(grid, { goal: { x: 1, y: 1 }, start: { x: 0, y: 0 } }, () => false), undefined);
});

test("gridReachable forwards the mask and returns the core's verdict", () => {
  const host = new FakeHost();
  host.gridReachableReturn = true;
  bindNative(host);
  const grid = createGrid(2, 1, 0);
  assert.equal(gridReachable(grid, { goal: { x: 1, y: 0 }, start: { x: 0, y: 0 } }, () => true), true);
  assert.deepEqual(host.gridReachableCalls[0]!.field.passable, [true, true]);
});

test("gridDistanceField wraps the core's distances in a Grid with an Infinity default", () => {
  const host = new FakeHost();
  host.gridDistanceReturn = [0, 1, 1, 2];
  bindNative(host);
  const grid = createGrid(2, 2, 0);
  const field = gridDistanceField(grid, { x: 0, y: 0 }, (value) => value === 0);
  assert.equal(field.get(0, 0), 0);
  assert.equal(field.get(1, 1), 2);
  assert.equal(field.get(9, 9), Number.POSITIVE_INFINITY);
  assert.deepEqual(host.gridDistanceCalls[0]!.start, { x: 0, y: 0 });
});

test("stepToward forwards from/target and returns the chosen next cell", () => {
  const host = new FakeHost();
  host.gridStepReturn = { x: 1, y: 0 };
  bindNative(host);
  const grid = createGrid(3, 1, 0);
  const next = stepToward(grid, { goal: { x: 2, y: 0 }, start: { x: 0, y: 0 } }, () => true);
  assert.deepEqual(next, { x: 1, y: 0 });
  const call = host.gridStepCalls[0]!;
  assert.deepEqual(call.from, { x: 0, y: 0 });
  assert.deepEqual(call.target, { x: 2, y: 0 });
});

test("tileSpace maps tiles to cell centres and back, idempotently", () => {
  const space = tileSpace({ x: 10, y: 20 }, 4);
  assert.deepEqual(space.tileToWorld(0, 0), { x: 12, y: 22 });
  assert.deepEqual(space.tileToWorld(2, 1), { x: 20, y: 26 });
  assert.deepEqual(space.worldToTile({ x: 21, y: 27 }), { x: 2, y: 1 });
  // snapToCell lands on the nearest centre and is idempotent.
  const snapped = space.snapToCell({ x: 21, y: 27 });
  assert.deepEqual(snapped, { x: 20, y: 26 });
  assert.deepEqual(space.snapToCell(snapped), snapped);
});
