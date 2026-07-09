/*
 * scoring.ts — the one-way hoop rule, pure and SDK-free. A shot scores ONLY when
 * the ball crosses the rim plane from above to below (downward) while its horizontal
 * position is inside the hoop opening AND it is actually moving down. That rejects:
 *   - a ball rising up through the opening from below,
 *   - a ball drifting sideways past the rim without dropping through,
 *   - a stationary ball parked inside the trigger.
 * The caller (session.ts) also latches "scored" per shot so a single make counts
 * exactly once, however the ball rattles afterward.
 */

import type { Vec3 } from "./vec.ts";
import { HOOP_X, HOOP_Y, HOOP_Z, TRIGGER_HALF_D, TRIGGER_HALF_W } from "./constants.ts";

/**
 * True iff the ball moved from `prevPos` to `curPos` this tick in a way that counts
 * as a made basket: it crossed the rim plane (`HOOP_Y`) top→bottom, is moving
 * downward, and its (x,z) is within the hoop opening.
 */
export const scoredThroughHoop = (prevPos: Vec3, curPos: Vec3, vel: Vec3): boolean => {
  const crossedDown = prevPos.y >= HOOP_Y && curPos.y < HOOP_Y;
  const movingDown = vel.y < 0;
  const insideX = Math.abs(curPos.x - HOOP_X) <= TRIGGER_HALF_W;
  const insideZ = Math.abs(curPos.z - HOOP_Z) <= TRIGGER_HALF_D;
  return crossedDown && movingDown && insideX && insideZ;
};
