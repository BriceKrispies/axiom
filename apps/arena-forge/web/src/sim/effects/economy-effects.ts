/*
 * economy-effects.ts — the effect interpreter for the ECONOMY context (the shop
 * phase). It runs a source unit's abilities for a given economy trigger
 * (`on_buy`, `on_play`, `on_sell`, `shop_start`, `round_end`) against the owning
 * player's warband, hand, gold, and shop. It is deterministic: every condition
 * reads explicit state, every random selector draws from the match `Rng` in
 * stable slot order. Combat-only operations can never appear here — content
 * validation rejects them for an economy trigger — so this interpreter implements
 * exactly the economy-legal verbs and no others.
 *
 * There is NO card-specific code: the interpreter switches on operation/selector/
 * condition KIND (generic mechanism); a new card only adds more ability data.
 */

import type { Ability, Condition, Operation, Selector, Trigger } from "./language.ts";
import { EFFECT_BOUNDS } from "./language.ts";
import type { LoadedContent } from "../content/load.ts";
import type { EventSink } from "../events.ts";
import type { GroupId, KeywordId } from "../ids.ts";
import type { MatchState, PlayerState, UnitInstance } from "../model.ts";
import type { Rng } from "../rng.ts";
import { goldForRound } from "../tuning.ts";
import type { Rules } from "../tuning.ts";

export interface EconomyEnv {
  readonly rules: Rules;
  readonly content: LoadedContent;
  readonly rng: Rng;
  readonly events: EventSink;
  readonly state: MatchState;
}

const living = (player: PlayerState): UnitInstance[] => player.warband.filter((u): u is UnitInstance => u !== null);

const unitKeywords = (content: LoadedContent, unit: UnitInstance): KeywordId[] => [
  ...content.card(unit.cardId).keywords,
  ...unit.grantedKeywords,
];

const inGroup = (content: LoadedContent, unit: UnitInstance, group: GroupId): boolean =>
  content.card(unit.cardId).groups.includes(group);

const sourceSlot = (player: PlayerState, source: UnitInstance): number =>
  player.warband.findIndex((u) => u === source);

const neighbors = (player: PlayerState, slot: number): UnitInstance[] =>
  slot < 0 ? [] : [player.warband[slot - 1], player.warband[slot + 1]].filter((u): u is UnitInstance => u != null);

const conditionHolds = (env: EconomyEnv, player: PlayerState, source: UnitInstance, cond: Condition): boolean => {
  const slot = sourceSlot(player, source);
  const band = living(player);
  switch (cond.kind) {
    case "source_is_forged":
      return source.forged;
    case "source_is_normal":
      return !source.forged;
    case "source_attack_at_least":
      return source.attack >= cond.value;
    case "source_health_at_least":
      return source.health >= cond.value;
    case "source_in_group":
      return inGroup(env.content, source, cond.group);
    case "source_has_keyword":
      return unitKeywords(env.content, source).includes(cond.keyword);
    case "source_position_leftmost":
      return slot >= 0 && band[0] === source;
    case "source_position_rightmost":
      return slot >= 0 && band[band.length - 1] === source;
    case "adjacent_in_group":
      return neighbors(player, slot).some((u) => inGroup(env.content, u, cond.group));
    case "round_at_least":
      return env.state.round >= cond.value;
    case "friendly_group_count_at_least":
      return band.filter((u) => inGroup(env.content, u, cond.group)).length >= cond.value;
    case "empty_warband_slots_at_least":
      return player.warband.filter((u) => u === null).length >= cond.value;
    default:
      return false;
  }
};

/** Resolve a selector to the affected units within the player's own board. Enemy
 * and combat-only selectors resolve to empty in the economy context. */
const resolve = (env: EconomyEnv, player: PlayerState, source: UnitInstance, sel: Selector): UnitInstance[] => {
  const band = living(player);
  const slot = sourceSlot(player, source);
  switch (sel.kind) {
    case "self":
      return [source];
    case "adjacent_friendly":
      return neighbors(player, slot);
    case "leftmost_friendly":
      return band.length > 0 ? [band[0] as UnitInstance] : [];
    case "rightmost_friendly":
      return band.length > 0 ? [band[band.length - 1] as UnitInstance] : [];
    case "random_friendly": {
      const pick = env.rng.pick(band);
      return pick ? [pick] : [];
    }
    case "lowest_attack_friendly":
      return band.length > 0 ? [band.slice().sort((a, b) => a.attack - b.attack || a.instanceId - b.instanceId)[0] as UnitInstance] : [];
    case "all_friendly":
      return band;
    case "friendly_in_group":
      return band.filter((u) => inGroup(env.content, u, sel.group));
    default:
      // attacker, defender, *_enemy, empty_friendly_slot — no economy meaning.
      return [];
  }
};

const applyOperation = (env: EconomyEnv, player: PlayerState, source: UnitInstance, op: Operation): void => {
  switch (op.kind) {
    case "modify_attack":
      for (const u of resolve(env, player, source, op.target)) {
        u.attack = Math.max(0, u.attack + op.amount);
      }
      return;
    case "modify_health":
      for (const u of resolve(env, player, source, op.target)) {
        u.health = Math.max(1, u.health + op.amount);
      }
      return;
    case "grant_keyword":
      for (const u of resolve(env, player, source, op.target)) {
        if (!u.grantedKeywords.includes(op.keyword)) {
          u.grantedKeywords.push(op.keyword);
        }
      }
      return;
    case "remove_keyword":
      for (const u of resolve(env, player, source, op.target)) {
        u.grantedKeywords = u.grantedKeywords.filter((k) => k !== op.keyword);
      }
      return;
    case "add_gold": {
      const before = player.gold;
      player.gold = Math.min(env.rules.maxGold, player.gold + op.amount);
      if (player.gold !== before) {
        env.events.emit({ kind: "gold_gained", playerId: player.id, amount: player.gold - before });
      }
      return;
    }
    case "discount_shop":
      for (const s of player.shop) {
        s.discount = Math.min(env.content.card(s.cardId).cost, s.discount + op.amount);
      }
      env.events.emit({ kind: "shop_discounted", playerId: player.id, amount: op.amount });
      return;
    case "transform_card":
      for (const u of resolve(env, player, source, op.target)) {
        const def = env.content.card(op.into);
        u.forged = false;
        u.attack = def.baseAttack;
        u.health = def.baseHealth;
        u.grantedKeywords = [];
        u.visualStage = 0;
        (u as { cardId: string }).cardId = op.into;
      }
      return;
    case "repeat": {
      const times = Math.min(op.times, EFFECT_BOUNDS.maxRepeat);
      for (let i = 0; i < times; i += 1) {
        applyOperation(env, player, source, op.op);
      }
      return;
    }
    default:
      // emit_cue and any combat-only op are cosmetic / disallowed here — no-op.
      return;
  }
};

const runAbility = (env: EconomyEnv, player: PlayerState, source: UnitInstance, ability: Ability): void => {
  const conditions = ability.conditions ?? [];
  if (!conditions.every((c) => conditionHolds(env, player, source, c))) {
    return;
  }
  for (const op of ability.operations) {
    applyOperation(env, player, source, op);
  }
};

/**
 * Fire one economy trigger for a single source unit: run every ability of the
 * unit's active profile whose trigger matches and whose conditions all hold, in
 * authored order.
 */
export const runEconomyTrigger = (env: EconomyEnv, player: PlayerState, source: UnitInstance, trigger: Trigger): void => {
  const def = env.content.card(source.cardId);
  const abilities = source.forged ? def.forged : def.normal;
  for (const ability of abilities) {
    if (ability.trigger === trigger) {
      runAbility(env, player, source, ability);
    }
  }
};

/** Fire a board-wide economy trigger (`shop_start` / `round_end`) for every
 * warband unit in stable slot order. */
export const runBoardEconomyTrigger = (env: EconomyEnv, player: PlayerState, trigger: Trigger): void => {
  for (const unit of living(player)) {
    runEconomyTrigger(env, player, unit, trigger);
  }
};

/** The passive per-round gold grant (data-driven), clamped to the max. */
export const grantRoundGold = (env: EconomyEnv, player: PlayerState): void => {
  const before = player.gold;
  player.gold = Math.min(env.rules.maxGold, player.gold + goldForRound(env.rules, env.state.round));
  if (player.gold !== before) {
    env.events.emit({ kind: "gold_gained", playerId: player.id, amount: player.gold - before });
  }
};
