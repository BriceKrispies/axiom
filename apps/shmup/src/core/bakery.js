/**
 * THE BAKERY — a worker pool for pure seed-to-bytes work.
 *
 * WHY. Measured with tools/bootprofile.mjs, the largest single block of real
 * main-thread work in this app's boot is procedural texture generation:
 * `src/ai/textures.js` alone is ~2.6 s of value-noise evaluation, and the two
 * fx sprite atlases are another ~1.1 s. That is ~40% of all the JavaScript boot
 * executes, and none of it touches the DOM, WebGL or any shared state — it is a
 * pure function from a seed to a pile of bytes.
 *
 * Pure functions from a seed to bytes are what a worker is for, and there are
 * several of them, so they parallelise across cores as well as getting off the
 * critical thread. While they run, the main thread is free to do the work that
 * genuinely cannot move: building the level, the viewmodels and the collision
 * BVH, all of which construct THREE object graphs.
 *
 * THIS FILE KNOWS NOTHING ABOUT TEXTURES. `src/core/` is shared substrate and
 * does not import subsystems (see ARCHITECTURE.md), so the pool is generic: it
 * is handed a worker URL and a registry of bakers by the composition root, the
 * same way the Engine is handed its subsystems. The recipes live in
 * `src/bakers.js`.
 *
 * DETERMINISM IS NOT NEGOTIABLE HERE. The pixel gate (tools/baseline.mjs +
 * tools/imagediff.mjs) requires byte-identical output, and this app's capture
 * story rests on a reproducible RNG stream. Two things make it safe:
 *
 *  1. A job carries a SEED, not an Rng. The caller draws exactly one `u32()`
 *     from its own stream — the same draw `rng.fork()` was already making — so
 *     the caller's stream advances exactly as it did before. The worker
 *     rebuilds the identical `Rng` from that seed.
 *  2. The worker runs THE SAME MODULE the main thread would have run. It is not
 *     a reimplementation: the worker and the synchronous fallback below call one
 *     function from one registry. Same code, same V8, same IEEE-754 doubles,
 *     same bytes.
 *
 * FALLBACK. If `Worker` is missing, the module fails to load, or a job throws,
 * the job is re-run synchronously on the main thread. Boot gets slower; it does
 * not get different, and it does not fail. The fallback shares the registry
 * rather than having one of its own — a fallback that can drift from the fast
 * path is a bug waiting for the one machine that takes it.
 */

/** How many workers to spin up. */
function poolSize(requested) {
  const cores = (typeof navigator !== 'undefined' && navigator.hardwareConcurrency) || 4;
  // One per core minus the main thread, capped at 8.
  //
  // The cap was 4, chosen when there were three chunky jobs and more workers
  // would only have added module-parse cost. Once the bakes were split to one
  // per texture set there are eleven, and the wall time of the pool is the
  // largest shard plus whatever queues behind it — so workers past the third
  // stopped being idle and started being the difference between one round and
  // two. Eight because the returns flatten there for this job mix, and because
  // a browser spawning a worker per core on a 32-thread machine is antisocial.
  return Math.max(1, Math.min(requested ?? 8, cores - 1));
}

export class Bakery {
  /**
   * @param opts.makeWorker  `() => Worker`, supplied by the composition root.
   *   A FACTORY, not a URL, and that is not a stylistic choice: a bundler can
   *   only emit a worker chunk when it can see the worker entry statically at
   *   the construction site. Passing `new URL('./x.worker.js', import.meta.url)`
   *   through a constructor parameter defeats that — measured: the dev server
   *   resolved it fine, the production build emitted no worker chunk at all,
   *   every worker 404'd, and the pool silently fell back to the main thread
   *   for a 3 s timeout plus every bake. It worked, slowly, in exactly the
   *   build where the optimisation matters most. A factory lets the caller use
   *   whatever form its bundler understands (`?worker` for Vite) and keeps this
   *   file bundler-agnostic.
   * @param opts.bakers  `{ [kind]: (payload) => result }`, the same registry the
   *   worker entry imports. Used for the synchronous fallback.
   * @param opts.workers  pool size hint; see poolSize().
   */
  constructor({ makeWorker, bakers = {}, workers, enabled = true } = {}) {
    this.bakers = bakers;
    this.enabled = enabled && typeof Worker !== 'undefined' && typeof makeWorker === 'function';
    this.size = this.enabled ? poolSize(workers) : 0;
    this._makeWorker = makeWorker;
    this._workers = [];
    this._idle = [];
    this._queue = [];
    this._pending = new Map();
    this._nextId = 1;
    this._readyCount = 0;
    this._readyResolve = null;
    this._readyAt = -1;
    this._ready = new Promise((res) => { this._readyResolve = res; });
    this.stats = {
      jobs: 0, onWorker: 0, onMainThread: 0, workerMs: 0, mainMs: 0,
      /** Per job: queued -> posted -> resolved, so a slow boot can be blamed on
       *  the right thing. A large `startupMs` means the workers were still
       *  loading their modules; a large `queueMs` means the pool is too small;
       *  a large `bakeMs` means the baker itself is slow. Without this split,
       *  "the worker did not help" is unfalsifiable. */
      timeline: [],
    };
    this._spawn();
  }

  _spawn() {
    for (let i = 0; i < this.size; i++) {
      try {
        const w = this._makeWorker();
        w.__id = `bakery-${i}`;
        w.__spawnedAt = performance.now();
        w.onmessage = (e) => this._onMessage(w, e.data);
        w.onerror = (e) => this._onWorkerError(w, e);
        this._workers.push(w);
        this._idle.push(w);
      } catch (err) {
        // A CSP, or a browser without module workers. Not fatal — everything
        // falls through to the synchronous path.
        console.warn('[bakery] worker unavailable, baking on the main thread:', err?.message ?? err);
        this.enabled = false;
        this.size = 0;
        return;
      }
    }
  }

  _onWorkerError(w, e) {
    console.warn('[bakery] worker error, falling back to the main thread:', e?.message ?? e);
    // Land every job that was in flight on this worker back on the main thread,
    // rather than leaving a caller awaiting a promise nobody will settle.
    for (const [id, p] of [...this._pending]) {
      if (p.worker !== w) continue;
      this._pending.delete(id);
      p.resolve(this._runLocal(p.kind, p.payload));
    }
    this._workers = this._workers.filter((x) => x !== w);
    this._idle = this._idle.filter((x) => x !== w);
    this.enabled = this._workers.length > 0;
    // Unblock anyone in ready(): with no workers left there is nothing to wait
    // for, and burning the full timeout is the worst of both worlds.
    if (!this.enabled) this._readyResolve(false);
    this._drain();
  }

  _onMessage(w, msg) {
    if (msg.ready) {
      // Offset from this worker's clock to the page's. Both timeOrigins are
      // Unix-epoch milliseconds, so the difference converts directly.
      w.__clockOffset = (msg.origin ?? performance.timeOrigin) - performance.timeOrigin;
      w.__readyAt = performance.now();
      this.stats.workerReady = (this.stats.workerReady ?? []);
      this.stats.workerReady.push({ id: w.__id, atMs: +w.__readyAt.toFixed(0), offsetMs: +w.__clockOffset.toFixed(0) });
      this._readyCount++;
      if (this._readyCount >= this._workers.length) {
        this._readyAt = performance.now();
        this._readyResolve(true);
      }
      return;
    }
    const p = this._pending.get(msg.id);
    this._pending.delete(msg.id);
    // Back of the queue: a worker that has just run a job is warm, and pushing
    // it behind the others keeps `shift()` handing out the longest-idle worker.
    this._idle.push(w);
    if (p) {
      this.stats.workerMs += msg.ms ?? 0;
      const done = performance.now();
      const off = w.__clockOffset ?? 0;
      // On the PAGE's clock, so these line up with the boot span tree.
      const startedAt = (msg.startedAt ?? 0) + off;
      const endedAt = (msg.endedAt ?? 0) + off;
      this.stats.timeline.push({
        kind: p.kind,
        worker: w.__id ?? '?',
        clockOffsetMs: +off.toFixed(0),
        queuedAt: +p.queuedAt.toFixed(0),
        // Queue -> the worker actually beginning. Large means the pool was not
        // warm (see ready()) or every worker was busy.
        latencyMs: +(startedAt - p.postedAt).toFixed(0),
        bakeMs: +(msg.ms ?? 0).toFixed(0),
        readyAt: +endedAt.toFixed(0),
        // How long the finished bytes sat in the message queue because this
        // thread was mid-task. Pure overlap: it cost the boot nothing.
        collectedAfterMs: +(done - endedAt).toFixed(0),
      });
      // A throw inside a baker is a defect, not a reason to lose the boot:
      // re-run it here so the failure surfaces with a main-thread stack.
      if (msg.error) {
        console.warn(`[bakery] "${p.kind}" failed in a worker, re-running locally:`, msg.error);
        p.resolve(this._runLocal(p.kind, p.payload));
      } else {
        p.resolve(msg.result);
      }
    }
    this._drain();
  }

  _drain() {
    while (this._queue.length && this._idle.length) {
      const job = this._queue.shift();
      // FRONT of the idle list, not the back. Measured: handing the first job
      // to the most recently spawned worker made that job wait ~2.7 s even
      // though the worker had already reported ready, while an older worker
      // given a job in the same tick started in 1 ms. Whatever the browser is
      // doing to a just-created worker thread while the main thread is
      // saturated, the oldest idle worker is reliably the responsive one.
      const w = this._idle.shift();
      job.postedAt = performance.now();
      this._pending.set(job.id, { ...job, worker: w });
      w.postMessage({ id: job.id, kind: job.kind, payload: job.payload });
    }
  }

  _runLocal(kind, payload) {
    const baker = this.bakers[kind];
    if (!baker) throw new Error(`[bakery] no baker named "${kind}"`);
    const t0 = performance.now();
    const result = baker(payload);
    this.stats.mainMs += performance.now() - t0;
    this.stats.onMainThread++;
    return result;
  }

  /**
   * Queue one bake. Resolves with the baker's plain-data result — typed arrays,
   * numbers, strings. Never rejects: a failure resolves with the main-thread
   * result instead, because a boot that dies because a texture would not bake in
   * a worker is strictly worse than a boot that is merely slow.
   */
  bake(kind, payload) {
    this.stats.jobs++;
    if (!this.enabled) return Promise.resolve(this._runLocal(kind, payload));
    this.stats.onWorker++;
    const id = this._nextId++;
    const queuedAt = performance.now();
    return new Promise((resolve) => {
      this._queue.push({ id, kind, payload, resolve, queuedAt, postedAt: queuedAt });
      this._drain();
    });
  }

  /**
   * Resolve once every worker has loaded its module graph — or after `timeoutMs`,
   * whichever comes first.
   *
   * WHY A CALLER SHOULD AWAIT THIS BEFORE DOING ANYTHING SLOW. A dedicated
   * worker's module script is fetched through the PARENT DOCUMENT's loader,
   * which runs on the main thread. If the main thread is already inside a long
   * synchronous task, the worker cannot finish starting, so every job posted to
   * it sits untouched until that task ends. Measured on this app before this
   * method existed: jobs queued at +244 ms did not begin baking until +3 160 ms,
   * because a 2 807 ms `init` task stood between them — the pool was doing
   * nothing during exactly the window it was supposed to be working.
   *
   * Paying ~200-400 ms here (one bundled module in a production build, a graph
   * of small ES modules over the dev server) buys the overlap back for seconds
   * of baking. It never rejects and never hangs: the timeout resolves anyway and
   * the jobs simply run wherever they can.
   */
  ready(timeoutMs = 3000) {
    if (!this.enabled || !this._workers.length) return Promise.resolve(false);
    return Promise.race([
      this._ready,
      new Promise((res) => setTimeout(() => res(false), timeoutMs)),
    ]);
  }

  /** Queue several bakes; resolves with all of them, in the order given. */
  bakeAll(jobs) {
    return Promise.all(jobs.map((j) => this.bake(j.kind, j.payload)));
  }

  dispose() {
    for (const w of this._workers) w.terminate();
    this._workers.length = 0;
    this._idle.length = 0;
    this._queue.length = 0;
    this._pending.clear();
  }
}

/**
 * Every transferable buffer reachable from a baker's result, so the worker can
 * MOVE its output instead of copying it. The soldier set alone is ~10 MB of
 * Uint8Array; structured-cloning that back would hand a good part of the saving
 * straight back.
 *
 * Walks one level into arrays and plain objects, which is the shape every baker
 * returns. De-duplicated, because two views onto one ArrayBuffer must not both
 * appear in a transfer list.
 */
export function transferablesOf(result, out = new Set()) {
  const consider = (v) => {
    if (!v) return;
    if (ArrayBuffer.isView(v)) out.add(v.buffer);
    else if (v instanceof ArrayBuffer) out.add(v);
    else if (Array.isArray(v)) v.forEach(consider);
    else if (typeof v === 'object') Object.values(v).forEach(consider);
  };
  consider(result);
  return [...out];
}
