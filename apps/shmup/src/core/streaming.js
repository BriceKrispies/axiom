/**
 * PROGRESSIVE BOOT — the work that does not have to happen before the first
 * frame, spread across the frames after it.
 *
 * WHY. Measured with tools/bootprofile.mjs, boot is ~4.4 s of main-thread
 * JavaScript and almost none of it is waiting on anything. Roughly half builds
 * things the player cannot see on frame 1: the two weapon viewmodels that are
 * not in their hands (0.5 s), the navigation grid and garrison for enemies that
 * have not engaged yet (0.6 s), and the shader pre-warm (0.6 s). Every
 * millisecond of that is a millisecond of black screen, and none of it is a
 * millisecond the player needed.
 *
 * So the engine stops waiting for it. A subsystem declares the deferrable half
 * of its own construction as a generator, the frame loop starts as soon as the
 * critical half is done, and this drains the generators a few milliseconds at a
 * time between frames.
 *
 * THE CONTRACT is a generator that does work and yields a label at each point
 * where it is safe to stop:
 *
 *   *stream(ctx) {
 *     this._buildWeapon('rifle'); yield 'rifle';
 *     this._buildWeapon('smg');   yield 'smg';
 *   }
 *
 * A yield is a suspension point, not a task boundary, so a subsystem gets to
 * decide its own granularity and keeps ordinary local state across chunks —
 * which is the whole reason this is a generator and not a queue of callbacks.
 *
 * THE BUDGET IS BETWEEN CHUNKS, NOT INSIDE THEM. One `next()` runs to its
 * yield, however long that takes; the budget only decides whether to run
 * another one this frame. A subsystem that yields once per 200 ms of work will
 * hitch for 200 ms no matter what the budget says, which is why `worstChunkMs`
 * is reported — it names the generator that needs to yield more often.
 *
 * DETERMINISM. Streaming changes WHEN work happens, never what it does or in
 * what order. Generators are drained in dependency order and each runs to
 * completion before the next starts, so the sequence of RNG draws is exactly
 * the sequence the old inline code made. Capture mode drains the whole thing
 * before raising `__READY__`, so the pixel gate sees a fully built world —
 * see `Engine.drainStream()`.
 */

/**
 * Yield this to say "not ready yet, ask me again next frame".
 *
 * A generator that is waiting on something asynchronous — a worker bake, a
 * fetch — cannot block, and must not busy-spin either: the drain loop would
 * burn the whole frame budget calling `next()` on a generator that is only
 * going to say "still waiting". Yielding WAIT ends the drain for this frame, so
 * a wait costs exactly one `next()` per frame.
 */
export const WAIT = Symbol('stream:wait');

export class Streamer {
  /**
   * @param opts.budgetMs  how long per frame to spend draining. The default is
   *   deliberately under half a 60 Hz frame: the point is a game that runs
   *   while it finishes loading, not one that finishes loading fractionally
   *   sooner while stuttering.
   */
  constructor({ budgetMs = 6 } = {}) {
    this.budgetMs = budgetMs;
    this._jobs = [];
    this._i = 0;
    this.done = false;
    this.stats = {
      chunks: 0,
      totalMs: 0,
      worstChunkMs: 0,
      worstChunk: null,
      /** Per generator, so a slow one is attributable. */
      byName: {},
    };
  }

  /** Register a generator under a name. Ignores a non-generator. */
  add(name, gen) {
    if (!gen || typeof gen.next !== 'function') return this;
    this._jobs.push({ name, gen, done: false });
    this.done = false;
    return this;
  }

  get pending() {
    return this._jobs.length - this._i;
  }

  /** Advance one chunk. Returns false once everything is drained. */
  _one() {
    while (this._i < this._jobs.length) {
      const job = this._jobs[this._i];
      const t0 = performance.now();
      let res;
      try {
        res = job.gen.next();
      } catch (err) {
        // A generator that throws must not strand the ones behind it, and must
        // not take the session with it: the game is already on screen.
        console.error(`[stream] "${job.name}" threw, skipping the rest of it:`, err);
        res = { done: true };
      }
      const ms = performance.now() - t0;
      // `res.value` may be the WAIT symbol, and a template literal throws on a
      // Symbol rather than stringifying it.
      const tag = res.value === WAIT ? 'wait' : (res.value ?? (res.done ? 'end' : '?'));
      const label = `${job.name}:${String(tag)}`;
      this.stats.chunks++;
      this.stats.totalMs += ms;
      this.stats.byName[job.name] = (this.stats.byName[job.name] ?? 0) + ms;
      if (ms > this.stats.worstChunkMs) {
        this.stats.worstChunkMs = ms;
        this.stats.worstChunk = label;
      }
      if (res.done) {
        job.done = true;
        this._i++;
        // A generator's final `next()` usually did no work; keep going rather
        // than spending a whole frame's budget on a return statement.
        if (ms < 1) continue;
      }
      this._waiting = res.value === WAIT;
      return true;
    }
    this.done = true;
    return false;
  }

  /**
   * Drain for up to `budgetMs`. Call once per frame, after the frame has been
   * rendered. Returns true while there is still work left.
   */
  step() {
    if (this.done) return false;
    const until = performance.now() + this.budgetMs;
    // Always run at least one chunk, so progress cannot stall on a frame that
    // was already over budget before streaming got a turn.
    do {
      if (!this._one()) return false;
      // A generator that is waiting on something async ends the frame; see WAIT.
      if (this._waiting) return true;
    } while (performance.now() < until);
    return true;
  }

  /**
   * Run everything to completion. Used by the capture harness.
   *
   * Async, because a generator may yield WAIT for something only a settled
   * promise can provide — a worker bake, most of all. A synchronous drain would
   * spin on that forever, since nothing can deliver the worker's message while
   * the main thread is inside the loop. Yielding to the macrotask queue between
   * waits is what lets it land.
   */
  async drainAll() {
    while (this._one()) {
      if (this._waiting) await new Promise((r) => setTimeout(r, 0));
    }
    return this.stats;
  }
}
