/*
 * physics.ts — the deterministic, fixed-step ball simulator, owned entirely by the
 * app (SDK-free). Semi-implicit Euler under gravity, a constant horizontal wind
 * acceleration, and linear air damping, resolved against the single ground plane.
 * Same inputs → same outputs every tick, so a drop is replayable and unit-testable.
 *
 * The damping term is what makes this game aimable. Because drag is proportional to
 * speed, both the fall and the drift converge: the ball reaches a terminal velocity
 * of |GRAVITY| / LINEAR_DAMPING, which bounds the fall time, and the wind converges
 * to a drift speed of |wind| / LINEAR_DAMPING, which bounds how far off-line it can
 * push you. Without drag the fall would accelerate without limit and the wind would
 * integrate quadratically — the player would be guessing, not judging.
 *
 * The ground is one plane, not a collider set: this world has exactly one surface
 * that matters, and the landing is scored on FIRST contact. Everything after that
 * first touch — the bounce, the roll, the settle — is flourish the score already
 * ignores.
 */

import { type Vec3, add, scale, vec3 } from "./vec.ts";
import {
  GRAVITY,
  GROUND_Y,
  LINEAR_DAMPING,
  POST_COLLISION_DAMPING,
  RESTITUTION_GROUND,
  TANGENTIAL_FRICTION,
} from "./constants.ts";

/** A ground contact, reported on the tick the ball touches down. */
export interface GroundContact {
  /** Where the ball met the ground (its centre projected onto the plane). */
  readonly point: Vec3;
  /** The downward closing speed at the moment of contact (≥ 0). */
  readonly impactSpeed: number;
}

/** The outcome of stepping the ball one tick. */
export interface StepResult {
  readonly pos: Vec3;
  readonly vel: Vec3;
  /** The ground contact made this step, or `null` if the ball is still in the air. */
  readonly contact: GroundContact | null;
}

/**
 * Advance the ball by a single fixed step under gravity + `wind` + air damping, then
 * resolve it against the ground plane. `wind` is a horizontal ACCELERATION (m/s²).
 */
export const stepBall = (pos: Vec3, vel: Vec3, radius: number, wind: Vec3, dt: number): StepResult => {
  // Damping first, then acceleration: the fixed point of v ← v(1 − k·dt) + a·dt is
  // v = a/k, which is exactly the terminal velocity the tuning is written against.
  const damped = scale(vel, Math.max(0, 1 - LINEAR_DAMPING * dt));
  const accelerated = add(damped, scale(vec3(wind.x, GRAVITY, wind.z), dt));
  const moved = add(pos, scale(accelerated, dt));

  const restY = GROUND_Y + radius;
  const penetrating = moved.y < restY;
  const descending = accelerated.y < 0;

  if (!penetrating || !descending) {
    return { contact: null, pos: moved, vel: accelerated };
  }

  const impactSpeed = -accelerated.y;
  const bounced = scale(
    vec3(
      accelerated.x * TANGENTIAL_FRICTION,
      impactSpeed * RESTITUTION_GROUND,
      accelerated.z * TANGENTIAL_FRICTION,
    ),
    POST_COLLISION_DAMPING,
  );

  return {
    contact: { impactSpeed, point: vec3(moved.x, GROUND_Y, moved.z) },
    pos: vec3(moved.x, restY, moved.z),
    vel: bounced,
  };
};

/** Give up predicting after this many ticks (20 s) — a guard, never reached in practice. */
const MAX_PREDICT_TICKS = 1200;

/** Where a drop lands, and how long it took. */
export interface Prediction {
  readonly point: Vec3;
  readonly seconds: number;
}

/**
 * Run the drop forward to its first ground contact and report where it lands. This
 * replays `stepBall` rather than solving the closed form, so the prediction is exact
 * with respect to the simulation the player actually gets — a reticle that agreed
 * with the algebra but not with the sim would be a lie told 60 times a second.
 *
 * Passing zero wind is the interesting case: the aim reticle shows where the toss
 * goes if the wind never touches it, leaving the wind correction as the one judgement
 * the game asks the player to make.
 */
export const predictLanding = (pos: Vec3, vel: Vec3, radius: number, wind: Vec3, dt: number): Prediction => {
  let p = pos;
  let v = vel;
  for (let i = 0; i < MAX_PREDICT_TICKS; i += 1) {
    const step = stepBall(p, v, radius, wind, dt);
    if (step.contact !== null) {
      return { point: step.contact.point, seconds: (i + 1) * dt };
    }
    p = step.pos;
    v = step.vel;
  }
  return { point: vec3(p.x, GROUND_Y, p.z), seconds: MAX_PREDICT_TICKS * dt };
};

/** The horizontal distance of a point from the world origin (the target centre). */
export const horizontalDistance = (p: Vec3): number => Math.hypot(p.x, p.z);

/** Terminal fall speed (m/s) implied by the tuning — the fall converges to this. */
export const terminalSpeed = (): number => Math.abs(GRAVITY) / LINEAR_DAMPING;
