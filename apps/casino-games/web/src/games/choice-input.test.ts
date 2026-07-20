/*
 * choice-input.test.ts — the selection contract shared by every choice game,
 * with focus on the opt-in `tapToConfirm` mode used by Treasure Chest Pick:
 *
 *  - default mode: a single press-release on a hovered target selects it;
 *  - tap-to-confirm: a TOUCH (no prior hover) takes two taps — the first arms
 *    (highlights) the target, the second opens it — while a DESKTOP click still
 *    opens in one action, because the mouse hovers (and so arms) the target
 *    before the click.
 */

import assert from "node:assert/strict";
import test from "node:test";

import type { InputFrame, PointerSample } from "@axiom/web-engine";
import { worldToCanvas } from "../presentation/cameras/picking.ts";
import { chestCamera, chestTargets } from "./treasure-chest-pick/game.ts";
import { initialChoice, stepChoice, type ChoiceCore } from "./choice-input.ts";

const CAMERA = chestCamera(9);
const TARGETS = chestTargets(9);
const COLUMNS = 3;

/** The on-screen point at the centre of chest `index`. */
const screenOf = (index: number): { readonly x: number; readonly y: number } => {
  const s = worldToCanvas(CAMERA, TARGETS[index]!.at);
  assert.ok(s !== null, `chest ${index} must be in front of the camera`);
  return s;
};

/** One input frame carrying a pointer sample (or no pointer at all). */
const frame = (pointer: PointerSample | undefined): InputFrame => ({
  down: new Set(),
  look: { x: 0, y: 0 },
  pointer,
  pressed: new Set(),
  released: new Set(),
});

const overChest = (index: number, down: boolean): PointerSample => ({ down, pos: screenOf(index) });

/** Fold a sequence of frames through stepChoice, returning every step result. */
const drive = (start: ChoiceCore, tapToConfirm: boolean, frames: readonly InputFrame[]) => {
  let core = start;
  return frames.map((f) => {
    const result = stepChoice(core, f, CAMERA, TARGETS, COLUMNS, tapToConfirm);
    core = result.core;
    return result;
  });
};

test("tap-to-confirm: a touch takes two taps — first arms the chest, second opens it", () => {
  // A tap is finger-down then finger-up over the chest; the browser then fires
  // pointerleave (no pointer), which the picking layer sees as no hover.
  const tap = (index: number): readonly InputFrame[] => [
    frame(overChest(index, true)), // finger down
    frame(overChest(index, false)), // finger up (sample survives one tick)
    frame(undefined), // pointerleave clears the pointer
  ];

  const steps = drive(initialChoice(4), true, [...tap(4), ...tap(4)]);

  // First tap: nothing selected, but chest 4 is now armed (highlighted).
  assert.deepEqual(
    steps.slice(0, 3).map((s) => s.selectedNow),
    [null, null, null],
    "the first tap must not open any chest",
  );
  assert.equal(steps[2]!.core.armed, 4, "the first tap arms the tapped chest");

  // Second tap on the armed chest opens it.
  assert.equal(steps[4]!.selectedNow, 4, "the second tap on the armed chest opens it");
});

test("tap-to-confirm: tapping a different chest re-arms rather than opening", () => {
  // Arm chest 4, then tap chest 0: chest 0 must only become armed, not open.
  const steps = drive(initialChoice(4), true, [
    frame(overChest(4, true)),
    frame(overChest(4, false)), // arms 4
    frame(undefined),
    frame(overChest(0, true)),
    frame(overChest(0, false)), // taps 0 while 4 was armed
  ]);
  assert.equal(steps[1]!.core.armed, 4, "chest 4 armed after its tap");
  assert.equal(steps[4]!.selectedNow, null, "tapping a different chest must not open it");
  assert.equal(steps[4]!.core.armed, 0, "it re-arms to the newly tapped chest");
});

test("tap-to-confirm: a desktop hover-then-click still opens in one click", () => {
  // The mouse hovers (button up) before pressing, which arms the chest, so the
  // press-release opens it immediately — desktop keeps its one-click feel.
  const steps = drive(initialChoice(4), true, [
    frame(overChest(0, false)), // hover: arms 0
    frame(overChest(0, true)), // press
    frame(overChest(0, false)), // release
  ]);
  assert.equal(steps[0]!.core.armed, 0, "hovering arms the chest under the cursor");
  assert.equal(steps[2]!.selectedNow, 0, "the click opens the hovered/armed chest at once");
});

test("default mode (other choice games) still selects on a single press-release", () => {
  // present-pop / gem-mine pass no tapToConfirm flag: a first tap selects.
  const steps = drive(initialChoice(0), false, [
    frame(overChest(2, true)), // press with no prior hover
    frame(overChest(2, false)), // release
  ]);
  assert.equal(steps[1]!.selectedNow, 2, "the default mode opens on the first release");
  assert.equal(steps[1]!.core.armed, null, "armed stays null when the mode is off");
});
