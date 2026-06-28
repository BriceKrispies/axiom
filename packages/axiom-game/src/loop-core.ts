/*
 * The pure, branchless, deterministic stepping core. Given a StepBudget and the
 * registered callbacks, it runs `budget.steps` fixed updates (constant dt) then
 * one render with `alpha = remainderNanos / fixedStepNanos`. It reads no clock and
 * touches no wasm — `GameLoop` and the platform edge supply the budget — so its
 * tests cover it fully with hand-crafted budgets and a fake bridge. Branchless:
 * the N updates run via `Array.from({length}).map`, never a `for`/`while`/`if`.
 */

import type { Frame, Sim } from "./sim.ts";
import { type StepBudget, interpolationAlpha } from "./step-budget.ts";
import { each } from "./branchless.ts";

/** A registered fixed-update callback: the only place sim state changes. */
export type FixedUpdate = (sim: Sim) => void;

/** A registered render callback: presentation only, reads `alpha`, never mutates. */
export type Render = (frame: Frame, alpha: number) => void;

/** Everything one frame's deterministic step needs, grouped to one argument. */
export interface FrameStep {
  readonly budget: StepBudget;
  readonly fixedUpdates: readonly FixedUpdate[];
  readonly renders: readonly Render[];
  readonly makeSim: (tick: number) => Sim;
  readonly makeFrame: (tick: number) => Frame;
  /** The monotonic tick the first fixed update of this frame runs at. */
  readonly startTick: number;
}

/*
 * Run the frame: `budget.steps` fixed updates at consecutive ticks, then one
 * render at the latest completed tick with the interpolation fraction. Returns
 * the next start tick (`startTick + steps`).
 */
export const stepFrame = (step: FrameStep): number => {
  const ticks = Array.from(
    { length: step.budget.steps },
    (_unused, offset): number => step.startTick + offset,
  );
  each(ticks, (tick): void => {
    each(step.fixedUpdates, (update): void => {
      update(step.makeSim(tick));
    });
  });
  const nextTick = step.startTick + step.budget.steps;
  const frame = step.makeFrame(nextTick);
  const alpha = interpolationAlpha(step.budget);
  each(step.renders, (render): void => {
    render(frame, alpha);
  });
  return nextTick;
};
