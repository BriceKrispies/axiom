/*
 * The platform-edge requestAnimationFrame driver for the pure `FixedStepper`
 * accumulator in `stepper.ts`. This is the impure boundary: it reads the wall
 * clock (`performance.now()`) and schedules frames (`requestAnimationFrame` /
 * `cancelAnimationFrame`). Each frame it measures the elapsed wall time, calls
 * `update(tick)` once per due fixed step (with that step's tick number), then
 * `render()`s once. Like the engine's other browser boundaries it sits OUTSIDE
 * the branchless / 100%-coverage spine laws (see the `.oxlintrc.json` override
 * and `test-exempt.json`); its correctness is proven by the live browser path,
 * and the deterministic stepping it drives is covered in `stepper.test.ts`.
 * Because the branch ban is off here, this file uses ordinary control flow.
 */

import { FixedStepper } from "./stepper.ts";

/** The per-frame hooks + fixed-step pacing the rAF driver needs. */
export interface LoopConfig {
  readonly fixedHz: number;
  readonly maxCatchUpSteps: number;
  /** Runs once per due fixed step, with that step's tick number. */
  readonly update: (tick: number) => void;
  /** Runs once per animation frame, after any fixed steps. */
  readonly render: () => void;
}

/**
 * Drive a `FixedStepper` with requestAnimationFrame: each frame, measure the
 * elapsed wall time via `performance.now()`, call `update(tick)` once per due
 * fixed step (with that step's tick number), then call `render()` once. Returns
 * a stop function that cancels the rAF chain.
 */
export const startLoop = (config: LoopConfig): (() => void) => {
  const stepper = new FixedStepper(config.fixedHz, config.maxCatchUpSteps);
  let lastMs = performance.now();
  let stopped = false;
  let rafId = 0;

  const frame = (): void => {
    if (stopped) {
      return;
    }
    const nowMs = performance.now();
    const steps = stepper.advance(nowMs - lastMs);
    lastMs = nowMs;
    const firstTick = stepper.tick - steps + 1;
    // Branch ban is off here: run `update` once per due step in wall-clock order.
    for (let stepOffset = 0; stepOffset < steps; stepOffset += 1) {
      config.update(firstTick + stepOffset);
    }
    config.render();
    rafId = requestAnimationFrame(frame);
  };

  rafId = requestAnimationFrame(frame);
  return (): void => {
    stopped = true;
    cancelAnimationFrame(rafId);
  };
};
