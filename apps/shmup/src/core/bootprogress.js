/**
 * BOOT PROGRESS — a loading bar that is actually telling the truth.
 *
 * Most loading bars count steps: "7 of 12 done" is 58%. That is a lie whenever
 * the steps differ in cost, and here they differ by two orders of magnitude —
 * `world:gate` is 18 ms and `world:buildings` is 549 ms. A step counter would
 * sprint to 80% and then sit there.
 *
 * So this weights every phase by how long it ACTUALLY TAKES, measured. The
 * weights in `bootweights.js` are generated from a real profile by
 * `node tools/bootprofile.mjs --emit-weights`, so the bar is calibrated by the
 * same instrument that measures the boot it describes, and re-calibrating after
 * a change is one command rather than a guess.
 *
 * It rides on the profiler's spans rather than on its own instrumentation:
 * every phase worth showing a player is already bracketed by `boot.time(...)`,
 * and a weight table generated from those same spans cannot drift out of sync
 * with them.
 *
 * FOUR THINGS MAKE IT ACCURATE RATHER THAN MERELY WEIGHTED
 *
 *  1. SUB-PHASE MOTION. `init:world` is over a second. A bar that jumps once
 *     when it ends is a bar that sits still for a second, so `BOOT_CHILDREN`
 *     lets the big phases report progress from the inside.
 *
 *  2. EXACT SUB-PROGRESS WHERE IT EXISTS. The phase that dominates a COLD boot
 *     is the GPU driver linking ~110 shader programs, and that one can be
 *     counted precisely — three hands back the set of materials it is compiling
 *     and each program answers `isReady()`. So during the longest and most
 *     opaque part of the wait, the bar moves on real completions. See
 *     `prewarmScene()` in prewarm.js.
 *
 *  3. CALIBRATION TO THIS MACHINE. The reference weights come from one GPU.
 *     As phases complete, the ratio of actual to expected is folded into a
 *     running scale factor and applied to everything outstanding, so a slower
 *     machine gets a correctly paced bar after the first couple of phases
 *     instead of stalling at the end. `reprice()` handles the one phase that
 *     varies by an order of magnitude rather than a factor.
 *
 *  4. IT NEVER GOES BACKWARDS. Re-pricing changes the denominator, which can
 *     mathematically move the fraction down. A bar that retreats reads as
 *     broken even when it is more truthful, so the reported value is clamped
 *     monotonically: a downward correction shows up as the bar slowing, which
 *     is the same information without the alarm.
 *
 * THE FINISH LINE IS THE FIRST PAINTED FRAME, not "fully loaded". Progressive
 * boot means the game is playable while weapons, navigation and the rest of the
 * pre-warm stream in behind it (see streaming.js), so the loading screen has
 * done its job the moment there is a game on screen. What comes after is
 * reported separately and unobtrusively.
 */

import { BOOT_WEIGHTS, BOOT_CHILDREN, BOOT_TOTAL_MS } from './bootweights.js';

/** What a player should read while a phase runs. Anything unlisted is skipped. */
const LABELS = {
  'bakery.ready': 'starting workers',
  'init:render': 'creating the renderer',
  'init:materials': 'preparing surfaces',
  'init:sky': 'building the sky',
  'init:physics': 'preparing collision',
  'init:world': 'building the level',
  'init:player': 'placing the player',
  'init:weapons': 'preparing weapons',
  'init:fx': 'preparing effects',
  'init:ai': 'preparing enemies',
  'init:ui': 'building the HUD',
  'engine.attach': 'wiring input',
  'prewarm.scene': 'compiling shaders',
  'boot-frames': 'drawing the first frame',
};

/** Finer labels for the sub-phases of the long ones. */
const CHILD_LABELS = {
  'world:registerProps': 'registering props',
  'world:ground': 'laying the ground',
  'world:buildings': 'raising buildings',
  'world:gate': 'building the gate',
  'world:perimeter': 'building the perimeter',
  'world:dressStreet': 'dressing the street',
  'world:dressBuildings': 'dressing interiors',
  'world:debris': 'scattering debris',
  'world:lights': 'placing lights',
  'world:finalize': 'merging geometry',
  'prewarmScene:compile': 'compiling shaders',
};

/**
 * Labels for sub-phases that are generated in a series — one span per building,
 * per bake — where naming each one individually would be a list that rots the
 * moment the level gains a building.
 */
const CHILD_PATTERNS = [
  [/^world:building\d+$/, 'raising buildings'],
  [/^mat:(alloc|bake):/, 'baking surfaces'],
  [/^weapons:(model|add):/, 'building weapons'],
];

export class BootProgress {
  constructor({ onChange = () => {} } = {}) {
    this.onChange = onChange;
    this.weights = { ...BOOT_WEIGHTS };
    this.doneMs = 0;
    this.current = null;
    this.currentFrac = 0;
    this.label = 'loading';
    /** actual/expected on this machine, learned as phases complete. */
    this.scale = 1;
    this._scaleSamples = 0;
    this._reported = 0;
    this._startedAt = performance.now();
    this._phaseStart = 0;
    /** Sub-phase accounting for whichever big phase is open. */
    this._childWeights = null;
    this._childTotal = 0;
    this._childDone = 0;
    this.finished = false;
  }

  /**
   * Drive this from a `boot` profiler. Every phase in the weight table becomes
   * a bar segment; everything else is ignored, so adding a span cannot
   * accidentally change the bar.
   */
  attach(boot) {
    return boot.observe((event, name) => {
      if (this.finished) return;
      if (event === 'b') {
        if (this.weights[name] !== undefined) this._begin(name);
        else if (this._childWeights?.[name] !== undefined) this._childBegin(name);
      } else if (event === 'e') {
        if (name === this.current) this._end(name);
        else if (this._childWeights?.[name] !== undefined) this._childEnd(name);
      }
    });
  }

  get totalMs() {
    return Object.values(this.weights).reduce((a, b) => a + b, 0) || BOOT_TOTAL_MS;
  }

  _expected(name) {
    return (this.weights[name] ?? 0) * this.scale;
  }

  _begin(name) {
    // A phase opening while another is open means the previous had no explicit
    // end. Close it at full expected cost rather than losing it.
    if (this.current) this._end(this.current);
    this.current = name;
    // A phase with no label still counts toward the bar, but it does not get to
    // put a span name on screen. `engine.collectStream` means nothing to a
    // player, and phases like it are sub-millisecond anyway — keeping the
    // previous label is both truer and quieter than showing an identifier.
    if (LABELS[name]) this.label = LABELS[name];
    this.currentFrac = 0;
    this._phaseStart = performance.now();
    this._childWeights = BOOT_CHILDREN[name] ?? null;
    this._childTotal = this._childWeights
      ? Object.values(this._childWeights).reduce((a, b) => a + b, 0)
      : 0;
    this._childDone = 0;
    this._emit();
  }

  _childBegin(name) {
    const l = CHILD_LABELS[name] ?? CHILD_PATTERNS.find((r) => r[0].test(name))?.[1];
    if (l) {
      this.label = l;
      this._emit();
    }
  }

  _childEnd(name) {
    this._childDone += this._childWeights[name] ?? 0;
    if (this._childTotal > 0) this.advance(this._childDone / this._childTotal);
  }

  /** Report exact progress inside the open phase, 0..1. */
  advance(frac) {
    if (this.finished || !this.current) return;
    const next = Math.min(1, Math.max(0, frac));
    if (next <= this.currentFrac) return;
    this.currentFrac = next;
    this._emit();
  }

  _end(name) {
    if (this.current !== name) return;
    const actual = performance.now() - this._phaseStart;
    const expected = this._expected(name);
    this.doneMs += this.weights[name] ?? 0;
    // Only phases with a meaningful reference cost teach us anything: a 2 ms
    // phase that took 6 ms is measurement noise, not a slow machine.
    if ((this.weights[name] ?? 0) >= 100 && actual > 0 && expected > 0) {
      this._scaleSamples++;
      this.scale *= 1 + (actual / expected - 1) / this._scaleSamples;
      this.scale = Math.min(20, Math.max(0.2, this.scale));
    }
    this.current = null;
    this.currentFrac = 0;
    this._childWeights = null;
    this._emit();
  }

  /**
   * Re-price a phase that turns out to cost far more than the reference.
   *
   * For the shader link specifically: it is ~0.6 s on a reload and ~7 s on a
   * first visit, because the difference is whether the GPU driver already has
   * the programs in its on-disk cache. No table can predict which one a given
   * load is, but the phase reports how many programs are done, so after a few
   * of them the real cost is knowable and the rest of the bar can be re-paced
   * around it. Without this the bar would reach 90% and then wait seven
   * seconds, which is exactly the failure it exists to prevent.
   */
  reprice(name, ms) {
    if (this.finished || this.weights[name] === undefined) return;
    const scaled = ms / Math.max(0.001, this.scale);
    if (scaled <= this.weights[name]) return; // only ever widen; see the clamp
    this.weights[name] = scaled;
    this._emit();
  }

  get fraction() {
    const total = Math.max(1, this.totalMs);
    const open = this.current ? (this.weights[this.current] ?? 0) * this.currentFrac : 0;
    const raw = (this.doneMs + open) / total;
    this._reported = Math.max(this._reported, Math.min(0.995, raw));
    return this._reported;
  }

  finish() {
    if (this.finished) return;
    this.finished = true;
    this._reported = 1;
    this.label = 'ready';
    this.onChange(1, this.label, performance.now() - this._startedAt);
  }

  _emit() {
    this.onChange(this.fraction, this.label, performance.now() - this._startedAt);
  }
}
