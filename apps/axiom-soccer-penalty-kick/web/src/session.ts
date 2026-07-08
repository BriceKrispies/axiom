/*
 * The 5-round session — a faithful port of `penalty_session.rs`. It wraps one
 * shot interaction, awards once per resolved shot, records round history, tracks
 * an app-local best score, plays the impact effect, and advances through
 * BetweenRounds / SessionComplete on the continue prompt. Reset restarts,
 * preserving best.
 */

import { type EffectDescriptor, type ImpactEffectState, describeEffect, effectAdvanced, effectForResult, effectSessionComplete } from "./effects.ts";
import { type InteractionState, type FlightState, interactionAdvance, interactionStart } from "./interaction.ts";
import { type ScoreAward, awardShot } from "./scoring.ts";
import { type Vec3, ZERO } from "./engine.ts";
import type { PenaltyInputIntent } from "./input.ts";
import type { ShotResult } from "./result.ts";

export const SESSION_ROUNDS = 5;

export type LoopState =
  | "RoundAiming"
  | "RoundCharging"
  | "RoundBallInFlight"
  | "BetweenRounds"
  | "SessionComplete";

export interface RoundState {
  readonly roundNumber: number;
  readonly targetX: number;
  readonly targetY: number;
  readonly power: number;
  readonly result: ShotResult;
  readonly award: ScoreAward;
  readonly finalBallPosition: Vec3;
}

export interface SessionState {
  readonly shot: InteractionState;
  readonly score: number;
  readonly streak: number;
  readonly best: number;
  readonly roundIndex: number;
  readonly history: readonly RoundState[];
  readonly loopState: LoopState;
  readonly lastAward: ScoreAward | null;
  readonly effect: ImpactEffectState | null;
}

export const sessionWithBest = (best: number): SessionState => ({
  shot: interactionStart(),
  score: 0,
  streak: 0,
  best,
  roundIndex: 0,
  history: [],
  loopState: "RoundAiming",
  lastAward: null,
  effect: null,
});

export const sessionNew = (): SessionState => sessionWithBest(0);

export const roundNumber = (session: SessionState): number => session.roundIndex + 1;

const loopFromShot = (state: FlightState): LoopState =>
  state === "Aiming" ? "RoundAiming" : state === "Charging" ? "RoundCharging" : "RoundBallInFlight";

const tickEffect = (session: SessionState): SessionState => ({
  ...session,
  effect: session.effect ? effectAdvanced(session.effect) : null,
});

const recordResolved = (
  session: SessionState,
  shot: InteractionState,
  targetX: number,
  targetY: number,
  power: number,
  result: ShotResult,
  finalBallPosition: Vec3,
): SessionState => {
  const award = awardShot(roundNumber(session), result.kind, power, targetX, targetY, session.score, session.streak);
  const item: RoundState = { roundNumber: roundNumber(session), targetX, targetY, power, result, award, finalBallPosition };
  return {
    ...session,
    shot,
    score: award.scoreAfter,
    streak: award.streakAfter,
    best: Math.max(session.best, award.scoreAfter),
    history: [...session.history, item],
    loopState: "BetweenRounds",
    lastAward: award,
    effect: effectForResult(result.kind, result.detail, finalBallPosition, award.total),
  };
};

const awardOnResolve = (session: SessionState, shot: InteractionState): SessionState => {
  if (shot.preview && shot.resolved) {
    return recordResolved(session, shot, shot.preview.targetX, shot.preview.targetY, shot.preview.power, shot.resolved.result, shot.resolved.finalBallPosition);
  }
  return { ...session, shot, loopState: "BetweenRounds" };
};

const stepShot = (session: SessionState, intent: PenaltyInputIntent): SessionState => {
  const shot = interactionAdvance(session.shot, intent);
  if (shot.state === "Resolved") return awardOnResolve(session, shot);
  return { ...session, shot, loopState: loopFromShot(shot.state) };
};

const continueRound = (session: SessionState): SessionState => {
  if (session.history.length < SESSION_ROUNDS) {
    return { ...session, shot: interactionStart(), roundIndex: session.history.length, loopState: "RoundAiming", lastAward: null, effect: null };
  }
  return { ...session, loopState: "SessionComplete", effect: effectSessionComplete(session.score) };
};

const stepBetween = (session: SessionState, intent: PenaltyInputIntent): SessionState => {
  if (intent.resetPressed) return tickEffect(session); // reset handled at the top of advance
  if (intent.continuePressed) return continueRound(session);
  return tickEffect(session);
};

/** Advance the session one fixed tick. Reset restarts (best preserved). */
export const sessionAdvance = (session: SessionState, intent: PenaltyInputIntent): SessionState => {
  if (intent.resetPressed) return sessionWithBest(session.best);
  switch (session.loopState) {
    case "RoundAiming":
    case "RoundCharging":
    case "RoundBallInFlight":
      return stepShot(session, intent);
    case "BetweenRounds":
      return stepBetween(session, intent);
    case "SessionComplete":
      return tickEffect(session);
  }
};

export const sessionEffectDescriptor = (session: SessionState): EffectDescriptor | null =>
  session.effect ? describeEffect(session.effect) : null;

export const sessionCameraOffset = (session: SessionState): Vec3 => (session.effect ? describeEffect(session.effect).cameraOffset : ZERO);
