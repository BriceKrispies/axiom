/*
 * gameplay.ts — the pure, fully-deterministic heart of Heat Check. No randomness: a
 * shot's outcome is a pure function of state at release. The scoring model is built
 * around an ADVANTAGE WINDOW — you earn a transient edge by genuinely beating the
 * defender, it decays fast, and you must release before it closes. Advantage +
 * separation (the SPACE you created) dominate quality; timing can upgrade a good look
 * but never rescues a smothered one; dribble fatigue kills wiggle-spam. The readiness
 * the player reads (SPACE / RHYTHM / BALANCE) is derived from the same breakdown that
 * decides the shot, and the miss arc matches the dominant failure. See heat-check.test.ts.
 */

import { type Vec3, clamp, vec3 } from "./vec.ts";
import type { BalanceTag, Feedback, RhythmTag, ShotArc, ShotBreakdown, ShotReason, ShotResult, SpaceTag } from "./types.ts";
import * as C from "./constants.ts";

/** Clamp to the unit interval — the range every sub-score lives in. */
export const clamp01 = (v: number): number => clamp(v, 0, 1);

/** The rhythm meter's phase (0..1) at a given tick. */
export const rhythmPhaseAt = (tick: number): number =>
  ((tick % C.RHYTHM_PERIOD_TICKS) + C.RHYTHM_PERIOD_TICKS) % C.RHYTHM_PERIOD_TICKS / C.RHYTHM_PERIOD_TICKS;

// ── advantage + fatigue dynamics (pure, tested) ──────────────────────────────────

/** Advantage after one tick of decay — faster as the defender recovers (gets balanced). */
export const stepAdvantage = (advantage: number, defenderBalance: number): number =>
  clamp01(
    advantage -
      (C.ADVANTAGE_DECAY_PER_SECOND + C.ADVANTAGE_DEFENDER_RECOVERY_DECAY * clamp01(defenderBalance)) / C.FIXED_HZ,
  );

/** Advantage after beating the defender — reduced by fatigue and by repeat-move spam. */
export const gainAdvantage = (advantage: number, fatigue: number, isRepeat: boolean): number =>
  clamp01(
    advantage + C.ADVANTAGE_GAIN_CROSSOVER * (1 - clamp01(fatigue)) * (isRepeat ? C.ADVANTAGE_REPEAT_MOVE_PENALTY : 1),
  );

/** Fatigue bleeds off each tick while the player commits to a direction (no reversal). */
export const stepFatigue = (fatigue: number): number => clamp01(fatigue - C.DRIBBLE_FATIGUE_DECAY / C.FIXED_HZ);

/** Fatigue rises on a reversal that came too soon after the last one (wiggle spam). */
export const gainFatigue = (fatigue: number): number => clamp01(fatigue + C.DRIBBLE_FATIGUE_GAIN);

// ── sub-scores ───────────────────────────────────────────────────────────────────

/**
 * Lateral space: further from the defender is better, and a defender knocked off
 * balance yields extra room. Standing still on a recovered defender scores ~0.
 */
export const computeSeparationScore = (playerX: number, defenderX: number, defenderBalance: number): number => {
  const dist = Math.abs(playerX - defenderX);
  const base = clamp01(dist / C.SPACE_REQUIRED_FOR_CLEAN_SHOT);
  const offBalanceBonus = (1 - clamp01(defenderBalance)) * 0.35;
  return clamp01(base + offBalanceBonus);
};

/** Release timing: peaks at the rhythm's center, falls off to 0 at the edges. */
export const computeTimingScore = (rhythmPhase: number): number => {
  const d = Math.abs(clamp01(rhythmPhase) - 0.5);
  const falloff = 0.5 - C.RHYTHM_PERFECT_HALF;
  return clamp01(1 - (d - C.RHYTHM_PERFECT_HALF) / falloff);
};

/** Body control: fast lateral speed tips you off balance; a plant steadies; fatigue shakes. */
export const computeStabilityScore = (playerVelX: number, plantTicks: number, fatigue = 0): number => {
  const speedPenalty = clamp01(Math.abs(playerVelX) / C.STABILITY_SPEED_REF);
  const plantBonus = plantTicks * C.STABILITY_PLANT_BONUS;
  const fatiguePenalty = clamp01(fatigue) * C.FATIGUE_STABILITY_PENALTY;
  return clamp01(1 - speedPenalty + plantBonus - fatiguePenalty);
};

/** Pressure PENALTY: HIGH when a balanced defender is smothering the shot at close range. */
export const computePressurePenalty = (
  playerX: number,
  defenderX: number,
  defenderBalance: number,
  finalWindow: boolean,
): number => {
  const dist = Math.abs(playerX - defenderX);
  const proximity = clamp01(1 - dist / C.CONTEST_RADIUS);
  return clamp01(proximity * clamp01(defenderBalance) + (finalWindow ? C.FINAL_CONTEST : 0));
};

/** Shot selection: taking an OPEN look (or one where you've beaten your man) is a good pick. */
export const computeShotSelection = (advantage: number, pressurePenalty: number): number => {
  const openness = 1 - clamp01(pressurePenalty);
  return clamp01(0.35 + 0.45 * openness + 0.2 * clamp01(advantage));
};

/** The small, capped heat contribution (never enough to fix bad shot selection). */
export const computeHeatBonus = (heat: number): number => clamp01(heat / C.HEAT_MAX) * C.HEAT_BONUS_WEIGHT;

/** The six components a shot's quality blends (pressurePenalty is subtracted). */
export interface QualityInput {
  readonly advantage: number;
  readonly separation: number;
  readonly timing: number;
  readonly stability: number;
  readonly shotSelection: number;
  readonly heat: number;
  readonly pressurePenalty: number;
}

/**
 * Blend into one 0..1 quality. Advantage + separation (the SPACE you made) dominate at
 * 45%; timing (25%) upgrades a good look but can't carry a smothered one; a heavy
 * pressure penalty sinks a contested shot even at perfect timing. Deterministic.
 */
export const computeShotQuality = (input: QualityInput): number =>
  clamp01(
    C.ADVANTAGE_WEIGHT * clamp01(input.advantage) +
      C.SEPARATION_WEIGHT * clamp01(input.separation) +
      C.TIMING_WEIGHT * clamp01(input.timing) +
      C.STABILITY_WEIGHT * clamp01(input.stability) +
      C.SELECTION_WEIGHT * clamp01(input.shotSelection) +
      computeHeatBonus(input.heat) -
      C.PRESSURE_PENALTY_WEIGHT * clamp01(input.pressurePenalty),
  );

/** Everything the breakdown needs from live state. */
export interface BreakdownInput {
  readonly playerX: number;
  readonly defenderX: number;
  readonly defenderBalance: number;
  readonly playerVelX: number;
  readonly plantTicks: number;
  readonly rhythmPhase: number;
  readonly heat: number;
  readonly finalWindow: boolean;
  readonly advantage: number;
  readonly fatigue: number;
}

/** Compute the full, explainable shot breakdown (the meter + the release share this). */
export const computeBreakdown = (i: BreakdownInput): ShotBreakdown => {
  const separation = computeSeparationScore(i.playerX, i.defenderX, i.defenderBalance);
  const timing = computeTimingScore(i.rhythmPhase);
  const stability = computeStabilityScore(i.playerVelX, i.plantTicks, i.fatigue);
  const pressurePenalty = computePressurePenalty(i.playerX, i.defenderX, i.defenderBalance, i.finalWindow);
  const shotSelection = computeShotSelection(i.advantage, pressurePenalty);
  const heatBonus = computeHeatBonus(i.heat);
  const quality = computeShotQuality({
    advantage: i.advantage,
    heat: i.heat,
    pressurePenalty,
    separation,
    shotSelection,
    stability,
    timing,
  });
  return { advantage: clamp01(i.advantage), heatBonus, pressurePenalty, quality, separation, shotSelection, stability, timing };
};

/** The bar a shot must clear — it creeps up as heat (and score) rise. */
export const computeRequiredQuality = (heat: number, score: number): number =>
  C.SHOT_REQUIRED_QUALITY + heat * C.REQUIRED_QUALITY_HEAT_STEP + Math.min(score, 40) * 0.0006;

/** Classify a shot from its quality — pure, random-free, quality alone decides. */
export const determineShotResult = (quality: number, required: number, swish: number): ShotResult =>
  quality >= swish ? "swish" : quality >= required ? "make" : "miss";

// ── readiness tags (three separate axes — no single "guaranteed make" truth) ─────

/** SPACE: how much room you've created (advantage window + defender pressure). */
export const computeSpaceTag = (advantage: number, pressurePenalty: number): SpaceTag => {
  if (advantage >= C.ADVANTAGE_WINDOW_STRONG_THRESHOLD) {
    return "broken";
  }
  if (pressurePenalty <= C.OPEN_PRESSURE_MAX || advantage >= C.ADVANTAGE_WINDOW_WEAK_THRESHOLD) {
    return "open";
  }
  if (pressurePenalty >= C.SMOTHERED_PRESSURE_MIN) {
    return "smothered";
  }
  return "contested";
};

/** RHYTHM: where the release lands against the timing window. */
export const computeRhythmTag = (timing: number, rhythmPhase: number): RhythmTag => {
  if (timing >= C.TIMING_PERFECT) {
    return "perfect";
  }
  if (timing >= C.TIMING_GOOD) {
    return "good";
  }
  return rhythmPhase < 0.5 ? "early" : "late";
};

/** BALANCE: how set the body is for the shot. */
export const computeBalanceTag = (stability: number, plantTicks: number): BalanceTag => {
  if (plantTicks >= C.BALANCE_PLANTED_TICKS && stability >= C.BALANCE_PLANTED_STABILITY) {
    return "planted";
  }
  if (stability >= C.BALANCE_SET_STABILITY) {
    return "set";
  }
  return "moving";
};

/**
 * The dominant deficit that shapes a MISS's arc: forced (deep-red), off balance, a
 * smothering contest, or a timing miss (early/late). Makes/swishes are perfect/clean.
 */
export const classifyShot = (
  result: ShotResult,
  breakdown: ShotBreakdown,
  rhythmPhase: number,
  required: number,
): ShotReason => {
  if (result === "swish") {
    return "perfect";
  }
  if (result === "make") {
    return "clean";
  }
  if (breakdown.quality < required - C.FORCED_MARGIN) {
    return "forced";
  }
  const dTiming = 1 - breakdown.timing;
  const dContest = clamp01(breakdown.pressurePenalty);
  const dBalance = 1 - breakdown.stability;
  const worst = Math.max(dTiming, dContest, dBalance);
  if (worst === dBalance) {
    return "offBalance";
  }
  if (worst === dContest) {
    return "contested";
  }
  return rhythmPhase < 0.5 ? "early" : "late";
};

const SPACE_TEXT: Record<SpaceTag, string> = { broken: "BROKEN ANKLES", contested: "CONTESTED", open: "OPEN", smothered: "SMOTHERED" };
const RHYTHM_MISS_TEXT: Record<RhythmTag, string> = { early: "EARLY", good: "GOOD TIMING", late: "LATE", perfect: "PERFECT TIMING" };

/**
 * The two-part post-shot feedback: a SPACE/failure label and a rhythm/quality label
 * ("OPEN / PERFECT", "CONTESTED / PERFECT TIMING", "OFF BALANCE / GOOD TIMING",
 * "BROKEN ANKLES / CLEAN", "FORCED / LATE"). `kind` colors it; `big` marks the earned ones.
 */
export const describeShot = (
  result: ShotResult,
  reason: ShotReason,
  breakdown: ShotBreakdown,
  rhythmPhase: number,
  advantage: number,
): Feedback => {
  const space = computeSpaceTag(advantage, breakdown.pressurePenalty);
  const rhythm = computeRhythmTag(breakdown.timing, rhythmPhase);

  if (result !== "miss") {
    const broken = space === "broken";
    const secondary = result === "swish" || breakdown.timing >= C.TIMING_PERFECT ? "PERFECT" : "CLEAN";
    return { big: result === "swish" || broken, kind: broken ? "broken" : "open", text: `${broken ? "BROKEN ANKLES" : "OPEN"} / ${secondary}` };
  }

  const kind: Feedback["kind"] =
    reason === "offBalance"
      ? "offBalance"
      : reason === "forced"
        ? "forced"
        : reason === "contested"
          ? space === "smothered"
            ? "smothered"
            : "contested"
          : space; // early/late miss keeps the space read (open look, bad timing)
  const primary =
    reason === "offBalance" ? "OFF BALANCE" : reason === "forced" ? "FORCED" : SPACE_TEXT[kind as SpaceTag];
  return { big: false, kind, text: `${primary} / ${RHYTHM_MISS_TEXT[rhythm]}` };
};

/**
 * Build the ball's flight deterministically FROM THE REASON so the miss matches why it
 * missed: EARLY short/front, LATE long/back, CONTESTED flat rim-out, OFF BALANCE pushed
 * left/right by the release-slide, FORCED an ugly heave, CLEAN in, PERFECT a swish.
 */
export const createShotArc = (reason: ShotReason, fromX: number, hoopX: number, playerVelX: number): ShotArc => {
  const start = vec3(fromX, C.RELEASE_Y, C.PLAYER_Z);
  const base = Math.max(C.HOOP_Y, C.RELEASE_Y);
  const dir = playerVelX > 0 ? 1 : playerVelX < 0 ? -1 : 1;
  const midZ = (C.PLAYER_Z + C.HOOP_Z) / 2;

  const shape: Record<ShotReason, { x: number; y: number; z: number; apex: number; result: ShotResult }> = {
    clean: { apex: base + 1.7, result: "make", x: hoopX + 0.06, y: C.HOOP_Y - 0.05, z: C.HOOP_Z },
    contested: { apex: base + 1.05, result: "miss", x: hoopX + 0.28, y: C.HOOP_Y + 0.04, z: C.HOOP_Z - 0.1 },
    early: { apex: base + 1.35, result: "miss", x: hoopX, y: C.HOOP_Y - 0.18, z: C.HOOP_Z - 0.95 },
    forced: { apex: base + 0.85, result: "miss", x: hoopX + dir * 0.85, y: C.HOOP_Y + 0.55, z: C.HOOP_Z - 0.5 },
    late: { apex: base + 1.65, result: "miss", x: hoopX, y: C.HOOP_Y + 0.2, z: C.HOOP_Z + 0.95 },
    offBalance: { apex: base + 1.5, result: "miss", x: hoopX + dir * (C.RIM_RADIUS + 0.5), y: C.HOOP_Y, z: C.HOOP_Z },
    perfect: { apex: base + 1.95, result: "swish", x: hoopX, y: C.HOOP_Y - 0.02, z: C.HOOP_Z },
  };
  const s = shape[reason];
  const control = vec3((fromX + s.x) / 2, s.apex, midZ);
  return { control, end: vec3(s.x, s.y, s.z), result: s.result, start };
};

/** Sample a shot arc at parameter `t` in [0,1] (quadratic Bézier). */
export const sampleArc = (arc: ShotArc, t: number): Vec3 => {
  const u = 1 - t;
  const a = u * u;
  const b = 2 * u * t;
  const c = t * t;
  return vec3(
    a * arc.start.x + b * arc.control.x + c * arc.end.x,
    a * arc.start.y + b * arc.control.y + c * arc.end.y,
    a * arc.start.z + b * arc.control.z + c * arc.end.z,
  );
};

/** Points for a resolved shot: 2 make / 3 swish (+1 deep), ×multiplier, ×2 in the final window. */
export interface ScoreInput {
  readonly result: ShotResult;
  readonly multiplier: number;
  readonly doublePoints: boolean;
  readonly deep: boolean;
}

export const applyScore = (input: ScoreInput): number => {
  if (input.result === "miss") {
    return 0;
  }
  const basePts = input.result === "swish" ? C.SWISH_POINTS : C.MAKE_POINTS;
  const deepPts = input.deep ? C.DEEP_BONUS_POINTS : 0;
  const raw = (basePts + deepPts) * input.multiplier;
  return input.doublePoints ? raw * 2 : raw;
};

/**
 * New heat after a shot: make +1, swish +2, miss −2 — and a bad miss (well under the
 * bar) resets momentum. Capped `0..HEAT_MAX`. `quality`/`required` optional.
 */
export const updateHeat = (heat: number, result: ShotResult, quality?: number, required?: number): number => {
  if (result === "swish") {
    return Math.min(C.HEAT_MAX, heat + C.SWISH_HEAT_GAIN);
  }
  if (result === "make") {
    return Math.min(C.HEAT_MAX, heat + C.MAKE_HEAT_GAIN);
  }
  const severe = quality !== undefined && required !== undefined && quality < required - C.SEVERE_MISS_MARGIN;
  return severe ? 0 : Math.max(0, heat - C.MISS_HEAT_DROP);
};

/** The streak + multiplier after a shot: makes build the streak, misses reset it. */
export const updateStreakMultiplier = (
  streak: number,
  result: ShotResult,
): { readonly streak: number; readonly multiplier: number } => {
  if (result === "miss") {
    return { multiplier: 1, streak: 0 };
  }
  const next = streak + 1;
  const multiplier = Math.min(C.STREAK_MULTIPLIER_CAP, 1 + Math.floor(next / C.STREAK_MULTIPLIER_STEP));
  return { multiplier, streak: next };
};

/** Defender balance after a tick: a sharp crossover buckles it; otherwise it recovers. */
export const updateDefenderBalance = (balance: number, sharpCrossover: boolean): number =>
  sharpCrossover ? C.DEFENDER_BEATEN_BALANCE : clamp01(balance + C.DEFENDER_BALANCE_RECOVERY);

/** Keep the player on the floor. */
export const clampPlayerPosition = (x: number): number => clamp(x, C.COURT_MIN_X, C.COURT_MAX_X);

/** Append a feedback event, keeping the buffer bounded (oldest dropped past the cap). */
export const pushFeedback = (list: readonly Feedback[], event: Feedback): Feedback[] => {
  const next = [...list, event];
  return next.length > C.FEEDBACK_MAX ? next.slice(next.length - C.FEEDBACK_MAX) : next;
};
