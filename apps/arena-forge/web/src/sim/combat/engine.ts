/*
 * engine.ts — the deterministic combat loop. Given two immutable warband
 * snapshots and an explicit combat seed, it produces a `CombatResult` and a full
 * event stream, with position materially affecting the outcome. The loop is:
 * build both sides → fire `combat_start` + `passive_aura` → the seed picks
 * initiative → sides alternate; each attack takes the leftmost living unit that
 * has not attacked this cycle, targets the leftmost living enemy (guards first),
 * deals SIMULTANEOUS damage, then resolves reaction triggers and deaths before
 * the next attack. It ends when a side is empty (or a hard action/event bound is
 * hit → a diagnostic draw). Nothing here reads wall-clock or unseeded randomness;
 * the same seed + snapshots reproduce the same events byte-for-byte.
 */

import type { LoadedContent } from "../content/load.ts";
import type { CombatSide } from "../events.ts";
import type { EventSink } from "../events.ts";
import type { InstanceId } from "../ids.ts";
import type { CombatVerdict, WarbandSnapshot } from "../model.ts";
import { Rng } from "../rng.ts";
import type { Rules } from "../tuning.ts";
import { EFFECT_BOUNDS } from "../effects/language.ts";
import type { Board, CombatUnit } from "./board.ts";
import { GUARD, buildSide, hasKeyword, living, other, sideOf, survivingTierSum } from "./board.ts";
import type { CombatEnv } from "./env.ts";
import { cemit, newCounters, terminate } from "./env.ts";
import type { Focus } from "./combat-effects.ts";
import { applyDamage, resolveDeaths, runTrigger } from "./combat-effects.ts";

export interface CombatResult {
  readonly combatId: number;
  readonly winnerSide: CombatSide | null;
  readonly aVerdict: CombatVerdict;
  readonly bVerdict: CombatVerdict;
  /** Survivors on the winning side (0 for a draw). */
  readonly survivors: number;
  /** Winner's forge rank (a consequence-formula input; 0 for a draw). */
  readonly winnerForgeRank: number;
  /** Sum of surviving winner tiers (a consequence-formula input; 0 for a draw). */
  readonly survivingTierSum: number;
  readonly bound: boolean;
}

export interface CombatConfig {
  readonly rules: Rules;
  readonly content: LoadedContent;
  readonly events: EventSink;
  readonly combatId: number;
  readonly seed: number;
  readonly allocate: () => InstanceId;
}

/** The leftmost living unit on a side that has not attacked this cycle; resets
 * the cycle when all living units have attacked. */
const nextAttacker = (board: Board, side: CombatSide): CombatUnit | null => {
  const units = living(board, side);
  if (units.length === 0) {
    return null;
  }
  const pending = units.find((u) => !u.hasAttacked);
  if (pending !== undefined) {
    return pending;
  }
  for (const u of units) {
    u.hasAttacked = false;
  }
  return units[0] as CombatUnit;
};

/** The defender: leftmost living enemy, honoring guard before other targets. */
const chooseDefender = (env: CombatEnv, board: Board, attackerSide: CombatSide): CombatUnit | null => {
  const enemies = living(board, other(attackerSide));
  if (enemies.length === 0) {
    return null;
  }
  const guards = enemies.filter((u) => hasKeyword(env.content, u, GUARD));
  const pool = guards.length > 0 ? guards : enemies;
  return pool[0] as CombatUnit;
};

const fireStart = (env: CombatEnv, board: Board): void => {
  for (const side of ["a", "b"] as const) {
    for (const unit of living(board, side)) {
      runTrigger(env, board, unit, "combat_start", { attacker: null, defender: null });
    }
  }
  for (const side of ["a", "b"] as const) {
    for (const unit of living(board, side)) {
      runTrigger(env, board, unit, "passive_aura", { attacker: null, defender: null });
    }
  }
  resolveDeaths(env, board);
};

const resolveAttack = (env: CombatEnv, board: Board, attacker: CombatUnit, defender: CombatUnit): void => {
  const focus: Focus = { attacker, defender };
  runTrigger(env, board, attacker, "before_attack", focus);
  if (!attacker.alive || !defender.alive) {
    return;
  }
  cemit(env, { kind: "attack_started", combatId: env.combatId, side: attacker.side, attacker: attacker.instanceId, defender: defender.instanceId });
  const toDefender = applyDamage(env, board, defender, attacker.attack);
  const toAttacker = applyDamage(env, board, attacker, defender.attack);
  cemit(env, { kind: "impact", combatId: env.combatId, attacker: attacker.instanceId, defender: defender.instanceId, amount: toDefender });
  runTrigger(env, board, attacker, "after_attack", focus);
  if (toDefender > 0) {
    runTrigger(env, board, defender, "on_damage", focus);
  }
  if (toAttacker > 0) {
    runTrigger(env, board, attacker, "on_damage", focus);
  }
  if (defender.alive && defender.health > 0) {
    runTrigger(env, board, defender, "on_survive_damage", focus);
  }
  if (attacker.alive && attacker.health > 0) {
    runTrigger(env, board, attacker, "on_survive_damage", focus);
  }
  resolveDeaths(env, board);
};

/** Run a full deterministic combat and return its result. */
export const runCombat = (config: CombatConfig, snapA: WarbandSnapshot, snapB: WarbandSnapshot): CombatResult => {
  const env: CombatEnv = {
    rules: config.rules,
    content: config.content,
    rng: new Rng(config.seed),
    events: config.events,
    combatId: config.combatId,
    counters: newCounters(),
    allocate: config.allocate,
  };
  const board: Board = { a: buildSide(snapA, "a"), b: buildSide(snapB, "b") };
  cemit(env, {
    kind: "combat_begin",
    combatId: config.combatId,
    a: snapA.ownerId,
    b: snapB.ghost ? null : snapB.ownerId,
    ghost: snapA.ghost || snapB.ghost,
  });

  fireStart(env, board);

  let attackerSide: CombatSide = env.rng.chance(1, 2) ? "a" : "b";
  let actions = 0;
  while (!env.counters.terminated && living(board, "a").length > 0 && living(board, "b").length > 0) {
    const attacker = nextAttacker(board, attackerSide);
    const defender = attacker === null ? null : chooseDefender(env, board, attackerSide);
    if (attacker !== null && defender !== null) {
      resolveAttack(env, board, attacker, defender);
      if (attacker.alive) {
        attacker.hasAttacked = true;
      }
    }
    actions += 1;
    env.counters.actions = actions;
    if (actions >= EFFECT_BOUNDS.maxCombatActions) {
      terminate(env, "action_bound");
    }
    attackerSide = other(attackerSide);
  }

  const aAlive = living(board, "a").length;
  const bAlive = living(board, "b").length;
  const bound = env.counters.terminated;
  const winnerSide: CombatSide | null = bound ? null : aAlive > 0 && bAlive === 0 ? "a" : bAlive > 0 && aAlive === 0 ? "b" : null;
  const aVerdict: CombatVerdict = winnerSide === "a" ? "win" : winnerSide === "b" ? "loss" : "draw";
  const bVerdict: CombatVerdict = winnerSide === "b" ? "win" : winnerSide === "a" ? "loss" : "draw";
  const survivors = winnerSide === null ? 0 : living(board, winnerSide).length;
  const winnerForgeRank = winnerSide === "a" ? snapA.forgeRank : winnerSide === "b" ? snapB.forgeRank : 0;
  const tierSum = winnerSide === null ? 0 : survivingTierSum(env.content, board, winnerSide);

  cemit(env, { kind: "combat_end", combatId: config.combatId, verdict: aVerdict, winner: winnerSide, survivors });
  return {
    combatId: config.combatId,
    winnerSide,
    aVerdict,
    bVerdict,
    survivors,
    winnerForgeRank,
    survivingTierSum: tierSum,
    bound,
  };
};
