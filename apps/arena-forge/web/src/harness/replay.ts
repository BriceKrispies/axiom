/*
 * replay.ts — reconstructs a match from a `MatchReplay`. Replay is pure input
 * substitution: build a fresh `Match` from the same seed and initial players,
 * then feed the logged commands back in — grouped by the round they were issued
 * in — advancing the phase machine deterministically between rounds. Because the
 * whole simulation is deterministic in (seed, command stream), the reconstructed
 * match reaches a state and event log byte-identical to the original. This is
 * what lets a match — including any single combat's event stream — be replayed
 * without re-running the original actors.
 */

import { Match } from "../sim/match.ts";
import type { LoadedContent } from "../sim/content/load.ts";
import type { Rules } from "../sim/tuning.ts";
import type { MatchReplay } from "./serialize.ts";
import { REPLAY_FORMAT_VERSION } from "./serialize.ts";

/** Rebuild and re-run a match from its replay record. Throws on an incompatible
 * format or content/rules version. */
export const replayMatch = (content: LoadedContent, replay: MatchReplay, rules?: Rules): Match => {
  if (replay.formatVersion !== REPLAY_FORMAT_VERSION) {
    throw new Error(`Arena Forge replay: unsupported format version ${replay.formatVersion}`);
  }
  if (replay.contentVersion !== content.version) {
    throw new Error(`Arena Forge replay: content version ${replay.contentVersion} != loaded ${content.version}`);
  }
  const match = new Match({ matchId: replay.matchId, seed: replay.seed, content, players: replay.players, ...(rules ? { rules } : {}) });

  const byRound = new Map<number, MatchReplay["commands"][number][]>();
  for (const cmd of replay.commands) {
    const list = byRound.get(cmd.round) ?? [];
    list.push(cmd);
    byRound.set(cmd.round, list);
  }

  match.start();
  const cap = match.rules.maxRounds * 4 + 8;
  let guard = 0;
  while (match.state.phase !== "match_complete" && guard < cap) {
    if (match.state.phase === "shop") {
      for (const cmd of byRound.get(match.state.round) ?? []) {
        match.submit(cmd.playerId, cmd.command);
      }
    }
    match.advancePhase();
    guard += 1;
  }
  return match;
};
