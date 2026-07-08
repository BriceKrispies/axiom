/*
 * The per-tick shot interaction state machine — a faithful port of
 * `penalty_interaction.rs`. It threads aim + power (Aiming/Charging), the lock
 * (LockedPreview), ball flight (BallInFlight), goalie contact (ContactDetected),
 * goal-plane arrival (ArrivedAtGoalPlane), and resolution (Resolved). One
 * `advance(intent)` per fixed tick, fully deterministic.
 */

import {
  type PenaltyBallFlight,
  type PenaltyBallPose,
  flightAdvanced,
  flightArrived,
  flightPose,
  launchFlight,
  restingPose,
} from "./ball.ts";
import {
  type ContactFrame,
  type GoalieAnimation,
  detectContact,
  goalieAdvanced,
  goalieAnimatedVolumes,
  goalieCollisionWorld,
  goalieIdle,
  goalieLocked,
} from "./goalie.ts";
import { type ResolvedShotState, goalPlaneCrossing, resolveFromContact, resolveFromCrossing } from "./result.ts";
import type { PenaltyInputIntent } from "./input.ts";

export type FlightState =
  | "Aiming"
  | "Charging"
  | "LockedPreview"
  | "BallInFlight"
  | "ContactDetected"
  | "ArrivedAtGoalPlane"
  | "Resolved";

const AIM_RATE = 8;
const CHARGE_PER_TICK = 8;
const POWER_MAX = 100;

export interface ShotPreview {
  readonly targetX: number;
  readonly targetY: number;
  readonly power: number;
  readonly releaseTick: number;
}

export interface InteractionState {
  readonly aimX: number;
  readonly aimY: number;
  readonly power: number;
  readonly state: FlightState;
  readonly tick: number;
  readonly chargeTicks: number;
  readonly preview: ShotPreview | null;
  readonly flight: PenaltyBallFlight | null;
  readonly contact: ContactFrame | null;
  readonly goalie: GoalieAnimation;
  readonly resolved: ResolvedShotState | null;
}

const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);

export const interactionStart = (): InteractionState => ({
  aimX: 0,
  aimY: 50,
  power: 0,
  state: "Aiming",
  tick: 0,
  chargeTicks: 0,
  preview: null,
  flight: null,
  contact: null,
  goalie: goalieIdle(),
  resolved: null,
});

const aimMoved = (aimX: number, aimY: number, ax: number, ay: number): { x: number; y: number } => ({
  x: clamp(aimX + Math.trunc((ax * AIM_RATE) / 100), -100, 100),
  y: clamp(aimY + Math.trunc((ay * AIM_RATE) / 100), 0, 100),
});

/** The ball's current pose: the live flight pose, else the ball at rest on the spot. */
export const ballPose = (state: InteractionState): PenaltyBallPose =>
  state.flight ? flightPose(state.flight) : restingPose();

// Aiming / Charging: fold this tick's aim + charge/release into the next shot state.
const stepActive = (self: InteractionState, intent: PenaltyInputIntent): InteractionState => {
  const aim = aimMoved(self.aimX, self.aimY, intent.aimXAxis, intent.aimYAxis);
  const held: InteractionState = { ...self, aimX: aim.x, aimY: aim.y, preview: null, flight: null, contact: null };
  const charged: InteractionState = {
    ...held,
    power: Math.min(self.power + CHARGE_PER_TICK, POWER_MAX),
    state: "Charging",
    chargeTicks: self.chargeTicks + 1,
  };
  const locked: InteractionState = {
    ...self,
    aimX: aim.x,
    aimY: aim.y,
    state: "LockedPreview",
    chargeTicks: 0,
    preview: { targetX: aim.x, targetY: aim.y, power: self.power, releaseTick: self.tick },
    flight: null,
    contact: null,
  };
  const chargeOrHold = intent.chargePressed ? charged : held;
  return intent.releasePressed ? locked : chargeOrHold;
};

const launch = (self: InteractionState): InteractionState =>
  self.preview
    ? { ...self, state: "BallInFlight", flight: launchFlight(self.preview.targetX, self.preview.targetY, self.preview.power) }
    : self;

const advanceFlight = (self: InteractionState): InteractionState => {
  const flight = self.flight ? flightAdvanced(self.flight) : null;
  const arrived = flight ? flightArrived(flight) : false;
  return { ...self, flight, state: arrived ? "ArrivedAtGoalPlane" : "BallInFlight" };
};

const resolveFromContactState = (self: InteractionState): InteractionState => {
  const position = ballPose(self).position;
  const result = self.contact ? resolveFromContact(self.contact) : resolveFromContact({ tick: self.tick, ballPosition: position, contact: null });
  return { ...self, state: "Resolved", resolved: { result, finalBallPosition: position, crossing: null } };
};

const resolveFromArrival = (self: InteractionState): InteractionState => {
  const position = ballPose(self).position;
  const crossing = goalPlaneCrossing(position);
  return { ...self, state: "Resolved", resolved: { result: resolveFromCrossing(crossing), finalBallPosition: position, crossing } };
};

const stepShot = (self: InteractionState, intent: PenaltyInputIntent): InteractionState => {
  switch (self.state) {
    case "Aiming":
    case "Charging":
      return stepActive(self, intent);
    case "LockedPreview":
      return launch(self);
    case "BallInFlight":
      return advanceFlight(self);
    case "ContactDetected":
      return resolveFromContactState(self);
    case "ArrivedAtGoalPlane":
      return resolveFromArrival(self);
    case "Resolved":
      return self;
  }
};

const nextGoalie = (self: InteractionState, shot: InteractionState): GoalieAnimation => {
  switch (shot.state) {
    case "Aiming":
    case "Charging":
      return goalieIdle();
    case "LockedPreview":
      return shot.preview ? goalieLocked(shot.preview.targetX, shot.preview.targetY) : goalieIdle();
    default:
      return goalieAdvanced(self.goalie);
  }
};

const detect = (shot: InteractionState, goalie: GoalieAnimation): { state: FlightState; contact: ContactFrame | null } => {
  if (shot.state === "BallInFlight" && shot.flight) {
    const volumes = goalieAnimatedVolumes(goalieCollisionWorld(goalie));
    const frame = detectContact(volumes, flightPose(shot.flight).position, shot.flight.elapsedTicks);
    if (frame.contact) return { state: "ContactDetected", contact: frame };
  }
  return { state: shot.state, contact: shot.contact };
};

/** Advance the interaction one fixed tick. `reset` short-circuits to a fresh Aiming shot. */
export const interactionAdvance = (self: InteractionState, intent: PenaltyInputIntent): InteractionState => {
  const tick = self.tick + 1;
  if (intent.resetPressed) {
    return { ...interactionStart(), tick };
  }
  const shot = { ...stepShot({ ...self, tick }, intent), tick };
  const goalie = nextGoalie(self, shot);
  const { state, contact } = detect(shot, goalie);
  return { ...shot, state, contact, goalie };
};
