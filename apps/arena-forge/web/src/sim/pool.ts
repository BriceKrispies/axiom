/*
 * pool.ts — the shared card pool, the one economy-wide scarcity constraint. Every
 * collectible card starts with `poolCount` copies; buying removes one, selling or
 * eliminating a player returns them, so the total copies of each card in the pool
 * plus every player's shop/hand/warband is CONSERVED across the whole match (a
 * property the tests assert directly). Rolls draw from the pool by the forge
 * rank's tier weights, in canonical card order, from the match's single `Rng` —
 * so an identical seed rolls identical shops.
 */

import type { LoadedContent } from "./content/load.ts";
import type { CardId, InstanceId } from "./ids.ts";
import type { PoolState, ShopSlot } from "./model.ts";
import type { Rng } from "./rng.ts";
import type { Rules } from "./tuning.ts";
import { ALL_TIERS, shopSizeForRank, tierWeightsForRank } from "./tuning.ts";

/** A fresh pool with every collectible card at its full `poolCount`. */
export const initPool = (content: LoadedContent): PoolState => {
  const counts: Record<CardId, number> = {};
  for (const card of content.collectibleCards) {
    counts[card.id] = card.poolCount;
  }
  return { counts };
};

export const poolCount = (pool: PoolState, id: CardId): number => pool.counts[id] ?? 0;

export const removeFromPool = (pool: PoolState, id: CardId): void => {
  const current = pool.counts[id] ?? 0;
  if (current > 0) {
    pool.counts[id] = current - 1;
  }
};

/** Return a copy to the pool (only for cards the pool tracks, i.e. collectibles). */
export const returnToPool = (pool: PoolState, id: CardId): void => {
  if (id in pool.counts) {
    pool.counts[id] = (pool.counts[id] ?? 0) + 1;
  }
};

/** Return `n` copies (a forged unit consumed `copiesToForge` copies, so removing
 * it from play returns that many to conserve the pool). */
export const returnCopies = (pool: PoolState, id: CardId, n: number): void => {
  for (let i = 0; i < n; i += 1) {
    returnToPool(pool, id);
  }
};

/** Total copies still in the pool — used by conservation checks. */
export const poolTotal = (pool: PoolState): number =>
  Object.values(pool.counts).reduce((sum, n) => sum + n, 0);

/**
 * Draw one card id from the pool appropriate to `rank`: choose a tier by the
 * rank's tier weights (restricted to tiers that still have stock), then a card of
 * that tier weighted by remaining copies, in canonical order. Removes the drawn
 * copy. Returns null only when the pool has nothing rollable at this rank.
 */
export const drawFromPool = (pool: PoolState, content: LoadedContent, rules: Rules, rank: number, rng: Rng): CardId | null => {
  const weights = tierWeightsForRank(rules, rank);
  // Effective tier weights: zero out tiers with no available stock.
  const tierStock = ALL_TIERS.map((tier) =>
    content.collectibleOfTier(tier).reduce((sum, card) => sum + poolCount(pool, card.id), 0),
  );
  const effective = ALL_TIERS.map((tier, i) => ((tierStock[i] ?? 0) > 0 ? (weights[i] ?? 0) : 0));
  const total = effective.reduce((sum, w) => sum + w, 0);
  if (total <= 0) {
    return null;
  }
  let roll = rng.range(total);
  let tierIndex = 0;
  for (let i = 0; i < effective.length; i += 1) {
    roll -= effective[i] ?? 0;
    if (roll < 0) {
      tierIndex = i;
      break;
    }
  }
  const tier = ALL_TIERS[tierIndex] ?? 1;
  const candidates = content.collectibleOfTier(tier);
  const stock = candidates.map((card) => poolCount(pool, card.id));
  const stockTotal = stock.reduce((sum, n) => sum + n, 0);
  let pick = rng.range(stockTotal);
  let chosen: CardId | null = null;
  for (let i = 0; i < candidates.length; i += 1) {
    pick -= stock[i] ?? 0;
    if (pick < 0) {
      chosen = candidates[i]?.id ?? null;
      break;
    }
  }
  if (chosen !== null) {
    removeFromPool(pool, chosen);
  }
  return chosen;
};

/**
 * Roll a whole shop for a forge rank, drawing `shopSizeForRank` cards from the
 * pool. Each drawn card becomes a `ShopSlot` with a fresh instance id from
 * `allocate`. A shop may be shorter than the rank size only if the pool is nearly
 * empty (a rare late-game degenerate case, handled deterministically).
 */
export const rollShop = (
  pool: PoolState,
  content: LoadedContent,
  rules: Rules,
  rank: number,
  rng: Rng,
  allocate: () => InstanceId,
): ShopSlot[] => {
  const size = shopSizeForRank(rules, rank);
  const slots: ShopSlot[] = [];
  for (let i = 0; i < size; i += 1) {
    const cardId = drawFromPool(pool, content, rules, rank, rng);
    if (cardId === null) {
      break;
    }
    slots.push({ instanceId: allocate(), cardId, discount: 0 });
  }
  return slots;
};

/** Return every card currently offered in a shop back to the pool (reroll/refresh). */
export const returnShop = (pool: PoolState, shop: readonly ShopSlot[]): void => {
  for (const slot of shop) {
    returnToPool(pool, slot.cardId);
  }
};
