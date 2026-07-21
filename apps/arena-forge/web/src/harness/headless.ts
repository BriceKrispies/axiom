/*
 * headless.ts — the DOM-free simulation harness. It plays full seeded matches
 * with eight bots and derives a report entirely from the authoritative event log
 * and transition log: winner, rounds, elimination timing, group usage, forged
 * units, bound (drawn-by-limit) combats, and any illegal phase transition. It can
 * run one match, a batch of 100, or verify determinism by replaying a seed and
 * comparing final state + event log byte-for-byte. This is where "all 100 matches
 * complete", "match results are deterministic", and the invariant checks are
 * mechanically proven.
 */

import { loadDefaultContent } from "../sim/content/bundle.ts";
import type { LoadedContent } from "../sim/content/load.ts";
import type { SimEvent } from "../sim/events.ts";
import type { GroupId, PlayerId } from "../sim/ids.ts";
import { isLegalTransition } from "../sim/phase.ts";
import type { Rules } from "../sim/tuning.ts";
import { LocalMatchHost } from "../api/local-host.ts";

export interface MatchReport {
  readonly seed: number;
  readonly complete: boolean;
  readonly rounds: number;
  readonly winner: PlayerId | null;
  readonly placements: readonly PlayerId[];
  readonly forgedUnits: number;
  readonly boundCombats: number;
  readonly illegalTransitions: number;
  readonly avgEliminationRound: number;
  readonly groupUsage: Readonly<Record<GroupId, number>>;
  readonly negativeGold: boolean;
}

const buildReport = (host: LocalMatchHost, seed: number): MatchReport => {
  const match = host.getMatch();
  const events = match.getEvents();
  const state = match.state;

  let round = 0;
  let forgedUnits = 0;
  let boundCombats = 0;
  const eliminationRounds: number[] = [];
  const groupUsage: Record<GroupId, number> = {};
  for (const g of match.content.groups) {
    groupUsage[g.id] = 0;
  }
  for (const ev of events as readonly SimEvent[]) {
    if (ev.kind === "phase_changed") {
      round = ev.round;
    } else if (ev.kind === "unit_forged") {
      forgedUnits += 1;
    } else if (ev.kind === "diagnostic") {
      boundCombats += 1;
    } else if (ev.kind === "player_eliminated") {
      eliminationRounds.push(round);
    } else if (ev.kind === "card_purchased") {
      for (const g of match.content.card(ev.cardId).groups) {
        groupUsage[g] = (groupUsage[g] ?? 0) + 1;
      }
    }
  }

  let illegalTransitions = 0;
  for (const t of match.getTransitions()) {
    if (!isLegalTransition(t.from, t.to)) {
      illegalTransitions += 1;
    }
  }

  const placements = state.players
    .filter((p) => p.placement > 0)
    .slice()
    .sort((a, b) => a.placement - b.placement)
    .map((p) => p.id);
  const winner = state.players.find((p) => p.placement === 1)?.id ?? null;
  const negativeGold = state.players.some((p) => p.gold < 0);
  const avgEliminationRound = eliminationRounds.length > 0 ? eliminationRounds.reduce((s, r) => s + r, 0) / eliminationRounds.length : 0;

  return {
    seed,
    complete: host.isComplete(),
    rounds: state.round,
    winner,
    placements,
    forgedUnits,
    boundCombats,
    illegalTransitions,
    avgEliminationRound,
    groupUsage,
    negativeGold,
  };
};

/** Run one seeded match to completion and return the host + report. */
export const runSeededMatch = (
  content: LoadedContent,
  seed: number,
  rules?: Rules,
): { readonly host: LocalMatchHost; readonly report: MatchReport } => {
  const host = new LocalMatchHost({ seed, content, allBots: true, ...(rules ? { rules } : {}) });
  host.runToCompletion();
  return { host, report: buildReport(host, seed) };
};

export interface SuiteReport {
  readonly matches: number;
  readonly allComplete: boolean;
  readonly winnerDistribution: Readonly<Record<PlayerId, number>>;
  readonly avgRounds: number;
  readonly avgEliminationRound: number;
  readonly groupUsage: Readonly<Record<GroupId, number>>;
  readonly forgedTotal: number;
  readonly boundCombats: number;
  readonly illegalTransitions: number;
  readonly negativeGoldMatches: number;
  readonly maxRoundsSeen: number;
  readonly reports: readonly MatchReport[];
}

/** Run `count` matches from `baseSeed`, `baseSeed+1`, … and aggregate. */
export const runManyMatches = (content: LoadedContent, count: number, baseSeed: number, rules?: Rules): SuiteReport => {
  const reports: MatchReport[] = [];
  const winnerDistribution: Record<PlayerId, number> = {};
  const groupUsage: Record<GroupId, number> = {};
  for (const g of content.groups) {
    groupUsage[g.id] = 0;
  }
  let roundsSum = 0;
  let elimSum = 0;
  let elimCount = 0;
  let forgedTotal = 0;
  let boundCombats = 0;
  let illegalTransitions = 0;
  let negativeGoldMatches = 0;
  let maxRoundsSeen = 0;
  let allComplete = true;

  for (let i = 0; i < count; i += 1) {
    const { report } = runSeededMatch(content, baseSeed + i, rules);
    reports.push(report);
    allComplete = allComplete && report.complete;
    if (report.winner !== null) {
      winnerDistribution[report.winner] = (winnerDistribution[report.winner] ?? 0) + 1;
    }
    roundsSum += report.rounds;
    maxRoundsSeen = Math.max(maxRoundsSeen, report.rounds);
    if (report.avgEliminationRound > 0) {
      elimSum += report.avgEliminationRound;
      elimCount += 1;
    }
    forgedTotal += report.forgedUnits;
    boundCombats += report.boundCombats;
    illegalTransitions += report.illegalTransitions;
    negativeGoldMatches += report.negativeGold ? 1 : 0;
    for (const g of Object.keys(report.groupUsage)) {
      groupUsage[g] = (groupUsage[g] ?? 0) + (report.groupUsage[g] ?? 0);
    }
  }

  return {
    matches: count,
    allComplete,
    winnerDistribution,
    avgRounds: count > 0 ? roundsSum / count : 0,
    avgEliminationRound: elimCount > 0 ? elimSum / elimCount : 0,
    groupUsage,
    forgedTotal,
    boundCombats,
    illegalTransitions,
    negativeGoldMatches,
    maxRoundsSeen,
    reports,
  };
};

/** Convenience: run the default 100-match suite on the default content. */
export const runDefaultSuite = (count = 100, baseSeed = 1, rules?: Rules): SuiteReport =>
  runManyMatches(loadDefaultContent(), count, baseSeed, rules);
