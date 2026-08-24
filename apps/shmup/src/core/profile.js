/**
 * BOOT PROFILER — where the seconds before the first frame actually go.
 *
 * WHY THIS EXISTS. Boot is tens of seconds cold, and the only visibility into
 * it was a scatter of `console.info('[x] built in Nms')` lines. Those tell you
 * a subsystem was slow; they do not tell you *what kind of work* was slow, and
 * they cannot be summed because they overlap — the materials texture bakes
 * happen INSIDE world's init, so `[world] built in 4600ms` already contains
 * `[materials] bake ... 3.5s`. Adding those two numbers double-counts.
 *
 * This module records a real nested span TREE with:
 *   - wall time and SELF time (wall minus children), so overlap stops lying;
 *   - the GPU work each span caused — shader compiles, program links, the
 *     blocking link-status poll, texture uploads and their bytes, draw calls,
 *     sync stalls — attributed to whichever span was open when the call was
 *     made (see glprobe.js);
 *   - long tasks that landed inside the span, so a stall with no instrumented
 *     cause is still visible;
 *   - arbitrary notes (`boot.note('tris', 614000)`).
 *
 * Every span is also emitted as a `performance.mark`/`measure` pair, so the
 * same tree shows up in the DevTools performance panel and in a CDP trace
 * without a second instrumentation pass.
 *
 * COST. A span is two `performance.now()` calls, one object and two marks.
 * Boot opens on the order of a hundred spans, so the instrument is sub-ms
 * against a multi-second boot. It is always on: a profiler you have to enable
 * is a profiler that is off the one time you needed it.
 *
 * READ IT WITH: `node tools/bootprofile.mjs`.
 */

const now = () => performance.now();

/** A single node in the boot tree. */
class Span {
  constructor(name, parent, depth) {
    this.name = name;
    this.parent = parent;
    this.depth = depth;
    this.t0 = now();
    this.t1 = -1;
    this.children = [];
    this.notes = null;
    /** GL counters at open; the delta is what this span caused. */
    this.gl0 = null;
    this.gl = null;
    this.longTasks = [];
    this.error = null;
  }

  get wall() {
    return (this.t1 < 0 ? now() : this.t1) - this.t0;
  }

  /** Wall time not accounted for by any child span. Detached children ran
   *  alongside this span rather than inside it, so they are not subtracted. */
  get self() {
    return this.wall - this.children.reduce((a, c) => a + (c.detached ? 0 : c.wall), 0);
  }
}

/**
 * The `gl` field of every span is a delta of these counters. A flat object of
 * numbers, so subtraction is one loop and the JSON stays small.
 */
export const GL_COUNTERS = {
  shaderCompiles: 0,
  shaderCompileMs: 0,
  programLinks: 0,
  programLinkMs: 0,
  /** Time blocked inside getProgramParameter(LINK_STATUS) — where a link the
   *  driver deferred actually lands. On a parallel-compile driver this, not
   *  programLinkMs, is the real cost of a program. */
  linkStatusWaits: 0,
  linkStatusMs: 0,
  /** Same, for the KHR_parallel_shader_compile completion poll. */
  completionPolls: 0,
  completionMs: 0,
  texUploads: 0,
  texUploadMs: 0,
  texBytes: 0,
  bufferUploads: 0,
  bufferUploadMs: 0,
  bufferBytes: 0,
  compileStatusWaits: 0,
  compileStatusMs: 0,
  readPixels: 0,
  readPixelsMs: 0,
  drawCalls: 0,
  /** Timed on purpose during boot — a driver that defers program linking does
   *  the link on the first draw that uses the program. See glprobe.js. */
  drawMs: 0,
  programBinds: 0,
  programBindMs: 0,
  /** Uniform/attribute reflection. On several drivers this — not LINK_STATUS —
   *  is where a deferred program link is actually forced to complete. */
  programQueries: 0,
  programQueryMs: 0,
  /** Distinct WebGLPrograms ever passed to useProgram. See glprobe.js. */
  distinctPrograms: 0,
  fenceWaits: 0,
  fenceWaitMs: 0,
  finishes: 0,
  finishMs: 0,
};

const COUNTER_KEYS = Object.keys(GL_COUNTERS);

const diffCounters = (a, b) => {
  const out = {};
  let any = false;
  for (const k of COUNTER_KEYS) {
    const d = b[k] - a[k];
    if (d !== 0) {
      out[k] = k.endsWith('Ms') ? +d.toFixed(2) : d;
      any = true;
    }
  }
  return any ? out : null;
};

class BootProfiler {
  constructor() {
    this.root = new Span('boot', null, 0);
    this.stack = [this.root];
    /** Live GL counters, mutated in place by glprobe.js. */
    this.counters = { ...GL_COUNTERS };
    this.root.gl0 = { ...this.counters };
    this.longTasks = [];
    this.finished = false;
    this.samples = null;
    /**
     * Named instants, in ms since this profiler was constructed.
     *
     * Progressive boot made "how long is boot" two different questions with two
     * different answers: when the player first sees the game, and when the last
     * deferred thing finishes loading behind them. One number cannot stand for
     * both, and the first one is the one worth optimising.
     */
    this.milestones = {};
    /**
     * Span listeners. The loading bar rides on these rather than on a second
     * set of hand-placed calls: every phase worth showing a player is already
     * bracketed by a span, and a weight table generated from a profile of those
     * same spans therefore cannot drift out of sync with them.
     */
    this._observers = [];
    this._observeLongTasks();
    this._startSampler();
  }

  /**
   * SAMPLED PROFILE — the half of the picture hand-instrumentation cannot give.
   *
   * The span tree only knows about work someone thought to wrap. The JS
   * Self-Profiling API samples the real stack on a timer, so a 4-second span
   * with no children still gets broken down by function and by source file.
   * That is how you find the cost you did not predict.
   *
   * Off unless `?jsprofile=1`, because sampling is not free and — more
   * importantly — the buffer it fills is proportional to boot length. The
   * header that permits it (`Document-Policy: js-profiling`) is set by the dev
   * server; without it the constructor throws and we simply carry on with the
   * span tree alone.
   */
  _startSampler() {
    try {
      const on = new URLSearchParams(location.search).get('jsprofile') === '1';
      if (!on || typeof Profiler !== 'function') return;
      const interval = Number(new URLSearchParams(location.search).get('jsinterval') ?? 10);
      // eslint-disable-next-line no-undef
      this._sampler = new Profiler({ sampleInterval: interval, maxBufferSize: 1_500_000 });
    } catch (err) {
      // No header, no support, or the buffer was refused. Not fatal.
      this._samplerError = String(err?.message ?? err);
    }
  }

  get current() {
    return this.stack[this.stack.length - 1];
  }

  _observeLongTasks() {
    // Long tasks report the stalls we did NOT instrument. A phase with 4 s of
    // wall time and 3.9 s of long tasks it cannot explain is a phase whose real
    // cost lives somewhere this file never looked.
    try {
      const obs = new PerformanceObserver((list) => {
        for (const e of list.getEntries()) {
          const rec = { start: +e.startTime.toFixed(1), ms: +e.duration.toFixed(1) };
          this.longTasks.push(rec);
          // Attribute to every span open across the task's midpoint.
          const mid = e.startTime + e.duration / 2;
          for (const s of this.stack) {
            if (s.t0 <= mid) s.longTasks.push(rec);
          }
        }
      });
      obs.observe({ type: 'longtask', buffered: true });
      this._obs = obs;
    } catch {
      // Firefox/Safari have no longtask entry type. The tree still works.
    }
  }

  /**
   * Open a span.
   *
   * `detached` is for work that runs CONCURRENTLY with the rest of boot rather
   * than inside it — the shader pre-warm, once progressive boot stopped
   * awaiting it. A stack cannot represent that: the pre-warm span would still be
   * open when the frame loop starts, so every later span would nest inside it
   * and its wall time would swallow theirs. A detached span hangs off the root,
   * keeps its own start and end, and never becomes anyone's parent — so
   * `selfMs` on the sequential spine stays meaningful and the concurrent phase
   * is still measured.
   */
  begin(name, { detached = false } = {}) {
    // After finish() the tree is published and immutable. Work that starts
    // later — pre-warm, in the progressive-boot path — is not boot, and
    // attaching it would both corrupt the snapshot's arithmetic and grow the
    // tree for the rest of the session. Hand back a span nobody is holding.
    if (this.finished) return new Span(name, this.root, 1);
    const parent = detached ? this.root : this.current;
    const s = new Span(name, parent, parent.depth + 1);
    s.detached = detached;
    s.gl0 = { ...this.counters };
    parent.children.push(s);
    if (!detached) this.stack.push(s);
    try { performance.mark(`${name}:b`); } catch { /* mark budget exhausted */ }
    this._notify('b', s);
    return s;
  }

  end(span) {
    if (span?.detached) {
      if (span.t1 < 0) {
        span.t1 = now();
        span.gl = diffCounters(span.gl0, this.counters);
        this._notify('e', span);
      }
      return span;
    }
    // Tolerate a mismatched end (a throw inside a span that was not wrapped):
    // unwind to the named span rather than corrupting the stack for everything
    // after it. A profiler must never be the reason a boot fails.
    const i = span ? this.stack.lastIndexOf(span) : this.stack.length - 1;
    const target = i > 0 ? i : this.stack.length - 1;
    while (this.stack.length > Math.max(1, target)) {
      const s = this.stack.pop();
      s.t1 = now();
      s.gl = diffCounters(s.gl0, this.counters);
      try {
        performance.mark(`${s.name}:e`);
        performance.measure(s.name, `${s.name}:b`, `${s.name}:e`);
      } catch { /* nothing depends on the mark surviving */ }
      this._notify('e', s);
    }
    return span;
  }

  /** Time a synchronous function. Returns whatever it returns. */
  time(name, fn, opts) {
    const s = this.begin(name, opts);
    try {
      return fn();
    } catch (err) {
      s.error = String(err?.message ?? err);
      throw err;
    } finally {
      this.end(s);
    }
  }

  /** Time an async function. Returns a promise for whatever it returns. */
  async timeAsync(name, fn, opts) {
    const s = this.begin(name, opts);
    try {
      return await fn();
    } catch (err) {
      s.error = String(err?.message ?? err);
      throw err;
    } finally {
      this.end(s);
    }
  }

  /**
   * Subscribe to span open/close: `fn(event, name, span)` with event 'b' | 'e'.
   *
   * The loading bar rides on these rather than on a second set of hand-placed
   * calls. Every phase worth showing a player is already bracketed by a span,
   * and the bar's weight table is generated from a profile of those same spans
   * (`tools/bootprofile.mjs --emit-weights`), so the two cannot drift apart.
   */
  observe(fn) {
    this._observers.push(fn);
    return () => {
      const i = this._observers.indexOf(fn);
      if (i >= 0) this._observers.splice(i, 1);
    };
  }

  _notify(event, span) {
    for (const fn of this._observers) {
      try {
        fn(event, span.name, span);
      } catch (err) {
        // A listener must never be able to break the thing it is watching.
        console.warn('[boot] span observer threw:', err);
      }
    }
  }

  /**
   * Record a named instant. First write wins, so a milestone cannot drift.
   *
   * Milestones outlive `finish()` on purpose: `loaded` lands seconds after the
   * first playable frame closed the tree, and a published profile that cannot
   * say when loading actually finished is the exact blind spot progressive boot
   * would otherwise create.
   */
  milestone(name) {
    if (this.milestones[name] === undefined) {
      this.milestones[name] = +(now() - this.root.t0).toFixed(1);
      try {
        if (window.__BOOTPROFILE__) window.__BOOTPROFILE__.milestones = { ...this.milestones };
      } catch { /* non-browser */ }
    }
    return this.milestones[name];
  }

  /** Attach a value to the currently open span. */
  note(key, value) {
    const s = this.current;
    (s.notes ??= {})[key] = value;
    return value;
  }

  /** A zero-length span, for a point event worth seeing on the timeline. */
  mark(name, notes) {
    const s = this.begin(name);
    if (notes) s.notes = notes;
    this.end(s);
  }

  /** Close everything still open and freeze the tree. */
  finish(reason = 'ready') {
    if (this.finished) return this.snapshot();
    while (this.stack.length > 1) this.end(this.current);
    this.root.t1 = now();
    this.root.gl = diffCounters(this.root.gl0, this.counters);
    this.root.notes = { ...(this.root.notes ?? {}), reason };
    this.finished = true;
    try { this._obs?.disconnect(); } catch { /* already gone */ }
    const snap = this.snapshot();
    try { window.__BOOTPROFILE__ = snap; } catch { /* non-browser */ }

    // The sampler stops asynchronously. Publish the trace on its own global and
    // a done flag rather than making finish() async — finish() is called from a
    // rAF callback that must raise __READY__ on the same frame.
    try { window.__BOOTSAMPLES_DONE__ = !this._sampler; } catch { /* non-browser */ }
    this._sampler
      ?.stop()
      .then((trace) => {
        this.samples = trace;
        window.__BOOTSAMPLES__ = trace;
      })
      .catch((err) => {
        window.__BOOTSAMPLES__ = { error: String(err?.message ?? err) };
      })
      .finally(() => {
        window.__BOOTSAMPLES_DONE__ = true;
      });
    return snap;
  }

  /** Plain-JSON tree, safe to hand across the CDP boundary. */
  snapshot() {
    const walk = (s) => ({
      name: s.name,
      start: +(s.t0 - this.root.t0).toFixed(1),
      ms: +s.wall.toFixed(1),
      selfMs: +s.self.toFixed(1),
      ...(s.detached ? { detached: true } : {}),
      ...(s.gl ? { gl: s.gl } : {}),
      ...(s.notes ? { notes: s.notes } : {}),
      ...(s.error ? { error: s.error } : {}),
      ...(s.longTasks.length
        ? { longTaskMs: +s.longTasks.reduce((a, t) => a + t.ms, 0).toFixed(1) }
        : {}),
      ...(s.children.length ? { children: s.children.map(walk) } : {}),
    });
    return {
      /** ms from navigation start to the profiler's own construction. Whatever
       *  happened before this — fetching and evaluating the module graph — is
       *  boot time this tree does not contain, and the tool reports it. */
      origin: +this.root.t0.toFixed(1),
      totalMs: +this.root.wall.toFixed(1),
      tree: walk(this.root),
      longTasks: this.longTasks,
      milestones: { ...this.milestones },
      counters: { ...this.counters },
      sampled: !!this._sampler,
      ...(this._samplerError ? { samplerError: this._samplerError } : {}),
    };
  }
}

/** The one boot profiler. Import it anywhere; it is constructed on module eval,
 *  which is itself the first thing in the boot it is measuring. */
export const boot = new BootProfiler();

// Handy for poking at boot from the console mid-load.
try { window.__BOOT__ = boot; } catch { /* non-browser (node import from a tool) */ }
