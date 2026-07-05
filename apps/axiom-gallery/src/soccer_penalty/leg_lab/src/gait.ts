/*
 * The deterministic gait cycle — everything the leg does as a pure function of the
 * simulation tick. TypeScript port of the Rust lab's `gait.rs`.
 *
 * One stride lasts `gaitDurationTicks` and splits into two phases:
 *   - PLANTED (the first `plantedFraction`): the foot is locked to a fixed
 *     WORLD-SPACE ground contact point while the hip advances forward over it.
 *   - SWING (the remainder): the foot lifts along a smooth sine arc and travels
 *     forward to the NEXT contact point, arriving as the next stride's plant begins.
 *
 * The hip advances forward at a constant rate (`hipForwardX`), so contact points
 * sit one stride apart along +X. Convention: +X walking, +Y up, ground at y = 0,
 * the whole gait in the z = 0 plane so a side camera reads it cleanly. The one
 * stateful piece — the hip-bob spring — lives in `hip-spring.ts`.
 */

import { type Vec3, vec3 } from "./vec3.ts";

/** The exposed, tunable constants of the gait (the single dial board). */
export interface GaitParams {
  /** Thigh (hip→knee) segment length, metres. */
  readonly thighLength: number;
  /** Shin (knee→foot) segment length, metres. */
  readonly shinLength: number;
  /** Forward distance per full stride, metres — also the contact-point spacing. */
  readonly strideLength: number;
  /** Peak height of the swing-foot arc above the ground, metres. */
  readonly stepHeight: number;
  /** Ticks in one full gait cycle. */
  readonly gaitDurationTicks: number;
  /** Fraction of the cycle the foot is planted, in (0, 1). */
  readonly plantedFraction: number;
  /** The hip-bob spring's smoothing time, ticks: larger is smoother/slower. */
  readonly smoothingStrength: number;
  /** Standing hip height above the ground, metres (the spring's rest value). */
  readonly hipHeight: number;
  /** Raw vertical drop of the hip during swing, metres, BEFORE spring smoothing. */
  readonly hipBob: number;
}

/** Which half of the stride the foot is in. */
export type FootPhase = "planted" | "swing";

/** The resolved gait phase at a tick. */
export interface GaitPhase {
  /** The completed stride index this tick falls in (`floor(tick / duration)`). */
  readonly cycle: number;
  /** Progress through the whole stride, `[0, 1)`. */
  readonly fraction: number;
  /** Planted or swinging. */
  readonly phase: FootPhase;
  /** Progress through the current phase, `[0, 1)`. */
  readonly phaseProgress: number;
}

/** Resolve the gait phase at `tick`. */
export const gaitPhase = (tick: number, p: GaitParams): GaitPhase => {
  const duration = Math.max(Math.trunc(p.gaitDurationTicks), 1);
  const cycle = Math.floor(tick / duration);
  const within = tick - cycle * duration;
  const fraction = within / duration;
  const plantedFraction = Math.min(Math.max(p.plantedFraction, 1e-3), 1 - 1e-3);
  if (fraction < plantedFraction) {
    return { cycle, fraction, phase: "planted", phaseProgress: fraction / plantedFraction };
  }
  return {
    cycle,
    fraction,
    phase: "swing",
    phaseProgress: (fraction - plantedFraction) / (1 - plantedFraction),
  };
};

/** Whether the foot is planted at `tick`. */
export const isPlanted = (phase: GaitPhase): boolean => phase.phase === "planted";

/** The world-space contact point for stride `cycle`: on the ground, one stride apart along +X. */
export const contactPoint = (cycle: number, p: GaitParams): Vec3 => vec3(p.strideLength * cycle, 0, 0);

/**
 * The desired WORLD-SPACE foot position at `tick` — the raw target the IK aims at.
 * Planted: exactly this stride's contact point (constant → no slide). Swing: a
 * forward sweep from this contact to the next, lifted by a half-sine arc.
 */
export const footTargetWorld = (tick: number, p: GaitParams): Vec3 => {
  const ph = gaitPhase(tick, p);
  const here = contactPoint(ph.cycle, p);
  if (ph.phase === "planted") {
    return here;
  }
  const s = ph.phaseProgress;
  return vec3(here.x + p.strideLength * s, p.stepHeight * Math.sin(Math.PI * s), 0);
};

/** The hip's forward X at `tick`: a constant-rate advance of one stride per cycle. */
export const hipForwardX = (tick: number, p: GaitParams): number =>
  p.strideLength * (tick / Math.max(Math.trunc(p.gaitDurationTicks), 1));

/** The RAW (unsmoothed) target hip height at `tick`: full standing height while planted, dropped by `hipBob` while swinging. */
export const hipRawHeight = (tick: number, p: GaitParams): number =>
  gaitPhase(tick, p).phase === "planted" ? p.hipHeight : p.hipHeight - p.hipBob;
