/*
 * combat-playback.ts — reconstructs a combat "frame" for the renderer to animate.
 * The simulation already decided the combat deterministically; playback is pure
 * REPLAY of the event stream: start from the two immutable warband snapshots,
 * then apply the combat's events up to a time cursor to derive current unit
 * health/slots/liveness plus the active attacker/defender and damage floaters.
 * The renderer never decides anything — it only shows what the events already
 * describe. Dropping frames (fast-forwarding the cursor) never changes results.
 */

import type { SimEvent } from "../sim/events.ts";
import type { CombatSide } from "../sim/events.ts";
import type { InstanceId } from "../sim/ids.ts";
import type { WarbandSnapshot } from "../sim/model.ts";

export interface PlayUnit {
  instanceId: InstanceId;
  cardId: string;
  side: CombatSide;
  slot: number;
  attack: number;
  health: number;
  maxHealth: number;
  alive: boolean;
  hitFlash: number;
}

export interface CombatFrame {
  units: PlayUnit[];
  attacker: InstanceId | null;
  defender: InstanceId | null;
  floaters: { unitId: InstanceId; text: string; color: string }[];
  index: number;
  total: number;
}

const seed = (snap: WarbandSnapshot, side: CombatSide): PlayUnit[] =>
  snap.slots.flatMap((u, slot) =>
    u === null ? [] : [{ instanceId: u.instanceId, cardId: u.cardId, side, slot, attack: u.attack, health: u.health, maxHealth: u.health, alive: true, hitFlash: 0 }],
  );

/** Reconstruct the combat state after the first `upto` events of the stream. */
export const reconstructFrame = (
  snapA: WarbandSnapshot,
  snapB: WarbandSnapshot,
  stream: readonly SimEvent[],
  upto: number,
): CombatFrame => {
  const units = [...seed(snapA, "a"), ...seed(snapB, "b")];
  const byId = new Map<InstanceId, PlayUnit>(units.map((u) => [u.instanceId, u]));
  let attacker: InstanceId | null = null;
  let defender: InstanceId | null = null;
  const floaters: CombatFrame["floaters"] = [];

  const bound = Math.min(upto, stream.length);
  for (let i = 0; i < bound; i += 1) {
    const ev = stream[i] as SimEvent;
    switch (ev.kind) {
      case "attack_started":
        attacker = ev.attacker;
        defender = ev.defender;
        break;
      case "unit_summoned": {
        const u: PlayUnit = { instanceId: ev.unitId, cardId: ev.cardId, side: ev.side, slot: ev.slot, attack: 0, health: 1, maxHealth: 1, alive: true, hitFlash: 0 };
        byId.set(ev.unitId, u);
        units.push(u);
        break;
      }
      case "unit_damaged": {
        const u = byId.get(ev.unitId);
        if (u) {
          u.health = ev.health;
          u.hitFlash = i;
          if (ev.amount > 0 && i >= bound - 3) {
            floaters.push({ unitId: ev.unitId, text: `-${ev.amount}`, color: "#ff6a6a" });
          }
        }
        break;
      }
      case "unit_healed": {
        const u = byId.get(ev.unitId);
        if (u) {
          u.health = ev.health;
        }
        break;
      }
      case "unit_stat_changed": {
        const u = byId.get(ev.unitId);
        if (u) {
          u.attack = ev.attack;
          u.health = ev.health;
          u.maxHealth = Math.max(u.maxHealth, ev.health);
        }
        break;
      }
      case "unit_moved": {
        const u = byId.get(ev.unitId);
        if (u) {
          u.slot = ev.to;
        }
        break;
      }
      case "unit_died": {
        const u = byId.get(ev.unitId);
        if (u) {
          u.alive = false;
        }
        break;
      }
      default:
        break;
    }
  }
  return { units, attacker, defender, floaters, index: bound, total: stream.length };
};
