/*
 * game.ts — Capsule Conveyor controller: a looping belt of capsules seen from
 * INSIDE the machine, resolved by the DESTINATION mechanic. Each capsule is a
 * slot (mass 1); the committed `plan.manifestation.destinationIndex` is the
 * capsule that must end up in the opening station on the right.
 *
 * The honest part: the player STOPS the belt (committing with the capsule
 * nearest the station as context), then the belt DECELERATES with an analytic
 * easeOutCubic profile whose travel distance is chosen — by adding whole loops
 * when needed — to glide the committed capsule to the station. Motion is
 * continuous in belt-phase space; there is no final-frame snap. All belt/capsule
 * kinematics are pure functions of (spec, session, extra).
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import type { DestinationSlot } from "../../chance-engine/probability/destination.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { thumpCue, tickCue } from "../../presentation/audio/cues.ts";
import type { MachineVolume } from "../../presentation/cameras/presets.ts";
import { machineInteriorCamera } from "../../presentation/cameras/presets.ts";
import { clamp01, easeOutCubic } from "../../presentation/stage/easing.ts";
import { v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

export interface CapsuleConveyorSpec {
  /** Number of capsules riding the belt. */
  readonly capsuleCount: number;
  /** Per-capsule tier id (null = an empty capsule), length `capsuleCount`. */
  readonly capsuleTiers: readonly (string | null)[];
}

export interface ConveyorExtra {
  /** Belt progress (in loops) captured at the STOP press; null until then. */
  readonly stopProgress: number | null;
}

export type ConveyorState = CasinoState<ConveyorExtra>;

// ── machine interior geometry ──────────────────────────────────────────────────

export const MACHINE_VOLUME: MachineVolume = { center: v3(0, 1.4, 0), size: v3(5.2, 3, 3) };
export const BELT_CENTER: EngineVec3 = v3(0, 1.4, 0);
export const BELT_RX = 1.95;
export const BELT_RY = 0.95;
export const CAPSULE_DIAMETER = 0.44;
/** Constant belt speed while running (loops per tick). */
export const BELT_SPEED = 0.008;
/** Minimum braking travel, in loops, before the committed capsule can land. */
export const MIN_BRAKE_LOOPS = 0.5;

export const conveyorCamera = (): ReturnType<typeof machineInteriorCamera> => machineInteriorCamera(MACHINE_VOLUME);

/** Destination slots for the mechanic: one per capsule, mass 1. */
export const slotsOf = (spec: CapsuleConveyorSpec): readonly DestinationSlot[] =>
  spec.capsuleTiers.map((tierId, index) => ({ id: `capsule${index}`, mass: 1, tierId }));

// ── belt phase math ──────────────────────────────────────────────────────────────

const frac = (x: number): number => x - Math.floor(x);

/** Capsule `i`'s loop phase in [0,1) at belt progress `s`. Station is at phase 0. */
export const capsulePhase = (i: number, count: number, s: number): number => frac(i / count + s);

const circularDistance = (phase: number): number => {
  const d = frac(phase);
  return Math.min(d, 1 - d);
};

/** The capsule whose phase is nearest the station (phase 0) at progress `s`. */
export const nearestCapsuleToStation = (count: number, s: number): number => {
  let best = 0;
  let bestDist = Number.POSITIVE_INFINITY;
  for (let i = 0; i < count; i += 1) {
    const dist = circularDistance(capsulePhase(i, count, s));
    if (dist < bestDist) {
      best = i;
      bestDist = dist;
    }
  }
  return best;
};

/** The committed destination capsule index. */
export const destinationIndexOf = (session: SessionState): number => {
  const m = session.committed?.manifestation;
  return m !== undefined && m.kind === "destination" ? m.destinationIndex : 0;
};

/** The braking travel (in loops) from `sStop` that lands capsule `j` at the
 * station, never less than `MIN_BRAKE_LOOPS` (whole loops added as needed). */
export const brakeDistance = (count: number, j: number, sStop: number): number => {
  const base = -j / count; // s values with capsule j at station are base + integer.
  const m = Math.ceil(sStop - base + MIN_BRAKE_LOOPS);
  return base + m - sStop;
};

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export interface ConveyorTimeline {
  readonly brakeEnd: number;
  readonly armEnd: number;
  readonly openEnd: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const conveyorTimeline = (presentationSpeed: number, reducedMotion: boolean): ConveyorTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const brakeEnd = t(150);
  const armEnd = brakeEnd + t(22);
  const openEnd = armEnd + t(26);
  const riseEnd = openEnd + t(34);
  return { armEnd, brakeEnd, openEnd, riseEnd, total: riseEnd + t(10) };
};

// ── belt progress + capsule world positions (all pure) ───────────────────────────

/** Belt progress (loops) in the current phase. Running: linear in tick; reveal:
 * eased deceleration from the captured stop progress to the landing progress. */
export const beltProgress = (spec: CapsuleConveyorSpec, state: ConveyorState, reducedMotion: boolean): number => {
  const session = state.session;
  const running = session.phase === "intro" || session.phase === "ready" || session.phase === "interacting";
  if (running) {
    return session.tick * BELT_SPEED;
  }
  const sStop = state.extra.stopProgress ?? session.tick * BELT_SPEED;
  if (session.phase === "committing") {
    return sStop;
  }
  const timeline = conveyorTimeline(session.config.presentationSpeed, reducedMotion);
  const distance = brakeDistance(spec.capsuleCount, destinationIndexOf(session), sStop);
  const settled = session.phase === "celebrating" || session.phase === "complete";
  const age = settled ? timeline.brakeEnd : Math.min(phaseAge(session), timeline.brakeEnd);
  return sStop + distance * easeOutCubic(clamp01(age / timeline.brakeEnd));
};

/** World position of capsule `i` on the elliptical belt loop. */
export const capsuleWorldPosition = (spec: CapsuleConveyorSpec, state: ConveyorState, reducedMotion: boolean, i: number): EngineVec3 => {
  const s = beltProgress(spec, state, reducedMotion);
  const theta = capsulePhase(i, spec.capsuleCount, s) * Math.PI * 2;
  const lift = liftHeightOf(spec, state, reducedMotion, i);
  return v3(BELT_CENTER.x + Math.cos(theta) * BELT_RX + lift.x, BELT_CENTER.y + Math.sin(theta) * BELT_RY + lift.y, BELT_CENTER.z);
};

/** The station arm lift applied to the destination capsule during the reveal. */
const liftHeightOf = (spec: CapsuleConveyorSpec, state: ConveyorState, reducedMotion: boolean, i: number): { readonly x: number; readonly y: number } => {
  const session = state.session;
  if (session.committed === null || i !== destinationIndexOf(session)) {
    return { x: 0, y: 0 };
  }
  const revealing = session.phase === "revealing" || session.phase === "celebrating" || session.phase === "complete";
  if (!revealing) {
    return { x: 0, y: 0 };
  }
  const timeline = conveyorTimeline(session.config.presentationSpeed, reducedMotion);
  const age = session.phase === "revealing" ? phaseAge(session) : timeline.total;
  const liftT = clamp01((age - timeline.brakeEnd) / (timeline.armEnd - timeline.brakeEnd));
  return { x: 0.18 * easeOutCubic(liftT), y: 0.34 * easeOutCubic(liftT) };
};

/** Which capsule is at (opening in) the station right now. */
export const openingCapsuleIndex = (spec: CapsuleConveyorSpec, state: ConveyorState, reducedMotion: boolean): number =>
  nearestCapsuleToStation(spec.capsuleCount, beltProgress(spec, state, reducedMotion));

// ── controller ─────────────────────────────────────────────────────────────────

export const initialConveyorExtra = (_session: SessionState): ConveyorExtra => ({ stopProgress: null });

export const stepCapsuleConveyor = (
  runtime: GameRuntime<CapsuleConveyorSpec>,
  state: ConveyorState,
  input: InputFrame,
  _ctx: TickContext,
): ConveyorState => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;

  if (session.phase === "ready") {
    return { ...state, session: transition(session, "interacting") };
  }

  if (session.phase === "interacting") {
    const pressedStop = input.pressed.has("primary") || (input.pointer?.down ?? false);
    if (pressedStop) {
      const sStop = session.tick * BELT_SPEED;
      const stopPosition = nearestCapsuleToStation(spec.capsuleCount, sStop);
      return {
        ...state,
        extra: { stopProgress: sStop },
        pendingContext: { stopPosition },
        session: transition(session, "committing"),
      };
    }
    return state;
  }

  if (session.phase === "revealing") {
    const timeline = conveyorTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Mechanism cues: a belt tick as each capsule sweeps the station while braking,
 * and a thunk when the arm seats the capsule in the station. */
export const conveyorCues = (
  runtime: GameRuntime<CapsuleConveyorSpec>,
  prev: ConveyorState,
  next: ConveyorState,
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const spec = runtime.config.gameSpecific;
  const reduced = runtime.settings.reducedMotion;
  const timeline = conveyorTimeline(session.config.presentationSpeed, reduced);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const before = nearestCapsuleToStation(spec.capsuleCount, beltProgress(spec, prev, reduced));
  const after = openingCapsuleIndex(spec, next, reduced);
  const swept = before !== after && phaseAge(session) <= timeline.brakeEnd;
  const seated = phaseAge(prev.session) < timeline.brakeEnd && phaseAge(session) >= timeline.brakeEnd;
  return [
    ...(swept ? tickCue(seed, after) : []),
    ...(seated ? thumpCue(seed, 1) : []),
  ];
};
