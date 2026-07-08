/*
 * The ball flight — a faithful TS port of `penalty_ball.rs`.
 *
 * The ball is a real physics projectile: an impulse at the strike, then gravity.
 * The Rust game runs `axiom-physics` (semi-implicit Euler at 60 Hz, g = (0,-9.8,0),
 * impulse = mass·velocity), calibrates the launch velocity with a two-probe solve
 * so the ball lands exactly on the aimed goal-plane point at tick N, and pins the
 * endpoints by ramping the sub-mm discrete-integration residual linearly. We
 * reimplement that integrator and that procedure exactly here (three lines of
 * integrator + the affine two-probe calibration), so the sampled arc matches.
 */

import { type Vec3, length, solveLaunchToTarget, sub, vec3 } from "./engine.ts";
import { BALL_RADIUS, GOAL_HALF_WIDTH, GOAL_HEIGHT, GOAL_LINE_Z, GROUND_Y, PENALTY_SPOT_Z } from "./scene-constants.ts";

const MAX_FLIGHT_TICKS = 60;
const MIN_FLIGHT_TICKS = 24;
const PATH_CAP = MAX_FLIGHT_TICKS + 1; // 61
const TRAIL_MAX = 6;

const DT = 1 / 60;
const GRAVITY: Vec3 = vec3(0, -9.8, 0); // physics default; NOT -9.81

const SHADOW_Y = GROUND_Y + 0.03; // 0.03
const SHADOW_BASE_X = BALL_RADIUS * 1.1; // 0.352
const SHADOW_BASE_Z = BALL_RADIUS * 1.0; // 0.32

// The aim span is 1.35× the true goal frame, so extreme aims land outside the
// posts/bar (enabling misses); the goal MOUTH test uses the true frame dims.
const AIM_HALF_SPAN = GOAL_HALF_WIDTH * 1.35; // 4.941
const AIM_TOP = GOAL_HEIGHT * 1.35; // 3.294

/** The ball's launch position — on the penalty spot, resting on the ground. */
export const penaltySpot = (): Vec3 => vec3(0, BALL_RADIUS, PENALTY_SPOT_Z);

/** Map the normalized aim (tx ∈ [-100,100], ty ∈ [0,100]) to a goal-plane point (z = 0). */
export const worldTarget = (tx: number, ty: number): Vec3 =>
  vec3(AIM_HALF_SPAN * (tx / 100), GROUND_Y + AIM_TOP * (ty / 100), GOAL_LINE_Z);

/** Flight length in ticks: power 0 → 60 (slow), power 100 → 24 (fast). Integer truncation, ported literally. */
export const flightTicks = (power: number): number => {
  const p = Math.min(Math.max(power, 0), 100);
  const reduce = Math.trunc(((MAX_FLIGHT_TICKS - MIN_FLIGHT_TICKS) * p) / 100); // trunc(36 * p / 100)
  return Math.min(Math.max(MAX_FLIGHT_TICKS - reduce, MIN_FLIGHT_TICKS), MAX_FLIGHT_TICKS);
};

/**
 * Calibrate the launch velocity so the ball lands exactly on `target` at tick
 * `n`, then pin the endpoints — the engine's projectile launch solve, specialized
 * with this game's gravity + 60 Hz step and the length-61 path cap.
 */
const integrateToTarget = (start: Vec3, target: Vec3, ticks: number): Vec3[] =>
  solveLaunchToTarget(start, target, GRAVITY, DT, ticks, PATH_CAP);

/** A resolved ball trajectory: a sampled path plus the flight length. */
export interface PenaltyBallTrajectory {
  readonly path: readonly Vec3[];
  readonly totalTicks: number;
}

export const trajectoryToTarget = (start: Vec3, target: Vec3, totalTicks: number): PenaltyBallTrajectory => {
  const n = Math.min(Math.max(totalTicks, MIN_FLIGHT_TICKS), MAX_FLIGHT_TICKS);
  return { path: integrateToTarget(start, target, n), totalTicks: n };
};

const positionAt = (trajectory: PenaltyBallTrajectory, elapsed: number): Vec3 =>
  trajectory.path[Math.min(Math.min(elapsed, trajectory.totalTicks), MAX_FLIGHT_TICKS)]!;

/** The ball's per-tick render pose: position, its shrinking blob shadow, and a short trail. */
export interface PenaltyBallPose {
  readonly position: Vec3;
  readonly radius: number;
  readonly shadowCenter: Vec3;
  readonly shadowRadiusX: number;
  readonly shadowRadiusZ: number;
  readonly trail: readonly Vec3[];
  readonly trailLen: number;
}

const makePose = (position: Vec3, trail: readonly Vec3[]): PenaltyBallPose => {
  const height = Math.max(position.y - BALL_RADIUS, 0);
  const factor = 1 / (1 + height * 0.5);
  return {
    position,
    radius: BALL_RADIUS,
    shadowCenter: vec3(position.x, SHADOW_Y, position.z),
    shadowRadiusX: SHADOW_BASE_X * factor,
    shadowRadiusZ: SHADOW_BASE_Z * factor,
    trail,
    trailLen: trail.length,
  };
};

const poseAt = (trajectory: PenaltyBallTrajectory, elapsed: number): PenaltyBallPose => {
  const n = Math.min(elapsed, TRAIL_MAX);
  const trail: Vec3[] = [];
  for (let i = 0; i < n; i += 1) {
    trail.push(positionAt(trajectory, elapsed - 1 - i)); // most-recent-first
  }
  return makePose(positionAt(trajectory, elapsed), trail);
};

/** The ball at rest on the penalty spot (no trail) — the pose shown before launch. */
export const restingPose = (): PenaltyBallPose => makePose(penaltySpot(), []);

/** A live shot: its trajectory + how many ticks have elapsed. */
export interface PenaltyBallFlight {
  readonly trajectory: PenaltyBallTrajectory;
  readonly elapsedTicks: number;
}

/** Build the flight for a locked shot preview (target + power). */
export const launchFlight = (targetX: number, targetY: number, power: number): PenaltyBallFlight => ({
  trajectory: trajectoryToTarget(penaltySpot(), worldTarget(targetX, targetY), flightTicks(power)),
  elapsedTicks: 0,
});

export const flightAdvanced = (flight: PenaltyBallFlight): PenaltyBallFlight => ({
  trajectory: flight.trajectory,
  elapsedTicks: Math.min(flight.elapsedTicks + 1, flight.trajectory.totalTicks),
});

export const flightArrived = (flight: PenaltyBallFlight): boolean => flight.elapsedTicks >= flight.trajectory.totalTicks;

export const flightPose = (flight: PenaltyBallFlight): PenaltyBallPose => poseAt(flight.trajectory, flight.elapsedTicks);

/** Straight-line distance the ball still has to travel toward the goal line (for the kicker cue). */
export const flightRemaining = (flight: PenaltyBallFlight): number => length(sub(worldTargetOf(flight), flightPose(flight).position));

const worldTargetOf = (flight: PenaltyBallFlight): Vec3 => flight.trajectory.path[flight.trajectory.totalTicks]!;
