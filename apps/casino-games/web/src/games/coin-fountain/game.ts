/*
 * game.ts — Coin Fountain controller. Single-reveal mechanic: one gameplay
 * draw commits the round the instant the token leaves the hand; the aim point
 * and charged launch strength ride along ONLY as presentation context — they
 * shape the token's arc, never the odds. The reticle is clamped to the basin
 * so every toss lands in water, and the token's flight is a continuous
 * analytic arc from the ledge to that aim point (no final-frame snap).
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { shimmerCue, thumpCue } from "../../presentation/audio/cues.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH } from "../../presentation/cameras/picking.ts";
import { clamp01, lerp } from "../../presentation/stage/easing.ts";
import { v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

/** Tunable fountain geometry + charge feel. */
export interface FountainSpec {
  /** Radius of the basin water the token must land in. */
  readonly basinRadius: number;
  /** Extra arc apex height per unit of launch strength (world units). */
  readonly maxArcHeight: number;
}

export const DEFAULT_FOUNTAIN_SPEC: FountainSpec = { basinRadius: 2.1, maxArcHeight: 2.6 };

// ── fountain geography ─────────────────────────────────────────────────────────

export const WATER_Y = 0.62;
/** The ledge the token is tossed from (front rim, toward the camera). */
export const LEDGE: EngineVec3 = { x: 0, y: 1.35, z: 2.7 };
/** Reticle stays this fraction inside the basin edge (never on the rim). */
export const AIM_MARGIN = 0.9;

export interface AimPoint {
  readonly x: number;
  readonly z: number;
}

const CHARGE_FULL_TICKS = 70;

export const chargeStrength = (chargeTicks: number): number => Math.min(1, chargeTicks / CHARGE_FULL_TICKS);

/** Clamp an aim point into the basin water (the reticle can't leave the water). */
export const clampAimToBasin = (spec: FountainSpec, x: number, z: number): AimPoint => {
  const limit = spec.basinRadius * AIM_MARGIN;
  const dist = Math.hypot(x, z);
  if (dist <= limit) {
    return { x, z };
  }
  const s = limit / dist;
  return { x: x * s, z: z * s };
};

/** The committed aim point, recovered from the sealed commitment context. */
export const committedAim = (spec: FountainSpec, session: SessionState): AimPoint => {
  const aim = session.inputContext?.aim;
  return aim === undefined
    ? { x: 0, z: 0 }
    : clampAimToBasin(spec, aim.x * spec.basinRadius, aim.y * spec.basinRadius);
};

/** The charged launch strength, recovered from the commitment context. */
export const committedStrength = (session: SessionState): number =>
  clamp01(session.inputContext?.launchStrength ?? 0);

// ── the reveal timeline ─────────────────────────────────────────────────────────

export interface TossTimeline {
  readonly flightEnd: number;
  readonly splashEnd: number;
  readonly columnEnd: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const tossTimeline = (presentationSpeed: number, reducedMotion: boolean): TossTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const flightEnd = t(42);
  const splashEnd = flightEnd + t(22);
  const columnEnd = splashEnd + t(24);
  const riseEnd = columnEnd + t(40);
  return { columnEnd, flightEnd, riseEnd, splashEnd, total: riseEnd + t(12) };
};

/**
 * The token's position at reveal age `age`: a continuous ballistic arc from
 * the ledge to the aim point on the water, its apex raised by the charged
 * strength. After it lands it stays at the aim point (the splash takes over).
 */
export const tokenAt = (spec: FountainSpec, aim: AimPoint, strength: number, tl: TossTimeline, age: number): EngineVec3 => {
  const u = clamp01(age / tl.flightEnd);
  const water: EngineVec3 = { x: aim.x, y: WATER_Y, z: aim.z };
  const apex = 1.4 + strength * spec.maxArcHeight;
  const arc = 4 * apex * u * (1 - u);
  return v3(lerp(LEDGE.x, water.x, u), lerp(LEDGE.y, water.y, u) + arc, lerp(LEDGE.z, water.z, u));
};

// ── controller ──────────────────────────────────────────────────────────────────

export interface FountainExtra {
  /** Reticle position on the basin (world x/z, clamped to water). */
  readonly aimX: number;
  readonly aimZ: number;
  /** Ticks the toss has been charging (0 = idle). */
  readonly chargeTicks: number;
}

export const initialFountainExtra = (_session: SessionState): FountainExtra => ({ aimX: 0, aimZ: 0, chargeTicks: 0 });

export type FountainState = CasinoState<FountainExtra>;

const KEY_AIM_STEP = 0.07;

export const stepFountain = (
  runtime: GameRuntime<FountainSpec>,
  state: FountainState,
  input: InputFrame,
  _ctx: TickContext,
): FountainState => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;

  if (session.phase === "ready") {
    const pointer = input.pointer;
    const fromPointer =
      pointer === undefined
        ? null
        : {
            x: ((pointer.pos.x / CANVAS_WIDTH) * 2 - 1) * spec.basinRadius * 1.2,
            z: ((pointer.pos.y / CANVAS_HEIGHT) * 2 - 1) * spec.basinRadius * 1.1,
          };
    const keyX = (input.down.has("right") ? 1 : 0) - (input.down.has("left") ? 1 : 0);
    const keyZ = (input.down.has("down") ? 1 : 0) - (input.down.has("up") ? 1 : 0);
    const raw = fromPointer ?? { x: state.extra.aimX + keyX * KEY_AIM_STEP, z: state.extra.aimZ + keyZ * KEY_AIM_STEP };
    const aim = clampAimToBasin(spec, raw.x, raw.z);
    const holding = input.down.has("primary") || (pointer?.down ?? false);
    if (holding) {
      return { ...state, extra: { aimX: aim.x, aimZ: aim.z, chargeTicks: state.extra.chargeTicks + 1 } };
    }
    if (state.extra.chargeTicks > 0) {
      return {
        ...state,
        extra: { aimX: aim.x, aimZ: aim.z, chargeTicks: state.extra.chargeTicks },
        pendingContext: {
          aim: { x: aim.x / spec.basinRadius, y: aim.z / spec.basinRadius },
          launchStrength: chargeStrength(state.extra.chargeTicks),
        },
        session: transition(session, "committing"),
      };
    }
    return { ...state, extra: { ...state.extra, aimX: aim.x, aimZ: aim.z } };
  }

  if (session.phase === "revealing") {
    const tl = tossTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= tl.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Reveal-mechanism cues: the splash plunk on landing and the shimmer as the
 * reward rises on its spout (win/loss fanfare is played centrally). */
export const fountainCues = (
  reducedMotion: boolean,
  prev: FountainState,
  next: FountainState,
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const tl = tossTimeline(session.config.presentationSpeed, reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...(crossed(tl.flightEnd) ? thumpCue(seed, 21) : []),
    ...(crossed(tl.columnEnd) ? shimmerCue(seed, 22) : []),
  ];
};
