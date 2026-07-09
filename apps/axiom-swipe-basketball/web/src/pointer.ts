/*
 * pointer.ts — a bounded ring buffer of recent pointer samples and the swipe
 * velocity estimate derived from them. SDK-free and fully testable. The buffer has
 * a FIXED capacity (`POINTER_HISTORY`) and never grows; a per-tick delta larger
 * than `MAX_POINTER_DELTA` (a tab-switch / lost-focus / missing-sample glitch) is
 * treated as invalid and clears the history so a garbage flick can't be thrown.
 *
 * Samples are stored in canvas pixels (top-left origin, y-down) with the tick they
 * arrived on; `releaseVelocity` returns pixels-per-tick over the recent window,
 * with screen-down +y (so an upward swipe reads negative y — `throw.ts` maps it to
 * lift).
 */

import { type Vec2, vec2 } from "./vec.ts";
import { MAX_POINTER_DELTA, POINTER_HISTORY, VELOCITY_WINDOW } from "./constants.ts";

interface Sample {
  readonly x: number;
  readonly y: number;
  readonly tick: number;
}

/** A fixed-capacity history of pointer samples with a recent-window velocity estimate. */
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
   * The swipe velocity in pixels-per-tick over the recent window, computed from the
   * oldest and newest samples within `VELOCITY_WINDOW` ticks of the latest. Returns
   * `(0,0)` if fewer than two usable samples or a zero time span.
   */
  public releaseVelocity(): Vec2 {
    const n = this.#samples.length;
    if (n < 2) {
      return vec2(0, 0);
    }
    const newest = this.#samples[n - 1]!;
    let oldest = newest;
    for (let i = n - 2; i >= 0; i -= 1) {
      const s = this.#samples[i]!;
      if (newest.tick - s.tick > VELOCITY_WINDOW) {
        break;
      }
      oldest = s;
    }
    const span = newest.tick - oldest.tick;
    if (span <= 0) {
      return vec2(0, 0);
    }
    return vec2((newest.x - oldest.x) / span, (newest.y - oldest.y) / span);
  }
}
