/*
 * The lab's leg dimensions, taken from the game's own kicker character.
 *
 * The Rust lab loaded `assets/soccer/kicker.figure` and read the right-leg chain
 * (thigh → shin → foot) out of it. The `@axiom/game` SDK has no figure/skeleton
 * surface, so there is no way to parse that asset from TypeScript today. To keep
 * the leg faithful to the game character, the right-leg measurements are mirrored
 * here as constants, copied verbatim from the figure's authoring source —
 * `apps/axiom-animation-lab/src/authoring.rs` (parts 6/7/8, the R thigh/shin/foot),
 * which is what bakes `assets/soccer/kicker.figure`. Re-authoring the kicker in the
 * lab means updating these four lines (or adding a `loadFigure` bridge to the SDK).
 *
 * Provenance (authoring.rs):
 *   R thigh: box (0.17, 0.48, 0.19); shin parented at (0, -0.48, 0) → thigh len 0.48
 *   R shin : box (0.15, 0.46, 0.16); foot parented at (0, -0.48, 0) → shin  len 0.48
 *   R foot : box (0.15, 0.11, 0.30)
 */

import type { Vec3 } from "./vec3.ts";
import type { GaitParams } from "./gait.ts";

/** The right leg's measurements — the two IK segment lengths plus render-box extents. */
export interface LegRig {
  /** Thigh (hip→knee) length. */
  readonly thighLength: number;
  /** Shin (knee→foot) length. */
  readonly shinLength: number;
  /** Thigh render-box extents (full size). */
  readonly thighBox: Vec3;
  /** Shin render-box extents (full size). */
  readonly shinBox: Vec3;
  /** Foot render-box extents (full size). */
  readonly footBox: Vec3;
}

/** The shared kicker character's right leg, mirrored from the figure authoring source. */
export const kickerLeg = (): LegRig => ({
  thighLength: 0.8,
  shinLength: 0.48,
  thighBox: { x: 0.17, y: 0.48, z: 0.19 },
  shinBox: { x: 0.15, y: 0.46, z: 0.16 },
  footBox: { x: 0.15, y: 0.11, z: 0.3 },
});

/**
 * The gait constants for this leg: the two segment lengths come from the imported
 * character; the rest are the lab's tuned gait-shape knobs, with the standing hip
 * height set as a fraction of the leg reach so the knee stays comfortably bent.
 */
export const gaitParamsFor = (rig: LegRig): GaitParams => ({
  thighLength: rig.thighLength,
  shinLength: rig.shinLength,
  strideLength: 0.62,
  stepHeight: 0.16,
  gaitDurationTicks: 64,
  plantedFraction: 0.6,
  smoothingStrength: 6,
  // 77% of full leg reach: enough droop for a visible forward knee bend without
  // over-extending near the annulus edge.
  hipHeight: (rig.thighLength + rig.shinLength) * 0.77,
  hipBob: 0.05,
});
