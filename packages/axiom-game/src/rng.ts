/*
 * The deterministic randomness projection (SPEC-01 §4.2). `Rng` is the author
 * surface over the native draw sequence: every primitive draw (a unit float, an
 * integer below a bound, a weighted index, a shuffle permutation, a named
 * sub-stream) is decided native-side and exposed through the `NativeBridge`;
 * this projection only *shapes* those draws into the author API.
 *
 * `pick`/`weighted`/`shuffle` reorder the author's OWN array client-side using
 * the indices the core chose — the draw sequence, not the JS array op, is what
 * determinism rides on (SPEC-01 §4.2). `Sim.rng` is the root stream (id
 * `ROOT_STREAM`), minted by the runtime app from `GameConfig.seed`; `stream`
 * descends named, independent, reproducible sub-streams.
 */

import { each, pick } from "./branchless.ts";
import type { NativeBridge } from "./native-bridge.ts";

/** The default probability `bool()` uses — an even coin (SPEC-01 §4.2). */
const EVEN_ODDS = 0.5;

/** The stream id of the game's root RNG (the seed-derived stream). */
export const ROOT_STREAM = 0;

/** The deterministic randomness surface for one stream (SPEC-01 §4.2). */
export interface Rng {
  /** A uniform float in `[0, 1)`. */
  readonly next: () => number;
  /** A uniform integer in `[0, maxExclusive)`. */
  readonly int: (maxExclusive: number) => number;
  /** A uniform float in `[min, max)`. */
  readonly range: (min: number, max: number) => number;
  /** `true` with probability `probability` (default an even coin). */
  readonly bool: (probability?: number) => boolean;
  /** One element of `items`, uniformly. */
  readonly pick: <Item>(items: readonly Item[]) => Item;
  /** One element of `items`, drawn proportionally to `weights`. */
  readonly weighted: <Item>(items: readonly Item[], weights: readonly number[]) => Item;
  /** Shuffle `array` in place (deterministic Fisher-Yates). */
  readonly shuffle: (array: unknown[]) => void;
  /** A named, independent, reproducible sub-stream. */
  readonly stream: (name: string) => Rng;
}

/** The `Rng` projection bound to one native stream id. */
export class StreamRng implements Rng {
  readonly #bridge: NativeBridge;
  readonly #stream: number;

  public constructor(bridge: NativeBridge, stream: number) {
    this.#bridge = bridge;
    this.#stream = stream;
  }

  public next(): number {
    return this.#bridge.rngUnit(this.#stream);
  }

  public int(maxExclusive: number): number {
    return this.#bridge.rngBelow(this.#stream, maxExclusive);
  }

  public range(min: number, max: number): number {
    return min + this.next() * (max - min);
  }

  public bool(probability: number = EVEN_ODDS): boolean {
    return this.next() < probability;
  }

  public pick<Item>(items: readonly Item[]): Item {
    return pick(items, this.#bridge.rngBelow(this.#stream, items.length));
  }

  public weighted<Item>(items: readonly Item[], weights: readonly number[]): Item {
    return pick(items, this.#bridge.rngWeighted(this.#stream, weights));
  }

  public shuffle(array: unknown[]): void {
    // The core chose the permutation; the projection applies it client-side: a pure `map` builds `reordered`, then `each` over the index range writes it back in place (no `for`, no side-effecting bare `.map`).
    const order = this.#bridge.rngPermutation(this.#stream, array.length);
    const snapshot = [...array];
    const reordered = order.map((sourceIndex): unknown => pick(snapshot, sourceIndex));
    const targets = Array.from({ length: array.length }, (_unused, index): number => index);
    each(targets, (index): void => {
      array[index] = pick(reordered, index);
    });
  }

  public stream(name: string): Rng {
    return new StreamRng(this.#bridge, this.#bridge.rngStream(this.#stream, name));
  }
}

/** Build the root `Rng` stream for a game over `bridge` (SPEC-01 §4.2). */
export const makeRng = (bridge: NativeBridge): Rng => new StreamRng(bridge, ROOT_STREAM);
