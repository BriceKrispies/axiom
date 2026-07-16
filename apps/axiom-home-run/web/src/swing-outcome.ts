/*
 * swing-outcome.ts — the authoritative, deterministic hit-outcome model.
 * `evaluateSwingOutcome` is called ONCE, the instant a swing commits (session.ts's
 * `stepSwing` transition ready → swing), and forward-simulates EXACTLY the same
 * per-tick sequence `session.ts` used to run reactively: `stepSwing` for the bat,
 * the same gravity/velocity integration for the pitch, and the real `sweptContact`
 * for contact detection. On a hit, it projects the post-contact flight with the
 * REAL `newFlight` + `stepFlight` from `ball.ts` — not a separate cinematic
 * trajectory — so home-run classification can never diverge from what the ball
 * will actually do. The result is a total, fully-populated `SwingOutcome`: both
 * the real launched ball AND the home-run cinematic consume this one record.
 *
 * Determinism: `evaluateSwingOutcome` is a pure function of its four arguments.
 * Identical `(swingState, pitchState, batterState, tuning)` always produces a
 * byte-identical `SwingOutcome` — no wall-clock, no unseeded randomness.
 */

import { type Vec3, ZERO, add, length, scale, vec3 } from "./vec.ts";
import type { BatterPosition, Contact, HomeRunReason, PitchFlightState, Swing, SwingOutcome } from "./types.ts";
import type { HomeRunCinematicTuning } from "./cinematic-constants.ts";
import { batTangent, stepSwing, sweptContact } from "./swing.ts";
import { beyondWall, newFlight, stepFlight } from "./ball.ts";
import * as C from "./constants.ts";

const normalize = (v: Vec3): Vec3 => {
  const len = length(v);
  return len > 1e-9 ? scale(v, 1 / len) : ZERO;
};

const NO_CONTACT: SwingOutcome = {
  batVelocityAtContact: ZERO,
  contactNormal: ZERO,
  contactOccurs: false,
  contactPoint: ZERO,
  contactQuality: 0,
  contactTick: -1,
  exitSpeed: 0,
  exitVelocity: ZERO,
  homeRunReason: "no-contact",
  isFair: false,
  isHomeRun: false,
  launchAngle: 0,
  launchDirection: ZERO,
  pitchVelocityAtContact: ZERO,
  projectedApex: ZERO,
  projectedDistance: 0,
  projectedLanding: ZERO,
  spray: 0,
};

/** Project the post-contact flight with the REAL `ball.ts` physics — the same
 * function the actual in-play ball uses — up to `tuning.maxPredictionSteps`. */
const projectFlight = (
  contact: Contact,
  tuning: HomeRunCinematicTuning,
): { readonly homeRunReason: HomeRunReason; readonly isHomeRun: boolean; readonly isFair: boolean; readonly apex: Vec3; readonly landing: Vec3; readonly distance: number } => {
  const flight = newFlight(contact.point, contact.exitVel, contact.exitSpeed, contact.loft, contact.spray);
  let apex = contact.point;
  let reachedWallLine = false;
  let done = false;
  // `trajectoryPredictionStepTicks` real ticks per bookkeeping "step" (default 1 — every
  // real tick tracked); `maxPredictionSteps` bounds the total prediction horizon. Each
  // individual tick still runs the exact real `stepFlight` physics — never approximated.
  for (let step = 0; step < tuning.maxPredictionSteps && !done; step += 1) {
    for (let sub = 0; sub < tuning.trajectoryPredictionStepTicks && !done; sub += 1) {
      done = stepFlight(flight);
      if (flight.pos.y > apex.y) {
        apex = flight.pos;
      }
      reachedWallLine = reachedWallLine || beyondWall(flight.pos.x, flight.pos.z);
    }
  }
  const distance = flight.homer ? Math.hypot(flight.pos.x, flight.pos.z) : flight.firstLandDist > 0 ? Math.max(flight.firstLandDist, Math.hypot(flight.pos.x, flight.pos.z)) : Math.hypot(flight.pos.x, flight.pos.z);
  const isFair = !flight.foul;
  const homeRunReason: HomeRunReason = flight.homer
    ? "clears-wall-fair"
    : !isFair
      ? "not-fair"
      : reachedWallLine
        ? "below-wall-height"
        : "does-not-clear-wall";
  return { apex, distance, homeRunReason, isFair, isHomeRun: flight.homer, landing: flight.pos };
};

/**
 * Forward-simulate a committed swing against the live pitch to deterministically
 * predict contact — or its absence. `swingState` is the swing record AS OF right
 * after the commit's own `stepSwing` call (state `"swing"`, `theta === THETA_READY`,
 * unmoved yet — mirrors how `session.ts` computes it before calling `#stepPitch`).
 * `pitchState` is the ball's pos/vel/gravity as of the START of that same tick.
 * `batterState` is frozen for the whole search — the swing, once committed, cannot
 * be nudged by a later lateral step.
 */
export const evaluateSwingOutcome = (
  swingState: Swing,
  pitchState: PitchFlightState,
  batterState: BatterPosition,
  tuning: HomeRunCinematicTuning,
): SwingOutcome => {
  let swing = swingState;
  let prevTheta = swingState.theta;
  let ballPos = pitchState.pos;
  let ballVel = pitchState.vel;

  for (let tick = 0; tick < tuning.swingContactSearchMaxTicks; tick += 1) {
    const prevBall = ballPos;
    ballVel = vec3(ballVel.x, ballVel.y - pitchState.gravityPerTick, ballVel.z);
    ballPos = add(ballPos, ballVel);

    if (swing.state === "swing") {
      const contact = sweptContact(prevTheta, swing.theta, swing.omega, batterState.x, prevBall, ballPos, ballVel.z);
      if (contact !== null) {
        const projected = projectFlight(contact, tuning);
        return {
          batVelocityAtContact: scale(batTangent(swing.theta), swing.omega * contact.r * C.FIXED_HZ),
          contactNormal: normalize(batTangent(swing.theta)),
          contactOccurs: true,
          contactPoint: contact.point,
          contactQuality: contact.quality,
          contactTick: tick,
          exitSpeed: contact.exitSpeed,
          exitVelocity: contact.exitVel,
          homeRunReason: projected.homeRunReason,
          isFair: projected.isFair,
          isHomeRun: projected.isHomeRun,
          launchAngle: contact.loft,
          launchDirection: normalize(contact.exitVel),
          pitchVelocityAtContact: ballVel,
          projectedApex: projected.apex,
          projectedDistance: projected.distance,
          projectedLanding: projected.landing,
          spray: contact.spray,
        };
      }
    }
    if (ballPos.z <= C.CATCHER_Z) {
      return NO_CONTACT;
    }

    prevTheta = swing.theta;
    swing = stepSwing(swing, false);
  }
  return NO_CONTACT;
};
