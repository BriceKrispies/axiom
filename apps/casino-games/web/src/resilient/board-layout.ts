/*
 * board-layout.ts — where the nine form buttons have to SIT so that pressing a
 * chest is pressing a chest.
 *
 * THE PROBLEM THIS SOLVES, AND WHY IT IS SOLVED THIS WAY ROUND. The engine
 * draws the board as a perspective 3×3 grid on a ground plane; a `<fieldset>`
 * lays its controls out as a flat CSS grid. Those two do not agree, and only
 * one of them is negotiable. The buttons are the game — they are what submits,
 * what focus lands on, what a screen reader announces, and what still works
 * with the script deleted — so the buttons are MOVED to meet the render, never
 * the other way round.
 *
 * So this file projects each chest's world anchor through the game's OWN camera
 * (`chestCamera`) with the game's OWN projection (`worldToCanvas`) and hands
 * back a box in PERCENTAGES of the canvas. Percentages because the canvas has a
 * fixed logical 960×600 backing store that CSS stretches: a percentage is exact
 * at every display size, with no resize listener to keep in sync and nothing to
 * drift.
 *
 * `pickX`/`pickY` are the same projected anchor `pickAt` hit-tests against, in
 * the same logical space — so feeding the game a pointer sample there selects
 * exactly the chest whose button was pressed, by construction rather than by
 * tuning.
 *
 * Pure: no DOM, no engine, no canvas. The one thing that could go wrong — a
 * chest that does not project — is reported by ABSENCE (a shorter list), which
 * the caller treats as "this rung cannot be mounted" and demotes. There is no
 * fallback coordinate to be silently wrong with.
 */

import { CHEST_HEIGHT, CHEST_WIDTH, chestCamera, chestPosition } from "../games/treasure-chest-pick/game.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH, worldToCanvas } from "../presentation/cameras/picking.ts";

/** One chest's button box, and the pointer sample that selects it. */
export interface ChestPlacement {
  readonly index: number;
  /** Where a pointer must be, in LOGICAL canvas coordinates, to hit this chest. */
  readonly pickX: number;
  readonly pickY: number;
  /** The closed chest's screen box, as percentages of the canvas. */
  readonly leftPct: number;
  readonly topPct: number;
  readonly widthPct: number;
  readonly heightPct: number;
}

/**
 * Project every chest of a `count`-slot board. The returned list is index-
 * ordered and may be SHORTER than `count` if a chest failed to project; a
 * caller that needs the whole board must check the length.
 */
export const chestPlacements = (count: number): readonly ChestPlacement[] => {
  const camera = chestCamera(count);
  return Array.from({ length: count }, (unused, index) => index).flatMap((index) => {
    const base = chestPosition(index, count);
    // Four points of the closed chest: the ground anchor (which is also the
    // pick target), the top of the lid arch, and the two side edges taken at
    // mid-height so the box is measured across the chest's real width.
    const foot = worldToCanvas(camera, base);
    const crown = worldToCanvas(camera, { x: base.x, y: CHEST_HEIGHT, z: base.z });
    const left = worldToCanvas(camera, { x: base.x - CHEST_WIDTH / 2, y: CHEST_HEIGHT / 2, z: base.z });
    const right = worldToCanvas(camera, { x: base.x + CHEST_WIDTH / 2, y: CHEST_HEIGHT / 2, z: base.z });
    if (foot === null || crown === null || left === null || right === null) {
      return [];
    }
    return [
      {
        heightPct: ((foot.y - crown.y) / CANVAS_HEIGHT) * 100,
        index,
        leftPct: (left.x / CANVAS_WIDTH) * 100,
        pickX: foot.x,
        pickY: foot.y,
        topPct: (crown.y / CANVAS_HEIGHT) * 100,
        widthPct: ((right.x - left.x) / CANVAS_WIDTH) * 100,
      },
    ];
  });
};
