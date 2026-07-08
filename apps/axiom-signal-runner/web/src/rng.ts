/*
 * A tiny deterministic PRNG (mulberry32) the level generator draws from. The sim
 * itself is RNG-free — every gameplay outcome is a pure function of (seed, intent
 * sequence) — so the only randomness in the whole game is this seeded stream, used
 * once at generation time. That keeps replays and screenshots byte-reproducible
 * without any dependency on the wasm bridge's RNG (which the Node tests can't reach).
 */

/** A seeded random stream. */
export interface Rng {
  /** The next float in [0, 1). */
  readonly next: () => number;
  /** The next float in [min, max). */
  readonly range: (min: number, max: number) => number;
  /** The next integer in [0, n). */
  readonly int: (n: number) => number;
  /** True with probability `p`. */
  readonly chance: (p: number) => boolean;
  /** A uniformly-picked element of `items`. */
  readonly pick: <T>(items: readonly T[]) => T;
}

/** Build a `Rng` seeded from a 32-bit integer. */
export const makeRng = (seed: number): Rng => {
  let state = seed >>> 0;
  const next = (): number => {
    state = (state + 0x6d_2b_79_f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
  const range = (min: number, max: number): number => min + next() * (max - min);
  const int = (n: number): number => Math.floor(next() * n);
  const chance = (p: number): boolean => next() < p;
  const pick = <T>(items: readonly T[]): T => items[int(items.length)] as T;
  return { chance, int, next, pick, range };
};
