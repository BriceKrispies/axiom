/*
 * conditions.ts — the deterministic setup for one round: where you stand, and how the
 * wind blows. SDK-free and pure, so a whole round is replayable from a seed and
 * unit-testable in bare Node.
 *
 * Both are per-ROUND, not per-ball. You stand in one place and throw the whole rack
 * from it, in one wind. Re-rolling the wind between throws would make the rack
 * unlearnable — the entire skill is reading one crosswind and correcting for it across
 * eight throws, and watching your grouping walk into the centre as you do.
 *
 * There is no RNG object and no hidden state: every value is a pure hash of
 * `(seed, round, field)`. Round 3 of seed 20260808 is the same round 3 forever, in any
 * process, in any order — which makes "the wind was brutal" a checkable claim.
 */

import { type Vec3, vec3 } from "./vec.ts";
import {
  DROP_ALTITUDE,
  LINEAR_DAMPING,
  STAND_OFFSET_MAX,
  STAND_OFFSET_MIN,
  WIND_ACCEL_MAX,
  WIND_ACCEL_MIN,
} from "./constants.ts";

/** Which value a hash call is for — distinct salts, so the fields never correlate. */
const FIELD_STAND_BEARING = 1;
const FIELD_STAND_RADIUS = 2;
const FIELD_WIND_BEARING = 3;
const FIELD_WIND_STRENGTH = 4;

const lerp = (a: number, b: number, t: number): number => a + (b - a) * t;

/**
 * A deterministic 32-bit avalanche hash of `(seed, index, field)` mapped to `[0, 1)`.
 * Integer ops only (`Math.imul`, `>>>`), so it is exactly reproducible — no floats
 * accumulate and no platform rounding can drift it.
 */
export const hash01 = (seed: number, index: number, field: number): number => {
  let h = (seed ^ Math.imul(index + 1, 0x9e3779b1) ^ Math.imul(field, 0x85ebca77)) >>> 0;
  h = Math.imul(h ^ (h >>> 16), 0x21f0aaad) >>> 0;
  h = Math.imul(h ^ (h >>> 15), 0x735a2d97) >>> 0;
  h = (h ^ (h >>> 15)) >>> 0;
  return h / 4294967296;
};

/** Everything that makes one round different from the next. */
export interface RoundConditions {
  /** Where you throw from — DROP_ALTITUDE up, offset horizontally from the target. */
  readonly stand: Vec3;
  /** Wind as a constant horizontal ACCELERATION (m/s²); `y` is always 0. */
  readonly wind: Vec3;
  /**
   * The drift speed the wind converges to (m/s) — `|wind| / LINEAR_DAMPING`. This is
   * the number the HUD shows, because it is the one the player can reason about: an
   * acceleration means nothing to the eye, a drift speed does.
   */
  readonly windSpeed: number;
  /** Wind bearing in radians (atan2-style, from +X toward +Z), for the HUD arrow. */
  readonly windBearing: number;
  /** The stand's horizontal distance from the target centre (m). */
  readonly standDistance: number;
}

/** The deterministic conditions for round `index` of the game seeded with `seed`. */
export const roundConditions = (seed: number, index: number): RoundConditions => {
  const standBearing = hash01(seed, index, FIELD_STAND_BEARING) * Math.PI * 2;
  const standDistance = lerp(STAND_OFFSET_MIN, STAND_OFFSET_MAX, hash01(seed, index, FIELD_STAND_RADIUS));
  const windBearing = hash01(seed, index, FIELD_WIND_BEARING) * Math.PI * 2;
  const windAccel = lerp(WIND_ACCEL_MIN, WIND_ACCEL_MAX, hash01(seed, index, FIELD_WIND_STRENGTH));

  return {
    stand: vec3(Math.cos(standBearing) * standDistance, DROP_ALTITUDE, Math.sin(standBearing) * standDistance),
    standDistance,
    wind: vec3(Math.cos(windBearing) * windAccel, 0, Math.sin(windBearing) * windAccel),
    windBearing,
    windSpeed: windAccel / LINEAR_DAMPING,
  };
};
