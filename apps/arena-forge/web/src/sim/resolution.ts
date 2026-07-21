/*
 * resolution.ts — turns a round's combat results into player consequences. It
 * computes each loser's damage from the data-driven formula
 * `damage = surviving_enemy_count + opponent_forge_rank + sum_of_surviving_enemy_tiers`
 * (clamped to the max), applies ALL damage only after every combat has completed,
 * eliminates players at <= 0 health SIMULTANEOUSLY, and assigns their final
 * placements by the documented tiebreak (higher post-damage health, lower damage
 * received, higher pre-combat health, stable id). Ghosts and byes never deal or
 * take player damage. It records each eliminated player's snapshot for future
 * ghosts and reports the winner when one player remains.
 */

import type { LoadedContent } from "./content/load.ts";
import type { EventSink } from "./events.ts";
import type { PlayerId } from "./ids.ts";
import { returnCopies, returnToPool } from "./pool.ts";
import type { CombatOutcome, MatchState, PlayerState } from "./model.ts";
import type { CombatResult } from "./combat/engine.ts";
import type { GhostStore } from "./pairing.ts";
import type { Pairing, WarbandSnapshot } from "./model.ts";
import type { Rules } from "./tuning.ts";

export interface RoundResult {
  readonly pairing: Pairing;
  readonly result: CombatResult;
  readonly snapA: WarbandSnapshot;
  readonly snapB: WarbandSnapshot;
}

/** The clamped base loss formula: surviving enemies + opponent forge rank + sum
 * of surviving enemy tiers, clamped to the max consequence. */
const baseLoss = (rules: Rules, r: CombatResult): number =>
  Math.min(rules.maxConsequence, Math.max(0, r.survivors + r.winnerForgeRank + r.survivingTierSum));

/** The anti-stalemate escalation term (0 until the start round). Added to a loss
 * after the clamp, AND dealt to both players on a draw, so no round past the
 * start is damage-free — this guarantees the match terminates even when two
 * static boards mutually wipe every round. */
const escalation = (rules: Rules, round: number): number =>
  Math.max(0, round - rules.escalationStartRound) * rules.consequenceEscalation;

const player = (state: MatchState, id: PlayerId): PlayerState => state.players[id] as PlayerState;

interface PendingDamage {
  readonly playerId: PlayerId;
  readonly amount: number;
  readonly outcome: CombatOutcome;
}

/** Compute (but do not yet apply) each live player's outcome + damage. */
const computeOutcomes = (state: MatchState, rules: Rules, results: readonly RoundResult[]): PendingDamage[] => {
  const pending: PendingDamage[] = [];
  const esc = escalation(rules, state.round);
  const damageFor = (verdict: CombatResult["aVerdict"], base: number): number =>
    verdict === "loss" ? base + esc : verdict === "draw" ? esc : 0;
  for (const rr of results) {
    const { pairing, result } = rr;
    const base = baseLoss(rules, result);
    // Player A is always a live player.
    const aOutcome: CombatOutcome = {
      round: state.round,
      verdict: result.aVerdict,
      damage: damageFor(result.aVerdict, base),
      survivors: result.survivors,
      opponentId: pairing.b,
      opponentGhost: pairing.b === null,
    };
    pending.push({ playerId: pairing.a, amount: aOutcome.damage, outcome: aOutcome });
    // Player B is live only when it is a real opponent (not a ghost / bye).
    if (pairing.b !== null) {
      const bOutcome: CombatOutcome = {
        round: state.round,
        verdict: result.bVerdict,
        damage: damageFor(result.bVerdict, base),
        survivors: result.survivors,
        opponentId: pairing.a,
        opponentGhost: false,
      };
      pending.push({ playerId: pairing.b, amount: bOutcome.damage, outcome: bOutcome });
    }
  }
  return pending;
};

/** Update rematch bookkeeping for the round's live pairings. */
const recordOpponents = (state: MatchState, results: readonly RoundResult[]): void => {
  for (const { pairing } of results) {
    const a = player(state, pairing.a);
    if (pairing.b !== null) {
      const b = player(state, pairing.b);
      a.lastOpponent = pairing.b;
      b.lastOpponent = pairing.a;
      a.opponentHistory.push(pairing.b);
      b.opponentHistory.push(pairing.a);
    } else {
      a.lastOpponent = null;
      a.opponentHistory.push(-1);
    }
  }
};

/**
 * Apply a round's resolution: damage, simultaneous elimination, placements, ghost
 * snapshots. Returns the ids eliminated this round and the match winner (if the
 * match ended).
 */
export const applyRoundResolution = (
  state: MatchState,
  rules: Rules,
  _content: LoadedContent,
  events: EventSink,
  ghostStore: GhostStore,
  results: readonly RoundResult[],
): { readonly eliminated: PlayerId[]; readonly matchWinner: PlayerId | null } => {
  const healthBefore = new Map<PlayerId, number>(state.players.map((p) => [p.id, p.health]));
  const pending = computeOutcomes(state, rules, results);

  // Apply all damage together, after every combat completed.
  for (const pd of pending) {
    const p = player(state, pd.playerId);
    p.combatResult = pd.outcome;
    if (pd.amount > 0) {
      p.health -= pd.amount;
      events.emit({ kind: "player_damaged", playerId: p.id, amount: pd.amount, health: p.health });
    }
  }
  recordOpponents(state, results);

  const activeBefore = state.players.filter((p) => !p.eliminated);
  const nowEliminated = activeBefore.filter((p) => p.health <= 0);
  // Snapshot each eliminated player's final warband for future ghost use.
  for (const rr of results) {
    if (rr.pairing.b !== null) {
      const b = player(state, rr.pairing.b);
      if (b.health <= 0) {
        ghostStore.snapshots.set(b.id, rr.snapB);
      }
    }
    const a = player(state, rr.pairing.a);
    if (a.health <= 0) {
      ghostStore.snapshots.set(a.id, rr.snapA);
    }
  }

  // Placement tiebreak: better (lower placement number) first.
  const ordered = nowEliminated.slice().sort(
    (a, b) =>
      b.health - a.health ||
      (state.round === b.combatResult?.round ? (b.combatResult?.damage ?? 0) - (a.combatResult?.damage ?? 0) : 0) ||
      (healthBefore.get(b.id) ?? 0) - (healthBefore.get(a.id) ?? 0) ||
      a.id - b.id,
  );
  let place = activeBefore.length - nowEliminated.length + 1;
  for (const p of ordered) {
    p.eliminated = true;
    p.placement = place;
    place += 1;
    // Return the eliminated player's owned cards to the shared pool (a forged
    // unit returns the copies it consumed, conserving the pool).
    for (const u of [...p.warband, ...p.hand]) {
      if (u !== null) {
        returnCopies(state.pool, u.cardId, u.forged ? rules.copiesToForge : 1);
      }
    }
    for (const slot of p.shop) {
      returnToPool(state.pool, slot.cardId);
    }
    // An eliminated player owns nothing further (their cards are back in the pool;
    // their final board lives on only as a ghost snapshot).
    p.warband = p.warband.map(() => null);
    p.hand = [];
    p.shop = [];
    events.emit({ kind: "player_eliminated", playerId: p.id, placement: p.placement });
  }

  const remaining = state.players.filter((p) => !p.eliminated);
  let matchWinner: PlayerId | null = null;
  if (remaining.length === 1) {
    const winner = remaining[0] as PlayerState;
    winner.placement = 1;
    matchWinner = winner.id;
  } else if (remaining.length === 0) {
    // Mutual elimination of the last players: the best tiebreak (placement 1) wins.
    const champ = ordered.find((p) => p.placement === 1);
    matchWinner = champ ? champ.id : null;
  }
  return { eliminated: ordered.map((p) => p.id), matchWinner };
};
