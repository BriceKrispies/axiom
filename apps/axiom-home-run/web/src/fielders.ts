/*
 * fielders.ts — the toy defenders. Each fielder owns a visible circular patrol
 * region and wanders inside it on a smooth two-frequency drift whose phases and
 * frequencies are seeded per fielder — independent, unsynchronized, replayable.
 * When a ball is hit, fielders whose region is near the projected landing point
 * chase it (clamped to a slightly expanded radius) and can catch or field a
 * reachable ball. No pathfinding, no base running — animated interception
 * hazards only. SDK-free and deterministic.
 */

import { type Vec3, hash01, mix } from "./vec.ts";
import type { FielderState } from "./types.ts";
import * as C from "./constants.ts";

/** The seeded wander target for fielder `i` at `tick` — always inside its circle. */
export const wanderPos = (seed: number, i: number, tick: number): { readonly x: number; readonly z: number } => {
  const spot = C.FIELDER_SPOTS[i]!;
  const f1 = mix(C.WANDER_FREQ_LO, C.WANDER_FREQ_HI, hash01(seed, i, 11));
  const f2 = mix(C.WANDER_FREQ_LO, C.WANDER_FREQ_HI, hash01(seed, i, 12));
  const p1 = hash01(seed, i, 13) * Math.PI * 2;
  const p2 = hash01(seed, i, 14) * Math.PI * 2;
  const p3 = hash01(seed, i, 15) * Math.PI * 2;
  const amp = spot.radius * C.WANDER_AMPLITUDE;
  let dx = amp * (Math.sin(tick * f1 + p1) * 0.62 + Math.sin(tick * f2 * 1.7 + p3) * 0.38);
  let dz = amp * (Math.sin(tick * f2 + p2) * 0.62 + Math.sin(tick * f1 * 1.7 + p1) * 0.38);
  // Clamp the combined offset inside the patrol circle (margin already in amp).
  const d = Math.hypot(dx, dz);
  const limit = spot.radius * 0.95;
  if (d > limit) {
    dx = (dx / d) * limit;
    dz = (dz / d) * limit;
  }
  return { x: spot.x + dx, z: spot.z + dz };
};

export const newFielders = (seed: number): FielderState[] =>
  C.FIELDER_SPOTS.map((_, i) => {
    const w = wanderPos(seed, i, 0);
    return { chasing: false, x: w.x, z: w.z };
  });

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
 * Advance every fielder one tick. While no ball is in play they track their
 * seeded wander; while one is, nearby fielders converge on the projected landing
 * point, clamped so they never abandon their own region.
 */
export const stepFielders = (
  fielders: FielderState[],
  seed: number,
  tick: number,
  landing: { readonly x: number; readonly z: number } | null,
): void => {
  for (let i = 0; i < fielders.length; i += 1) {
    const f = fielders[i]!;
    const spot = C.FIELDER_SPOTS[i]!;
    let tx: number;
    let tz: number;
    const reachable =
      landing !== null && Math.hypot(landing.x - spot.x, landing.z - spot.z) <= spot.radius * C.FIELDER_REACH_MULT;
    if (reachable) {
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
      const w = wanderPos(seed, i, tick);
      tx = w.x;
      tz = w.z;
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
  }
};

/** The index of a fielder able to catch/field the ball right now, or -1. */
export const catchingFielder = (fielders: readonly FielderState[], ball: Vec3): number => {
  if (ball.y > C.CATCH_HEIGHT) {
    return -1;
  }
  for (let i = 0; i < fielders.length; i += 1) {
    const f = fielders[i]!;
    if (Math.hypot(ball.x - f.x, ball.z - f.z) <= C.CATCH_RADIUS) {
      return i;
    }
  }
  return -1;
};
