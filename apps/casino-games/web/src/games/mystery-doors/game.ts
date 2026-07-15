/*
 * game.ts — the Mystery Doors controller: the mount spec's mechanic, per-tick
 * step, and the door-opening timeline. A short row of freestanding doors on
 * the showcase stage; the choice-population adapter preassigns what waits
 * behind each door before the player can possibly choose; the reveal follows a
 * classic cadence — the knob turns, the door CRACKS a few degrees while a
 * colored light spills through the gap, a held pause, then the door SWINGS
 * wide on its hinge to show the reward vignette (or a friendly empty room).
 *
 * Idle rattle/breathing draws exclusively from the AMBIENT stream keyed by
 * tick window and door slot — never from the population and never from the
 * presentation seed — so no wobble can hint at contents. The focused test
 * pins the crack-before-swing ordering and the ambient-only rattle.
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sample01, sampleInt } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { thumpCue, tickCue } from "../../presentation/audio/cues.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { clamp01, easeOutBack, easeOutCubic, smoothstep } from "../../presentation/stage/easing.ts";
import { v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";
import { initialChoice, stepChoice } from "../choice-input.ts";
import type { ChoiceCore } from "../choice-input.ts";

export interface DoorsSpec {
  /** Idle breathe/rattle liveliness in [0, 1]. */
  readonly breatheLiveliness: number;
}

export interface DoorsExtra {
  readonly choice: ChoiceCore;
}

export type DoorsState = CasinoState<DoorsExtra>;

const DOOR_SPACING = 2.6;
export const DOOR_MAX_SWING = 1.95;
export const DOOR_CRACK = 0.16;

/** Door `index` world position — a single row centered on the stage. */
export const doorPosition = (index: number, count: number): EngineVec3 =>
  v3((index - (count - 1) / 2) * DOOR_SPACING, 0, 0);

export const doorsCamera = (count: number): ReturnType<typeof showcaseCamera> =>
  showcaseCamera(v3(0, 1.1, 0), 5.4 + count * 0.6, 1.4, 0.92);

export const doorTargets = (count: number): readonly PickTarget[] =>
  Array.from({ length: count }, (_, index) => ({
    at: v3(doorPosition(index, count).x, 1.2, 0),
    index,
    radiusPx: 92,
  }));

// ── the opening timeline (ticks from entering "revealing", speed-scaled) ────────

export interface DoorTimeline {
  readonly knobEnd: number;
  readonly crackEnd: number;
  readonly pauseEnd: number;
  readonly swingEnd: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const doorTimeline = (presentationSpeed: number, reducedMotion: boolean): DoorTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const knobEnd = t(12);
  const crackEnd = knobEnd + t(16);
  const pauseEnd = crackEnd + t(14);
  const swingEnd = pauseEnd + t(26);
  const riseEnd = swingEnd + t(26);
  return { crackEnd, knobEnd, pauseEnd, riseEnd, swingEnd, total: riseEnd + t(8) };
};

/** Reveal-phase age: −1 before any reveal exists, the phase age while
 * revealing, and the timeline total once celebrating/complete. */
export const revealAgeOf = (session: SessionState, timelineTotal: number): number => {
  if (session.phase === "revealing") {
    return phaseAge(session);
  }
  return session.phase === "celebrating" || session.phase === "complete" ? timelineTotal : -1;
};

// ── the door-opening pose (a pure function of reveal age) ───────────────────────

export interface DoorOpenPose {
  /** Knob roll in radians. */
  readonly knob: number;
  /** Hinge swing angle in radians (crack a few degrees, then swing wide). */
  readonly swing: number;
  /** Colored light-spill strength in [0, 1] (begins exactly at the crack). */
  readonly spill: number;
}

/**
 * The selected door's opening pose at `revealAge` ticks. The swing rises to
 * DOOR_CRACK during the crack window and only continues to DOOR_MAX_SWING
 * during the swing window; the light spill turns on with the crack, never
 * before it. A pure function of (revealAge, timeline).
 */
export const doorOpenPose = (revealAge: number, timeline: DoorTimeline): DoorOpenPose => {
  if (revealAge < 0) {
    return { knob: 0, spill: 0, swing: 0 };
  }
  const knobT = clamp01(revealAge / timeline.knobEnd);
  const crackT = clamp01((revealAge - timeline.knobEnd) / (timeline.crackEnd - timeline.knobEnd));
  const swingT = clamp01((revealAge - timeline.pauseEnd) / (timeline.swingEnd - timeline.pauseEnd));
  const cracked = DOOR_CRACK * smoothstep(crackT);
  const swung = cracked + (DOOR_MAX_SWING - DOOR_CRACK) * easeOutBack(swingT);
  return {
    knob: 0.7 * easeOutCubic(knobT),
    spill: smoothstep(crackT),
    swing: swingT > 0 ? swung : cracked,
  };
};

// ── idle breathe / rattle (AMBIENT stream only) ─────────────────────────────────

export interface DoorDance {
  /** Small in-place body sway in radians. */
  readonly sway: number;
  /** Door-panel rattle in radians (a hint of movement in the frame). */
  readonly rattle: number;
}

/**
 * Idle dance pose for door `index` at `tick`. Time is cut into windows; each
 * window elects one rattler that jiggles briefly, over a gentle shared sway.
 * EVERY draw is from the AMBIENT stream keyed by (window, slot) — it never
 * reads the presentation seed, so the same ambient seed yields the same pose
 * regardless of the committed outcome. The focused test pins this.
 */
export const doorDance = (index: number, count: number, tick: number, seed: number, liveliness: number): DoorDance => {
  if (liveliness <= 0) {
    return { rattle: 0, sway: 0 };
  }
  const window = Math.floor(tick / 108);
  const rattler = sampleInt(count, seed, "ambient", window, 0);
  const phase = sample01(seed, "ambient", window, 10 + index) * Math.PI * 2;
  const local = (tick % 108) / 108;
  const envelope = Math.sin(Math.PI * local);
  const isRattling = index === rattler;
  return {
    rattle: isRattling ? Math.sin(local * Math.PI * 8 + phase) * 0.05 * liveliness * envelope : 0,
    sway: Math.sin(tick * 0.028 + phase) * 0.014 * liveliness,
  };
};

// ── controller ─────────────────────────────────────────────────────────────────

export const initialDoorsExtra = (_session: SessionState): DoorsExtra => ({ choice: initialChoice(0) });

/** Per-tick controller. Selection commits; the reveal advances on the door
 * timeline and hands off to "celebrating" when it completes. */
export const stepDoors = (
  runtime: GameRuntime<DoorsSpec>,
  state: DoorsState,
  input: InputFrame,
  _ctx: TickContext,
): DoorsState => {
  const session = state.session;
  const count = session.config.choiceCount ?? 3;

  if (session.phase === "ready") {
    const result = stepChoice(state.extra.choice, input, doorsCamera(count), doorTargets(count), count);
    if (result.selectedNow !== null) {
      return {
        ...state,
        extra: { choice: result.core },
        pendingContext: { selectedIndex: result.selectedNow },
        session: transition(session, "committing"),
      };
    }
    return { ...state, extra: { choice: result.core } };
  }

  if (session.phase === "revealing") {
    const timeline = doorTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Reveal-mechanism cues: a knob tick, a thump as the door swings wide (the
 * win/loss fanfare is played centrally by the harness). */
export const doorsCues = (prev: DoorsState, next: DoorsState, reducedMotion: boolean): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const timeline = doorTimeline(session.config.presentationSpeed, reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...(crossed(timeline.knobEnd) ? tickCue(seed, 1) : []),
    ...(crossed(timeline.pauseEnd) ? thumpCue(seed, 2) : []),
  ];
};
