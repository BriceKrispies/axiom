/*
 * selection.ts — pick the basketball under the pointer, in screen space. SDK-free
 * (it uses the pure `projection.ts` camera math) and testable. Each selectable ball
 * is projected to canvas pixels; the pointer picks the nearest whose projected
 * position falls within a generous radius (its on-screen size × a forgiveness
 * factor). Balls behind the camera or already in flight are ignored. Returns the
 * ball index, or `-1` when the pointer hits none.
 */

import type { Vec2, Vec3 } from "./vec.ts";
import { type Mat4, project } from "./projection.ts";
import { BALL_RADIUS, SELECT_RADIUS_FACTOR } from "./constants.ts";

/** The minimal ball view selection needs. */
export interface Selectable {
  readonly pos: Vec3;
  /** Whether this ball can currently be grabbed (at rest in the rack, not in flight). */
  readonly selectable: boolean;
}

/** Return the index of the nearest selectable ball under `pointer`, or `-1` if none. */
export const pickBall = (pointer: Vec2, balls: readonly Selectable[], viewProj: Mat4, viewport: Vec2): number => {
  let best = -1;
  let bestDist = Number.POSITIVE_INFINITY;
  for (let i = 0; i < balls.length; i += 1) {
    const ball = balls[i]!;
    if (!ball.selectable) {
      continue;
    }
    const p = project(ball.pos, viewProj, viewport);
    if (p.w <= 0) {
      continue;
    }
    const screenRadius = Math.max(24, BALL_RADIUS * p.pixelsPerMetre * SELECT_RADIUS_FACTOR);
    const dist = Math.hypot(pointer.x - p.pos.x, pointer.y - p.pos.y);
    if (dist <= screenRadius && dist < bestDist) {
      best = i;
      bestDist = dist;
    }
  }
  return best;
};
