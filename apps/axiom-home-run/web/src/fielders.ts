/*
 * fielders.ts — the toy defenders. Each fielder holds a fixed position (its
 * `FIELDER_SPOTS` spot) and stands watching; the ONLY time it moves is to chase a
 * hit ball whose projected landing falls near its region — it runs to intercept
 * (clamped to a slightly expanded radius), can catch/field what it reaches, then
 * walks back to its spot. No wander, no pathfinding, no base running. The walk/run
 * gait is driven by the fielder's actual displacement (`traveled`/`facing`/`speed`),
 * so it is procedural and deterministic. SDK-free.
 */

import { type Vec3 } from "./vec.ts";
import type { FielderState } from "./types.ts";
import * as C from "./constants.ts";

export const newFielders = (): FielderState[] =>
  C.FIELDER_SPOTS.map((spot) => ({
    // Standing on the spot, facing home plate (toward the batter).
    chasing: false,
    facing: Math.atan2(-spot.x, -spot.z),
    speed: 0,
    traveled: 0,
    x: spot.x,
    z: spot.z,
  }));

/** Below this per-tick step the fielder is treated as standing (facing held, no
 * gait advance) — the view then points it at the batter/ball it is watching. */
const WALK_EPS = 0.004;

/** Project where a ball in flight will next reach ground level (closed form). */
export const projectLanding = (
  pos: Vec3,
  vel: Vec3,
  gravityPerTick: number,
): { readonly x: number; readonly z: number } => {
  const h = pos.y - C.BALL_RADIUS;
  const disc = vel.y * vel.y + 2 * gravityPerTick * Math.max(0, h);
  const t = gravityPerTick > 0 ? (vel.y + Math.sqrt(disc)) / gravityPerTick : 0;
  return { x: pos.x + vel.x * t, z: pos.z + vel.z * t };
};

/**
 * Advance every fielder one tick. A fielder whose region is near a reachable
 * `landing` runs to intercept it (clamped so it never abandons its region);
 * otherwise it holds — walking back to its fixed spot if a chase displaced it,
 * then standing still. Movement bookkeeping (`traveled`/`speed`/`facing`) feeds
 * the procedural walk gait.
 */
export const stepFielders = (fielders: FielderState[], landing: { readonly x: number; readonly z: number } | null): void => {
  for (let i = 0; i < fielders.length; i += 1) {
    const f = fielders[i]!;
    const spot = C.FIELDER_SPOTS[i]!;
    let tx: number;
    let tz: number;
    const reachable =
      landing !== null && Math.hypot(landing.x - spot.x, landing.z - spot.z) <= spot.radius * C.FIELDER_REACH_MULT;
    if (f.cover !== undefined) {
      // Covering a base for a force play — walk onto the bag (top priority).
      tx = f.cover.x;
      tz = f.cover.z;
      f.chasing = true;
    } else if (reachable) {
      // Chase the landing point, clamped to a slightly expanded radius.
      let cx = landing.x - spot.x;
      let cz = landing.z - spot.z;
      const d = Math.hypot(cx, cz);
      const limit = spot.radius * C.FIELDER_CHASE_CLAMP;
      if (d > limit) {
        cx = (cx / d) * limit;
        cz = (cz / d) * limit;
      }
      tx = spot.x + cx;
      tz = spot.z + cz;
      f.chasing = true;
    } else {
      // Hold the fixed spot (walk back to it if a prior chase moved us off).
      tx = spot.x;
      tz = spot.z;
      f.chasing = false;
    }
    const dx = tx - f.x;
    const dz = tz - f.z;
    const d = Math.hypot(dx, dz);
    const step = Math.min(d, C.FIELDER_SPEED);
    if (d > 1e-6) {
      f.x += (dx / d) * step;
      f.z += (dz / d) * step;
    }
    const moved = step > 0 && d > 1e-6 ? step : 0;
    f.traveled += moved;
    f.speed = moved * C.FIXED_HZ;
    f.facing = moved > WALK_EPS ? Math.atan2(dx, dz) : f.facing;
  }
};

/** The index of a fielder able to catch/field the ball right now, or -1. A ball on
 * the ground is scooped from a wider reach than a pinpoint air catch. */
export const catchingFielder = (fielders: readonly FielderState[], ball: Vec3): number => {
  if (ball.y > C.CATCH_HEIGHT) {
    return -1;
  }
  const reach = ball.y <= C.GROUND_BALL_HEIGHT ? C.GROUND_FIELD_RADIUS : C.CATCH_RADIUS;
  for (let i = 0; i < fielders.length; i += 1) {
    const f = fielders[i]!;
    if (Math.hypot(ball.x - f.x, ball.z - f.z) <= reach) {
      return i;
    }
  }
  return -1;
};
