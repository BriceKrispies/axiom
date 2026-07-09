/*
 * throw.ts — the constrained arcade throw model: the heart of the "swipe to shoot"
 * feel. SDK-free and directly unit-tested.
 *
 * The smoothed release gesture (canvas-px/tick, +y DOWN — see pointer.ts) is
 * decomposed into THREE independent intents and mapped to a launch velocity:
 *   - POWER  — from how hard the flick goes UP; scales forward speed (and lift).
 *   - LATERAL aim — the sideways flick; scales ±X, bounded.
 *   - UPWARD intent — folded into power; NEVER copied straight to world +Y.
 * The FORWARD (−Z) component dominates, and vertical lift is hard-clamped to a
 * fraction of forward speed (`THROW_VERTICAL_TO_FORWARD_MAX_RATIO`), so a hard
 * upward flick becomes a FAST, FLAT arc into the machine — not a tall rainbow. Raw
 * screen-Y velocity is never used as raw world-Y velocity.
 *
 * Weak flick  → power ≈ 0 → slow forward + little lift → falls short.
 * Medium flick → mid power → a quick arc that can drop through the hoop.
 * Hard flick   → full power → drives hard/flat toward the hoop (may brick long).
 */

import { type Vec2, type Vec3, clamp, vec3 } from "./vec.ts";
import {
  THROW_FORWARD_MAX,
  THROW_FORWARD_MIN,
  THROW_GESTURE_DEADZONE,
  THROW_GESTURE_FULL,
  THROW_LATERAL_MAX,
  THROW_VERTICAL_MAX,
  THROW_VERTICAL_MIN,
  THROW_VERTICAL_TO_FORWARD_MAX_RATIO,
} from "./constants.ts";

const lerp = (a: number, b: number, t: number): number => a + (b - a) * t;

/** The decomposed throw intents (exported for tests / a potential debug readout). */
export interface ThrowIntents {
  /** Normalised flick strength in `[0, 1]`. */
  readonly power: number;
  /** Forward launch speed (m/s), the dominant component. */
  readonly forward: number;
  /** Upward launch speed (m/s), clamped to a fraction of `forward`. */
  readonly vertical: number;
  /** Lateral launch speed (m/s, ±X). */
  readonly lateral: number;
}

/** Decompose a smoothed swipe (px/tick, +y down) into the three throw intents. */
export const throwIntents = (swipe: Vec2): ThrowIntents => {
  const upward = Math.max(0, -swipe.y);
  // Power comes from the UPWARD flick strength, not from copying screen-Y velocity.
  const power = clamp((upward - THROW_GESTURE_DEADZONE) / (THROW_GESTURE_FULL - THROW_GESTURE_DEADZONE), 0, 1);

  const forward = lerp(THROW_FORWARD_MIN, THROW_FORWARD_MAX, power);
  const verticalDesired = lerp(THROW_VERTICAL_MIN, THROW_VERTICAL_MAX, power);
  // Hard clamp keeps the shot forward-dominant: lift can never rainbow past this.
  const vertical = Math.min(verticalDesired, forward * THROW_VERTICAL_TO_FORWARD_MAX_RATIO);

  const lateralNorm = clamp(swipe.x / THROW_GESTURE_FULL, -1, 1);
  const lateral = lateralNorm * THROW_LATERAL_MAX;

  return { forward, lateral, power, vertical };
};

/** Map a smoothed swipe velocity (px/tick, +y down) to a world launch velocity (m/s). */
export const swipeToThrow = (swipe: Vec2): Vec3 => {
  const intents = throwIntents(swipe);
  return vec3(intents.lateral, intents.vertical, -intents.forward);
};
