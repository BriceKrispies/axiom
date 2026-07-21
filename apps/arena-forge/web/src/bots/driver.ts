/*
 * driver.ts — runs a bot's whole shop turn through the public command surface.
 * It repeatedly asks the policy for scored candidates, plays the best positive
 * one, and submits it via the SAME authoritative entry point the human uses —
 * never mutating state directly. Every step produces a `DecisionRecord`. A hard
 * step cap guarantees termination even if a policy misbehaves.
 */

import type { Command, CommandResult } from "../sim/commands.ts";
import type { PlayerId } from "../sim/ids.ts";
import type { BotContext, BotPolicy, DecisionRecord } from "./policy.ts";

export interface BotDeps {
  readonly snapshot: () => BotContext;
  readonly submit: (playerId: PlayerId, command: Command) => CommandResult;
}

const MAX_STEPS = 40;

/** Play out one bot's full shop turn; returns its decision records in order. */
export const runBotTurn = (deps: BotDeps, playerId: PlayerId, policy: BotPolicy): DecisionRecord[] => {
  const records: DecisionRecord[] = [];
  for (let step = 0; step < MAX_STEPS; step += 1) {
    const ctx = deps.snapshot();
    const player = ctx.state.players[playerId];
    if (player === undefined || player.eliminated || ctx.state.phase !== "shop") {
      break;
    }
    const ranked = policy
      .candidates(ctx, player)
      .slice()
      .sort((a, b) => b.score - a.score || (a.label < b.label ? -1 : a.label > b.label ? 1 : 0));
    const best = ranked[0];
    const chosen = best !== undefined && best.score > 0 ? best : null;
    records.push({
      playerId,
      round: ctx.state.round,
      policy: policy.name,
      considered: ranked.slice(0, 5).map((c) => ({ label: c.label, score: c.score })),
      selected: chosen === null ? "pass" : chosen.label,
      score: chosen?.score ?? 0,
      tiebreak: "score_desc_then_label_asc",
    });
    if (chosen === null) {
      break;
    }
    const result = deps.submit(playerId, chosen.command);
    if (!result.ok) {
      break;
    }
  }
  return records;
};
