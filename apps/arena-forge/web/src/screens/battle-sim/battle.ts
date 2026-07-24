/*
 * battle.ts — the headless Battle Simulator match runner. It turns two preset teams
 * into the exact inputs the AUTHORITATIVE combat engine consumes — two immutable
 * warband snapshots — runs the real `runCombat`, and hands back the snapshots, the
 * combat's event stream, and the result. It decides nothing about how a fight
 * plays out; `runCombat` does, deterministically, so the same teams + seed always
 * reproduce byte-for-byte. Reusing the real engine (not a battle-only copy) is the
 * point: what you watch here is exactly what the game would compute.
 *
 * Instance-id ranges are kept disjoint per side (player 100xxx, enemy 200xxx, and
 * combat-summoned tokens 900xxx) because playback keys units by instance id across
 * BOTH snapshots — a collision would merge two fighters into one on screen.
 */

import type { LoadedContent } from "../../sim/content/load.ts";
import type { SimEvent } from "../../sim/events.ts";
import { EventSink } from "../../sim/events.ts";
import type { InstanceId } from "../../sim/ids.ts";
import type { UnitInstance, WarbandSnapshot } from "../../sim/model.ts";
import { WARBAND_SLOTS } from "../../sim/model.ts";
import { snapshotWarband } from "../../sim/combat/board.ts";
import type { CombatResult } from "../../sim/combat/engine.ts";
import { runCombat } from "../../sim/combat/engine.ts";
import { DEFAULT_RULES } from "../../sim/tuning.ts";
import type { PresetUnit } from "./presets.ts";

const PLAYER_ID_BASE = 100001;
const ENEMY_ID_BASE = 200001;
const TOKEN_ID_BASE = 900001;
const COMBAT_ID = 1;

/** Everything a viewer needs to replay a battle: the two immutable team snapshots,
 * the combat's ordered event stream, and its decided result. */
export interface BattleData {
  readonly snapA: WarbandSnapshot;
  readonly snapB: WarbandSnapshot;
  readonly stream: readonly SimEvent[];
  readonly result: CombatResult;
}

/** Materialize preset units into a real, fixed-length warband (slot 0 leftmost,
 * remaining slots empty). Stats are base + forged rule, mirroring how a played,
 * forged unit is stored in match state. */
const toWarband = (content: LoadedContent, units: readonly PresetUnit[], idBase: number): (UnitInstance | null)[] => {
  const slots: (UnitInstance | null)[] = Array.from({ length: WARBAND_SLOTS }, () => null);
  units.slice(0, WARBAND_SLOTS).forEach((unit, i) => {
    const card = content.card(unit.cardId);
    slots[i] = {
      instanceId: idBase + i,
      cardId: unit.cardId,
      forged: unit.forged,
      attack: card.baseAttack + (unit.forged ? card.forgedStats.attack : 0),
      health: card.baseHealth + (unit.forged ? card.forgedStats.health : 0),
      grantedKeywords: [],
      visualStage: unit.forged ? 1 : 0,
    };
  });
  return slots;
};

/**
 * Run a full deterministic battle between two preset teams. `playerUnits` become
 * side "a", `enemyUnits` side "b"; `seed` drives combat initiative and any random
 * effects, so a different seed is a genuine rematch (different swings, same teams).
 */
export const runBattle = (
  content: LoadedContent,
  playerUnits: readonly PresetUnit[],
  enemyUnits: readonly PresetUnit[],
  seed: number,
): BattleData => {
  const snapA = snapshotWarband(0, 0, toWarband(content, playerUnits, PLAYER_ID_BASE), false);
  const snapB = snapshotWarband(1, 0, toWarband(content, enemyUnits, ENEMY_ID_BASE), false);
  const sink = new EventSink();
  let nextToken = TOKEN_ID_BASE;
  const allocate = (): InstanceId => {
    const id = nextToken;
    nextToken += 1;
    return id;
  };
  const result = runCombat(
    { rules: DEFAULT_RULES, content, events: sink, combatId: COMBAT_ID, seed: seed >>> 0, allocate },
    snapA,
    snapB,
  );
  return { snapA, snapB, stream: sink.combatStream(COMBAT_ID), result };
};

/** A flat `instanceId → forged` map across both snapshots, so playback can show a
 * forged unit's upgraded figure (the combat frame itself carries no forge flag). */
export const forgedById = (data: BattleData): Map<InstanceId, boolean> => {
  const map = new Map<InstanceId, boolean>();
  for (const snap of [data.snapA, data.snapB]) {
    for (const slot of snap.slots) {
      if (slot !== null) {
        map.set(slot.instanceId, slot.forged);
      }
    }
  }
  return map;
};
