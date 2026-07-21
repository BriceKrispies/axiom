/*
 * policy.ts — the bot decision contract and the shared reasoning helpers every
 * policy uses. Bots are ordinary actors: they NEVER touch match state, they only
 * enumerate legal commands and submit them through the same authoritative API the
 * human uses. Each decision is explainable — `decide` returns the chosen command
 * plus a `DecisionRecord` listing the considered actions, their scores, the
 * selection, and the deterministic tiebreak — so a replay can show exactly why a
 * bot did what it did. Policies are assigned to bot slots deterministically from
 * the player id.
 */

import type { Command } from "../sim/commands.ts";
import type { CardDefinition } from "../sim/content/schema.ts";
import type { LoadedContent } from "../sim/content/load.ts";
import type { GroupId, PlayerId } from "../sim/ids.ts";
import type { MatchState, PlayerState, UnitInstance } from "../sim/model.ts";
import { WARBAND_SLOTS } from "../sim/model.ts";
import { forgeUpgradeCost } from "../sim/tuning.ts";
import type { Rules } from "../sim/tuning.ts";

export interface Candidate {
  readonly command: Command;
  readonly label: string;
  readonly score: number;
}

export interface DecisionRecord {
  readonly playerId: PlayerId;
  readonly round: number;
  readonly policy: string;
  readonly considered: readonly { readonly label: string; readonly score: number }[];
  readonly selected: string;
  readonly score: number;
  readonly tiebreak: string;
}

export interface BotContext {
  readonly state: MatchState;
  readonly content: LoadedContent;
  readonly rules: Rules;
}

export interface BotPolicy {
  readonly name: string;
  /** Enumerate candidate commands with scores for this player, this step. */
  candidates: (ctx: BotContext, player: PlayerState) => Candidate[];
}

// ── shared board/economy reasoning ──────────────────────────────────────────
export const power = (attack: number, health: number): number => attack + health;

export const unitPower = (u: UnitInstance): number => power(u.attack, u.health);

export const cardPower = (def: CardDefinition): number => power(def.baseAttack, def.baseHealth);

export const firstEmptySlot = (player: PlayerState): number => player.warband.findIndex((u) => u === null);

export const warbandUnits = (player: PlayerState): UnitInstance[] =>
  player.warband.filter((u): u is UnitInstance => u !== null);

export const weakestWarband = (player: PlayerState): UnitInstance | null => {
  const units = warbandUnits(player);
  if (units.length === 0) {
    return null;
  }
  return units.slice().sort((a, b) => unitPower(a) - unitPower(b) || a.instanceId - b.instanceId)[0] as UnitInstance;
};

export const shopCost = (ctx: BotContext, player: PlayerState, index: number): number => {
  const slot = player.shop[index];
  if (slot === undefined) {
    return Infinity;
  }
  return Math.max(0, ctx.content.card(slot.cardId).cost - slot.discount);
};

/** How many of a group the player already commits (warband + hand). */
export const groupCount = (ctx: BotContext, player: PlayerState, group: GroupId): number =>
  [...warbandUnits(player), ...player.hand].filter((u) => ctx.content.card(u.cardId).groups.includes(group)).length;

/** Normal copies of a card the player holds (forge progress). */
export const normalCopies = (player: PlayerState, cardId: string): number =>
  [...warbandUnits(player), ...player.hand].filter((u) => u.cardId === cardId && !u.forged).length;

export const canUpgrade = (ctx: BotContext, player: PlayerState): boolean => {
  const cost = forgeUpgradeCost(ctx.rules, player.forgeRank);
  return cost !== null && player.gold >= cost;
};

export const warbandFull = (player: PlayerState): boolean => firstEmptySlot(player) < 0;

/** Assign a policy to a bot slot deterministically from its id. */
export const policyForPlayer = (policies: readonly BotPolicy[], id: PlayerId): BotPolicy =>
  policies[id % policies.length] as BotPolicy;

export const WARBAND_CAPACITY = WARBAND_SLOTS;
