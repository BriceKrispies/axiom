/*
 * Two-bone inverse kinematics for a single leg: hip → knee → foot.
 *
 * TypeScript port of the Rust lab's `leg_ik.rs`. Given the hip position, a desired
 * foot target, and the two segment lengths (thigh, shin), solve for the knee so
 * the chain reaches the target with the knee bent CONSISTENTLY toward one forward
 * direction — it can never flip. The solve is planar (the plane spanned by the
 * hip→target line and the forward bend axis, i.e. world X-Y for the side view).
 *
 * Pure and deterministic: a closed-form law-of-cosines solution, with the target
 * clamped into the reachable annulus so the square roots never go negative and the
 * knee never straightens fully (the singular pose that lets a knee flip).
 */

import { type Vec3, add, cross, dot, length, normalizeOr, scale, sub, vec3 } from "./vec3.ts";

/**
 * How far inside the reachable annulus (`|thigh−shin|`, `thigh+shin`) the target
 * is kept. Staying strictly inside keeps the knee bent (never a straight,
 * flip-prone chain) and keeps the law-of-cosines argument inside `[-1, 1]`.
 */
export const REACH_MARGIN = 0.02;

/** The solved leg pose: the three joint positions plus whether the raw target was reachable. */
export interface LegPose {
  readonly hip: Vec3;
  readonly knee: Vec3;
  readonly foot: Vec3;
  readonly reachable: boolean;
}

/**
 * Solve the two-bone chain hip → knee → foot. `bendDir` is the forward direction
 * the knee bends toward (only its component perpendicular to the hip→foot line is
 * used; its sign fixes the bend side). For the side lab pass `+X`.
 */
export const solveTwoBone = (hip: Vec3, target: Vec3, thigh: number, shin: number, bendDir: Vec3): LegPose => {
  const toTarget = sub(target, hip);
  const distRaw = length(toTarget);

  // Clamp the reach into the open annulus (|thigh−shin|, thigh+shin).
  const inner = Math.abs(thigh - shin);
  const min = inner + REACH_MARGIN;
  const max = thigh + shin - REACH_MARGIN;
  const dist = Math.min(Math.max(distRaw, min), max);
  const reachable = distRaw >= inner && distRaw <= thigh + shin;

  // Direction hip→foot (fall back to straight down if hip == target).
  const dir = normalizeOr(toTarget, vec3(0, -1, 0));
  const foot = add(hip, scale(dir, dist));

  // Distance along the hip→foot line to the knee's projection, and the
  // perpendicular knee offset (law-of-cosines closure). `Math.max(_, 0)` guards
  // the sqrt against tiny negative round-off at the annulus edges.
  const along = (thigh * thigh - shin * shin + dist * dist) / (2 * dist);
  const perpLen = Math.sqrt(Math.max(thigh * thigh - along * along, 0));
  const perp = perpendicularToward(dir, bendDir);
  const knee = add(add(hip, scale(dir, along)), scale(perp, perpLen));

  return { hip, knee, foot, reachable };
};

/**
 * The unit vector perpendicular to `dir` pointing toward `bend`'s side — `bend`
 * with its `dir` component removed, normalized. Falls back to a stable
 * perpendicular when `bend` is parallel to `dir`.
 */
const perpendicularToward = (dir: Vec3, bend: Vec3): Vec3 => {
  const proj = scale(dir, dot(bend, dir));
  return normalizeOr(sub(bend, proj), fallbackPerpendicular(dir));
};

/** Any unit vector perpendicular to `dir`, for the degenerate parallel-bend case. */
const fallbackPerpendicular = (dir: Vec3): Vec3 => {
  const viaUp = cross(dir, vec3(0, 1, 0));
  if (length(viaUp) >= 1e-6) {
    return scale(viaUp, 1 / length(viaUp));
  }
  const viaFwd = cross(dir, vec3(1, 0, 0));
  return length(viaFwd) >= 1e-6 ? scale(viaFwd, 1 / length(viaFwd)) : vec3(1, 0, 0);
};

/**
 * The signed bend of the knee off the hip→foot LINE, along `bendDir` — the knee's
 * perpendicular displacement from the leg line (not its raw offset from the hip).
 * Positive means the knee is on the forward side; the solver guarantees it is
 * always positive, so this is the no-flip invariant made checkable.
 */
export const kneeBendOffset = (pose: LegPose, bendDir: Vec3): number => {
  const line = normalizeOr(sub(pose.foot, pose.hip), vec3(0, -1, 0));
  const hipToKnee = sub(pose.knee, pose.hip);
  const perpendicular = sub(hipToKnee, scale(line, dot(hipToKnee, line)));
  return dot(perpendicular, bendDir);
};

/** The interior knee angle (hip–knee–foot) in radians, `[0, π]`. π = straight. */
export const kneeAngle = (pose: LegPose): number => {
  const a = sub(pose.hip, pose.knee);
  const b = sub(pose.foot, pose.knee);
  const denom = length(a) * length(b);
  if (denom <= 1e-6) {
    return Math.PI;
  }
  return Math.acos(Math.min(Math.max(dot(a, b) / denom, -1), 1));
};
