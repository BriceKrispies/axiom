/*
 * board-layout.test.ts — the buttons have to land ON the chests.
 *
 * This is the load-bearing claim of the engine rung: the form control a player
 * presses must sit over the chest the engine painted, and the pointer sample
 * the game is then fed must select that same chest. Both are pure projections,
 * so both are pinned here rather than eyeballed in a screenshot.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { chestCamera, chestTargets } from "../games/treasure-chest-pick/game.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH, pickAt } from "../presentation/cameras/picking.ts";
import { chestPlacements } from "./board-layout.ts";

const COUNT = 9;

test("every chest on the shipped board projects — the rung is mountable", () => {
  // The caller treats a short list as "this rung cannot be mounted" and demotes,
  // so a full list is the precondition for the engine board existing at all.
  const placements = chestPlacements(COUNT);
  assert.equal(placements.length, COUNT);
  assert.deepEqual(
    placements.map((placement) => placement.index),
    Array.from({ length: COUNT }, (unused, index) => index),
  );
});

test("the pick point selects the chest it was computed for, and only that one", () => {
  const camera = chestCamera(COUNT);
  const targets = chestTargets(COUNT);
  chestPlacements(COUNT).forEach((placement) => {
    const hit = pickAt(camera, targets, { down: false, pos: { x: placement.pickX, y: placement.pickY } });
    assert.equal(hit, placement.index, `pick point for chest ${placement.index} selected ${String(hit)}`);
  });
});

test("every button box is a real, on-screen rectangle", () => {
  chestPlacements(COUNT).forEach((placement) => {
    assert.ok(placement.widthPct > 0, `chest ${placement.index} has no width`);
    assert.ok(placement.heightPct > 0, `chest ${placement.index} has no height`);
    assert.ok(placement.leftPct >= 0, `chest ${placement.index} runs off the left edge`);
    assert.ok(placement.topPct >= 0, `chest ${placement.index} runs off the top edge`);
    assert.ok(placement.leftPct + placement.widthPct <= 100, `chest ${placement.index} runs off the right edge`);
    assert.ok(placement.topPct + placement.heightPct <= 100, `chest ${placement.index} runs off the bottom edge`);
    // and the pick point belongs to its own box: horizontally within it, and
    // vertically ON its bottom edge — the box runs from the top of the lid arch
    // down to the chest's base, and the base IS the world anchor the game
    // hit-tests against. So the control and the target cannot end up describing
    // two different chests.
    const x = (placement.pickX / CANVAS_WIDTH) * 100;
    const y = (placement.pickY / CANVAS_HEIGHT) * 100;
    assert.ok(x > placement.leftPct && x < placement.leftPct + placement.widthPct);
    assert.ok(Math.abs(y - (placement.topPct + placement.heightPct)) < 1e-9);
  });
});

test("no two buttons overlap — a press is never ambiguous", () => {
  const placements = chestPlacements(COUNT);
  placements.forEach((a) => {
    placements
      .filter((b) => b.index > a.index)
      .forEach((b) => {
        const apart =
          a.leftPct + a.widthPct <= b.leftPct ||
          b.leftPct + b.widthPct <= a.leftPct ||
          a.topPct + a.heightPct <= b.topPct ||
          b.topPct + b.heightPct <= a.topPct;
        assert.ok(apart, `chests ${a.index} and ${b.index} overlap`);
      });
  });
});

test("the board reads in DOM order: rows back-to-front, left to right", () => {
  // Chest 1 is the top-left button AND the top-left chest. If the projection
  // ever reordered the grid, keyboard tab order would stop matching what is on
  // screen even though every individual hit test still passed.
  const placements = chestPlacements(COUNT);
  [0, 3, 6].forEach((row) => {
    assert.ok((placements[row + 1] as { leftPct: number }).leftPct > (placements[row] as { leftPct: number }).leftPct);
    assert.ok((placements[row + 2] as { leftPct: number }).leftPct > (placements[row + 1] as { leftPct: number }).leftPct);
  });
  [0, 1, 2].forEach((column) => {
    assert.ok((placements[column + 3] as { topPct: number }).topPct > (placements[column] as { topPct: number }).topPct);
    assert.ok((placements[column + 6] as { topPct: number }).topPct > (placements[column + 3] as { topPct: number }).topPct);
  });
});
