/*
 * The HUD model — a port of `penalty_hud.rs`'s `from_session`. Pure derivation of
 * the arcade read-out from the session/shot state, consumed by the DOM overlay in
 * the harness (a 3D game draws nothing through the render hook, so the HUD is DOM).
 */

import { type SessionState, SESSION_ROUNDS, roundNumber, sessionEffectDescriptor } from "./session.ts";
import { detailText, resultText } from "./result.ts";
import type { FlightState } from "./interaction.ts";

const RETICLE_CENTER_X = 0.5;
const RETICLE_CENTER_Y = 0.42;
const RETICLE_HALF_WIDTH = 0.14;
const RETICLE_HALF_HEIGHT = 0.1;

const instructionFor = (state: FlightState, resolved: string | null): string => {
  switch (state) {
    case "Aiming":
      return "AIM";
    case "Charging":
      return "HOLD";
    case "LockedPreview":
      return "RELEASE";
    case "BallInFlight":
      return "FLIGHT";
    case "ContactDetected":
      return "CONTACT";
    case "ArrivedAtGoalPlane":
      return "ARRIVED";
    case "Resolved":
      return resolved ?? "RESULT";
  }
};

export interface HudModel {
  readonly score: number;
  readonly roundCurrent: number;
  readonly roundTotal: number;
  readonly best: number;
  readonly power: number;
  readonly powerFill: number;
  readonly powerLabel: string;
  readonly reticleX: number;
  readonly reticleY: number;
  readonly instruction: string;
  readonly result: string | null;
  readonly resultDetail: string | null;
  readonly award: string | null;
  readonly prompt: string | null;
  readonly banner: string | null;
  readonly bannerScale: number;
  readonly sessionComplete: boolean;
}

export const readHudModel = (session: SessionState): HudModel => {
  const shot = session.shot;
  const locked = shot.state !== "Aiming" && shot.state !== "Charging";
  const power = Math.max(shot.power, 0);
  const nx = shot.aimX / 100;
  const ny = (shot.aimY - 50) / 50;
  const resolvedText = shot.resolved ? resultText(shot.resolved.result.kind) : null;
  const effect = sessionEffectDescriptor(session);
  const prompt =
    session.loopState === "SessionComplete" ? "PLAY AGAIN" : session.loopState === "BetweenRounds" ? "CONTINUE" : null;

  return {
    score: session.score,
    roundCurrent: roundNumber(session),
    roundTotal: SESSION_ROUNDS,
    best: session.best,
    power,
    powerFill: power / 100,
    powerLabel: locked ? "LOCKED" : "POWER",
    reticleX: RETICLE_CENTER_X + nx * RETICLE_HALF_WIDTH,
    reticleY: RETICLE_CENTER_Y - ny * RETICLE_HALF_HEIGHT,
    instruction: instructionFor(shot.state, resolvedText),
    result: shot.resolved ? resultText(shot.resolved.result.kind) : null,
    resultDetail: shot.resolved ? detailText(shot.resolved.result.detail) : null,
    award: session.lastAward ? `+${session.lastAward.total}` : null,
    prompt,
    banner: effect ? effect.banner.text : null,
    bannerScale: effect ? effect.banner.scale : 1,
    sessionComplete: session.loopState === "SessionComplete",
  };
};
