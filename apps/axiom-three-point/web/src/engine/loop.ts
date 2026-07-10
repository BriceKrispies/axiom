/*
 * engine/loop.ts — the deterministic fixed-step game loop. `FixedStepper` is the
 * pure, node-testable core: an accumulator that converts irregular wall-clock
 * frame times into an exact count of fixed simulation steps, clamping any single
 * advance to `MAX_ADVANCE_MS` (a background-tab stall must not fast-forward the
 * sim) and DROPPING — never banking — whole-step time beyond `maxCatchUpSteps`
 * (a spiral-of-death guard: a slow frame runs a bounded catch-up, then the sim
 * simply loses the rest). `startLoop` is the thin browser driver: a
 * requestAnimationFrame chain that measures elapsed time with `performance.now()`,
 * runs `update` once per due fixed step, and `render`s once per animation frame.
 * No DOM access beyond requestAnimationFrame / performance.
 */

/** A single `advance` never feeds more than this much wall time into the
 * accumulator; a long stall (background tab, debugger pause) is truncated. */
const MAX_ADVANCE_MS = 100;

/** A pure fixed-step accumulator: wall milliseconds in, fixed step counts out. */
export class FixedStepper {
  readonly #stepMs: number;
  readonly #maxCatchUpSteps: number;
  #accMs = 0;
  #tick = 0;

  public constructor(fixedHz: number, maxCatchUpSteps: number) {
    this.#stepMs = 1000 / fixedHz;
    this.#maxCatchUpSteps = maxCatchUpSteps;
  }

  /** The total number of fixed steps issued so far (increments once per step
   * returned by `advance`). */
  public get tick(): number {
    return this.#tick;
  }

  /**
   * Feed `elapsedMs` of wall time (clamped to `MAX_ADVANCE_MS`) and return how
   * many fixed steps are due now, at most `maxCatchUpSteps`. Whole-step time
   * beyond the cap is dropped — only the sub-step fractional remainder is kept —
   * so a stall can never bank a burst of future catch-up steps.
   */
  public advance(elapsedMs: number): number {
    this.#accMs += Math.min(Math.max(elapsedMs, 0), MAX_ADVANCE_MS);
    const due = Math.floor(this.#accMs / this.#stepMs);
    const steps = Math.min(due, this.#maxCatchUpSteps);
    this.#accMs -= steps * this.#stepMs;
    if (due > steps) {
      this.#accMs %= this.#stepMs;
    }
    this.#tick += steps;
    return steps;
  }
}

/**
 * Drive a `FixedStepper` with requestAnimationFrame: each frame, measure the
 * elapsed wall time via `performance.now()`, call `update(tick)` once per due
 * fixed step (with that step's tick number), then call `render()` once. Returns
 * a stop function that cancels the rAF chain.
 */
export function startLoop(opts: {
  fixedHz: number;
  maxCatchUpSteps: number;
  update: (tick: number) => void;
  render: () => void;
}): () => void {
  const stepper = new FixedStepper(opts.fixedHz, opts.maxCatchUpSteps);
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
    for (let i = 0; i < steps; i += 1) {
      opts.update(firstTick + i);
    }
    opts.render();
    rafId = requestAnimationFrame(frame);
  };

  rafId = requestAnimationFrame(frame);
  return (): void => {
    stopped = true;
    cancelAnimationFrame(rafId);
  };
}
