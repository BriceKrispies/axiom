/*
 * model.ts — the authoritative, serializable state of an Arena Forge match. This
 * file declares SHAPES only; every transition lives in a dedicated owner
 * (`phase.ts`, `economy.ts`, `forge.ts`, `combat/…`, `pairing.ts`, …). The state
 * is plain data — arrays keyed by stable ids, integers for all gameplay math —
 * so it snapshots and replays byte-for-byte. The match's `Rng` and instance-id
 * allocator live on the `Match` orchestrator (`match.ts`); only their numeric
 * snapshots are serialized alongside this state.
 */

import type { ArchetypeId, CardId, InstanceId, KeywordId, PlayerId } from "./ids.ts";

/** The seven ordered warband positions. Empty slots are meaningful battlefield
 * positions during combat, so a warband is a fixed-length slot array. */
export const WARBAND_SLOTS = 7;

/** The strict phase machine. Only phase-appropriate commands are accepted. */
export type Phase =
  | "lobby"
  | "shop"
  | "combat_prepare"
  | "combat"
  | "combat_resolve"
  | "round_transition"
  | "match_complete";

/** The data-derived arena presentation stage (drives the visual progression). */
export type ArenaStage = "workshop" | "kindled" | "tempered" | "masterwork";

export type CombatVerdict = "win" | "loss" | "draw";

/**
 * A card instance a player owns, in shop / hand / warband. `attack` and `health`
 * are the CURRENT permanent stats (base + forged rule + permanent economy buffs);
 * combat works on transient copies of these and never writes back. `forged`
 * selects the forged ability profile and the forged visual stage.
 */
export interface UnitInstance {
  readonly instanceId: InstanceId;
  readonly cardId: CardId;
  forged: boolean;
  attack: number;
  health: number;
  /** Keywords granted permanently on top of the card definition's own. */
  grantedKeywords: KeywordId[];
  /** 0 = normal visual stage, 1 = forged visual stage. */
  visualStage: number;
}

/** One offered card in a player's shop. Returns to the shared pool on reroll or
 * shop refresh. `discount` is applied by economy effects; `cost - discount`
 * (floored at 0) is what the player pays. */
export interface ShopSlot {
  readonly instanceId: InstanceId;
  readonly cardId: CardId;
  discount: number;
}

/** The outcome of a player's combat this round, surfaced to the UI. */
export interface CombatOutcome {
  readonly round: number;
  readonly verdict: CombatVerdict;
  /** Player health damage the LOSER took (0 for a win or a draw). */
  readonly damage: number;
  /** Surviving units on the winning side (0 for a draw). */
  readonly survivors: number;
  /** The opponent's player id, or null when the opponent was a ghost. */
  readonly opponentId: PlayerId | null;
  readonly opponentGhost: boolean;
}

/** Everything about one of the eight lobby slots. */
export interface PlayerState {
  readonly id: PlayerId;
  readonly name: string;
  health: number;
  gold: number;
  forgeRank: number;
  shop: ShopSlot[];
  /** When true, the next shop refresh keeps this shop (consumed after one
   * boundary). */
  shopFrozen: boolean;
  hand: UnitInstance[];
  /** Length {@link WARBAND_SLOTS}; `null` is an empty (but positional) slot. */
  warband: (UnitInstance | null)[];
  eliminated: boolean;
  /** Final placement 1..8, or 0 while still active. */
  placement: number;
  lastOpponent: PlayerId | null;
  opponentHistory: PlayerId[];
  combatResult: CombatOutcome | null;
  presentationStage: ArenaStage;
  isBot: boolean;
}

/** One combat pairing for the round. `b` is null when `a` faces a ghost; the
 * ghost is the snapshot of the player named by `ghostOf`. */
export interface Pairing {
  readonly a: PlayerId;
  readonly b: PlayerId | null;
  readonly ghostOf: PlayerId | null;
}

/**
 * The shared card pool. `counts[cardId]` is the number of copies still available
 * to roll. Order is NEVER read from this map — every roll iterates the canonical
 * card list from loaded content and reads the count here, so pool traversal is
 * deterministic regardless of key order.
 */
export interface PoolState {
  counts: Record<CardId, number>;
}

/** A snapshot of a player's warband taken when the shop timer expires — combat
 * runs from this immutable copy, never from the live player state. */
export interface WarbandSnapshot {
  readonly ownerId: PlayerId;
  readonly forgeRank: number;
  readonly ghost: boolean;
  /** Deep copies of the seven slots at snapshot time. */
  readonly slots: readonly (UnitInstance | null)[];
}

/** The full authoritative match state. */
export interface MatchState {
  readonly matchId: string;
  readonly seed: number;
  round: number;
  phase: Phase;
  /** Authoritative tick counter (advanced by the host). */
  tick: number;
  /** The tick at which the current timed phase (shop) ends. */
  phaseDeadlineTick: number;
  /** Length 8, indexed by `PlayerId`. */
  players: PlayerState[];
  /** Active players in current seeding order (drives pairing). */
  activeOrder: PlayerId[];
  pairings: Pairing[];
  /** The eliminated player used as a ghost last round (not reused next round). */
  ghostUsedLastRound: PlayerId | null;
  pool: PoolState;
  commandSeq: number;
  eventSeq: number;
}

/** The data-driven per-archetype identity summary, surfaced to the UI. */
export interface ArchetypeSummary {
  readonly id: ArchetypeId;
  readonly name: string;
  readonly description: string;
}
