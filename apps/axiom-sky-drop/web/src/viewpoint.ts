/*
 * viewpoint.ts — the camera, solved as a PURE function of where you stand. SDK-free,
 * so the framing is unit-testable: "the target is on screen", "the whole reach is on
 * screen", "the camera never dips below the ground" are assertions rather than things
 * you squint at in a screenshot.
 *
 * It also defines the horizontal basis the HUD orients the wind arrow in, which lives
 * here next to the framing rather than somewhere else — the arrow means "this way
 * across the screen", and that is a fact about the camera.
 *
 * ## The camera does not follow the ball
 *
 * It is pinned to the stand for the whole round. Two reasons, and the second is the
 * hard one:
 *
 *   - A camera that stays put is what makes 180 m read as 180 m. A thrown ball falls
 *     away from you and shrinks toward a target that stays exactly where it was. A
 *     chase camera holds the ball at constant size against a growing target, which
 *     reads as the ground rising rather than the ball falling.
 *   - There are several balls in the air at once. Following any one of them would be
 *     an arbitrary choice, and it would yank the frame away from the ball still in
 *     your hand — the only one you can still do anything about.
 *
 * A target 180 m below and 26–48 m sideways sits within ~15° of straight down, so the
 * only angle that shows the stand and the target together is near-vertical. That is
 * why `CAMERA_PITCH` is 86° and why the look point is biased along the stand→target
 * line instead of aimed at either end.
 */

import { type Vec3, add, cross, lerp as lerpVec, normalize, scale, sub, vec3 } from "./vec.ts";
import {
  CAMERA_DIST,
  CAMERA_LOOK_BIAS,
  CAMERA_PITCH,
  GROUND_Y,
} from "./constants.ts";

const WORLD_UP: Vec3 = vec3(0, 1, 0);
/** Used when the stand is exactly over the target and "toward the target" is undefined. */
const FALLBACK_FORWARD: Vec3 = vec3(0, 0, -1);

/** The camera's horizontal basis — the frame screen directions are read in. */
export interface AimBasis {
  /** Horizontal unit vector pointing away from the camera, toward the target. Screen-up. */
  readonly forward: Vec3;
  /** Horizontal unit vector to the camera's right. Screen-right. */
  readonly right: Vec3;
}

/** A solved camera pose. */
export interface Viewpoint {
  readonly position: Vec3;
  readonly target: Vec3;
}

/** The horizontal basis at `standPos`, facing `aimPoint`. */
export const aimBasis = (standPos: Vec3, aimPoint: Vec3): AimBasis => {
  const toTarget = vec3(aimPoint.x - standPos.x, 0, aimPoint.z - standPos.z);
  const flat = Math.hypot(toTarget.x, toTarget.z);
  const forward = flat > 1e-6 ? scale(toTarget, 1 / flat) : FALLBACK_FORWARD;
  return { forward, right: normalize(cross(forward, WORLD_UP)) };
};

/** Solve the fixed camera for a round thrown from `standPos` toward `aimPoint`. */
export const standView = (standPos: Vec3, aimPoint: Vec3): Viewpoint => {
  const { forward } = aimBasis(standPos, aimPoint);

  // Up and BACK along the stand→target line, so the target is always the far side of
  // the frame and the thrower's hand the near side.
  const lifted = add(standPos, scale(WORLD_UP, CAMERA_DIST * Math.sin(CAMERA_PITCH)));
  const position = sub(lifted, scale(forward, CAMERA_DIST * Math.cos(CAMERA_PITCH)));

  return {
    position: vec3(position.x, Math.max(position.y, GROUND_Y + 1.2), position.z),
    target: lerpVec(standPos, aimPoint, CAMERA_LOOK_BIAS),
  };
};
