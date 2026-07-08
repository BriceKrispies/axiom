/*
 * The deterministic input contract — the TS twin of `PenaltyInputIntent`. One
 * struct = "what the player asks for this tick", fully device-decoupled: the
 * harness/game maps real keyboard+mouse into this, and the interaction state
 * machine consumes only this. No browser APIs here.
 *
 * Aim axes are integers in [-100, 100] (full deflection = ±100). Precedence
 * within a tick, enforced downstream: reset > release > charge > hold.
 */

export const AIM_AXIS_MIN = -100;
export const AIM_AXIS_MAX = 100;

export const clampAxis = (v: number): number => Math.min(Math.max(Math.trunc(v), AIM_AXIS_MIN), AIM_AXIS_MAX);

export interface PenaltyInputIntent {
  /** Horizontal aim: negative = left, positive = right, [-100, 100]. */
  readonly aimXAxis: number;
  /** Vertical aim: negative = down, positive = up, [-100, 100]. */
  readonly aimYAxis: number;
  /** The player is holding the shot button (charging power). */
  readonly chargePressed: boolean;
  /** The player released the shot button this tick (freeze / lock). */
  readonly releasePressed: boolean;
  /** Reset aim + power to the start of the shot. */
  readonly resetPressed: boolean;
  /** Continue from a between-rounds / session-complete prompt. */
  readonly continuePressed: boolean;
}

export const NEUTRAL_INTENT: PenaltyInputIntent = {
  aimXAxis: 0,
  aimYAxis: 0,
  chargePressed: false,
  releasePressed: false,
  resetPressed: false,
  continuePressed: false,
};

/** Build an intent, clamping the axes to their legal integer range. */
export const makeIntent = (partial: Partial<PenaltyInputIntent>): PenaltyInputIntent => ({
  aimXAxis: clampAxis(partial.aimXAxis ?? 0),
  aimYAxis: clampAxis(partial.aimYAxis ?? 0),
  chargePressed: partial.chargePressed ?? false,
  releasePressed: partial.releasePressed ?? false,
  resetPressed: partial.resetPressed ?? false,
  continuePressed: partial.continuePressed ?? false,
});
