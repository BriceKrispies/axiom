/*
 * choice-input.ts — shared selection handling for every choice-population
 * game (chests, cards, doors, presents, digs, portals, rocks). Resolves
 * pointer hover via the picking helper, keyboard focus via grid navigation,
 * and the moment of selection — one behavior, seven games.
 */

import type { Camera3D, InputFrame } from "@axiom/web-engine";
import type { PickTarget } from "../presentation/cameras/picking.ts";
import { pickAt } from "../presentation/cameras/picking.ts";

export interface ChoiceCore {
  /** Pointer-hovered index (null when the pointer is off every target). */
  readonly hovered: number | null;
  /** Keyboard focus index (always valid; drawn as a focus ring). */
  readonly focused: number;
  /** The committed selection once made. */
  readonly selected: number | null;
  /** True while the pointer button was down over a target (pressed state). */
  readonly pressing: boolean;
}

export const initialChoice = (focused = 0): ChoiceCore => ({ focused, hovered: null, pressing: false, selected: null });

export interface ChoiceStepResult {
  readonly core: ChoiceCore;
  /** Set on the exact tick a selection happens. */
  readonly selectedNow: number | null;
}

/**
 * Fold one tick of input into the selection state. Keyboard: arrows move the
 * focus through a `columns`-wide grid, primary selects the focused target.
 * Pointer: hover tracks the target under the cursor; a press-release on a
 * hovered target selects it.
 */
export const stepChoice = (
  core: ChoiceCore,
  input: InputFrame,
  camera: Camera3D,
  targets: readonly PickTarget[],
  columns: number,
): ChoiceStepResult => {
  if (core.selected !== null || targets.length === 0) {
    return { core, selectedNow: null };
  }
  const count = targets.length;
  const dx = (input.pressed.has("right") ? 1 : 0) - (input.pressed.has("left") ? 1 : 0);
  const dy = (input.pressed.has("down") ? 1 : 0) - (input.pressed.has("up") ? 1 : 0);
  const moved = dx !== 0 || dy !== 0;
  const row = Math.floor(core.focused / columns);
  const col = core.focused % columns;
  const rows = Math.ceil(count / columns);
  const nextCol = Math.min(Math.max(col + dx, 0), columns - 1);
  const nextRow = Math.min(Math.max(row + dy, 0), rows - 1);
  const focused = moved ? Math.min(nextRow * columns + nextCol, count - 1) : core.focused;

  const hovered = pickAt(camera, targets, input.pointer);
  const pointerDown = input.pointer?.down ?? false;
  const clicked = core.pressing && !pointerDown && hovered !== null;
  const keyed = input.pressed.has("primary");

  const selectedNow = clicked ? hovered : keyed ? focused : null;
  return {
    core: {
      focused: hovered ?? focused,
      hovered,
      pressing: pointerDown && hovered !== null,
      selected: selectedNow,
    },
    selectedNow,
  };
};
