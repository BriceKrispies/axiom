/*
 * cinematic.ts — the home-run cinematic's own state machine. `session.ts` calls
 * `enterCinematicPhase` at each event boundary it already knows about (a swing
 * commits to a predicted homer, real contact lands, the ball visibly separates
 * from the bat, the flight resolves, the result hold ends) and `stepCinematic`
 * every real tick the cinematic is live. Presentation values (letterbox, zoom,
 * camera blend, time scale) EASE continuously across phase boundaries — nothing
 * pops — while `session.ts` decides WHEN a boundary is crossed from the real
 * game data (contact tick, ball velocity, flight completion). Pure and total:
 * identical `(state, tuning)` inputs always produce identical output.
 */

import type { CinematicPhase, CinematicState } from "./types.ts";
import type { HomeRunCinematicTuning } from "./cinematic-constants.ts";
import { clamp, clamp01 } from "./vec.ts";

export const newCinematic = (): CinematicState => ({
  camBlend: 0,
  elapsedTicks: 0,
  impactParticles: 0,
  letterbox: 0,
  phase: "none",
  phaseTicks: 0,
  timeScale: 1,
  zoom: 0,
});

/** Enter a new cinematic sub-phase. `phaseTicks` resets; every continuous
 * presentation value (letterbox/zoom/camBlend/timeScale) is carried over
 * unchanged so `stepCinematic` eases from wherever it already was. */
export const enterCinematicPhase = (state: CinematicState, phase: CinematicPhase): CinematicState => ({ ...state, phase, phaseTicks: 0 });

/** Linear step-toward-target over exactly `durationTicks` ticks — lands exactly
 * on `target`, never overshoots, stays within whatever range `current`/`target`
 * already are (both callers here only ever pass 0…1 values). */
const approach = (current: number, target: number, durationTicks: number): number => {
  const step = durationTicks > 0 ? 1 / durationTicks : 1;
  return target > current ? Math.min(target, current + step) : Math.max(target, current - step);
};

/** The time-scale TARGET for the current phase/phaseTicks — the schedule the
 * prompt describes: heavy slow motion through contact, moderate as the ball
 * separates, full speed for most of the flight. */
const timeScaleTarget = (phase: CinematicPhase, phaseTicks: number, tuning: HomeRunCinematicTuning): number => {
  const targets: Record<CinematicPhase, number> = {
    anticipation: tuning.contactSlowMotionScale,
    ballFollow: phaseTicks > tuning.contactSlowMotionDurationTicks ? 1 : tuning.postContactSlowMotionScale,
    celebration: 1,
    contact: tuning.contactSlowMotionScale,
    landing: 1,
    none: 1,
  };
  return targets[phase];
};

/** Bars are UP (max scrunch) only through anticipation + contact — ball-follow
 * begins retracting them immediately, per "after the ball clearly separates,
 * start retracting the letterbox bars." */
const LETTERBOX_UP: readonly CinematicPhase[] = ["anticipation", "contact"];
/** The cinematic director owns the shot through landing; celebration hands the
 * camera back — "return control only after the play has completed." */
const CAMERA_OWNED: readonly CinematicPhase[] = ["anticipation", "contact", "ballFollow", "landing"];

/** Advance the cinematic's continuous presentation state by one real tick.
 * A no-op (cheap, stable) while `phase === "none"`. */
export const stepCinematic = (state: CinematicState, tuning: HomeRunCinematicTuning): CinematicState => {
  if (state.phase === "none") {
    return state;
  }
  const phaseTicks = state.phaseTicks + 1;
  const elapsedTicks = state.elapsedTicks + 1;

  const letterboxTarget = LETTERBOX_UP.includes(state.phase) ? 1 : 0;
  const letterbox = approach(state.letterbox, letterboxTarget, letterboxTarget === 1 ? tuning.letterboxEntranceDurationTicks : tuning.letterboxExitDurationTicks);

  // Zoom eases out on the same beat as the letterbox bars.
  const zoom = approach(state.zoom, letterboxTarget, tuning.cinematicCameraBlendDurationTicks);

  const camBlendTarget = CAMERA_OWNED.includes(state.phase) ? 1 : 0;
  const camBlend = approach(state.camBlend, camBlendTarget, tuning.cinematicCameraBlendDurationTicks);

  const target = timeScaleTarget(state.phase, phaseTicks, tuning);
  const rampDuration = target > state.timeScale ? tuning.timeScaleRecoveryDurationTicks : tuning.cinematicCameraBlendDurationTicks;
  // Never below the configured minimum (the deepest slow-mo), never above full speed.
  const timeScale = clamp(approach(state.timeScale, target, rampDuration), tuning.contactSlowMotionScale, 1);

  const impactParticles =
    state.phase === "contact" ? Math.min(tuning.impactParticleMaxCount, phaseTicks * 2) : Math.max(0, state.impactParticles - 1);

  return { camBlend: clamp01(camBlend), elapsedTicks, impactParticles, letterbox: clamp01(letterbox), phase: state.phase, phaseTicks, timeScale, zoom: clamp01(zoom) };
};
