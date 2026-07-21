/*
 * combat-effects.ts — the effect interpreter for the COMBAT context. It resolves
 * an ability's selectors and conditions against the live `Board` and applies its
 * operations (damage, heal, summon, move, swap, copy, stat/keyword changes). It
 * is deterministic: conditions read explicit board state, random selectors draw
 * from the combat's single seeded `Rng` in stable slot order, and every effect
 * emits typed events through the bounded sink. Death resolution (deathrattles)
 * runs to a fixed point after each ability so "death effects resolve before the
 * next attack" holds, and every unbounded shape (summons, copies, events) is
 * capped by `CombatCounters`.
 */

import type { Ability, Condition, Operation, Selector, Trigger } from "../effects/language.ts";
import { EFFECT_BOUNDS } from "../effects/language.ts";
import type { CombatSide } from "../events.ts";
import type { GroupId } from "../ids.ts";
import type { Board, CombatUnit } from "./board.ts";
import { ARMORED, buildSideUnit, living, nearestEmptySlot, other, sideOf, unitKeywords } from "./board.ts";
import type { CombatEnv } from "./env.ts";
import { cemit, terminate } from "./env.ts";

export interface Focus {
  readonly attacker: CombatUnit | null;
  readonly defender: CombatUnit | null;
}

export const NO_FOCUS: Focus = { attacker: null, defender: null };

const inGroup = (env: CombatEnv, unit: CombatUnit, group: GroupId): boolean =>
  env.content.card(unit.cardId).groups.includes(group);

const neighbors = (board: Board, unit: CombatUnit): CombatUnit[] => {
  const arr = sideOf(board, unit.side);
  return [arr[unit.slot - 1], arr[unit.slot + 1]].filter((u): u is CombatUnit => u != null && u.alive);
};

export const conditionHolds = (env: CombatEnv, board: Board, source: CombatUnit, cond: Condition): boolean => {
  const friends = living(board, source.side);
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
      return inGroup(env, source, cond.group);
    case "source_has_keyword":
      return unitKeywords(env.content, source).includes(cond.keyword);
    case "source_position_leftmost":
      return friends[0] === source;
    case "source_position_rightmost":
      return friends[friends.length - 1] === source;
    case "adjacent_in_group":
      return neighbors(board, source).some((u) => inGroup(env, u, cond.group));
    case "round_at_least":
      return true; // round is not a combat input; treated as satisfied in combat.
    case "friendly_group_count_at_least":
      return friends.filter((u) => inGroup(env, u, cond.group)).length >= cond.value;
    case "empty_warband_slots_at_least":
      return sideOf(board, source.side).filter((u) => u === null).length >= cond.value;
    default:
      return false;
  }
};

export const resolveSelector = (env: CombatEnv, board: Board, source: CombatUnit, focus: Focus, sel: Selector): CombatUnit[] => {
  const friends = living(board, source.side);
  const enemies = living(board, other(source.side));
  const byAttackAsc = (list: CombatUnit[]): CombatUnit[] => list.slice().sort((x, y) => x.attack - y.attack || x.instanceId - y.instanceId);
  switch (sel.kind) {
    case "self":
      return source.alive ? [source] : [];
    case "attacker":
      return focus.attacker && focus.attacker.alive ? [focus.attacker] : [];
    case "defender":
      return focus.defender && focus.defender.alive ? [focus.defender] : [];
    case "adjacent_friendly":
      return neighbors(board, source);
    case "leftmost_friendly":
      return friends.length > 0 ? [friends[0] as CombatUnit] : [];
    case "rightmost_friendly":
      return friends.length > 0 ? [friends[friends.length - 1] as CombatUnit] : [];
    case "random_friendly": {
      const pick = env.rng.pick(friends);
      return pick ? [pick] : [];
    }
    case "random_enemy": {
      const pick = env.rng.pick(enemies);
      return pick ? [pick] : [];
    }
    case "lowest_attack_friendly":
      return friends.length > 0 ? [byAttackAsc(friends)[0] as CombatUnit] : [];
    case "highest_attack_enemy":
      return enemies.length > 0 ? [byAttackAsc(enemies)[enemies.length - 1] as CombatUnit] : [];
    case "all_friendly":
      return friends;
    case "all_enemy":
      return enemies;
    case "friendly_in_group":
      return friends.filter((u) => inGroup(env, u, sel.group));
    default:
      return []; // empty_friendly_slot resolves to no unit (summon handles slots)
  }
};

/** Apply raw damage to a unit (armor-mitigated), emit, and leave death to the
 * fixed-point resolver. Returns the damage actually dealt. */
export const applyDamage = (env: CombatEnv, board: Board, target: CombatUnit, amount: number): number => {
  const mitigated = unitKeywords(env.content, target).includes(ARMORED) ? 1 : 0;
  const dealt = Math.max(0, amount - mitigated);
  target.health -= dealt;
  cemit(env, { kind: "unit_damaged", combatId: env.combatId, side: target.side, unitId: target.instanceId, amount: dealt, health: target.health });
  return dealt;
};

const summon = (env: CombatEnv, board: Board, side: CombatSide, tokenCardId: string, anchor: number): void => {
  env.counters.summons += 1;
  if (env.counters.summons > EFFECT_BOUNDS.maxSummonsPerCombat) {
    terminate(env, "summon_bound");
    return;
  }
  const slot = nearestEmptySlot(board, side, anchor);
  if (slot < 0) {
    return;
  }
  const unit = buildSideUnit(env.content, env.allocate, tokenCardId, side, slot);
  sideOf(board, side)[slot] = unit;
  cemit(env, { kind: "unit_summoned", combatId: env.combatId, side, unitId: unit.instanceId, cardId: tokenCardId, slot });
  // Fire summon-reaction triggers (friendly first, then enemy), in slot order.
  for (const u of living(board, side)) {
    if (u !== unit) {
      runTrigger(env, board, u, "on_friendly_summon", NO_FOCUS);
    }
  }
  for (const u of living(board, other(side))) {
    runTrigger(env, board, u, "on_enemy_summon", NO_FOCUS);
  }
};

const moveToEnd = (board: Board, unit: CombatUnit, to: "leftmost" | "rightmost"): number => {
  const arr = sideOf(board, unit.side);
  const order = to === "leftmost" ? [...arr.keys()] : [...arr.keys()].reverse();
  for (const i of order) {
    if (arr[i] === null) {
      arr[unit.slot] = null;
      arr[i] = unit;
      unit.slot = i;
      return i;
    }
  }
  return unit.slot;
};

export const applyOperation = (env: CombatEnv, board: Board, source: CombatUnit, focus: Focus, op: Operation, depth: number): void => {
  if (env.counters.terminated) {
    return;
  }
  switch (op.kind) {
    case "modify_attack":
      for (const u of resolveSelector(env, board, source, focus, op.target)) {
        u.attack = Math.max(0, u.attack + op.amount);
        cemit(env, { kind: "unit_stat_changed", combatId: env.combatId, side: u.side, unitId: u.instanceId, attack: u.attack, health: u.health });
      }
      return;
    case "modify_health":
      for (const u of resolveSelector(env, board, source, focus, op.target)) {
        u.health += op.amount;
        cemit(env, { kind: "unit_stat_changed", combatId: env.combatId, side: u.side, unitId: u.instanceId, attack: u.attack, health: u.health });
      }
      return;
    case "deal_damage":
      for (const u of resolveSelector(env, board, source, focus, op.target)) {
        applyDamage(env, board, u, op.amount);
      }
      return;
    case "heal":
      for (const u of resolveSelector(env, board, source, focus, op.target)) {
        u.health += op.amount;
        cemit(env, { kind: "unit_healed", combatId: env.combatId, side: u.side, unitId: u.instanceId, amount: op.amount, health: u.health });
      }
      return;
    case "grant_keyword":
      for (const u of resolveSelector(env, board, source, focus, op.target)) {
        if (!u.keywords.includes(op.keyword)) {
          u.keywords.push(op.keyword);
          cemit(env, { kind: "unit_keyword_changed", combatId: env.combatId, side: u.side, unitId: u.instanceId, keyword: op.keyword, granted: true });
        }
      }
      return;
    case "remove_keyword":
      for (const u of resolveSelector(env, board, source, focus, op.target)) {
        u.keywords = u.keywords.filter((k) => k !== op.keyword);
        cemit(env, { kind: "unit_keyword_changed", combatId: env.combatId, side: u.side, unitId: u.instanceId, keyword: op.keyword, granted: false });
      }
      return;
    case "summon_token": {
      const at = resolveSelector(env, board, source, focus, op.at);
      const anchor = at.length > 0 ? (at[0] as CombatUnit).slot : source.slot;
      const count = Math.min(op.count, EFFECT_BOUNDS.maxSummonPerOperation);
      for (let i = 0; i < count; i += 1) {
        summon(env, board, source.side, op.token, anchor);
      }
      return;
    }
    case "move_unit":
      for (const u of resolveSelector(env, board, source, focus, op.target)) {
        const to = moveToEnd(board, u, op.to);
        cemit(env, { kind: "unit_moved", combatId: env.combatId, side: u.side, unitId: u.instanceId, from: u.slot, to });
      }
      return;
    case "swap_with": {
      const a = resolveSelector(env, board, source, focus, op.target)[0];
      const b = resolveSelector(env, board, source, focus, op.other)[0];
      if (a && b && a !== b && a.side === b.side) {
        const arr = sideOf(board, a.side);
        arr[a.slot] = b;
        arr[b.slot] = a;
        const as = a.slot;
        a.slot = b.slot;
        b.slot = as;
        cemit(env, { kind: "unit_moved", combatId: env.combatId, side: a.side, unitId: a.instanceId, from: b.slot, to: a.slot });
      }
      return;
    }
    case "copy_ability": {
      env.counters.copies += 1;
      if (env.counters.copies > EFFECT_BOUNDS.maxCopiedAbilities) {
        terminate(env, "copy_bound");
        return;
      }
      const donor = resolveSelector(env, board, source, focus, op.from)[0];
      if (donor) {
        const abilities = donor.forged ? env.content.card(donor.cardId).forged : env.content.card(donor.cardId).normal;
        const combatStart = abilities.find((a) => a.trigger === "combat_start");
        for (const inner of combatStart?.operations ?? []) {
          applyOperation(env, board, source, focus, inner, depth + 1);
        }
      }
      return;
    }
    case "repeat": {
      const times = Math.min(op.times, EFFECT_BOUNDS.maxRepeat);
      for (let i = 0; i < times && !env.counters.terminated; i += 1) {
        applyOperation(env, board, source, focus, op.op, depth + 1);
      }
      return;
    }
    case "transform_card":
      for (const u of resolveSelector(env, board, source, focus, op.target)) {
        const def = env.content.card(op.into);
        u.cardId = op.into;
        u.attack = def.baseAttack + (u.forged ? def.forgedStats.attack : 0);
        u.health = def.baseHealth + (u.forged ? def.forgedStats.health : 0);
        cemit(env, { kind: "unit_stat_changed", combatId: env.combatId, side: u.side, unitId: u.instanceId, attack: u.attack, health: u.health });
      }
      return;
    case "emit_cue":
      cemit(env, { kind: "cue", combatId: env.combatId, cue: op.cue });
      return;
    default:
      return; // add_gold / discount_shop are economy-only (validation forbids here)
  }
};

const runAbility = (env: CombatEnv, board: Board, source: CombatUnit, ability: Ability, focus: Focus): void => {
  const conditions = ability.conditions ?? [];
  if (!conditions.every((c) => conditionHolds(env, board, source, c))) {
    return;
  }
  cemit(env, { kind: "ability_triggered", combatId: env.combatId, side: source.side, unitId: source.instanceId, trigger: ability.trigger });
  for (const op of ability.operations) {
    applyOperation(env, board, source, focus, op, 0);
  }
  resolveDeaths(env, board);
};

/** Fire one trigger for one unit (its active profile's matching abilities). */
export function runTrigger(env: CombatEnv, board: Board, source: CombatUnit, trigger: Trigger, focus: Focus): void {
  if (env.counters.terminated) {
    return;
  }
  if (trigger !== "on_death" && !source.alive) {
    return;
  }
  const def = env.content.card(source.cardId);
  const abilities = source.forged ? def.forged : def.normal;
  for (const ability of abilities) {
    if (ability.trigger === trigger) {
      runAbility(env, board, source, ability, focus);
    }
  }
}

/** Resolve deaths to a fixed point: dying units fire `on_death`, are cleared from
 * their slot, and any new deaths their deathrattles caused are resolved too. */
export function resolveDeaths(env: CombatEnv, board: Board): void {
  let guard = 0;
  while (guard < EFFECT_BOUNDS.maxSummonsPerCombat && !env.counters.terminated) {
    guard += 1;
    const dying = [...living(board, "a"), ...living(board, "b")].filter((u) => u.health <= 0);
    if (dying.length === 0) {
      return;
    }
    for (const unit of dying) {
      if (!unit.alive) {
        continue;
      }
      unit.alive = false;
      cemit(env, { kind: "unit_died", combatId: env.combatId, side: unit.side, unitId: unit.instanceId, slot: unit.slot });
      runTrigger(env, board, unit, "on_death", NO_FOCUS);
      sideOf(board, unit.side)[unit.slot] = null;
    }
  }
}
