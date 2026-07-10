/*
 * physics.ts — the deterministic in-app ball physics the shot rides on. Pure,
 * SDK-free, fully testable under `node --test`.
 *
 * WHY THIS EXISTS (the engine limitation, and the smallest workaround): the
 * `@axiom/game` `sim.physics` facade spawns bodies with NO colliders, exposes no
 * restitution/contacts/triggers, and writes poses back only to the 2D world — it
 * cannot drive a 3D basketball. This module follows the repo's established
 * precedent (the soccer-penalty-kick app's `engine.ts`): a semi-implicit-Euler
 * integrator with real moving-sphere-vs-static-sphere and vs-AABB contacts, plus
 * the impulse response soccer's overlap-only tests lacked. The released ball is
 * genuinely simulated — gravity, damping, spin, rim/backboard/floor bounces — not
 * an interpolated arc.
 *
 * The colliders are built from the SAME constants the visual scene uses
 * (`constants.ts`), so the rim you see is exactly the rim you hit: the torus is
 * approximated by `RIM_COLLIDER_COUNT` static spheres of radius `RIM_TUBE` on the
 * `RIM_RADIUS` circle. With 4 substeps per 60 Hz tick the ball moves ≲ 0.04 m per
 * substep — far under the ball radius (0.12) and every collider thickness — so
 * discrete stepping cannot tunnel at this game's speeds.
 */

import { type Quat, type Vec3, IDENTITY_QUAT, clampToBox, dot, integrateOrientation, normalize, scale, sub, vec3 } from "./vec.ts";
import type { ContactSurface } from "./types.ts";
import {
  BACKBOARD_CENTER,
  BACKBOARD_HALF,
  BALL_RADIUS,
  DT,
  GRAVITY_Y,
  PHYSICS_SUBSTEPS,
  POLE_CENTER,
  POLE_HALF,
  RIM_COLLIDER_COUNT,
  RIM_RADIUS,
  RIM_TUBE,
  RIM_X,
  RIM_Y,
  RIM_Z,
  SHOT_TUNING,
} from "./constants.ts";

/** The live ball's mutable physics state. */
export interface BallState {
  pos: Vec3;
  vel: Vec3;
  /** Angular velocity (rad/s, world frame) — drives the visible spin. */
  angVel: Vec3;
  orient: Quat;
}

/** One surface hit this step (already impulse-resolved). */
export interface ContactEvent {
  readonly surface: ContactSurface;
  /** Normal approach speed at impact (m/s) — scales audio/flash intensity. */
  readonly speed: number;
  readonly position: Vec3;
}

/** One substep's vertical motion, fed to the basket detector. */
export interface SubstepSample {
  readonly prevY: number;
  readonly y: number;
  readonly velY: number;
  /** Squared horizontal distance of the ball center to the rim axis. */
  readonly horizDistSq: number;
}

export interface StepResult {
  readonly contacts: readonly ContactEvent[];
  readonly samples: readonly SubstepSample[];
}

/** A fresh ball at `pos` with launch velocity `vel` and spin `angVel`. */
export const makeBall = (pos: Vec3, vel: Vec3, angVel: Vec3): BallState => ({
  angVel,
  orient: IDENTITY_QUAT,
  pos,
  vel,
});

export const cloneBall = (b: BallState): BallState => ({
  angVel: b.angVel,
  orient: b.orient,
  pos: b.pos,
  vel: b.vel,
});

/** The rim torus as static collider spheres — also the net-strand anchor ring. */
export const RIM_COLLIDER_CENTERS: readonly Vec3[] = Array.from({ length: RIM_COLLIDER_COUNT }, (_, i) => {
  const a = (2 * Math.PI * i) / RIM_COLLIDER_COUNT;
  return vec3(RIM_X + Math.cos(a) * RIM_RADIUS, RIM_Y, RIM_Z + Math.sin(a) * RIM_RADIUS);
});

/**
 * Resolve the ball against one static sphere: positional snap out along the contact
 * normal + restitution impulse on the approaching normal velocity, with a light
 * tangential graze so rim rolls die down. Returns the impact speed, or null.
 */
const resolveSphere = (ball: BallState, center: Vec3, radius: number, restitution: number): number | null => {
  const reach = radius + BALL_RADIUS;
  const diff = sub(ball.pos, center);
  const distSq = dot(diff, diff);
  if (distSq > reach * reach) return null;
  const n = normalize(diff);
  ball.pos = { x: center.x + n.x * reach, y: center.y + n.y * reach, z: center.z + n.z * reach };
  const vn = dot(ball.vel, n);
  if (vn >= 0) return null;
  const tangential = sub(ball.vel, scale(n, vn));
  const graze = scale(tangential, 0.985);
  ball.vel = {
    x: graze.x - n.x * vn * restitution,
    y: graze.y - n.y * vn * restitution,
    z: graze.z - n.z * vn * restitution,
  };
  return -vn;
};

/** Resolve the ball against a static AABB (same snap + impulse shape). */
const resolveBox = (ball: BallState, center: Vec3, half: Vec3, restitution: number, fallback: Vec3): number | null => {
  const closest = clampToBox(ball.pos, center, half);
  const d = sub(ball.pos, closest);
  const distSq = dot(d, d);
  if (distSq > BALL_RADIUS * BALL_RADIUS) return null;
  const dist = Math.sqrt(distSq);
  const n = dist <= 1e-6 ? fallback : scale(d, 1 / dist);
  ball.pos = { x: closest.x + n.x * BALL_RADIUS, y: closest.y + n.y * BALL_RADIUS, z: closest.z + n.z * BALL_RADIUS };
  const vn = dot(ball.vel, n);
  if (vn >= 0) return null;
  const tangential = sub(ball.vel, scale(n, vn));
  ball.vel = {
    x: tangential.x - n.x * vn * restitution,
    y: tangential.y - n.y * vn * restitution,
    z: tangential.z - n.z * vn * restitution,
  };
  return -vn;
};

/**
 * Advance the ball one fixed tick (PHYSICS_SUBSTEPS substeps): semi-implicit Euler
 * (`v += g·h; pos += v·h`), per-second dampings, then contact resolution against the
 * rim ring, backboard, pole, and floor. Mutates `ball`; returns the tick's contacts
 * and per-substep vertical samples for the basket detector.
 */
export const stepBall = (ball: BallState): StepResult => {
  const h = DT / PHYSICS_SUBSTEPS;
  const linKeep = Math.max(0, 1 - SHOT_TUNING.ballLinearDamping * h);
  const angKeep = Math.max(0, 1 - SHOT_TUNING.ballAngularDamping * h);
  const contacts: ContactEvent[] = [];
  const samples: SubstepSample[] = [];

  for (let s = 0; s < PHYSICS_SUBSTEPS; s += 1) {
    const prevY = ball.pos.y;
    ball.vel = { x: ball.vel.x * linKeep, y: (ball.vel.y + GRAVITY_Y * h) * linKeep, z: ball.vel.z * linKeep };
    ball.angVel = scale(ball.angVel, angKeep);
    ball.pos = { x: ball.pos.x + ball.vel.x * h, y: ball.pos.y + ball.vel.y * h, z: ball.pos.z + ball.vel.z * h };
    ball.orient = integrateOrientation(ball.orient, ball.angVel, h);

    for (const center of RIM_COLLIDER_CENTERS) {
      const speed = resolveSphere(ball, center, RIM_TUBE, SHOT_TUNING.rimRestitution);
      if (speed !== null) contacts.push({ position: ball.pos, speed, surface: "rim" });
    }
    const board = resolveBox(ball, BACKBOARD_CENTER, BACKBOARD_HALF, SHOT_TUNING.backboardRestitution, vec3(0, 0, 1));
    if (board !== null) contacts.push({ position: ball.pos, speed: board, surface: "backboard" });
    const pole = resolveBox(ball, POLE_CENTER, POLE_HALF, SHOT_TUNING.ballRestitution, vec3(0, 0, 1));
    if (pole !== null) contacts.push({ position: ball.pos, speed: pole, surface: "pole" });

    if (ball.pos.y < BALL_RADIUS) {
      const impact = -ball.vel.y;
      ball.pos = { x: ball.pos.x, y: BALL_RADIUS, z: ball.pos.z };
      if (ball.vel.y < 0) {
        ball.vel = {
          x: ball.vel.x * 0.96,
          y: -ball.vel.y * SHOT_TUNING.ballRestitution,
          z: ball.vel.z * 0.96,
        };
        ball.angVel = scale(ball.angVel, 0.8);
        contacts.push({ position: ball.pos, speed: impact, surface: "floor" });
      }
    }

    const dx = ball.pos.x - RIM_X;
    const dz = ball.pos.z - RIM_Z;
    samples.push({ horizDistSq: dx * dx + dz * dz, prevY, velY: ball.vel.y, y: ball.pos.y });
  }

  return { contacts, samples };
};

/**
 * Predict a launch's path by running THE SAME `stepBall` on a clone — the debug
 * trajectory preview (and its honesty test) share every constant with the real shot.
 */
export const predictTrajectory = (start: BallState, points: number, strideTicks: number): Vec3[] => {
  const ghost = cloneBall(start);
  const path: Vec3[] = [];
  for (let i = 0; i < points; i += 1) {
    for (let k = 0; k < strideTicks; k += 1) stepBall(ghost);
    path.push(ghost.pos);
  }
  return path;
};
