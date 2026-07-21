/*
 * rng.ts — the ONE source of randomness in Arena Forge. The match owns exactly
 * one seed; every stochastic decision (shop rolls, combat initiative, random
 * effect selectors, pairing swaps) draws from a `Rng` derived deterministically
 * from that seed plus an explicit, named context. There is no `Math.random`, no
 * wall clock, and no hidden global state anywhere in the simulation — replaying
 * the same seed and command stream reproduces every draw byte-for-byte.
 *
 * The generator is mulberry32: a single unsigned-32-bit state, fast, and more
 * than adequate for gameplay. All state is kept as an unsigned 32-bit integer
 * (`>>> 0`), and every public draw returns an integer — Arena Forge does all
 * gameplay math in integers.
 */

/**
 * Mix an arbitrary list of integers into a stable unsigned-32-bit seed. Used to
 * derive an independent sub-stream from the match seed plus a context (e.g. the
 * round number and a phase tag), so unrelated systems never share a stream and
 * their draw order can never bleed into one another.
 */
export const deriveSeed = (...parts: readonly number[]): number => {
  // A splitmix32-style avalanche fold: each part is absorbed then mixed.
  let h = 0x9e3779b9 >>> 0;
  for (const part of parts) {
    h = (h ^ (part >>> 0)) >>> 0;
    h = Math.imul(h ^ (h >>> 16), 0x21f0aaad) >>> 0;
    h = Math.imul(h ^ (h >>> 15), 0x735a2d97) >>> 0;
    h = (h ^ (h >>> 15)) >>> 0;
  }
  return h >>> 0;
};

/**
 * A deterministic integer random source. Construct one from a seed (usually via
 * `deriveSeed`), then draw with `nextU32`, `range`, `pick`, or `shuffle`. Two
 * `Rng`s built from the same seed produce identical sequences forever.
 */
export class Rng {
  private state: number;

  public constructor(seed: number) {
    this.state = seed >>> 0;
  }

  /** The raw unsigned-32-bit draw (mulberry32). */
  public nextU32(): number {
    this.state = (this.state + 0x6d2b79f5) >>> 0;
    let t = this.state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t = (t + Math.imul(t ^ (t >>> 7), t | 61)) ^ t;
    return (t ^ (t >>> 14)) >>> 0;
  }

  /**
   * A uniform integer in `[0, maxExclusive)`. Uses rejection sampling so the
   * distribution is exactly uniform (no modulo bias) and still fully
   * deterministic. `maxExclusive <= 0` yields 0 — a defensive floor callers rely
   * on when a pool is empty.
   */
  public range(maxExclusive: number): number {
    if (maxExclusive <= 0) {
      return 0;
    }
    const bound = maxExclusive >>> 0;
    const limit = (0x100000000 - (0x100000000 % bound)) >>> 0;
    let draw = this.nextU32();
    while (draw >= limit && limit !== 0) {
      draw = this.nextU32();
    }
    return draw % bound;
  }

  /** A uniform integer in the inclusive range `[lo, hi]`. */
  public rangeInclusive(lo: number, hi: number): number {
    return lo + this.range(hi - lo + 1);
  }

  /** `true` with probability `numerator/denominator`, using integer draws. */
  public chance(numerator: number, denominator: number): boolean {
    return this.range(denominator) < numerator;
  }

  /**
   * Pick one element of a non-empty array. Returns `undefined` only for an empty
   * array, so callers must handle the empty case explicitly (the type forces it).
   */
  public pick<T>(items: readonly T[]): T | undefined {
    if (items.length === 0) {
      return undefined;
    }
    return items[this.range(items.length)];
  }

  /**
   * A Fisher–Yates shuffle producing a new array (the input is never mutated),
   * so the source order stays stable and the draw is reproducible.
   */
  public shuffle<T>(items: readonly T[]): T[] {
    const out = items.slice();
    for (let i = out.length - 1; i > 0; i -= 1) {
      const j = this.range(i + 1);
      const a = out[i] as T;
      const b = out[j] as T;
      out[i] = b;
      out[j] = a;
    }
    return out;
  }

  /** A snapshot of the internal state, for serializing a match mid-stream. */
  public snapshot(): number {
    return this.state >>> 0;
  }

  /** Restore a state captured by `snapshot` (used by replay/deserialize). */
  public restore(state: number): void {
    this.state = state >>> 0;
  }
}
