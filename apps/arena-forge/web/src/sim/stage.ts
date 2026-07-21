/*
 * stage.ts — derives a player's arena presentation stage from authoritative match
 * state. The stage (`workshop` → `kindled` → `tempered` → `masterwork`) is a pure
 * function of forge rank, number of forged units, and warband power, against the
 * data-defined thresholds in the rules. It is simulation-owned (so it replays
 * deterministically and drives a `stage_changed` event), but it carries no visual
 * data itself — the presentation layer maps the stage id to actual machinery,
 * lighting, and frames.
 */

import type { LoadedContent } from "./content/load.ts";
import type { ArenaStage, PlayerState, UnitInstance } from "./model.ts";
import type { Rules } from "./tuning.ts";

/** Total attack + health across a player's warband — a stage input and a
 * pairing tiebreak. */
export const warbandPower = (player: PlayerState): number =>
  player.warband.reduce((sum, u) => sum + (u === null ? 0 : u.attack + u.health), 0);

export const forgedCount = (player: PlayerState): number =>
  player.warband.filter((u): u is UnitInstance => u !== null && u.forged).length;

/** The highest stage whose thresholds the player meets (thresholds are ordered
 * strongest-first in the rules, so the first match wins). */
export const computeStage = (rules: Rules, _content: LoadedContent, player: PlayerState): ArenaStage => {
  const power = warbandPower(player);
  const forged = forgedCount(player);
  for (const t of rules.stageThresholds) {
    if (player.forgeRank >= t.rank && forged >= t.forged && power >= t.power) {
      return t.stage;
    }
  }
  return "workshop";
};
