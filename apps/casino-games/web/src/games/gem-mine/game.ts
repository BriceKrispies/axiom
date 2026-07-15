/*
 * game.ts — the Gem Mine controller: mechanic, rock layout, the staged strike
 * timeline, the analytic fragment ballistics, and the per-tick step. The
 * player picks one rock from a cluster on the mine floor; a pickaxe swings in
 * and strikes it across staged beats — each beat cracks the rock further —
 * and on the final strike the rock breaks into bounded fragments, revealing a
 * gem sized and colored by rarity (or an honest empty stone core).
 *
 * The choice-population adapter preassigned every rock's contents at session
 * start; the strike ceremony only breaks open what was already there. Rock
 * layout and per-index tint are FIXED tables (no stream). Idle micro-wobble
 * and dust draw only from the AMBIENT / PARTICLES streams and take no
 * population input. Fragment arcs are pure in the presentation seed.
 */

import type { EngineVec3, InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { clamp01 } from "../../presentation/stage/easing.ts";
import { v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";
import type { ChoiceCore } from "../choice-input.ts";
import { initialChoice, stepChoice } from "../choice-input.ts";

export interface MineSpec {
  /** Idle rock micro-wobble liveliness in [0, 1]. */
  readonly wobbleLiveliness: number;
}

export interface MineExtra {
  readonly choice: ChoiceCore;
}

export type MineState = CasinoState<MineExtra>;

export const MINE_MIN_CHOICES = 3;
export const MINE_MAX_CHOICES = 9;
export const MINE_DEFAULT_CHOICES = 5;
export const MINE_COLUMNS = 3;

/** Clamp a configured choice count into this game's supported range. */
export const mineChoiceCountOf = (raw: number | undefined): number =>
  Math.min(MINE_MAX_CHOICES, Math.max(MINE_MIN_CHOICES, Math.round(raw ?? MINE_DEFAULT_CHOICES)));

export const mineChoiceCount = (session: SessionState): number => mineChoiceCountOf(session.config.choiceCount);

export const MINE_SPACING = 1.55;

/** Rock cluster position (3 columns, rows recede in −Z). Pure function of index. */
export const rockPosition = (index: number, count: number): EngineVec3 => {
  const rows = Math.ceil(count / MINE_COLUMNS);
  const col = index % MINE_COLUMNS;
  const row = Math.floor(index / MINE_COLUMNS);
  return v3((col - (MINE_COLUMNS - 1) / 2) * MINE_SPACING, 0, (row - (rows - 1) / 2) * MINE_SPACING * 0.95);
};

export const mineCamera = (count: number): ReturnType<typeof showcaseCamera> =>
  showcaseCamera(v3(0, 0.5, 0), 4.4 + Math.ceil(count / MINE_COLUMNS) * 0.7, 0.9, 0.9);

export const rockTargets = (count: number): readonly PickTarget[] =>
  Array.from({ length: count }, (_, index) => ({
    at: rockPosition(index, count),
    index,
    radiusPx: 74,
  }));

/** A small fixed per-index rotation offset so no two rocks read the same, and a
 * subtle tint index that implies nothing about contents. Pure tables. */
export const rockYaw = (index: number): number => ((index * 1.31) % 1) * Math.PI * 2;
export const rockTintIndex = (index: number): number => index % 3;

// ── the staged strike timeline (ticks from entering "revealing", speed-scaled) ─

export const CRACK_STAGES = 3;

export interface MineTimeline {
  /** The pickaxe finishes swinging in from the side. */
  readonly approachEnd: number;
  /** The three strikes; each lands a crack stage. */
  readonly strikes: readonly [number, number, number];
  /** The rock breaks apart (final strike). */
  readonly breakAt: number;
  /** Fragments have finished their flight and the gem has risen. */
  readonly revealEnd: number;
  readonly total: number;
}

export const mineTimeline = (presentationSpeed: number, reducedMotion: boolean): MineTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const approachEnd = t(28);
  const beat = t(20);
  const strikes: readonly [number, number, number] = [approachEnd + beat, approachEnd + beat * 2, approachEnd + beat * 3];
  const breakAt = strikes[2];
  const revealEnd = breakAt + t(38);
  return { approachEnd, breakAt, revealEnd, strikes, total: revealEnd + t(10) };
};

/** How many crack stages have landed by `age` (0..CRACK_STAGES). */
export const crackStagesAt = (timeline: MineTimeline, age: number): number =>
  timeline.strikes.filter((strike) => age >= strike).length;

/** The pickaxe swing amount in [0, 1] during each strike beat (0 elsewhere). */
export const pickSwing = (timeline: MineTimeline, age: number): number => {
  const beat = timeline.strikes[0] - timeline.approachEnd;
  if (age < timeline.approachEnd || age >= timeline.breakAt || beat <= 0) {
    return 0;
  }
  const local = ((age - timeline.approachEnd) % beat) / beat;
  return Math.sin(local * Math.PI);
};

// ── analytic fragment ballistics (TRAJECTORY stream — pure in the seed) ────────

export interface Fragment {
  readonly position: EngineVec3;
  readonly spin: number;
  readonly size: number;
}

export const FRAGMENT_MIN = 4;
export const FRAGMENT_MAX = 6;

/** The fragment count for a break: 4–6, from the trajectory stream (bounded). */
export const fragmentCount = (presentationSeed: number): number =>
  FRAGMENT_MIN + Math.floor(sample01(presentationSeed, "trajectory", 0) * (FRAGMENT_MAX - FRAGMENT_MIN + 1));

/**
 * Fragment `i`'s pose at `age` ticks past the break: a ballistic arc launched
 * outward with per-fragment angle/speed/spin from the trajectory stream. Pure
 * in (origin, presentationSeed, i, age) — replays bit-for-bit.
 */
export const fragmentPose = (origin: EngineVec3, presentationSeed: number, i: number, age: number): Fragment => {
  const clamped = Math.max(0, age);
  const t = clamped / 60;
  const angle = sample01(presentationSeed, "trajectory", i, 1) * Math.PI * 2;
  const speed = 0.9 + sample01(presentationSeed, "trajectory", i, 2) * 1.6;
  const up = 1.6 + sample01(presentationSeed, "trajectory", i, 3) * 1.8;
  const size = 0.1 + sample01(presentationSeed, "trajectory", i, 4) * 0.09;
  const y = Math.max(0.04, origin.y + 0.3 + (up * t - 5.2 * t * t));
  return {
    position: v3(origin.x + Math.cos(angle) * speed * t, y, origin.z + Math.sin(angle) * speed * t),
    size,
    spin: clamped * (0.14 + sample01(presentationSeed, "trajectory", i, 5) * 0.2),
  };
};

// ── idle micro-wobble (AMBIENT stream only — no population input) ─────────────

/** Rock `index`'s idle wobble angle: tiny, pure in (index, tick, seed,
 * liveliness), drawing only from the ambient stream. */
export const rockWobble = (index: number, tick: number, seed: number, liveliness: number): number => {
  const window = Math.floor(tick / 130);
  const amp = sample01(seed, "ambient", 80 + index, window) * 0.02 * liveliness;
  return Math.sin((tick / 46) * Math.PI * 2 + index * 2.1) * amp;
};

// ── the controller ──────────────────────────────────────────────────────────────

export const initialMineExtra = (_session: SessionState): MineExtra => ({ choice: initialChoice(0) });

/** Per-tick controller: selection commits; the reveal advances on the staged
 * timeline and hands off to "celebrating" when the gem has fully risen. */
export const stepMine = (
  runtime: GameRuntime<MineSpec>,
  state: MineState,
  input: InputFrame,
  _ctx: TickContext,
): MineState => {
  const session = state.session;
  const count = mineChoiceCount(session);

  if (session.phase === "ready") {
    const result = stepChoice(state.extra.choice, input, mineCamera(count), rockTargets(count), MINE_COLUMNS);
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
    const timeline = mineTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= timeline.total) {
      return { ...state, session: transition(session, "celebrating") };
    }
  }

  return state;
};

/** Strike cues: a thump on each pick strike, a shimmer as the gem is unveiled
 * (the win/loss fanfare is played centrally by the harness). */
export const mineCues = (
  prev: MineState,
  next: MineState,
  reducedMotion: boolean,
  thump: (seed: number, key: number) => readonly ToneSpec[],
  shimmer: (seed: number, key: number) => readonly ToneSpec[],
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const timeline = mineTimeline(session.config.presentationSpeed, reducedMotion);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...timeline.strikes.flatMap((strike, i) => (crossed(strike) ? thump(seed, 30 + i) : [])),
    ...(crossed(timeline.breakAt + 2) ? shimmer(seed, 34) : []),
  ];
};

/** The strike progress used by the view (also its own pure export for tests):
 * clamps the reveal age into the timeline. */
export const strikeProgress = (timeline: MineTimeline, age: number): number =>
  clamp01((age - timeline.approachEnd) / (timeline.breakAt - timeline.approachEnd));
