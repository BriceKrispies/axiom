/*
 * motion.ts — a bounded history of where the held ball has been, and the smoothed
 * velocity it is carrying when you let go. SDK-free and fully testable.
 *
 * This is the arcade cabinet's `PointerHistory`, moved one step closer to the world.
 * The cabinet sampled the POINTER in canvas pixels and then mapped pixels-per-tick
 * through a tuned curve to get a launch speed. Here the samples are the ball's own
 * position in METRES, so the release velocity is not a mapping of the gesture — it
 * *is* the ball's velocity, read straight off the motion the player just made. Throw
 * it hard and it leaves fast; ease it out and it drifts. Nothing in between
 * interprets you.
 *
 * `releaseVelocity` does NOT just difference the last two samples: it averages the
 * per-pair velocities across the last `MOTION_SAMPLE_WINDOW` samples with a
 * triangular (middle-emphasised) weighting, so a single jittery sample at lift-off —
 * a finger stopping dead a frame before it releases — cannot dominate the throw. That
 * weighting is inherited directly from the cabinet, where it did the same job.
 */

import { type Vec3, scale, sub, vec3 } from "./vec.ts";
import { MOTION_HISTORY, MOTION_SAMPLE_WINDOW } from "./constants.ts";

interface Sample {
  readonly pos: Vec3;
  readonly tick: number;
}

/** A fixed-capacity history of held-ball positions with a smoothed velocity estimate. */
export class BallMotion {
  readonly #samples: Sample[] = [];

  /** The number of retained samples (never exceeds `MOTION_HISTORY`). */
  public get size(): number {
    return this.#samples.length;
  }

  /** Drop all samples (on grab, on release, and at the start of each drop). */
  public clear(): void {
    this.#samples.length = 0;
  }

  /** Record where the ball is on `tick`, evicting the oldest to stay within capacity. */
  public push(pos: Vec3, tick: number): void {
    this.#samples.push({ pos, tick });
    if (this.#samples.length > MOTION_HISTORY) {
      this.#samples.shift();
    }
  }

  /**
   * The smoothed release velocity in m/s: a triangular-weighted average of the
   * per-pair velocities over the last `MOTION_SAMPLE_WINDOW` samples. Returns zero
   * with fewer than two usable samples — a ball that was grabbed and released without
   * moving is simply dropped.
   */
  public releaseVelocity(secondsPerTick: number): Vec3 {
    const n = this.#samples.length;
    if (n < 2) {
      return vec3(0, 0, 0);
    }
    const start = Math.max(0, n - (MOTION_SAMPLE_WINDOW + 1));
    const window = this.#samples.slice(start);
    const pairs = window.length - 1;

    let sum = vec3(0, 0, 0);
    let weightSum = 0;
    for (let j = 0; j < pairs; j += 1) {
      const a = window[j]!;
      const b = window[j + 1]!;
      const span = b.tick - a.tick;
      if (span <= 0) {
        continue;
      }
      // Triangular weight: peaks mid-window, so neither the first (still accelerating)
      // nor the last (lift-off jitter) pair can dominate.
      const weight = Math.min(j + 1, pairs - j);
      const perTick = scale(sub(b.pos, a.pos), 1 / span);
      sum = vec3(
        sum.x + weight * perTick.x,
        sum.y + weight * perTick.y,
        sum.z + weight * perTick.z,
      );
      weightSum += weight;
    }
    if (weightSum <= 0) {
      return vec3(0, 0, 0);
    }
    // Samples are metres-per-tick; scale to metres-per-second.
    return scale(sum, 1 / (weightSum * secondsPerTick));
  }
}
