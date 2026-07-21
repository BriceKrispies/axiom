/*
 * events.ts — the ONE event stream that carries the simulation's decisions to
 * everything downstream: the presentation layer, the replay tool, and the test
 * harness. Every economy action, every combat beat, and every presentation cue
 * is a typed `SimEvent` with a monotonic `seq`. This is the boundary the spec
 * demands: the renderer ANIMATES this stream and may drop or simplify effects,
 * but it never PRODUCES one — simulation state is decided entirely inside the
 * sim and merely reported here.
 *
 * A single combat's event stream is exactly the slice of the log with a matching
 * `combatId`, so "replay combat from its event stream" is just a filter. The
 * `EventSink` stamps `seq` from the match's `eventSeq`, guaranteeing a stable
 * total order across a match that reproduces byte-for-byte on replay.
 */

import type { ArenaStage, CombatVerdict, Pairing } from "./model.ts";
import type { CardId, GroupId, InstanceId, KeywordId, PlayerId } from "./ids.ts";
import type { ForgeReward } from "./content/schema.ts";

/** Which side of a combat a unit is on (a is the pairing's `a`, b the opponent). */
export type CombatSide = "a" | "b";

/**
 * A single simulation event WITHOUT its sequence number — exactly what an emitter
 * hands the sink. The economy/match variants have no `combatId`; the combat
 * variants all carry one so a combat's stream is filterable. Kept union-shaped
 * (rather than `Omit<SimEvent,"seq">`, which would collapse to common keys) so
 * every discriminated field survives.
 */
export type SimEventData =
  // ── match / economy ──────────────────────────────────────────────────────
  | { readonly kind: "match_start"; readonly matchId: string; readonly seed: number }
  | { readonly kind: "phase_changed"; readonly from: string; readonly to: string; readonly round: number }
  | { readonly kind: "shop_start"; readonly round: number; readonly playerId: PlayerId }
  | { readonly kind: "card_purchased"; readonly playerId: PlayerId; readonly cardId: CardId; readonly instanceId: InstanceId; readonly cost: number }
  | { readonly kind: "card_sold"; readonly playerId: PlayerId; readonly cardId: CardId; readonly instanceId: InstanceId; readonly refund: number }
  | { readonly kind: "shop_rerolled"; readonly playerId: PlayerId; readonly cost: number }
  | { readonly kind: "shop_freeze_changed"; readonly playerId: PlayerId; readonly frozen: boolean }
  | { readonly kind: "forge_rank_increased"; readonly playerId: PlayerId; readonly rank: number; readonly cost: number }
  | { readonly kind: "card_played"; readonly playerId: PlayerId; readonly instanceId: InstanceId; readonly slot: number }
  | { readonly kind: "unit_reordered"; readonly playerId: PlayerId; readonly instanceId: InstanceId; readonly from: number; readonly to: number }
  | { readonly kind: "unit_forged"; readonly playerId: PlayerId; readonly cardId: CardId; readonly instanceId: InstanceId; readonly slot: number }
  | { readonly kind: "forge_reward_granted"; readonly playerId: PlayerId; readonly reward: ForgeReward }
  | { readonly kind: "gold_gained"; readonly playerId: PlayerId; readonly amount: number }
  | { readonly kind: "shop_discounted"; readonly playerId: PlayerId; readonly amount: number }
  | { readonly kind: "command_rejected"; readonly playerId: PlayerId; readonly reason: string; readonly command: string }
  | { readonly kind: "group_synergy"; readonly playerId: PlayerId; readonly group: GroupId; readonly count: number }
  | { readonly kind: "pairings_set"; readonly round: number; readonly pairings: readonly Pairing[] }
  | { readonly kind: "player_damaged"; readonly playerId: PlayerId; readonly amount: number; readonly health: number }
  | { readonly kind: "player_eliminated"; readonly playerId: PlayerId; readonly placement: number }
  | { readonly kind: "stage_changed"; readonly playerId: PlayerId; readonly stage: ArenaStage }
  | { readonly kind: "match_won"; readonly playerId: PlayerId }
  // ── combat ───────────────────────────────────────────────────────────────
  | { readonly kind: "combat_begin"; readonly combatId: number; readonly a: PlayerId; readonly b: PlayerId | null; readonly ghost: boolean }
  | { readonly kind: "ability_triggered"; readonly combatId: number; readonly side: CombatSide; readonly unitId: InstanceId; readonly trigger: string }
  | { readonly kind: "attack_started"; readonly combatId: number; readonly side: CombatSide; readonly attacker: InstanceId; readonly defender: InstanceId }
  | { readonly kind: "impact"; readonly combatId: number; readonly attacker: InstanceId; readonly defender: InstanceId; readonly amount: number }
  | { readonly kind: "unit_damaged"; readonly combatId: number; readonly side: CombatSide; readonly unitId: InstanceId; readonly amount: number; readonly health: number }
  | { readonly kind: "unit_healed"; readonly combatId: number; readonly side: CombatSide; readonly unitId: InstanceId; readonly amount: number; readonly health: number }
  | { readonly kind: "unit_stat_changed"; readonly combatId: number; readonly side: CombatSide; readonly unitId: InstanceId; readonly attack: number; readonly health: number }
  | { readonly kind: "unit_keyword_changed"; readonly combatId: number; readonly side: CombatSide; readonly unitId: InstanceId; readonly keyword: KeywordId; readonly granted: boolean }
  | { readonly kind: "unit_summoned"; readonly combatId: number; readonly side: CombatSide; readonly unitId: InstanceId; readonly cardId: CardId; readonly slot: number }
  | { readonly kind: "unit_moved"; readonly combatId: number; readonly side: CombatSide; readonly unitId: InstanceId; readonly from: number; readonly to: number }
  | { readonly kind: "unit_died"; readonly combatId: number; readonly side: CombatSide; readonly unitId: InstanceId; readonly slot: number }
  | { readonly kind: "cue"; readonly combatId: number; readonly cue: string }
  | { readonly kind: "diagnostic"; readonly combatId: number; readonly reason: string }
  | { readonly kind: "combat_end"; readonly combatId: number; readonly verdict: CombatVerdict; readonly winner: CombatSide | null; readonly survivors: number };

/** A `SimEventData` stamped with its assigned sequence number (the logged form). */
export type SimEvent = SimEventData & { readonly seq: number };

/** What an emitter hands the sink: a seq-less event. */
export type EmittedEvent = SimEventData;

/**
 * The append-only event log for a match. Emitters call `emit` with a seq-less
 * event; the sink stamps the next `seq` (mirrored into `MatchState.eventSeq` by
 * the match) and pushes it. `since` supports incremental draining by the UI, and
 * `all` exposes the full ordered log for replay/serialize.
 */
export class EventSink {
  private readonly log: SimEvent[] = [];
  private seq: number;

  public constructor(startSeq = 0) {
    this.seq = startSeq;
  }

  public emit(event: EmittedEvent): SimEvent {
    const stamped = { ...event, seq: this.seq } as SimEvent;
    this.seq += 1;
    this.log.push(stamped);
    return stamped;
  }

  /** The next seq that will be assigned (mirrors `MatchState.eventSeq`). */
  public nextSeq(): number {
    return this.seq;
  }

  public all(): readonly SimEvent[] {
    return this.log;
  }

  /** Events with `seq >= from`, in order — for incremental UI draining. */
  public since(from: number): readonly SimEvent[] {
    return this.log.filter((event) => event.seq >= from);
  }

  /** All combat events for one combat, in order — the combat's own stream. */
  public combatStream(combatId: number): readonly SimEvent[] {
    return this.log.filter((event) => "combatId" in event && event.combatId === combatId);
  }
}
