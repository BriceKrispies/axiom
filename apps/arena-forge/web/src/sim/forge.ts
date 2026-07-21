/*
 * forge.ts — Arena Forge's original upgrade mechanic. Whenever a player holds
 * three NORMAL copies of one card across hand + warband, they deterministically
 * combine into a single forged unit: remove exactly three normal copies, place
 * one forged instance at a deterministic destination, apply the card's forged
 * stat rule, switch it to its forged ability profile and forged visual stage,
 * and grant the data-driven forge reward. The rule is entirely data-driven — the
 * engine has NO card-specific forging code; a card only supplies its
 * `forgedStats`, `forged` abilities, forged visual profile, and optional reward.
 */

import { EFFECT_BOUNDS } from "./effects/language.ts";
import type { LoadedContent } from "./content/load.ts";
import type { ForgeReward } from "./content/schema.ts";
import type { EventSink } from "./events.ts";
import type { CardId, InstanceId } from "./ids.ts";
import type { PlayerState, UnitInstance } from "./model.ts";
import type { Rules } from "./tuning.ts";

/** The environment forging needs — structurally satisfied by the shop env. */
export interface ForgeEnv {
  readonly rules: Rules;
  readonly content: LoadedContent;
  readonly events: EventSink;
  readonly allocate: () => InstanceId;
}

interface Located {
  readonly unit: UnitInstance;
  readonly place: "warband" | "hand";
  readonly index: number;
}

/** All NORMAL copies of a card across warband (slot order) then hand (index
 * order) — a stable ordering that fixes which three combine and where they go. */
const normalCopies = (player: PlayerState, cardId: CardId): Located[] => {
  const out: Located[] = [];
  player.warband.forEach((u, index) => {
    if (u !== null && u.cardId === cardId && !u.forged) {
      out.push({ unit: u, place: "warband", index });
    }
  });
  player.hand.forEach((u, index) => {
    if (u.cardId === cardId && !u.forged) {
      out.push({ unit: u, place: "hand", index });
    }
  });
  return out;
};

const applyForgeReward = (env: ForgeEnv, player: PlayerState, reward: ForgeReward): void => {
  env.events.emit({ kind: "forge_reward_granted", playerId: player.id, reward });
  if (reward.kind === "gold") {
    const before = player.gold;
    player.gold = Math.min(env.rules.maxGold, player.gold + reward.amount);
    if (player.gold !== before) {
      env.events.emit({ kind: "gold_gained", playerId: player.id, amount: player.gold - before });
    }
  } else {
    for (const slot of player.shop) {
      slot.discount = Math.min(env.content.card(slot.cardId).cost, slot.discount + reward.amount);
    }
    env.events.emit({ kind: "shop_discounted", playerId: player.id, amount: reward.amount });
  }
};

/** Attempt exactly one forge; returns true if one happened. */
const forgeOnce = (env: ForgeEnv, player: PlayerState): boolean => {
  // Canonical card order makes "which card forges first" deterministic.
  for (const def of env.content.collectibleCards) {
    const copies = normalCopies(player, def.id);
    if (copies.length < env.rules.copiesToForge) {
      continue;
    }
    const chosen = copies.slice(0, env.rules.copiesToForge);
    const destination = chosen[0] as Located;

    // Remove the three copies from their locations.
    const removeIds = new Set(chosen.map((c) => c.unit.instanceId));
    player.warband = player.warband.map((u) => (u !== null && removeIds.has(u.instanceId) ? null : u));
    player.hand = player.hand.filter((u) => !removeIds.has(u.instanceId));

    const forged: UnitInstance = {
      instanceId: env.allocate(),
      cardId: def.id,
      forged: true,
      attack: def.baseAttack + def.forgedStats.attack,
      health: def.baseHealth + def.forgedStats.health,
      grantedKeywords: [],
      visualStage: 1,
    };

    // Deterministic destination: the leftmost consumed copy's warband slot, else
    // the first empty warband slot, else the hand.
    const slot =
      destination.place === "warband"
        ? destination.index
        : player.warband.findIndex((u) => u === null);
    if (slot >= 0 && slot < player.warband.length) {
      player.warband[slot] = forged;
    } else {
      player.hand.push(forged);
    }

    env.events.emit({
      kind: "unit_forged",
      playerId: player.id,
      cardId: def.id,
      instanceId: forged.instanceId,
      slot: slot >= 0 ? slot : -1,
    });
    applyForgeReward(env, player, def.forgeReward ?? env.rules.defaultForgeReward);
    return true;
  }
  return false;
};

/** Resolve all pending forges for a player (a forge can enable another; bounded). */
export const resolveForges = (env: ForgeEnv, player: PlayerState): void => {
  let guard = 0;
  const limit = EFFECT_BOUNDS.maxSummonsPerCombat; // a generous, finite backstop
  while (guard < limit && forgeOnce(env, player)) {
    guard += 1;
  }
};
