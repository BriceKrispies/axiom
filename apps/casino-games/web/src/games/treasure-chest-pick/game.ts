/*
 * game.ts — the Treasure Chest Pick controller: the mount spec's mechanic,
 * per-tick step, and reveal timeline. Nine carved-wood chests in a 3×3 grid;
 * the choice-population adapter preassigns which chests hold prizes before
 * the player can possibly choose; the reveal follows the classic cadence —
 * focus, anticipation brace, LATCH FALLS FIRST, pause, lid pops with
 * overshoot, warm light, reward (or honest empty interior).
 *
 * Idle "chest dances" draw exclusively from the AMBIENT stream keyed by tick
 * window and grid slot — never from the population — so no wobble can hint
 * at contents. The dance test pins this.
 */

import type { InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import type { EngineVec3 } from "@axiom/web-engine";
import { sample01, sampleInt } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { tabletopCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";
import type { ChoiceCore } from "../choice-input.ts";
import { initialChoice, stepChoice } from "../choice-input.ts";

export interface ChestSpec {
  /** Idle dance liveliness in [0, 1]. */
  readonly danceLiveliness: number;
}

export interface ChestExtra {
  readonly choice: ChoiceCore;
  /** Tick at which the reveal began (session tick space), for cue edges. */
  readonly revealStartTick: number | null;
}

export type ChestState = CasinoState<ChestExtra>;

export const CHEST_COLUMNS = 3;
export const CHEST_SPACING = 2.05;

/** Grid slot world position (3 columns, rows recede in −Z). */
export const chestPosition = (index: number, count: number): EngineVec3 => {
  const columns = CHEST_COLUMNS;
  const rows = Math.ceil(count / columns);
  const col = index % columns;
  const row = Math.floor(index / columns);
  return v3((col - (columns - 1) / 2) * CHEST_SPACING, 0, (row - (rows - 1) / 2) * CHEST_SPACING * 0.92);
};

export const chestCamera = (count: number): ReturnType<typeof tabletopCamera> =>
  tabletopCamera(v3(0, 0.42, -0.1), 3.6 + Math.ceil(count / CHEST_COLUMNS) * 0.55);

export const chestTargets = (count: number): readonly PickTarget[] =>
  Array.from({ length: count }, (_, index) => ({
    at: chestPosition(index, count),
    index,
    radiusPx: 78,
  }));

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export interface RevealTimeline {
  readonly braceEnd: number;
  readonly latchStart: number;
  readonly latchEnd: number;
  readonly pauseEnd: number;
  readonly lidEnd: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const revealTimeline = (presentationSpeed: number, reducedMotion: boolean): RevealTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const braceEnd = t(22);
  const latchStart = braceEnd;
  const latchEnd = latchStart + t(16);
  const pauseEnd = latchEnd + t(12);
  const lidEnd = pauseEnd + t(14);
  const riseEnd = lidEnd + t(34);
  return { braceEnd, latchEnd, latchStart, lidEnd, pauseEnd, riseEnd, total: riseEnd + t(8) };
};

/**
 * The idle dance pose for chest `index` at `tick` — AMBIENT stream only.
 * Time is cut into windows; each window elects one dancer (and, rarely, a
 * second) and gives it a small scoot + twist + squash figure.
 */
export interface DancePose {
  readonly scootX: number;
  readonly twist: number;
  readonly squash: number;
}

export const dancePose = (index: number, count: number, tick: number, seed: number, liveliness: number): DancePose => {
  const window = Math.floor(tick / 96);
  const dancer = sampleInt(count, seed, "ambient", window, 0);
  const second = sampleInt(count, seed, "ambient", window, 1);
  const duet = sample01(seed, "ambient", window, 2) < 0.2;
  const isDancing = index === dancer || (duet && index === second);
  if (!isDancing || liveliness <= 0) {
    return { scootX: 0, squash: 0, twist: 0 };
  }
  const local = (tick % 96) / 96;
  const envelope = Math.sin(Math.PI * local);
  const figure = sample01(seed, "ambient", window, 3 + index);
  return {
    scootX: Math.sin(local * Math.PI * 4 + figure * 6) * 0.05 * liveliness * envelope,
    squash: Math.abs(Math.sin(local * Math.PI * 6)) * 0.045 * liveliness * envelope,
    twist: Math.sin(local * Math.PI * 2 + figure * 4) * 0.07 * liveliness * envelope,
  };
};

export const initialChestExtra = (_session: SessionState): ChestExtra => ({
  choice: initialChoice(4),
  revealStartTick: null,
});

/** Per-tick controller. Selection commits; the reveal advances on the shared
 * timeline and hands off to "celebrating" when it completes. */
export const stepChest = (
  runtime: GameRuntime<ChestSpec>,
  state: ChestState,
  input: InputFrame,
  _ctx: TickContext,
): ChestState => {
  const session = state.session;
  const count = session.config.choiceCount ?? 9;

  if (session.phase === "ready") {
    const result = stepChoice(state.extra.choice, input, chestCamera(count), chestTargets(count), CHEST_COLUMNS);
    if (result.selectedNow !== null) {
      return {
        ...state,
        extra: { ...state.extra, choice: result.core },
        pendingContext: { selectedIndex: result.selectedNow },
        session: transition(session, "committing"),
      };
    }
    return { ...state, extra: { ...state.extra, choice: result.core } };
  }

  if (session.phase === "revealing") {
    const start = state.extra.revealStartTick ?? session.phaseStartTick;
    const timeline = revealTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    const withStart: ChestState =
      state.extra.revealStartTick === null ? { ...state, extra: { ...state.extra, revealStartTick: start } } : state;
    if (phaseAge(session) >= timeline.total) {
      return { ...withStart, session: transition(session, "celebrating") };
    }
    return withStart;
  }

  return state;
};

/** Reveal-mechanism cues: latch thump when the latch lands, pop when the lid
 * flies (the win/loss fanfare itself is played centrally by the harness). */
export const chestCues = (
  prev: ChestState,
  next: ChestState,
  thump: (seed: number, key: number) => readonly ToneSpec[],
  shimmer: (seed: number, key: number) => readonly ToneSpec[],
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing") {
    return [];
  }
  const timeline = revealTimeline(session.config.presentationSpeed, false);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const crossed = (mark: number): boolean => before < mark && after >= mark;
  return [
    ...(crossed(timeline.latchEnd) ? thump(seed, 1) : []),
    ...(crossed(timeline.lidEnd) ? shimmer(seed, 2) : []),
  ];
};
