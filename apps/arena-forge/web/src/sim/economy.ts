/*
 * economy.ts — the authoritative command applier for the shop phase. It is the
 * ONLY writer of player economy state. Every command is validated FULLY before
 * any mutation, so a rejected or cancelled action is perfectly transactional: the
 * player never loses gold or a card to a failed buy, an invalid drop slot, or a
 * cancelled drag. Each accepted command runs the relevant economy effect triggers
 * (`on_buy` / `on_play` / `on_sell`) and then resolves any pending forges; each
 * rejection emits a `command_rejected` event with a stable reason and changes
 * nothing else.
 */

import type { Command, CommandResult, RejectionReason } from "./commands.ts";
import { REJECT } from "./commands.ts";
import type { EconomyEnv } from "./effects/economy-effects.ts";
import { runEconomyTrigger } from "./effects/economy-effects.ts";
import { resolveForges } from "./forge.ts";
import type { InstanceId } from "./ids.ts";
import type { PlayerState, UnitInstance } from "./model.ts";
import { WARBAND_SLOTS } from "./model.ts";
import { removeFromPool, returnCopies, returnShop, rollShop } from "./pool.ts";
import { forgeUpgradeCost } from "./tuning.ts";

/** The shop env adds instance-id allocation to the pure economy effect env. */
export interface ShopEnv extends EconomyEnv {
  readonly allocate: () => InstanceId;
}

interface Found {
  readonly unit: UnitInstance;
  readonly place: "hand" | "warband";
  readonly index: number;
}

const findUnit = (player: PlayerState, id: InstanceId): Found | null => {
  const handIndex = player.hand.findIndex((u) => u.instanceId === id);
  if (handIndex >= 0) {
    return { unit: player.hand[handIndex] as UnitInstance, place: "hand", index: handIndex };
  }
  const bandIndex = player.warband.findIndex((u) => u !== null && u.instanceId === id);
  if (bandIndex >= 0) {
    return { unit: player.warband[bandIndex] as UnitInstance, place: "warband", index: bandIndex };
  }
  return null;
};

const reject = (env: ShopEnv, player: PlayerState, command: Command, reason: RejectionReason): CommandResult => {
  env.events.emit({ kind: "command_rejected", playerId: player.id, reason, command: command.type });
  return { ok: false, reason };
};

const buy = (env: ShopEnv, player: PlayerState, command: Command & { type: "buy" }): CommandResult => {
  const slot = player.shop[command.shopIndex];
  if (command.shopIndex < 0 || slot === undefined) {
    return reject(env, player, command, REJECT.badShopIndex);
  }
  const def = env.content.card(slot.cardId);
  const cost = Math.max(0, def.cost - slot.discount);
  if (player.gold < cost) {
    return reject(env, player, command, REJECT.notEnoughGold);
  }
  const dest = command.destination;
  if (dest.to === "hand" && player.hand.length >= env.rules.handLimit) {
    return reject(env, player, command, REJECT.handFull);
  }
  if (dest.to === "warband" && (dest.slot < 0 || dest.slot >= WARBAND_SLOTS)) {
    return reject(env, player, command, REJECT.badSlot);
  }
  if (dest.to === "warband" && player.warband[dest.slot] !== null) {
    return reject(env, player, command, REJECT.slotOccupied);
  }

  // Validated — now mutate.
  player.gold -= cost;
  player.shop = player.shop.filter((s) => s.instanceId !== slot.instanceId);
  const unit: UnitInstance = {
    instanceId: env.allocate(),
    cardId: def.id,
    forged: false,
    attack: def.baseAttack,
    health: def.baseHealth,
    grantedKeywords: [],
    visualStage: 0,
  };
  env.events.emit({ kind: "card_purchased", playerId: player.id, cardId: def.id, instanceId: unit.instanceId, cost });
  if (dest.to === "hand") {
    player.hand.push(unit);
    runEconomyTrigger(env, player, unit, "on_buy");
  } else {
    player.warband[dest.slot] = unit;
    runEconomyTrigger(env, player, unit, "on_buy");
    runEconomyTrigger(env, player, unit, "on_play");
    env.events.emit({ kind: "card_played", playerId: player.id, instanceId: unit.instanceId, slot: dest.slot });
  }
  resolveForges(env, player);
  return { ok: true };
};

const sell = (env: ShopEnv, player: PlayerState, command: Command & { type: "sell" }): CommandResult => {
  const found = findUnit(player, command.instanceId);
  if (found === null) {
    return reject(env, player, command, REJECT.unknownInstance);
  }
  runEconomyTrigger(env, player, found.unit, "on_sell");
  if (found.place === "hand") {
    player.hand = player.hand.filter((u) => u.instanceId !== command.instanceId);
  } else {
    player.warband[found.index] = null;
  }
  returnCopies(env.state.pool, found.unit.cardId, found.unit.forged ? env.rules.copiesToForge : 1);
  const before = player.gold;
  player.gold = Math.min(env.rules.maxGold, player.gold + env.rules.sellValue);
  env.events.emit({
    kind: "card_sold",
    playerId: player.id,
    cardId: found.unit.cardId,
    instanceId: command.instanceId,
    refund: player.gold - before,
  });
  return { ok: true };
};

const reroll = (env: ShopEnv, player: PlayerState, command: Command): CommandResult => {
  if (player.gold < env.rules.rerollCost) {
    return reject(env, player, command, REJECT.notEnoughGold);
  }
  player.gold -= env.rules.rerollCost;
  returnShop(env.state.pool, player.shop);
  player.shop = rollShop(env.state.pool, env.content, env.rules, player.forgeRank, env.rng, env.allocate);
  player.shopFrozen = false;
  env.events.emit({ kind: "shop_rerolled", playerId: player.id, cost: env.rules.rerollCost });
  return { ok: true };
};

const setFreeze = (env: ShopEnv, player: PlayerState, command: Command & { type: "set_freeze" }): CommandResult => {
  if (player.shopFrozen === command.frozen) {
    return reject(env, player, command, REJECT.noChange);
  }
  player.shopFrozen = command.frozen;
  env.events.emit({ kind: "shop_freeze_changed", playerId: player.id, frozen: command.frozen });
  return { ok: true };
};

const upgradeForge = (env: ShopEnv, player: PlayerState, command: Command): CommandResult => {
  const cost = forgeUpgradeCost(env.rules, player.forgeRank);
  if (cost === null) {
    return reject(env, player, command, REJECT.maxForgeRank);
  }
  if (player.gold < cost) {
    return reject(env, player, command, REJECT.notEnoughGold);
  }
  player.gold -= cost;
  player.forgeRank += 1;
  env.events.emit({ kind: "forge_rank_increased", playerId: player.id, rank: player.forgeRank, cost });
  return { ok: true };
};

const playCard = (env: ShopEnv, player: PlayerState, command: Command & { type: "play_card" }): CommandResult => {
  const handIndex = player.hand.findIndex((u) => u.instanceId === command.instanceId);
  if (handIndex < 0) {
    return reject(env, player, command, REJECT.notInHand);
  }
  if (command.slot < 0 || command.slot >= WARBAND_SLOTS) {
    return reject(env, player, command, REJECT.badSlot);
  }
  if (player.warband[command.slot] !== null) {
    return reject(env, player, command, REJECT.slotOccupied);
  }
  const unit = player.hand[handIndex] as UnitInstance;
  player.hand = player.hand.filter((u) => u.instanceId !== command.instanceId);
  player.warband[command.slot] = unit;
  env.events.emit({ kind: "card_played", playerId: player.id, instanceId: unit.instanceId, slot: command.slot });
  runEconomyTrigger(env, player, unit, "on_play");
  resolveForges(env, player);
  return { ok: true };
};

const returnToHand = (env: ShopEnv, player: PlayerState, command: Command & { type: "return_to_hand" }): CommandResult => {
  const bandIndex = player.warband.findIndex((u) => u !== null && u.instanceId === command.instanceId);
  if (bandIndex < 0) {
    return reject(env, player, command, REJECT.notInWarband);
  }
  if (player.hand.length >= env.rules.handLimit) {
    return reject(env, player, command, REJECT.handFull);
  }
  const unit = player.warband[bandIndex] as UnitInstance;
  player.warband[bandIndex] = null;
  player.hand.push(unit);
  env.events.emit({ kind: "unit_reordered", playerId: player.id, instanceId: unit.instanceId, from: bandIndex, to: -1 });
  return { ok: true };
};

const reorder = (env: ShopEnv, player: PlayerState, command: Command & { type: "reorder" }): CommandResult => {
  const bandIndex = player.warband.findIndex((u) => u !== null && u.instanceId === command.instanceId);
  if (bandIndex < 0) {
    return reject(env, player, command, REJECT.notInWarband);
  }
  if (command.slot < 0 || command.slot >= WARBAND_SLOTS) {
    return reject(env, player, command, REJECT.badSlot);
  }
  if (command.slot === bandIndex) {
    return reject(env, player, command, REJECT.noChange);
  }
  // Drag reorder: swap with whatever occupies the destination (possibly null).
  const moving = player.warband[bandIndex] as UnitInstance;
  player.warband[bandIndex] = player.warband[command.slot] ?? null;
  player.warband[command.slot] = moving;
  env.events.emit({ kind: "unit_reordered", playerId: player.id, instanceId: moving.instanceId, from: bandIndex, to: command.slot });
  return { ok: true };
};

/**
 * Apply one already-authenticated command to a player during the shop phase.
 * Phase and liveness are checked by the caller (the phase machine / match); this
 * validates the command's own preconditions and mutates on success only.
 */
export const applyShopCommand = (env: ShopEnv, player: PlayerState, command: Command): CommandResult => {
  switch (command.type) {
    case "buy":
      return buy(env, player, command);
    case "sell":
      return sell(env, player, command);
    case "reroll":
      return reroll(env, player, command);
    case "set_freeze":
      return setFreeze(env, player, command);
    case "upgrade_forge_rank":
      return upgradeForge(env, player, command);
    case "play_card":
      return playCard(env, player, command);
    case "return_to_hand":
      return returnToHand(env, player, command);
    case "reorder":
      return reorder(env, player, command);
    default:
      return { ok: false, reason: REJECT.noChange };
  }
};
