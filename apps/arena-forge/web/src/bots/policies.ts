/*
 * policies.ts — the three deterministic bot personalities. Each is a `BotPolicy`
 * that enumerates scored candidate commands for the current state; the driver
 * plays the highest score (ties broken by label). All three build ONLY legal,
 * affordable commands, so every submission is accepted and the decision log stays
 * clean. They pursue genuinely different strategies — banking toward forge rank,
 * committing to one group's synergy + forging, or maximizing immediate board
 * power — so a lobby of bots produces varied, non-identical games.
 */

import type { Command } from "../sim/commands.ts";
import type { CardDefinition } from "../sim/content/schema.ts";
import type { GroupId } from "../sim/ids.ts";
import type { PlayerState } from "../sim/model.ts";
import { forgeUpgradeCost } from "../sim/tuning.ts";
import type { BotContext, BotPolicy, Candidate } from "./policy.ts";
import {
  canUpgrade,
  cardPower,
  firstEmptySlot,
  groupCount,
  normalCopies,
  shopCost,
  unitPower,
  warbandFull,
  warbandUnits,
  weakestWarband,
} from "./policy.ts";

const buyDest = (player: PlayerState): { to: "warband"; slot: number } | { to: "hand" } | null => {
  const slot = firstEmptySlot(player);
  if (slot >= 0) {
    return { to: "warband", slot };
  }
  if (player.hand.length < 6) {
    return { to: "hand" };
  }
  return null;
};

const buyCandidates = (ctx: BotContext, player: PlayerState, score: (def: CardDefinition) => number): Candidate[] => {
  const dest = buyDest(player);
  if (dest === null) {
    return [];
  }
  return player.shop
    .map((slot, index) => ({ slot, index }))
    .filter(({ index }) => shopCost(ctx, player, index) <= player.gold)
    .map(({ slot, index }): Candidate => {
      const def = ctx.content.card(slot.cardId);
      return { command: { type: "buy", shopIndex: index, destination: dest }, label: `buy:${def.id}`, score: score(def) };
    });
};

const playCandidates = (player: PlayerState, bonus: number): Candidate[] => {
  const slot = firstEmptySlot(player);
  if (slot < 0) {
    return [];
  }
  return player.hand.map((u): Candidate => ({
    command: { type: "play_card", instanceId: u.instanceId, slot },
    label: `play:${u.instanceId}`,
    score: 45 + unitPower(u) + bonus,
  }));
};

const upgradeCandidate = (ctx: BotContext, player: PlayerState, score: number): Candidate[] =>
  canUpgrade(ctx, player) ? [{ command: { type: "upgrade_forge_rank" }, label: "upgrade", score }] : [];

// ── economy: bank toward forge rank, fill the board cheaply ──────────────────
export const ECONOMY_BOT: BotPolicy = {
  name: "economy",
  candidates: (ctx, player) => {
    const upgradeCost = forgeUpgradeCost(ctx.rules, player.forgeRank) ?? Infinity;
    const surplus = player.gold - upgradeCost;
    return [
      ...playCandidates(player, 0),
      ...upgradeCandidate(ctx, player, ctx.state.round >= 2 && surplus >= 0 ? 90 : -1),
      ...buyCandidates(ctx, player, (def) => (player.gold >= upgradeCost + def.cost ? 30 + cardPower(def) : firstEmptySlot(player) >= 0 && warbandUnits(player).length < 2 ? 20 + cardPower(def) : -1)),
    ];
  },
};

const pickTargetGroup = (ctx: BotContext, player: PlayerState): GroupId => {
  const counts = ctx.content.groups
    .map((g) => ({ id: g.id, n: groupCount(ctx, player, g.id) }))
    .sort((a, b) => b.n - a.n || (a.id < b.id ? -1 : 1));
  const top = counts[0];
  if (top && top.n > 0) {
    return top.id;
  }
  // No commitment yet: pick deterministically by player id.
  const groups = ctx.content.groups;
  return (groups[player.id % groups.length] as { id: GroupId }).id;
};

// ── synergy: commit to one group, chase copies to forge ──────────────────────
export const SYNERGY_BOT: BotPolicy = {
  name: "synergy",
  candidates: (ctx, player) => {
    const target = pickTargetGroup(ctx, player);
    const rerollWorth = player.gold >= ctx.rules.rerollCost + 3;
    const hasTargetInShop = player.shop.some((s) => ctx.content.card(s.cardId).groups.includes(target));
    return [
      ...playCandidates(player, 10),
      ...buyCandidates(ctx, player, (def) => {
        const inGroup = def.groups.includes(target);
        const nearForge = normalCopies(player, def.id) === ctx.rules.copiesToForge - 1;
        return (inGroup ? 60 : 15) + (nearForge ? 40 : 0) + cardPower(def);
      }),
      ...upgradeCandidate(ctx, player, player.gold >= (forgeUpgradeCost(ctx.rules, player.forgeRank) ?? Infinity) + 4 ? 35 : -1),
      ...(rerollWorth && !hasTargetInShop && !warbandFull(player)
        ? [{ command: { type: "reroll" } as Command, label: "reroll", score: 25 }]
        : []),
    ];
  },
};

// ── tempo: maximize immediate board power ────────────────────────────────────
export const TEMPO_BOT: BotPolicy = {
  name: "tempo",
  candidates: (ctx, player) => {
    const weakest = weakestWarband(player);
    const sellUpgrades: Candidate[] = [];
    if (warbandFull(player) && weakest) {
      for (let i = 0; i < player.shop.length; i += 1) {
        const def = ctx.content.card((player.shop[i] as { cardId: string }).cardId);
        if (shopCost(ctx, player, i) <= player.gold && cardPower(def) > unitPower(weakest) + 2) {
          sellUpgrades.push({ command: { type: "sell", instanceId: weakest.instanceId }, label: `sell:${weakest.instanceId}`, score: 55 });
          break;
        }
      }
    }
    return [
      ...playCandidates(player, 20),
      ...buyCandidates(ctx, player, (def) => 40 + cardPower(def) * 2),
      ...sellUpgrades,
      ...upgradeCandidate(ctx, player, player.gold >= (forgeUpgradeCost(ctx.rules, player.forgeRank) ?? Infinity) + 6 ? 20 : -1),
    ];
  },
};

export const DEFAULT_POLICIES: readonly BotPolicy[] = [ECONOMY_BOT, SYNERGY_BOT, TEMPO_BOT];
