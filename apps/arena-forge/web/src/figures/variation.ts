/*
 * variation.ts — deterministic per-figure variation. Every stochastic choice in
 * figure generation (whether a mirror twin appears, a bounded repeat count, small
 * proportion jitter) is derived from a stable seed = hash(cardId, seedSalt) folded
 * with a named channel — NEVER `Math.random` and NEVER wall-clock. So reloading or
 * replaying the same card reproduces byte-identical geometry. This is the figure
 * analogue of the sim's own seeded `Rng`.
 */

/** FNV-1a over the card id, mixed with the figure's seed salt. */
export const figureSeed = (cardId: string, salt: number): number => {
  let h = (0x811c9dc5 ^ (salt >>> 0)) >>> 0;
  for (let i = 0; i < cardId.length; i += 1) {
    h ^= cardId.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
};

/** Fold a named channel into a seed to derive an independent sub-stream. */
export const channel = (seed: number, name: string): number => {
  let h = seed >>> 0;
  for (let i = 0; i < name.length; i += 1) {
    h ^= name.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  // A final avalanche so adjacent channels diverge.
  h = Math.imul(h ^ (h >>> 15), 0x2c1b3c6d) >>> 0;
  h = Math.imul(h ^ (h >>> 12), 0x297a2d39) >>> 0;
  return (h ^ (h >>> 15)) >>> 0;
};

/** A uniform integer in `[lo, hi]` for a channel. */
export const pickInt = (seed: number, name: string, lo: number, hi: number): number => {
  if (hi <= lo) {
    return lo;
  }
  return lo + (channel(seed, name) % (hi - lo + 1));
};

/** A uniform float in `[lo, hi)` for a channel. */
export const pickFloat = (seed: number, name: string, lo: number, hi: number): number =>
  lo + (channel(seed, name) / 0x100000000) * (hi - lo);

export const pickBool = (seed: number, name: string): boolean => (channel(seed, name) & 1) === 1;
