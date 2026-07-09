/*
 * pointer.ts — a bounded ring buffer of recent pointer samples and the SMOOTHED
 * release-gesture velocity derived from them. SDK-free and fully testable. The
 * buffer has a FIXED capacity (`POINTER_HISTORY`) and never grows; a per-tick delta
 * larger than `MAX_POINTER_DELTA` (a tab-switch / lost-focus / missing-sample
 * glitch) is treated as invalid and clears the history so a garbage flick can't be
 * thrown.
 *
 * `releaseVelocity` does NOT just take the last sample pair: it averages the
 * per-pair velocities across the last `THROW_SAMPLE_WINDOW` samples with a
 * triangular (middle-emphasised) weighting, so a single jittery final sample — a
 * finger twitch at lift-off — cannot dominate the shot. Samples are canvas pixels
 * (top-left origin, y-down) with the tick they arrived on; the result is
 * pixels-per-tick with screen-down +y (upward swipe ⇒ negative y).
 */

import { type Vec2, vec2 } from "./vec.ts";
import { MAX_POINTER_DELTA, POINTER_HISTORY, THROW_SAMPLE_WINDOW } from "./constants.ts";

interface Sample {
  readonly x: number;
  readonly y: number;
  readonly tick: number;
}

/** A fixed-capacity history of pointer samples with a smoothed release-velocity estimate. */
export class PointerHistory {
  readonly #samples: Sample[] = [];

  /** The number of retained samples (never exceeds `POINTER_HISTORY`). */
  public get size(): number {
    return this.#samples.length;
  }

  /** Drop all samples (on release, on selecting a new ball, or on a glitch). */
  public clear(): void {
    this.#samples.length = 0;
  }

  /**
   * Record one pointer sample. A jump larger than `MAX_POINTER_DELTA` from the last
   * sample is a focus/tab glitch: discard the whole history and start fresh from
   * this sample. Otherwise append, evicting the oldest to stay within capacity.
   */
  public push(x: number, y: number, tick: number): void {
    const last = this.#samples[this.#samples.length - 1];
    if (last !== undefined && (Math.abs(x - last.x) > MAX_POINTER_DELTA || Math.abs(y - last.y) > MAX_POINTER_DELTA)) {
      this.#samples.length = 0;
    }
    this.#samples.push({ tick, x, y });
    if (this.#samples.length > POINTER_HISTORY) {
      this.#samples.shift();
    }
  }

  /**
   * The smoothed release velocity (px/tick): a triangular-weighted average of the
   * per-pair velocities over the last `THROW_SAMPLE_WINDOW` samples. The middle of
   * the swipe carries the most weight and the newest pair the least, so a final
   * jitter can't dominate. Returns `(0,0)` with fewer than two usable samples.
   */
  public releaseVelocity(): Vec2 {
    const n = this.#samples.length;
    if (n < 2) {
      return vec2(0, 0);
    }
    // The most-recent window of samples (at most THROW_SAMPLE_WINDOW + 1 → that many pairs).
    const start = Math.max(0, n - (THROW_SAMPLE_WINDOW + 1));
    const window = this.#samples.slice(start);
    const pairs = window.length - 1;

    let sumX = 0;
    let sumY = 0;
    let sumW = 0;
    for (let j = 0; j < pairs; j += 1) {
      const a = window[j]!;
      const b = window[j + 1]!;
      const span = b.tick - a.tick;
      if (span <= 0) {
        continue;
      }
      // Triangular weight: peaks in the middle of the window, so neither the first
      // (start-up) nor the last (lift-off jitter) pair can dominate.
      const weight = Math.min(j + 1, pairs - j);
      sumX += (weight * (b.x - a.x)) / span;
      sumY += (weight * (b.y - a.y)) / span;
      sumW += weight;
    }
    if (sumW <= 0) {
      return vec2(0, 0);
    }
    return vec2(sumX / sumW, sumY / sumW);
  }
}
