/*
 * pitch.ts — the deterministic seeded pitch sequence. A pitch is a pure function
 * of (seed, pitchIndex): profile selection honors the round's difficulty ramp
 * (early pitches easy, hard profiles only late), then a small seeded jitter is
 * applied around the profile's aim so no two pitches are metronome-identical —
 * yet the same seed always reproduces the same ten pitches. SDK-free.
 */

import { type Vec3, hash01, vec3 } from "./vec.ts";
import type { PitchSpec } from "./types.ts";
import * as C from "./constants.ts";

/** The profile pool legal at `pitchIndex`, with hard profiles weighted late. */
export const pitchPool = (pitchIndex: number): readonly C.PitchProfile[] => {
  if (pitchIndex < C.EASY_ONLY_BEFORE) {
    return C.PITCH_PROFILES.filter((p) => p.tier === "easy");
  }
  if (pitchIndex < C.HARD_ALLOWED_FROM) {
    return C.PITCH_PROFILES.filter((p) => p.tier !== "hard");
  }
  // Late round: everything, with each hard profile appearing HARD_LATE_WEIGHT times.
  const weighted: C.PitchProfile[] = [];
  for (const p of C.PITCH_PROFILES) {
    const copies = p.tier === "hard" ? C.HARD_LATE_WEIGHT : 1;
    for (let k = 0; k < copies; k += 1) {
      weighted.push(p);
    }
  }
  return weighted;
};

/** Select + jitter the pitch for `(seed, pitchIndex)` — pure and replayable. */
export const selectPitch = (seed: number, pitchIndex: number): PitchSpec => {
  const pool = pitchPool(pitchIndex);
  const profile = pool[Math.min(pool.length - 1, Math.floor(hash01(seed, pitchIndex, 1) * pool.length))]!;
  const speed = profile.speed * (1 + (hash01(seed, pitchIndex, 2) - 0.5) * 2 * C.JITTER_SPEED);
  return {
    gravity: profile.gravity,
    mph: Math.round(speed * C.MPH_PER_UNIT),
    name: profile.name,
    profileId: profile.id,
    speed,
    targetX: profile.targetX + (hash01(seed, pitchIndex, 3) - 0.5) * 2 * C.JITTER_X,
    targetY: profile.targetY + (hash01(seed, pitchIndex, 4) - 0.5) * 2 * C.JITTER_Y,
  };
};

/** The seeded idle gap (ticks) before this pitch's wind-up begins. */
export const pitchGapTicks = (seed: number, pitchIndex: number): number =>
  C.GAP_TICKS + Math.floor(hash01(seed, pitchIndex, 5) * C.GAP_JITTER_TICKS);

/**
 * Solve the release velocity (per TICK) that carries the ball from PITCH_RELEASE
 * to the plate-crossing target under the pitch's own gravity. Closed form: the
 * z-speed is the profile speed; x and y are fitted to arrive on target.
 */
export const solvePitch = (spec: PitchSpec): { readonly vel: Vec3; readonly gravityPerTick: number } => {
  const release = C.PITCH_RELEASE;
  const vz = -spec.speed / C.FIXED_HZ;
  const flightTicks = release.z / (spec.speed / C.FIXED_HZ);
  const gravityPerTick = spec.gravity / (C.FIXED_HZ * C.FIXED_HZ);
  const vx = (spec.targetX - release.x) / flightTicks;
  // Discrete semi-implicit Euler drops Σ g·k = g·T(T+1)/2, not the continuous ½gT².
  const vy = (spec.targetY - release.y + 0.5 * gravityPerTick * flightTicks * (flightTicks + 1)) / flightTicks;
  return { gravityPerTick, vel: vec3(vx, vy, vz) };
};
