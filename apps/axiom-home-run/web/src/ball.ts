/*
 * ball.ts — ball flight for a ball IN PLAY (post-contact), the toy stadium's
 * boundary rules (foul wedge, outfield wall line, home-run volume), the arcade
 * outcome classification, and the scoring table application. Pure, mutation-in-
 * place on one explicit `BallFlight` record owned by the session. SDK-free.
 */

import { type Vec3, vec3 } from "./vec.ts";
import type { Outcome } from "./types.ts";
import * as C from "./constants.ts";

/** The in-play flight state the session owns while a hit ball is live. */
export interface BallFlight {
  pos: Vec3;
  vel: Vec3;
  bounces: number;
  /** Horizontal distance from home of the FIRST ground touch (0 until it lands). */
  firstLandDist: number;
  /** Set when the ball clears the wall line above wall height, in fair territory. */
  homer: boolean;
  /** Set when the ball first touches ground (or the backstop) in foul territory. */
  foul: boolean;
  /** Launch parameters frozen at contact (classification inputs). */
  readonly exitSpeed: number;
  readonly loft: number;
  readonly spray: number;
  ticks: number;
}

export const newFlight = (pos: Vec3, vel: Vec3, exitSpeed: number, loft: number, spray: number): BallFlight => ({
  bounces: 0,
  exitSpeed,
  firstLandDist: 0,
  foul: Math.abs(spray) > C.FOUL_ANGLE,
  homer: false,
  loft,
  pos,
  spray,
  ticks: 0,
  vel,
});

/** Whether XZ is inside the fair wedge (between the foul lines, in front of home). */
export const isFair = (x: number, z: number): boolean => z >= 0 && Math.abs(x) <= z;

/** Whether XZ is at/beyond the outfield wall line |x| + z = WALL_LINE. */
export const beyondWall = (x: number, z: number): boolean => Math.abs(x) + z >= C.WALL_LINE;

/**
 * Advance the flight one tick: gravity, ground bounces → roll → rest, the wall
 * (home run above it, a dead bounce off it below), and the foul-side backstop.
 * Returns true once the ball has come to rest (or died at the backstop).
 */
export const stepFlight = (b: BallFlight): boolean => {
  b.ticks += 1;
  const g = C.GRAVITY / (C.FIXED_HZ * C.FIXED_HZ);
  b.vel = vec3(b.vel.x, b.vel.y - g, b.vel.z);
  let next = vec3(b.pos.x + b.vel.x, b.pos.y + b.vel.y, b.pos.z + b.vel.z);

  // The outfield wall line — crossing it airborne above the wall is a HOME RUN;
  // hitting it below is a dead bounce back into play.
  if (!b.homer && !beyondWall(b.pos.x, b.pos.z) && beyondWall(next.x, next.z)) {
    if (b.foul) {
      // Foul territory past the corners — let it die where it is.
      return true;
    }
    if (next.y >= C.WALL_HEIGHT) {
      b.homer = true;
    } else {
      // Reflect the wall-normal velocity component. Wall normals: ∓(±1,0,1)/√2.
      const sx = b.pos.x >= 0 ? 1 : -1;
      const inv = Math.SQRT1_2;
      const nx = -sx * inv;
      const nz = -inv;
      const vn = b.vel.x * nx + b.vel.z * nz;
      b.vel = vec3(
        (b.vel.x - 2 * vn * nx) * C.WALL_RESTITUTION,
        b.vel.y * C.WALL_RESTITUTION,
        (b.vel.z - 2 * vn * nz) * C.WALL_RESTITUTION,
      );
      next = vec3(b.pos.x + b.vel.x, b.pos.y + b.vel.y, b.pos.z + b.vel.z);
    }
  }

  // Ground contact: bounce, then roll, then rest.
  if (next.y <= C.BALL_RADIUS && b.vel.y < 0) {
    if (b.firstLandDist === 0) {
      b.firstLandDist = Math.hypot(next.x, next.z);
      if (!isFair(next.x, next.z) && !b.homer) {
        b.foul = true;
      }
    }
    b.bounces += 1;
    next = vec3(next.x, C.BALL_RADIUS, next.z);
    b.vel = vec3(b.vel.x * C.BOUNCE_FRICTION, -b.vel.y * C.BOUNCE_RESTITUTION, b.vel.z * C.BOUNCE_FRICTION);
    if (b.bounces > 3 || Math.abs(b.vel.y) * C.FIXED_HZ < 1.2) {
      b.vel = vec3(b.vel.x, 0, b.vel.z);
    }
  }
  // Rolling decay + rest.
  if (next.y <= C.BALL_RADIUS + 1e-6 && b.vel.y === 0) {
    b.vel = vec3(b.vel.x * C.ROLL_DECAY, 0, b.vel.z * C.ROLL_DECAY);
    const speed = Math.hypot(b.vel.x, b.vel.z) * C.FIXED_HZ;
    if (speed < C.REST_SPEED) {
      b.pos = next;
      return true;
    }
  }
  // Foul balls that reach the backstop die there.
  if (next.z <= C.CATCHER_Z) {
    b.pos = next;
    return true;
  }
  b.pos = next;
  return b.ticks >= C.FLIGHT_TIMEOUT_TICKS;
};

/** Classify a RESOLVED flight (rest / timeout — catches are classified by the caller). */
export const classifyFlight = (b: BallFlight): Outcome => {
  if (b.homer) {
    return "homer";
  }
  if (b.foul) {
    return "foul";
  }
  if (b.exitSpeed < C.WEAK_EXIT_SPEED) {
    return "weak";
  }
  if (b.loft < C.GROUNDER_LOFT) {
    return "grounder";
  }
  const dist = b.firstLandDist > 0 ? b.firstLandDist : Math.hypot(b.pos.x, b.pos.z);
  if (b.loft > C.POPUP_LOFT && dist < C.POPUP_MAX_DIST) {
    return "popup";
  }
  return "clean";
};

/** Classify a ball a fielder reached: robbed in the air, or fielded on the ground. */
export const classifyCaught = (b: BallFlight): Outcome => {
  if (b.foul) {
    return "foul";
  }
  if (b.bounces === 0) {
    return b.loft > C.POPUP_LOFT ? "popup" : "weak";
  }
  return b.loft < C.GROUNDER_LOFT ? "grounder" : "weak";
};

/**
 * Points for one outcome. Clean hits earn a distance bonus; home runs earn a
 * bigger one AND the consecutive-homer streak multiplier.
 */
export const scoreFor = (outcome: Outcome, distance: number, homerStreak: number): number => {
  const base = C.SCORE_TABLE[outcome];
  if (outcome === "clean") {
    return base + Math.round(distance * C.CLEAN_DIST_BONUS);
  }
  if (outcome === "homer") {
    const mult = Math.min(Math.max(1, homerStreak), C.STREAK_MULT_CAP);
    return (base + Math.round(distance * C.HOMER_DIST_BONUS)) * mult;
  }
  return base;
};
