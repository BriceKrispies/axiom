/*
 * sessions.ts — per-player round state, in memory, keyed by a cookie.
 *
 * THE OUTCOME IS DECIDED BY THE APP'S REAL CHANCE ENGINE. This file imports
 * `startRound` from `apps/casino-games/web/src/chest-round/round.ts`, which
 * imports `planChoicePopulation`, `baseConfig`, `validateConfig` and the seeded
 * streams. Nothing about fairness is reimplemented here — that is precisely why
 * this stand-in is Node and not Rust: the chance engine is pure TypeScript with
 * no renderer dependency, so a server can run the SHIPPED code instead of a
 * second, drifting translation of it.
 *
 * WHAT A SESSION OWNS. One seed (drawn once, at this boundary, and recorded),
 * a round number, and the pick already made in the current round — if any.
 *
 * WHY THE PICK IS RECORDED. A form POST is not idempotent from the browser's
 * side: reload the result page and it re-POSTs. Recording the pick makes a
 * repeat POST REPLAY the same answer instead of opening a second chest, which
 * is the same commitment rule the engine build enforces — an outcome, once
 * revealed, cannot be rerolled. `replay: true` on the response says so out loud.
 *
 * The store is deliberately a plain Map with a TTL sweep: this is a stand-in for
 * whatever real backend ships, and pretending otherwise would invite someone to
 * treat it as one.
 */

import { CHEST_COUNT, startRound, type PickResult, type Round } from "../../../apps/casino-games/web/src/chest-round/round.ts";
import type { PickResponse, RevealedChest, RevealedReward, RoundResponse } from "../../../apps/casino-games/web/src/resilient/contract.ts";

/** The documented four-tier ladder's win rate (see the app README). */
export const TARGET_WIN_RATE = 0.44;

/** How long an untouched session survives before it is swept. */
const SESSION_TTL_MS = 2 * 60 * 60 * 1000;
/** A hard cap so a crawler cannot grow the map without bound. */
const MAX_SESSIONS = 5000;

export interface SessionState {
  readonly id: string;
  readonly seed: number;
  roundNumber: number;
  round: Round;
  picked: number | null;
  lastSeenMs: number;
}

/** Draw one seed at the outermost boundary, exactly as the browser shells do. */
const drawSeed = (): number => {
  const buf = new Uint32Array(1);
  crypto.getRandomValues(buf);
  return (buf[0] ?? 1) >>> 0;
};

export interface SessionStore {
  /** Fetch (or open) the session for a cookie value. */
  readonly acquire: (id: string | null) => SessionState;
  /** Deal a fresh board for a session, advancing its round number. */
  readonly nextRound: (session: SessionState) => void;
  readonly size: () => number;
}

export interface SessionStoreOptions {
  /** Injected for tests; defaults to boundary entropy. */
  readonly seedSource?: () => number;
  /** Injected for tests; defaults to `crypto.randomUUID`. */
  readonly idSource?: () => string;
  readonly nowMs?: () => number;
}

export const createSessionStore = (options: SessionStoreOptions = {}): SessionStore => {
  const seedSource = options.seedSource ?? drawSeed;
  const idSource = options.idSource ?? ((): string => crypto.randomUUID());
  const nowMs = options.nowMs ?? ((): number => Date.now());
  const sessions = new Map<string, SessionState>();

  const sweep = (now: number): void => {
    [...sessions.entries()]
      .filter(([, state]) => now - state.lastSeenMs > SESSION_TTL_MS)
      .forEach(([id]) => sessions.delete(id));
    // Oldest-first eviction if the TTL sweep was not enough.
    const overflow = sessions.size - MAX_SESSIONS;
    [...sessions.entries()]
      .sort((a, b) => a[1].lastSeenMs - b[1].lastSeenMs)
      .slice(0, Math.max(0, overflow))
      .forEach(([id]) => sessions.delete(id));
  };

  const open = (): SessionState => {
    const seed = seedSource();
    return { id: idSource(), lastSeenMs: nowMs(), picked: null, round: startRound(seed, 1, TARGET_WIN_RATE), roundNumber: 1, seed };
  };

  return {
    acquire: (id: string | null): SessionState => {
      const now = nowMs();
      sweep(now);
      const existing = id === null ? undefined : sessions.get(id);
      const session = existing ?? open();
      session.lastSeenMs = now;
      sessions.set(session.id, session);
      return session;
    },
    nextRound: (session: SessionState): void => {
      session.roundNumber += 1;
      session.round = startRound(session.seed, session.roundNumber, TARGET_WIN_RATE);
      session.picked = null;
    },
    size: (): number => sessions.size,
  };
};

const rewardOf = (result: PickResult): RevealedReward | null =>
  result.tier === null
    ? null
    : { rarity: result.tier.rarity, rewardLabel: result.tier.reward.label, tierId: result.tier.id, tierLabel: result.tier.label };

/** The whole board, revealed. The population was committed before the pick, so
 * disclosing it after the fact cannot change what was already decided. */
const boardOf = (round: Round): readonly RevealedChest[] =>
  Array.from({ length: CHEST_COUNT }, (unused, index) => ({ index, reward: rewardOf(round.reveal(index)) }));

/**
 * Resolve a pick against a session. Returns the SAME answer for a repeat pick
 * in the same round (flagged `replay`), whichever chest the repeat named.
 */
export const resolvePick = (session: SessionState, requested: number): PickResponse => {
  const replay = session.picked !== null;
  const picked = session.picked ?? requested;
  session.picked = picked;
  const result = session.round.reveal(picked);
  return {
    board: boardOf(session.round),
    chestCount: CHEST_COUNT,
    kind: "pick",
    picked,
    replay,
    reward: rewardOf(result),
    round: session.roundNumber,
    seed: session.seed,
    targetWinRate: TARGET_WIN_RATE,
    winnerCount: session.round.winnerCount,
    won: result.won,
  };
};

/** The session's current board, with nothing revealed. */
export const describeRound = (session: SessionState): RoundResponse => ({
  chestCount: CHEST_COUNT,
  kind: "round",
  round: session.roundNumber,
  seed: session.seed,
  targetWinRate: TARGET_WIN_RATE,
});
