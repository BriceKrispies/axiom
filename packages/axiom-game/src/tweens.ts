/*
 * The tick-sampled tween projection (SPEC-09 §4.2). `Sim.tweens.add(spec)`
 * registers an interpolation over `[from, to]`; the native `TweenApi` owns the
 * eased curve and reports, each fixed tick, which tweens to sample and their
 * eased value. The TS layer only holds the author's `onUpdate`/`onComplete`
 * closures and applies the sampled value — the easing math is NOT re-implemented
 * here (it routes to the bridge, like the RNG draw sequence).
 *
 * The same pump cadence as timers drives sampling: `TickPump.pump(tick)` asks the
 * bridge for the active and completed tween ids and dispatches the held closures
 * over them with `each`/`whenPresent`, never an `if`. `duration` is authored in
 * seconds and converted to whole ticks against the loop's `fixedHz` at `add`
 * time, so a tween advances deterministically on the fixed tick.
 */

import type { Handle } from "./vocabulary.ts";
import type { TickPump } from "./pump.ts";

/** The easing curves the native `TweenApi` samples (SPEC-09 §4.2). */
export type Ease =
  | "backOut"
  | "cubicOut"
  | "expoOut"
  | "linear"
  | "quadIn"
  | "quadInOut"
  | "quadOut";

/*
 * The easing names in their dense native index order. `add` maps an author's
 * `Ease` to its index with `EASES.indexOf(...)` and passes the index across the
 * bridge; the native core selects the curve by index. The ORDER here is the
 * contract — index 0 is `linear`, the default when `ease` is omitted.
 */
export const EASES: readonly Ease[] = [
  "linear",
  "quadIn",
  "quadOut",
  "quadInOut",
  "cubicOut",
  "expoOut",
  "backOut",
];

/** A tween description: endpoints, duration (seconds), easing, and sink closures. */
export interface TweenSpec {
  /** The value at the start of the tween. */
  readonly from: number;
  /** The value at the end of the tween. */
  readonly to: number;
  /** The tween's duration in seconds (converted to whole fixed ticks at `add`). */
  readonly duration: number;
  /** The easing curve; defaults to `linear` when omitted. */
  readonly ease?: Ease;
  /** Receives the eased value each tick the tween is sampled. */
  readonly onUpdate: (value: number) => void;
  /** Runs once when the tween reaches its end. */
  readonly onComplete?: () => void;
}

/** The tween factory on `Sim.tweens` (SPEC-09 §4.2). */
export interface Tweens {
  /** Register a tween, returning its handle; it is sampled each fixed tick. */
  readonly add: (spec: TweenSpec) => Handle;
  /** Cancel a tween so it stops sampling (a stale handle is a clean no-op). */
  readonly cancel: (id: Handle) => void;
}

/** Build the `Tweens` projection bound to `pump` and the running `tick`. */
export const makeTweens = (pump: TickPump, tick: number): Tweens => ({
  add: (spec: TweenSpec): Handle => pump.addTween(tick, spec),
  cancel: (id: Handle): void => {
    pump.cancelTween(id);
  },
});
