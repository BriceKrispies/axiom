/*
 * The deterministic fixed-step accumulator (stepper.ts) — the pure node-testable
 * core split out of the (platform-edge) `raf-loop.ts` driver. `FixedStepper`
 * converts irregular wall-clock frame times into an exact count of fixed
 * simulation steps: it clamps any single advance to `MAX_ADVANCE_MS` (a
 * background-tab stall must not fast-forward the sim) and DROPS — never banks —
 * whole-step time beyond `maxCatchUpSteps` (a spiral-of-death guard: a slow
 * frame runs a bounded catch-up, then the sim simply loses the rest). No DOM,
 * no wall clock — milliseconds in, fixed step counts out — so it is a fully
 * covered, branchless spine unit.
 */

/** A single `advance` never feeds more than this much wall time into the
 * accumulator; a long stall (background tab, debugger pause) is truncated. */
const MAX_ADVANCE_MS = 100;

/** Milliseconds in one second — the step period is `MS_PER_SECOND / fixedHz`. */
const MS_PER_SECOND = 1000;

/** The smallest wall time a single advance may contribute (a negative elapsed,
 * from a clock that ran backwards, is treated as no time at all). */
const MIN_ADVANCE_MS = 0;

/** A pure fixed-step accumulator: wall milliseconds in, fixed step counts out. */
export class FixedStepper {
  readonly #stepMs: number;
  readonly #maxCatchUpSteps: number;
  #accMs = 0;
  #tick = 0;

  public constructor(fixedHz: number, maxCatchUpSteps: number) {
    this.#stepMs = MS_PER_SECOND / fixedHz;
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
   *
   * The residual is `(accMs - steps·stepMs) mod stepMs` in every case: when no
   * cap is hit that value is already the sub-step fraction (a modulo below the
   * divisor is the identity), and when the cap is hit the modulo drops the
   * excess whole steps — one branchless expression for both the run-all and the
   * drop-the-rest paths.
   */
  public advance(elapsedMs: number): number {
    this.#accMs += Math.min(Math.max(elapsedMs, MIN_ADVANCE_MS), MAX_ADVANCE_MS);
    const due = Math.floor(this.#accMs / this.#stepMs);
    const steps = Math.min(due, this.#maxCatchUpSteps);
    this.#accMs = (this.#accMs - steps * this.#stepMs) % this.#stepMs;
    this.#tick += steps;
    return steps;
  }
}
