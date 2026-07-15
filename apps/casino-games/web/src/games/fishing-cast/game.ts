/*
 * game.ts — Fishing Cast controller. Single-reveal mechanic: one Bernoulli
 * gameplay draw decides the round, the tier stream picks the conditional
 * reward — BEFORE any of the water theater plays. The player aims a reticle
 * over the pond and casts; the region under the reticle travels with the
 * commitment as CONTEXT ONLY (it selects which reward family manifests at
 * the dock — fish, treasure, or capsule), never the outcome itself. The
 * bobber's flight is an analytic ballistic arc from the rod tip to the aim
 * point: continuous every tick, no final-frame snap.
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sampleInt } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { shimmerCue, thumpCue, tickCue } from "../../presentation/audio/cues.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH } from "../../presentation/cameras/picking.ts";
import { clamp01, lerp, pulse } from "../../presentation/stage/easing.ts";
import { v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

/** One visual fishing region: a labelled circle of pond marked by ring buoys. */
export interface FishingRegion {
  readonly label: string;
  readonly x: number;
  readonly z: number;
  readonly radius: number;
}

export interface FishingSpec {
  readonly regions: readonly FishingRegion[];
}

/** The default pond: three storied spots, none luckier than another. */
export const DEFAULT_FISHING_REGIONS: readonly FishingRegion[] = [
  { label: "shallows", radius: 0.95, x: -1.5, z: 0.7 },
  { label: "deep pool", radius: 1.05, x: 0.95, z: -1.15 },
  { label: "reed bed", radius: 0.8, x: 1.7, z: 1.0 },
];

// ── pond geography (shared by controller, view, and tests) ─────────────────────

export const POND_RADIUS = 3;
/** The reticle (and therefore every cast) is clamped inside the water. */
export const AIM_LIMIT = POND_RADIUS * 0.92;
export const WATER_Y = 0.06;
export const ROD_TIP: EngineVec3 = { x: 0.55, y: 1.55, z: 2.85 };
/** Where the reel-in ends and the catch surfaces, just off the dock. */
export const CATCH_POINT: EngineVec3 = { x: 0, y: WATER_Y, z: 2.3 };

export interface AimPoint {
  readonly x: number;
  readonly z: number;
}

/** Clamp an aim point into the fishable disc (the reticle cannot leave water). */
export const clampAimToPond = (x: number, z: number): AimPoint => {
  const dist = Math.hypot(x, z);
  if (dist <= AIM_LIMIT) {
    return { x, z };
  }
  const s = AIM_LIMIT / dist;
  return { x: x * s, z: z * s };
};

/** The region index under an aim point: the first region containing it, or —
 * when the reticle floats between rings — the nearest region center. */
export const regionIndexAt = (spec: FishingSpec, x: number, z: number): number => {
  const inside = spec.regions.findIndex((r) => Math.hypot(x - r.x, z - r.z) <= r.radius);
  if (inside >= 0) {
    return inside;
  }
  let best = 0;
  let bestDist = Number.POSITIVE_INFINITY;
  spec.regions.forEach((r, i) => {
    const d = Math.hypot(x - r.x, z - r.z);
    if (d < bestDist) {
      best = i;
      bestDist = d;
    }
  });
  return best;
};

/** The reward FAMILY a region manifests. Presentation only — never the odds. */
export type CatchFamily = "fish" | "treasure" | "capsule";

const FAMILIES: readonly CatchFamily[] = ["fish", "treasure", "capsule"];

export const familyOfRegion = (regionIndex: number): CatchFamily =>
  FAMILIES[((regionIndex % FAMILIES.length) + FAMILIES.length) % FAMILIES.length] as CatchFamily;

/** The three warm loss catches: an old leaf, a rubber duck, a waving crab. */
export type LossCatch = "leaf" | "duck" | "crab";

const LOSS_CATCHES: readonly LossCatch[] = ["leaf", "duck", "crab"];

export const lossCatchOf = (presentationSeed: number): LossCatch =>
  LOSS_CATCHES[sampleInt(LOSS_CATCHES.length, presentationSeed, "trajectory", 7)] as LossCatch;

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export interface CastTimeline {
  readonly flickEnd: number;
  readonly flightEnd: number;
  readonly splashEnd: number;
  readonly dipStart: number;
  readonly dipEnd: number;
  readonly reelEnd: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const castTimeline = (presentationSpeed: number, reducedMotion: boolean): CastTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const flickEnd = t(10);
  const flightEnd = flickEnd + t(40);
  const splashEnd = flightEnd + t(20);
  const dipStart = splashEnd + t(52);
  const dipEnd = dipStart + t(10);
  const reelEnd = dipEnd + t(38);
  const riseEnd = reelEnd + t(32);
  return { dipEnd, dipStart, flickEnd, flightEnd, reelEnd, riseEnd, splashEnd, total: riseEnd + t(10) };
};

/** The committed aim point, recovered from the sealed commitment context. */
export const committedAim = (session: SessionState): AimPoint => {
  const aim = session.inputContext?.aim;
  return aim === undefined ? { x: 0, z: 0 } : clampAimToPond(aim.x * POND_RADIUS, aim.y * POND_RADIUS);
};

/**
 * The bobber's position at reveal age `age` — one continuous analytic path:
 * rod tip → ballistic arc to the aim → float with a gentle bobble → the sharp
 * dip → reel-in toward the dock. Every segment starts where the previous one
 * ended, so the per-tick delta is always small (the test pins the bound).
 */
export const bobberAt = (aim: AimPoint, tl: CastTimeline, age: number): EngineVec3 => {
  const water: EngineVec3 = { x: aim.x, y: WATER_Y, z: aim.z };
  if (age <= tl.flickEnd) {
    return ROD_TIP;
  }
  if (age <= tl.flightEnd) {
    const u = clamp01((age - tl.flickEnd) / (tl.flightEnd - tl.flickEnd));
    const arc = 4 * 1.15 * u * (1 - u);
    return v3(lerp(ROD_TIP.x, water.x, u), lerp(ROD_TIP.y, water.y, u) + arc, lerp(ROD_TIP.z, water.z, u));
  }
  // Floating: a bobble that ramps in from zero, so the splash-down is seamless.
  const floatAge = age - tl.flightEnd;
  const bobble = Math.sin(floatAge * 0.11) * 0.045 * Math.min(1, floatAge / 14);
  const dipT = clamp01((age - tl.dipStart) / (tl.dipEnd - tl.dipStart));
  const dip = age >= tl.dipStart ? -0.24 * pulse(dipT) : 0;
  if (age <= tl.dipEnd) {
    return v3(water.x, water.y + bobble + dip, water.z);
  }
  const reelU = clamp01((age - tl.dipEnd) / (tl.reelEnd - tl.dipEnd));
  return v3(
    lerp(water.x, CATCH_POINT.x, reelU),
    water.y + bobble * (1 - reelU),
    lerp(water.z, CATCH_POINT.z, reelU),
  );
};

// ── controller ──────────────────────────────────────────────────────────────────

export interface FishingExtra {
  /** Reticle position on the water (world x/z, clamped to the pond). */
  readonly aimX: number;
  readonly aimZ: number;
  /** True while the pointer button is held (a release casts). */
  readonly pressing: boolean;
}

export const initialFishingExtra = (_session: SessionState): FishingExtra => ({
  aimX: 0,
  aimZ: -0.4,
  pressing: false,
});

export type FishingState = CasinoState<FishingExtra>;

const KEY_AIM_STEP = 0.085;

export const stepFishing = (
  runtime: GameRuntime<FishingSpec>,
  state: FishingState,
  input: InputFrame,
  _ctx: TickContext,
): FishingState => {
  const session = state.session;

  if (session.phase === "ready") {
    const pointer = input.pointer;
    const fromPointer =
      pointer === undefined
        ? null
        : {
            x: ((pointer.pos.x / CANVAS_WIDTH) * 2 - 1) * POND_RADIUS * 1.15,
            z: ((pointer.pos.y / CANVAS_HEIGHT) * 2 - 1) * POND_RADIUS * 1.05,
          };
    const keyX = (input.down.has("right") ? 1 : 0) - (input.down.has("left") ? 1 : 0);
    const keyZ = (input.down.has("down") ? 1 : 0) - (input.down.has("up") ? 1 : 0);
    const raw = fromPointer ?? { x: state.extra.aimX + keyX * KEY_AIM_STEP, z: state.extra.aimZ + keyZ * KEY_AIM_STEP };
    const aim = clampAimToPond(raw.x, raw.z);
    const pointerDown = pointer?.down ?? false;
    const casts = input.pressed.has("primary") || (state.extra.pressing && !pointerDown);
    const extra: FishingExtra = { aimX: aim.x, aimZ: aim.z, pressing: pointerDown };
    if (casts) {
      return {
        ...state,
        extra,
        pendingContext: {
          aim: { x: aim.x / POND_RADIUS, y: aim.z / POND_RADIUS },
          castRegion: regionIndexAt(runtime.config.gameSpecific, aim.x, aim.z),
        },
        session: transition(session, "committing"),
      };
    }
    return { ...state, extra };
  }

  if (session.phase === "revealing") {
    const tl = castTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= tl.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Reveal-mechanism cues: splash thump, the sharp dip tick, the reel shimmer —
 * and one friendly squeak when the loss catch turns out to be the duck. */
export const fishingCues = (
  reducedMotion: boolean,
  prev: FishingState,
  next: FishingState,
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const tl = castTimeline(session.config.presentationSpeed, reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const plan = session.committed;
  const seed = plan?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  const duck = plan !== null && !plan.win && lossCatchOf(plan.presentationSeed) === "duck";
  const squeak: readonly ToneSpec[] = [
    { duration: 0.08, freq: 940, volume: 0.12, wave: "sine" },
    { delay: 0.07, duration: 0.1, freq: 640, volume: 0.1, wave: "sine" },
  ];
  return [
    ...(crossed(tl.flightEnd) ? thumpCue(seed, 11) : []),
    ...(crossed(tl.dipStart) ? tickCue(seed, 12) : []),
    ...(crossed(tl.reelEnd) ? shimmerCue(seed, 13) : []),
    ...(crossed(tl.riseEnd) && duck ? squeak : []),
  ];
};
