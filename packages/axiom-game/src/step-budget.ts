/*
 * The integer step budget that crosses the accumulator -> loop boundary, the TS
 * mirror of the Rust `axiom_frame::StepBudget` (and the `StepReport` the
 * axiom-game-runtime wasm boundary exports). Every field is an explicit integer
 * count: `steps` drives the deterministic sim; `remainderNanos / fixedStepNanos`
 * is the presentation-only `0..1` interpolation fraction the renderer wants.
 */

/** How many fixed steps a frame runs, plus the sub-step remainder banked after. */
export interface StepBudget {
  /** The number of fixed simulation steps to run this frame. */
  readonly steps: number;
  /** Sub-step time left banked, in `[0, fixedStepNanos)`. */
  readonly remainderNanos: number;
  /** The fixed step size, so the loop can compute `remainderNanos / fixedStepNanos`. */
  readonly fixedStepNanos: number;
}

/** The `0..1` interpolation fraction between the last two ticks for this budget. */
export const interpolationAlpha = (budget: StepBudget): number =>
  budget.remainderNanos / budget.fixedStepNanos;
