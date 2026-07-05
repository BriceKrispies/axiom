/*
 * The hip-bob smoothing primitive: a critically-damped spring (Unity-style
 * `SmoothDamp`). TypeScript port of the Rust lab's `hip_spring.rs`.
 *
 * The gait produces a RAW hip height that steps abruptly between its planted and
 * swing levels; feeding that step through this spring one tick at a time turns it
 * into a gentle, overshoot-free bob — the "spring smoothing" that makes the root
 * read as intentionally animated rather than snapped. It is the single piece of
 * per-tick state in the lab; everything else is a closed form of the tick.
 */

/** One fixed simulation tick as the unit of time (`dt = 1`, since the gait is tick-driven). */
const TICK_DT = 1;

/** A critically-damped spring that eases a scalar toward a moving target, one tick at a time. */
export class HipBobSpring {
  private value: number;
  private velocity: number;

  /** A spring resting at `initial` with zero velocity. */
  constructor(initial: number) {
    this.value = initial;
    this.velocity = 0;
  }

  /** The current smoothed value. */
  current(): number {
    return this.value;
  }

  /** Advance one tick toward `target` with smoothing time `smoothing` (ticks); larger is smoother/slower. */
  step(target: number, smoothing: number): number {
    const smoothTime = Math.max(smoothing, 1e-3);
    const omega = 2 / smoothTime;
    const x = omega * TICK_DT;
    const exp = 1 / (1 + x + 0.48 * x * x + 0.235 * x * x * x);
    const change = this.value - target;
    const temp = (this.velocity + omega * change) * TICK_DT;
    this.velocity = (this.velocity - omega * temp) * exp;
    this.value = target + (change + temp) * exp;
    return this.value;
  }
}
