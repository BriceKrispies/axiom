/*
 * game.ts — the Treasure Map controller: mechanic, fixed dig-site layout,
 * reveal timeline, and per-tick step for the island-map dig game. The player
 * picks one X-marked dig site on an illustrated island; the choice-population
 * adapter preassigned every site's contents before the round began, so the
 * crew's little dig ceremony only unearths what was already there.
 *
 * The dig layout is a FIXED TABLE indexed by site — never a stream draw — so
 * positions can leak nothing. Idle decoration (compass sway, X-marker pulse)
 * draws exclusively from the AMBIENT stream keyed by tick window and site
 * index, never from the population; the layout test pins the purity.
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { tabletopCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { clamp01, smoothstep } from "../../presentation/stage/easing.ts";
import { lerpV3, v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";
import type { ChoiceCore } from "../choice-input.ts";
import { initialChoice, stepChoice } from "../choice-input.ts";

export interface MapSpec {
  /** Compass-rose idle sway liveliness in [0, 1]. */
  readonly compassLiveliness: number;
  /** X-marker idle pulse intensity in [0, 1]. */
  readonly markerPulse: number;
}

export interface MapExtra {
  readonly choice: ChoiceCore;
}

export type MapState = CasinoState<MapExtra>;

export const MAP_MIN_CHOICES = 3;
export const MAP_MAX_CHOICES = 10;
export const MAP_DEFAULT_CHOICES = 6;
export const MAP_COLUMNS = 3;

/** Clamp a configured choice count into this game's supported range. */
export const mapChoiceCountOf = (raw: number | undefined): number =>
  Math.min(MAP_MAX_CHOICES, Math.max(MAP_MIN_CHOICES, Math.round(raw ?? MAP_DEFAULT_CHOICES)));

export const mapChoiceCount = (session: SessionState): number => mapChoiceCountOf(session.config.choiceCount);

/** The fixed dig-site layout table: one authored spot per index, spread across
 * the island. A pure function of index — no stream, no seed, no round. */
const DIG_SPOTS: readonly EngineVec3[] = [
  v3(-1.5, 0, -0.7),
  v3(0.2, 0, -1.0),
  v3(1.7, 0, -0.55),
  v3(-1.95, 0, 0.3),
  v3(-0.3, 0, 0.2),
  v3(1.35, 0, 0.5),
  v3(-1.0, 0, 1.05),
  v3(0.6, 0, 1.15),
  v3(1.95, 0, 1.1),
  v3(-2.15, 0, -0.15),
];

export const digPosition = (index: number): EngineVec3 => DIG_SPOTS[index % DIG_SPOTS.length] as EngineVec3;

export const mapCamera = (count: number): ReturnType<typeof tabletopCamera> =>
  tabletopCamera(v3(0, 0, 0.2), 4.1 + Math.max(0, count - 6) * 0.12);

export const digTargets = (count: number): readonly PickTarget[] =>
  Array.from({ length: count }, (_, index) => ({
    at: digPosition(index),
    index,
    radiusPx: 62,
  }));

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export const DIG_BEATS = 3;

export interface MapRevealTimeline {
  /** The digger finishes hopping to the chosen X. */
  readonly travelEnd: number;
  /** The three shovel hits (dust puff + hole growth on each). */
  readonly hits: readonly [number, number, number];
  /** Digging done — the hole's contents come up. */
  readonly digEnd: number;
  /** The reward (or honest empty sparkle) has fully risen. */
  readonly riseEnd: number;
  readonly total: number;
}

export const mapRevealTimeline = (presentationSpeed: number, reducedMotion: boolean): MapRevealTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const travelEnd = t(48);
  const beat = t(16);
  const hits: readonly [number, number, number] = [travelEnd + beat, travelEnd + beat * 2, travelEnd + beat * 3];
  const digEnd = hits[2];
  const riseEnd = digEnd + t(32);
  return { digEnd, hits, riseEnd, total: riseEnd + t(10), travelEnd };
};

// ── idle decoration (AMBIENT stream only — cannot correlate with winners) ─────

/** The compass rose's current angle: a slow steady turn plus a gentle per-window
 * sway elected on the ambient stream. */
export const compassAngle = (tick: number, seed: number, liveliness: number): number => {
  const window = Math.floor(tick / 140);
  const sway = (sample01(seed, "ambient", 700, window) - 0.5) * 1.4;
  const local = (tick % 140) / 140;
  return tick * 0.0025 + sway * Math.sin(Math.PI * local) * liveliness;
};

/** The subtle pulse scale of X marker `index`: amplitude per (window, index)
 * from the ambient stream, phased by the tick clock. */
export const markerPulseScale = (index: number, tick: number, seed: number, intensity: number): number => {
  const window = Math.floor(tick / 90);
  const amp = sample01(seed, "ambient", 20 + index, window) * 0.09 * intensity;
  return 1 + Math.sin(((tick % 90) / 90) * Math.PI * 2) * amp;
};

// ── the dig journey ─────────────────────────────────────────────────────────────

/** Where the digger enters the map (a beach corner, off the island). */
export const MAP_ENTRY: EngineVec3 = v3(-3.1, 0, 2.3);

const HOP_COUNT = 5;

/** The digger's hop-along position at travel progress `t` in [0, 1]: smooth
 * ground path from the beach entry toward `target` with a small hop bounce. */
export const diggerPose = (target: EngineVec3, t: number): EngineVec3 => {
  const s = clamp01(t);
  const ground = lerpV3(MAP_ENTRY, target, smoothstep(s));
  const hop = Math.abs(Math.sin(s * Math.PI * HOP_COUNT)) * 0.16 * (1 - 0.35 * s);
  return v3(ground.x, hop, ground.z);
};

/** Shovel swing amount in [0, 1] during the dig beats (0 outside them). */
export const shovelSwing = (timeline: MapRevealTimeline, age: number): number => {
  const beat = timeline.hits[0] - timeline.travelEnd;
  if (age < timeline.travelEnd || age >= timeline.digEnd || beat <= 0) {
    return 0;
  }
  const local = ((age - timeline.travelEnd) % beat) / beat;
  return Math.sin(local * Math.PI);
};

// ── the controller ──────────────────────────────────────────────────────────────

export const initialMapExtra = (_session: SessionState): MapExtra => ({ choice: initialChoice(1) });

/** Per-tick controller: selection commits; the reveal advances on the shared
 * timeline and hands off to "celebrating" when the rise completes. */
export const stepMap = (
  runtime: GameRuntime<MapSpec>,
  state: MapState,
  input: InputFrame,
  _ctx: TickContext,
): MapState => {
  const session = state.session;
  const count = mapChoiceCount(session);

  if (session.phase === "ready") {
    const result = stepChoice(state.extra.choice, input, mapCamera(count), digTargets(count), MAP_COLUMNS);
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
    const timeline = mapRevealTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Dig-mechanism cues: a shovel thump on each dig hit, a shimmer as the hole's
 * contents come up (win/loss fanfare is played centrally by the harness). */
export const mapCues = (
  prev: MapState,
  next: MapState,
  reducedMotion: boolean,
  thump: (seed: number, key: number) => readonly ToneSpec[],
  shimmer: (seed: number, key: number) => readonly ToneSpec[],
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const timeline = mapRevealTimeline(session.config.presentationSpeed, reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...timeline.hits.flatMap((hit, i) => (crossed(hit) ? thump(seed, 10 + i) : [])),
    ...(crossed(timeline.digEnd + 2) ? shimmer(seed, 14) : []),
  ];
};
