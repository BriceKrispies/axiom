/*
 * selection.ts — is the pointer on the ball? SDK-free (it uses the pure
 * `projection.ts` camera math) and testable.
 *
 * The ball is projected to canvas pixels and the pointer has to land within a
 * generous radius of it — its on-screen size times a forgiveness factor, with a pixel
 * floor so the ball stays grabbable on a small screen even when it is only a few
 * pixels across.
 *
 * There is only one ball, so this could have been skipped and any press treated as a
 * grab. It is here because the game asks you to *pick the ball up*: a throw you begin
 * by touching the ball is a different, more physical act than one you begin by
 * touching the background, and the forgiveness factor is what keeps that from being
 * fiddly rather than tactile.
 */

import type { Vec2, Vec3 } from "./vec.ts";
import { type Mat4, project } from "./projection.ts";
import { BALL_RADIUS, GRAB_RADIUS_FACTOR, GRAB_RADIUS_MIN_PX } from "./constants.ts";

/** Whether `pointer` (canvas px) is close enough to the ball to pick it up. */
export const pointerGrabsBall = (
  pointer: Vec2,
  ballPos: Vec3,
  viewProj: Mat4,
  viewport: Vec2,
): boolean => {
  const projected = project(ballPos, viewProj, viewport);
  if (projected.w <= 0) {
    return false;
  }
  const screenRadius = Math.max(
    GRAB_RADIUS_MIN_PX,
    BALL_RADIUS * projected.pixelsPerMetre * GRAB_RADIUS_FACTOR,
  );
  return Math.hypot(pointer.x - projected.pos.x, pointer.y - projected.pos.y) <= screenRadius;
};
