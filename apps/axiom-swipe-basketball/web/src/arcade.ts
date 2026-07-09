/*
 * arcade.ts — the 60-second score-attack STATE MACHINE, pure and SDK-free (no
 * physics, no rendering), so every scoring rule is unit-testable in bare Node. The
 * session owns one `ArcadeState` and drives it: `startIfReady` on the first shot,
 * `tick` every fixed step, `registerShot` on release, `registerMake`/`registerMiss`
 * on the outcome. The scene + HUD only read the state.
 *
 * Rules (from the brief):
 *   - normal make 2, swish 3, bank 3, golden 5 base points;
 *   - every STREAK_STEP consecutive makes bumps the multiplier by 1, capped at
 *     STREAK_MULT_CAP; a miss resets it to 1×;
 *   - during the final FINAL_TICKS the awarded points are doubled;
 *   - at 0 the round is over (best score is banked).
 */

import {
  FINAL_MULTIPLIER,
  FINAL_TICKS,
  GOLDEN_EVERY,
  POINTS_BANK,
  POINTS_GOLDEN,
  POINTS_NORMAL,
  POINTS_SWISH,
  ROUND_TICKS,
  STREAK_MULT_CAP,
  STREAK_STEP,
} from "./constants.ts";

/** The three phases of a round. */
export type RoundPhase = "ready" | "playing" | "gameover";

/** A made shot's quality, from what the ball touched on the way in. */
export type ShotQuality = "swish" | "bank" | "rim";

/** A feedback event the HUD floats as text / flashes on. */
export type FeedbackKind = "swish" | "bank" | "gold" | "streak" | "timebonus" | "miss";

/** One feedback event (drained by the HUD each frame). */
export interface ArcadeEvent {
  readonly kind: FeedbackKind;
  /** The floating-text label, e.g. "SWISH +6", "STREAK ×2", "GOLD BALL". */
  readonly text: string;
  /** A big event (golden / streak-up) earns a stronger flash + shake. */
  readonly big: boolean;
}

/** The whole score-attack state. */
export interface ArcadeState {
  phase: RoundPhase;
  score: number;
  best: number;
  /** Ticks left in the round. */
  timeRemaining: number;
  /** Consecutive makes without a miss. */
  consecutiveMakes: number;
  /** Current streak multiplier (1 … STREAK_MULT_CAP). */
  multiplier: number;
  shots: number;
  makes: number;
  /** Feedback events since the HUD last drained them. */
  events: ArcadeEvent[];
}

/** Classify a made shot from what it touched on the way through. */
export const classifyShot = (touchedRim: boolean, touchedBackboard: boolean): ShotQuality =>
  touchedBackboard ? "bank" : touchedRim ? "rim" : "swish";

/** Base points for a quality + golden flag (golden trumps all). */
export const basePoints = (quality: ShotQuality, golden: boolean): number =>
  golden ? POINTS_GOLDEN : quality === "bank" ? POINTS_BANK : quality === "swish" ? POINTS_SWISH : POINTS_NORMAL;

/** Whether the spawned-ball index (1-based count) is a golden ball. */
export const isGoldenSpawn = (spawnIndex: number): boolean => spawnIndex % GOLDEN_EVERY === 0;

/** A fresh round, carrying `best` forward. */
export const newRound = (best: number): ArcadeState => ({
  best,
  consecutiveMakes: 0,
  events: [],
  makes: 0,
  multiplier: 1,
  phase: "ready",
  score: 0,
  shots: 0,
  timeRemaining: ROUND_TICKS,
});

/** True while the round is in its final doubling window. */
export const inFinalWindow = (state: ArcadeState): boolean =>
  state.phase === "playing" && state.timeRemaining <= FINAL_TICKS;

/** Begin the round on the first shot (ready → playing). No-op otherwise. */
export const startIfReady = (state: ArcadeState): void => {
  if (state.phase === "ready") {
    state.phase = "playing";
  }
};

/** Bank the best score (called when the round ends). */
const bankBest = (state: ArcadeState): void => {
  state.best = Math.max(state.best, state.score);
};

/** Advance the round clock one tick; end the round at zero. */
export const tick = (state: ArcadeState): void => {
  if (state.phase !== "playing") {
    return;
  }
  state.timeRemaining -= 1;
  if (state.timeRemaining <= 0) {
    state.timeRemaining = 0;
    state.phase = "gameover";
    bankBest(state);
  }
};

/** Count a released shot. */
export const registerShot = (state: ArcadeState): void => {
  state.shots += 1;
};

/** Record a made basket: award points (streak × final double), bump the streak, emit events. */
export const registerMake = (state: ArcadeState, quality: ShotQuality, golden: boolean): void => {
  const base = basePoints(quality, golden);
  const doubled = inFinalWindow(state);
  const points = base * state.multiplier * (doubled ? FINAL_MULTIPLIER : 1);
  state.score += points;
  state.makes += 1;
  state.consecutiveMakes += 1;

  const bumped = Math.min(1 + Math.floor(state.consecutiveMakes / STREAK_STEP), STREAK_MULT_CAP);
  const leveledUp = bumped > state.multiplier;
  state.multiplier = bumped;
  bankBest(state);

  // Quality / golden headline.
  const qualityEvent: ArcadeEvent = golden
    ? { big: true, kind: "gold", text: `GOLD BALL +${points}` }
    : quality === "bank"
      ? { big: false, kind: "bank", text: `BANK +${points}` }
      : quality === "swish"
        ? { big: false, kind: "swish", text: `SWISH +${points}` }
        : { big: false, kind: "swish", text: `+${points}` };
  state.events.push(qualityEvent);
  if (leveledUp) {
    state.events.push({ big: true, kind: "streak", text: `STREAK ×${state.multiplier}` });
  }
  if (doubled) {
    state.events.push({ big: false, kind: "timebonus", text: "TIME BONUS ×2" });
  }
};

/** Record a missed shot: break the streak. */
export const registerMiss = (state: ArcadeState): void => {
  const hadStreak = state.consecutiveMakes > 0;
  state.consecutiveMakes = 0;
  state.multiplier = 1;
  if (hadStreak) {
    state.events.push({ big: false, kind: "miss", text: "STREAK LOST" });
  }
};

/** Restart from game-over on a tap / R (best is preserved). */
export const restart = (state: ArcadeState): ArcadeState => newRound(state.best);

/** Drain and clear the pending feedback events (the HUD calls this each frame). */
export const drainEvents = (state: ArcadeState): readonly ArcadeEvent[] => {
  const out = state.events.slice();
  state.events.length = 0;
  return out;
};
