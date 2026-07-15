/*
 * game.ts — the Safe Cracker controller. A cheerful toy prize-vault face-on
 * with three rotating dials. COMBINATION mechanic with the scratch-style
 * afterCommit hand-off: the FIRST stop press both COMMITS the outcome (an exact
 * three-symbol combination) and stops dial 1; the harness commits and returns
 * control (committing → interacting); the next two presses stop dials 2 and 3.
 *
 * Each dial spins live until stopped; the visible stop is player timing, but
 * the SYMBOL it lands on is presentation-eased to the committed one — the dial
 * eases continuously from its live angle to the committed symbol's angle, so
 * there is no snap and the settled symbols always equal the committed
 * combination. Once the third dial's ease completes, the reveal begins: on a
 * win the bolts retract one at a time and the door swings open; on a loss the
 * handle wiggles and the dials give a sympathetic wobble.
 */

import type { InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import type { CombinationSpace, WinningCombination } from "../../chance-engine/probability/combination.ts";
import { sample01, sampleRange } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { clamp01, easeOutCubic } from "../../presentation/stage/easing.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";

// ── the game-specific spec ──────────────────────────────────────────────────────

export const DIAL_COUNT = 3;
export const SAFE_SYMBOLS = 6;

export interface SafeSpec {
  /** Number of symbols per dial — fixed at 6. */
  readonly symbols: number;
  /** Winning three-symbol combinations and their tiers. */
  readonly combos: readonly WinningCombination[];
}

/** The default table: a three-of-a-kind on each symbol wins, richer for the
 * rarer symbols (symbol 0 = the star → jackpot). */
export const DEFAULT_SAFE_COMBOS: readonly WinningCombination[] = [
  { combo: [0, 0, 0], tierId: "jackpot" },
  { combo: [1, 1, 1], tierId: "rare" },
  { combo: [2, 2, 2], tierId: "uncommon" },
  { combo: [3, 3, 3], tierId: "uncommon" },
  { combo: [4, 4, 4], tierId: "common" },
  { combo: [5, 5, 5], tierId: "common" },
];

export const DEFAULT_SAFE_SPEC: SafeSpec = { combos: DEFAULT_SAFE_COMBOS, symbols: SAFE_SYMBOLS };

export const safeSpace = (spec: SafeSpec): CombinationSpace => ({
  reels: DIAL_COUNT,
  symbolsPerReel: spec.symbols,
  winningCombos: spec.combos,
});

// ── dial angle mathematics (continuous; exact at settle) ─────────────────────────

export const EASE_TICKS_BASE = 30;

/** The rest angle at which symbol `s` faces front. */
export const symbolAngle = (symbol: number, symbols: number): number => (symbol / symbols) * Math.PI * 2;

/** The symbol shown at a settled dial angle (inverse of `symbolAngle`). */
export const settledSymbol = (angle: number, symbols: number): number => {
  const step = (Math.PI * 2) / symbols;
  return ((Math.round(angle / step) % symbols) + symbols) % symbols;
};

/** The live free-spin angle of dial `k` at `tick` — pure in (tick, k, seed),
 * drawn from the AMBIENT stream (pre-commitment decoration, stable all round). */
export const liveDialAngle = (tick: number, k: number, seed: number): number => {
  const speed = 0.18 + sample01(seed, "ambient", k, 0) * 0.12;
  const phase = sample01(seed, "ambient", k, 1) * Math.PI * 2;
  return phase + tick * speed;
};

/** The forward ease target for a dial stopping at `fromAngle`: the least angle
 * ≥ `fromAngle` that is congruent to the committed symbol's rest angle, plus
 * `extraTurns` full rotations for a graceful slowdown. Congruent mod 2π, so the
 * settled symbol is exactly `symbol` — and continuous from `fromAngle`. */
export const dialTarget = (fromAngle: number, symbol: number, symbols: number, extraTurns = 1): number => {
  const base = symbolAngle(symbol, symbols);
  const forward = (((base - fromAngle) % (Math.PI * 2)) + Math.PI * 2) % (Math.PI * 2);
  return fromAngle + forward + Math.max(0, Math.floor(extraTurns)) * Math.PI * 2;
};

/** The settled symbol dial `k` will show, given its stop tick and the committed
 * combination — this is what the reveal reads. Equals `combination[k]`. */
export const settledDialSymbol = (
  stopTick: number,
  k: number,
  combination: readonly number[],
  seed: number,
  symbols: number,
): number => settledSymbol(dialTarget(liveDialAngle(stopTick, k, seed), combination[k] ?? 0, symbols), symbols);

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export const NUM_BOLTS = 4;

export interface SafeTimeline {
  readonly anticipation: number;
  readonly boltStagger: number;
  readonly boltRetract: number;
  readonly boltsEnd: number;
  readonly doorStart: number;
  readonly doorEnd: number;
  readonly winTotal: number;
  readonly lossTotal: number;
}

export const safeTimeline = (presentationSpeed: number, reducedMotion: boolean): SafeTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const anticipation = t(16);
  const boltStagger = t(9);
  const boltRetract = t(12);
  const boltsEnd = anticipation + (NUM_BOLTS - 1) * boltStagger + boltRetract;
  const doorStart = boltsEnd + t(6);
  const doorEnd = doorStart + t(32);
  return {
    anticipation,
    boltRetract,
    boltStagger,
    boltsEnd,
    doorEnd,
    doorStart,
    lossTotal: anticipation + t(34),
    winTotal: doorEnd + t(20),
  };
};

/** The tick (from entering revealing) at which bolt `k` begins retracting —
 * strictly increasing in `k`, so the bolts fire one at a time. */
export const boltRetractStart = (k: number, timeline: SafeTimeline): number =>
  timeline.anticipation + k * timeline.boltStagger;

export const safeRevealTotal = (timeline: SafeTimeline, win: boolean): number =>
  win ? timeline.winTotal : timeline.lossTotal;

// ── the controller ──────────────────────────────────────────────────────────────

export interface SafeExtra {
  /** Session tick each dial began its stop-ease, or null while free-spinning. */
  readonly stops: readonly (number | null)[];
  readonly pointerWasDown: boolean;
}

export type SafeState = CasinoState<SafeExtra>;

export const initialSafeExtra = (_session: SessionState): SafeExtra => ({
  pointerWasDown: false,
  stops: [null, null, null],
});

/** Count of dials that have been stopped. */
export const stopsMade = (extra: SafeExtra): number => extra.stops.filter((stop) => stop !== null).length;

export const stepSafe = (
  runtime: GameRuntime<SafeSpec>,
  state: SafeState,
  input: InputFrame,
  _ctx: TickContext,
): SafeState => {
  const session = state.session;
  const pointerDown = input.pointer?.down ?? false;
  const clicked = pointerDown && !state.extra.pointerWasDown;
  const pressEdge = input.pressed.has("primary") || clicked;
  const extra = pointerDown === state.extra.pointerWasDown ? state.extra : { ...state.extra, pointerWasDown: pointerDown };
  const tracked: SafeState = { ...state, extra };

  if (session.phase === "ready") {
    if (pressEdge) {
      // The first press commits AND stops dial 1 (registered on arrival below).
      return { ...tracked, pendingContext: {}, session: transition(session, "committing") };
    }
    return tracked;
  }

  if (session.phase === "interacting") {
    const made = stopsMade(extra);
    if (made === 0) {
      // Control has just returned from the commit: dial 1 stops now.
      return { ...tracked, extra: { ...extra, stops: [session.tick, extra.stops[1] ?? null, extra.stops[2] ?? null] } };
    }
    if (made < DIAL_COUNT && pressEdge) {
      const stops = extra.stops.map((stop, i) => (i === made ? session.tick : stop));
      return { ...tracked, extra: { ...extra, stops } };
    }
    if (made === DIAL_COUNT) {
      const easeTicks = speedTicks(EASE_TICKS_BASE, session.config.presentationSpeed);
      const lastStop = extra.stops[DIAL_COUNT - 1] ?? session.tick;
      if (session.tick >= lastStop + easeTicks) {
        return { ...tracked, session: transition(session, "revealing") };
      }
    }
    return tracked;
  }

  if (session.phase === "revealing" && session.committed !== null) {
    const timeline = safeTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    if (phaseAge(session) >= safeRevealTotal(timeline, session.committed.win)) {
      return { ...tracked, session: transition(session, "celebrating") };
    }
  }

  return tracked;
};

/** The current display angle of dial `k`: free-spin, easing, or settled. */
export const dialDisplayAngle = (state: SafeState, k: number): number => {
  const session = state.session;
  const seed = session.seed;
  const stop = state.extra.stops[k] ?? null;
  if (stop === null) {
    return liveDialAngle(session.tick, k, seed);
  }
  const combination =
    session.committed !== null && session.committed.manifestation.kind === "combination"
      ? session.committed.manifestation.combination
      : [];
  const from = liveDialAngle(stop, k, seed);
  const extraTurns = 1 + Math.floor(sampleRange(0, 2, session.committed?.presentationSeed ?? seed, "trajectory", k, 0));
  const target = dialTarget(from, combination[k] ?? 0, SAFE_SYMBOLS, extraTurns);
  const easeTicks = speedTicks(EASE_TICKS_BASE, session.config.presentationSpeed);
  const u = clamp01((session.tick - stop) / easeTicks);
  return from + (target - from) * easeOutCubic(u);
};

/** Reveal-mechanism cues: a clunk as each bolt lands, and the handle spin
 * (the win/loss fanfare itself is played by the harness). */
export const safeCues = (
  prev: SafeState,
  next: SafeState,
  clunk: (seed: number, key: number) => readonly ToneSpec[],
): readonly ToneSpec[] => {
  const session = next.session;
  if (session.phase !== "revealing" || prev.session.phase !== "revealing" || session.committed === null || !session.committed.win) {
    return [];
  }
  const timeline = safeTimeline(session.config.presentationSpeed, false);
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const seed = session.committed.presentationSeed;
  return Array.from({ length: NUM_BOLTS }, (_, k) => k)
    .filter((k) => {
      const mark = boltRetractStart(k, timeline) + timeline.boltRetract;
      return before < mark && after >= mark;
    })
    .flatMap((k) => clunk(seed, k + 1));
};
