/*
 * serialize.ts — the versioned match snapshot + replay format. A `MatchReplay`
 * captures everything needed to reproduce a match exactly: the content and rules
 * versions (so a replay is rejected against incompatible content), the seed, the
 * initial player records, the ordered command log, and the phase-transition
 * records. Because the simulation is deterministic, this small record replays to
 * a byte-identical final state and event log (see `replay.ts`). A combat's own
 * stream is recoverable the same way — it is the slice of the reproduced event
 * log for that `combatId`.
 */

import type { Match, LoggedCommand, MatchPlayerInit, PhaseTransition } from "../sim/match.ts";

export interface MatchReplay {
  readonly formatVersion: number;
  readonly contentVersion: number;
  readonly rulesVersion: number;
  readonly matchId: string;
  readonly seed: number;
  readonly players: readonly MatchPlayerInit[];
  readonly commands: readonly LoggedCommand[];
  readonly transitions: readonly PhaseTransition[];
}

export const REPLAY_FORMAT_VERSION = 1;

/** Capture a completed (or in-progress) match as a replay record. */
export const serializeReplay = (match: Match): MatchReplay => ({
  formatVersion: REPLAY_FORMAT_VERSION,
  contentVersion: match.content.version,
  rulesVersion: match.rules.rulesVersion,
  matchId: match.state.matchId,
  seed: match.state.seed,
  players: match.state.players.map((p) => ({ name: p.name, isBot: p.isBot })),
  commands: match.getCommandLog().map((c) => ({ ...c })),
  transitions: match.getTransitions().map((t) => ({ ...t })),
});

/** A compact fingerprint of a match's final state + event log, for equality
 * checks in replay/determinism tests. */
export const matchFingerprint = (match: Match): string =>
  JSON.stringify({ state: match.state, events: match.getEvents() });
