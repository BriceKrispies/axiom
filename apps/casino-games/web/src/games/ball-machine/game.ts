/*
 * game.ts — Ball Machine controller: the single-reveal mechanic behind a
 * gumball-style dispenser seen from INSIDE the machine. The player presses the
 * big button (or primary); the WIN/TIER is committed by the harness before any
 * ball moves; the reveal then agitates the chamber (analytic, bounded swirls
 * from the trajectory stream keyed off the committed presentation seed), rolls
 * ONE presentation-only ball down a smooth multi-segment chute path — never a
 * final-frame snap — and pops it open at the pickup door to show the committed
 * result.
 *
 * Every function here is pure: ball poses are analytic functions of
 * (spec, session, index), so the scene and the tests read the exact same
 * positions.
 */

import type { Camera3D, EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sample01, sampleInt } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { shimmerCue, thumpCue, tickCue } from "../../presentation/audio/cues.ts";
import type { MachineVolume } from "../../presentation/cameras/presets.ts";
import { machineInteriorCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { pickAt } from "../../presentation/cameras/picking.ts";
import { clamp01, smoothstep } from "../../presentation/stage/easing.ts";
import { addV3, lerpV3, v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

export interface BallMachineSpec {
  /** Number of capsule balls resting in the bowl. */
  readonly ballCount: number;
  /** Base agitation length in ticks (before presentation-speed scaling). */
  readonly agitationTicks: number;
}

export interface BallExtra {
  /** Previous tick's pointer-down state (press-edge detection on the button). */
  readonly pointerWasDown: boolean;
}

export type BallState = CasinoState<BallExtra>;

// ── machine interior geometry ──────────────────────────────────────────────────

export const MACHINE_VOLUME: MachineVolume = { center: v3(0, 1.55, 0), size: v3(4.8, 3.1, 3.4) };
export const BOWL_CENTER: EngineVec3 = v3(-0.2, 0.62, 0);
export const GLOBE_CENTER: EngineVec3 = v3(-0.2, 1.38, 0);
export const GLOBE_RADIUS = 1.36;
export const BALL_DIAMETER = 0.32;
export const DOOR_AT: EngineVec3 = v3(1.55, 0.34, 0.95);
export const BUTTON_AT: EngineVec3 = v3(1.78, 1.02, 0.5);

export const ballCamera = (): Camera3D => machineInteriorCamera(MACHINE_VOLUME);

export const BUTTON_TARGET: PickTarget = { at: BUTTON_AT, index: 0, radiusPx: 72 };

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export interface BallTimeline {
  readonly agitationEnd: number;
  readonly chuteEnd: number;
  readonly openEnd: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const ballTimeline = (spec: BallMachineSpec, presentationSpeed: number, reducedMotion: boolean): BallTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const agitationEnd = t(spec.agitationTicks);
  const chuteEnd = agitationEnd + t(64);
  const openEnd = chuteEnd + t(26);
  const riseEnd = openEnd + t(36);
  return { agitationEnd, chuteEnd, openEnd, riseEnd, total: riseEnd + t(10) };
};

// ── ball poses (all analytic) ──────────────────────────────────────────────────

/** Resting slot of ball `index`: rings of seven packed in the bowl. */
export const restPosition = (index: number, _count: number): EngineVec3 => {
  const ring = Math.floor(index / 7);
  const slot = index % 7;
  const radius = 0.34 + ring * 0.5;
  const angle = (slot / 7) * Math.PI * 2 + ring * 0.45;
  return v3(
    BOWL_CENTER.x + Math.cos(angle) * radius,
    BOWL_CENTER.y + 0.2 + ring * 0.06,
    BOWL_CENTER.z + Math.sin(angle) * radius,
  );
};

/** Tiny settle jitter while idle — AMBIENT stream only, so it can hint nothing. */
const idleJitter = (seed: number, tick: number, index: number): EngineVec3 => {
  const phase = sample01(seed, "ambient", index, 0) * Math.PI * 2;
  const rate = 0.05 + sample01(seed, "ambient", index, 1) * 0.04;
  return v3(
    Math.sin(tick * rate + phase) * 0.012,
    Math.abs(Math.sin(tick * rate * 1.3 + phase)) * 0.014,
    Math.cos(tick * rate * 0.8 + phase) * 0.012,
  );
};

/** Agitation swirl for ball `index`: bounded, ramps in, damps back to rest by
 * `agitationEnd` — trajectory stream keyed off the committed presentation seed. */
const agitatedPosition = (
  index: number,
  presentationSeed: number,
  age: number,
  agitationEnd: number,
  reducedMotion: boolean,
): EngineVec3 => {
  const base = restPosition(index, 0);
  const p = (k: number): number => sample01(presentationSeed, "trajectory", index, k);
  const ramp = clamp01(age / 20);
  const damp = clamp01((agitationEnd - age) / 30);
  const soft = reducedMotion ? 0.55 : 1;
  const amp = ramp * damp * soft;
  const f1 = 0.1 + p(0) * 0.08;
  const f2 = 0.12 + p(1) * 0.07;
  const ph1 = p(2) * Math.PI * 2;
  const ph2 = p(3) * Math.PI * 2;
  return v3(
    base.x + Math.sin(age * f1 + ph1) * 0.3 * amp,
    base.y + Math.abs(Math.sin(age * f2 + ph2)) * (0.3 + p(4) * 0.28) * amp,
    base.z + Math.cos(age * f1 * 0.9 + ph2) * 0.26 * amp,
  );
};

/** WHICH ball rolls out — presentation only; the win/tier is `plan.win`/`plan.tierId`. */
export const dispensedIndexOf = (count: number, presentationSeed: number): number =>
  sampleInt(count, presentationSeed, "trajectory", 101);

/** The dispensed ball's chute waypoints: bowl funnel → channel → pickup door. */
const chuteWaypoints = (from: EngineVec3): readonly EngineVec3[] => [
  from,
  v3(BOWL_CENTER.x, BOWL_CENTER.y - 0.06, 0),
  v3(0.55, 0.34, 0.3),
  v3(1.15, 0.3, 0.68),
  DOOR_AT,
];

/** A point along a polyline at arc-length fraction `t` — continuous by construction. */
export const pathPoint = (points: readonly EngineVec3[], t: number): EngineVec3 => {
  const lengths: number[] = [];
  let total = 0;
  for (let i = 0; i + 1 < points.length; i += 1) {
    const a = points[i] as EngineVec3;
    const b = points[i + 1] as EngineVec3;
    const len = Math.hypot(b.x - a.x, b.y - a.y, b.z - a.z);
    lengths.push(len);
    total += len;
  }
  let remaining = clamp01(t) * total;
  for (let i = 0; i < lengths.length; i += 1) {
    const len = lengths[i] as number;
    if (remaining <= len || i === lengths.length - 1) {
      return lerpV3(points[i] as EngineVec3, points[i + 1] as EngineVec3, len === 0 ? 0 : clamp01(remaining / len));
    }
    remaining -= len;
  }
  return points[points.length - 1] as EngineVec3;
};

/** World position of ball `index` in any phase — the scene and the tests share it. */
export const ballWorldPosition = (
  spec: BallMachineSpec,
  session: SessionState,
  reducedMotion: boolean,
  index: number,
): EngineVec3 => {
  const rest = restPosition(index, spec.ballCount);
  const plan = session.committed;
  const settled = session.phase === "celebrating" || session.phase === "complete";
  const revealAge = session.phase === "revealing" ? phaseAge(session) : settled ? Number.MAX_SAFE_INTEGER : -1;
  if (revealAge < 0 || plan === null) {
    return addV3(rest, idleJitter(session.seed, session.tick, index));
  }
  const timeline = ballTimeline(spec, session.config.presentationSpeed, reducedMotion);
  const age = Math.min(revealAge, timeline.total);
  const dispensed = dispensedIndexOf(spec.ballCount, plan.presentationSeed);
  if (index !== dispensed || age < timeline.agitationEnd) {
    return agitatedPosition(index, plan.presentationSeed, age, timeline.agitationEnd, reducedMotion);
  }
  const u = clamp01((age - timeline.agitationEnd) / (timeline.chuteEnd - timeline.agitationEnd));
  return pathPoint(chuteWaypoints(rest), smoothstep(u));
};

// ── controller ─────────────────────────────────────────────────────────────────

export const initialBallExtra = (_session: SessionState): BallExtra => ({ pointerWasDown: false });

export const stepBallMachine = (
  runtime: GameRuntime<BallMachineSpec>,
  state: BallState,
  input: InputFrame,
  _ctx: TickContext,
): BallState => {
  const session = state.session;

  if (session.phase === "ready") {
    const pointerDown = input.pointer?.down ?? false;
    const overButton = pickAt(ballCamera(), [BUTTON_TARGET], input.pointer) === 0;
    const clicked = pointerDown && !state.extra.pointerWasDown && overButton;
    const next: BallState = { ...state, extra: { pointerWasDown: pointerDown } };
    if (input.pressed.has("primary") || clicked) {
      return { ...next, pendingContext: {}, session: transition(session, "committing") };
    }
    return next;
  }

  if (session.phase === "revealing") {
    const spec = runtime.config.gameSpecific;
    const timeline = ballTimeline(spec, session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Mechanism cues: crank ratchet ticks during agitation, the chute thunk when
 * the ball lands at the door, and the pop when the capsule opens. */
export const ballCues = (
  runtime: GameRuntime<BallMachineSpec>,
  prev: BallState,
  next: BallState,
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const timeline = ballTimeline(runtime.config.gameSpecific, session.config.presentationSpeed, runtime.settings.reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  const ratchet = 9;
  const ratcheted = after <= timeline.agitationEnd && Math.floor(after / ratchet) > Math.floor(before / ratchet);
  return [
    ...(ratcheted ? tickCue(seed, Math.floor(after / ratchet)) : []),
    ...(crossed(timeline.chuteEnd) ? thumpCue(seed, 1) : []),
    ...(crossed(timeline.openEnd) ? shimmerCue(seed, 2) : []),
  ];
};
