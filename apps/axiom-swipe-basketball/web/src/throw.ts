/*
 * throw.ts — the pure mapping from a 2D pointer swipe to a 3D launch velocity, the
 * heart of the "swipe to shoot" feel. SDK-free and directly unit-tested.
 *
 * The swipe arrives in screen pixels-per-tick with +y pointing DOWN (see
 * pointer.ts), so an upward flick has negative y. The mapping the brief asks for:
 *   - horizontal swipe  → world +X launch (aim left/right),
 *   - upward swipe       → world +Y lift (the arc; only upward motion lifts),
 *   - overall flick speed → world −Z forward (toward the hoop), with a small base
 *     so even a straight-up flick still carries forward.
 * The result is clamped so an absurd flick can't leave the world.
 */

import { type Vec2, type Vec3, length, scale, vec3 } from "./vec.ts";
import {
  FORWARD_BASE,
  FORWARD_SCALE,
  LIFT_SCALE,
  MAX_THROW_SPEED,
  THROW_SCALE_X,
} from "./constants.ts";

/** Map a screen swipe velocity (px/tick, +y down) to a world launch velocity (m/s). */
export const swipeToThrow = (swipe: Vec2): Vec3 => {
  const flickSpeed = Math.hypot(swipe.x, swipe.y);
  const upward = Math.max(0, -swipe.y);
  const raw = vec3(
    swipe.x * THROW_SCALE_X,
    upward * LIFT_SCALE,
    -(FORWARD_BASE + flickSpeed * FORWARD_SCALE),
  );
  const speed = length(raw);
  return speed > MAX_THROW_SPEED ? scale(raw, MAX_THROW_SPEED / speed) : raw;
};
