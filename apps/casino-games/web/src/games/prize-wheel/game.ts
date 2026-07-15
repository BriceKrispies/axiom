/*
 * game.ts — Prize Wheel controller. Destination mechanic: the wheel's segments
 * ARE the destination slots, their drawn arc widths are exactly the compiled
 * per-slot probabilities (so the picture never lies about the odds), and the
 * committed segment is resolved at release. The spin is an analytic easing
 * profile that ends with the pointer inside the committed segment — several
 * full rotations, deterministic variation from the trajectory stream, no
 * final-frame snap.
 */

import type { InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { DestinationSlot } from "../../chance-engine/probability/destination.ts";
import { destinationProbabilities } from "../../chance-engine/probability/destination.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { tickCue } from "../../presentation/audio/cues.ts";
import { easeOutCubic } from "../../presentation/stage/easing.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

/** One authored wheel segment. Arc width is NOT authored — it is compiled
 * from (targetWinRate, mass) so the drawing always matches the odds. */
export interface WheelSegment {
  readonly label: string;
  /** Reward tier granted, or null for a "spin again" (losing) segment. */
  readonly tierId: string | null;
  /** Relative mass within its winning/losing group. */
  readonly mass: number;
}

export interface WheelSpec {
  readonly segments: readonly WheelSegment[];
}

export interface WheelExtra {
  /** Ticks the launch has been charging (0 = idle). */
  readonly chargeTicks: number;
  /** Wheel angle at rest before the spin (radians). */
  readonly restAngle: number;
  /** The spin profile, fixed at commitment. */
  readonly spin: { readonly target: number; readonly ticks: number } | null;
}

export type WheelState = CasinoState<WheelExtra>;

export const POINTER_ANGLE = Math.PI / 2;
const CHARGE_FULL_TICKS = 80;

export const destinationSlotsOf = (spec: WheelSpec): readonly DestinationSlot[] =>
  spec.segments.map((segment, index) => ({ id: `${index}:${segment.label}`, mass: segment.mass, tierId: segment.tierId }));

/** Segment arc boundaries in radians, proportional to compiled probability. */
export const segmentArcs = (
  spec: WheelSpec,
  targetWinRate: number,
): readonly { readonly start: number; readonly end: number; readonly center: number }[] => {
  const probabilities = destinationProbabilities(destinationSlotsOf(spec), targetWinRate);
  let acc = 0;
  return probabilities.map((p) => {
    const start = acc;
    acc += p * Math.PI * 2;
    return { center: (start + acc) / 2, end: acc, start };
  });
};

export const chargeStrength = (chargeTicks: number): number => Math.min(1, chargeTicks / CHARGE_FULL_TICKS);

/** The full spin profile for a committed destination: 3–6 rotations by launch
 * strength, landing with the pointer inside the committed segment (≤ ±25% of
 * the arc off its center, from the trajectory stream). */
export const spinProfile = (
  spec: WheelSpec,
  session: SessionState,
  restAngle: number,
  strength: number,
): { readonly target: number; readonly ticks: number } => {
  const plan = session.committed;
  const arcs = segmentArcs(spec, session.config.targetWinRate);
  const index = plan !== null && plan.manifestation.kind === "destination" ? plan.manifestation.destinationIndex : 0;
  const arc = arcs[index] ?? { center: 0, end: 0, start: 0 };
  const seed = plan?.presentationSeed ?? session.seed;
  const jitter = (sample01(seed, "trajectory", session.round, 0) - 0.5) * (arc.end - arc.start) * 0.5;
  const rotations = 3 + Math.round(strength * 3) + (sample01(seed, "trajectory", session.round, 1) < 0.5 ? 0 : 1);
  const finalPointerAngle = POINTER_ANGLE - (arc.center + jitter);
  const base = ((finalPointerAngle - restAngle) % (Math.PI * 2) + Math.PI * 2) % (Math.PI * 2);
  return {
    target: restAngle + base + rotations * Math.PI * 2,
    ticks: speedTicks(150 + Math.round(strength * 80), session.config.presentationSpeed),
  };
};

/** The wheel's current angle (pure function of phase + spin profile). */
export const wheelAngle = (state: WheelState): number => {
  const { restAngle, spin } = state.extra;
  const session = state.session;
  if (spin === null) {
    return restAngle;
  }
  if (session.phase === "revealing") {
    return restAngle + (spin.target - restAngle) * easeOutCubic(phaseAge(session) / spin.ticks);
  }
  if (session.phase === "celebrating" || session.phase === "complete") {
    return spin.target;
  }
  return restAngle;
};

export const initialWheelExtra = (_session: SessionState): WheelExtra => ({ chargeTicks: 0, restAngle: 0, spin: null });

export const stepWheel = (
  runtime: GameRuntime<WheelSpec>,
  state: WheelState,
  input: InputFrame,
  _ctx: TickContext,
): WheelState => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;

  if (session.phase === "ready") {
    const holding = input.down.has("primary") || (input.pointer?.down ?? false);
    if (holding) {
      return { ...state, extra: { ...state.extra, chargeTicks: state.extra.chargeTicks + 1 } };
    }
    if (state.extra.chargeTicks > 0) {
      // Release → commit with the launch strength as context.
      return {
        ...state,
        pendingContext: { launchStrength: chargeStrength(state.extra.chargeTicks) },
        session: transition(session, "committing"),
      };
    }
    return state;
  }

  if (session.phase === "committing" && session.committed !== null && state.extra.spin === null) {
    const strength = chargeStrength(state.extra.chargeTicks);
    return { ...state, extra: { ...state.extra, spin: spinProfile(spec, session, state.extra.restAngle, strength) } };
  }

  if (session.phase === "revealing") {
    const spin = state.extra.spin;
    if (spin !== null && phaseAge(session) >= spin.ticks + speedTicks(26, session.config.presentationSpeed)) {
      return { ...state, extra: { ...state.extra, chargeTicks: 0, restAngle: spin.target }, session: transition(session, "celebrating") };
    }
    return state;
  }

  return state;
};

/** Divider-tick cues: one blip each time a segment boundary sweeps past the
 * pointer between two ticks. */
export const wheelCues = (spec: WheelSpec, prev: WheelState, next: WheelState): readonly ToneSpec[] => {
  if (next.session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const arcs = segmentArcs(spec, next.session.config.targetWinRate);
  const a = wheelAngle(prev);
  const b = wheelAngle(next);
  const seed = next.session.committed?.presentationSeed ?? next.session.seed;
  const crossings = arcs.filter((arc) => {
    const boundary = arc.start;
    // Pointer-relative boundary angle: crossed when (angle + boundary) passes POINTER_ANGLE modulo 2π.
    const rel = (x: number): number => Math.floor((x + boundary - POINTER_ANGLE) / (Math.PI * 2));
    return rel(b) > rel(a);
  }).length;
  return crossings > 0 ? tickCue(seed, Math.round(b * 100)) : [];
};
