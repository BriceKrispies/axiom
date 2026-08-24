/**
 * The worker half of the bakery — message plumbing only.
 *
 * A worker entry module calls `serveBakes(BAKERS)` and is done. All knowledge
 * of WHAT to bake lives in the registry the composition root passes in, which
 * the main thread imports too, so the worker path and the synchronous fallback
 * in bakery.js cannot diverge.
 *
 * Split out of the worker entry so that `src/core/` keeps the plumbing and the
 * composition root keeps the recipes — the same division as Engine/subsystems.
 */

import { transferablesOf } from './bakery.js';

/** Install the message handler that runs bakers out of `bakers`. */
export function serveBakes(bakers) {
  // Announce that this worker's module graph is loaded. The pool waits for
  // these before boot starts its first long task — see Bakery.ready().
  // `timeOrigin` travels with it: a worker's performance.now() is relative to
  // the WORKER's creation, not the document's, so the pool needs the offset to
  // put a worker's timings on the page's clock. Without it, "when did this bake
  // actually run" is unanswerable — the arrival time of the result message only
  // says when the main thread was next free to read it.
  self.postMessage({ ready: true, origin: performance.timeOrigin });

  self.onmessage = (e) => {
    const { id, kind, payload } = e.data;
    const t0 = performance.now();
    try {
      const baker = bakers[kind];
      if (!baker) throw new Error(`no baker named "${kind}"`);
      const result = baker(payload);
      // TRANSFER, do not clone — see transferablesOf().
      self.postMessage(
        { id, result, ms: performance.now() - t0, startedAt: t0, endedAt: performance.now() },
        transferablesOf(result)
      );
    } catch (err) {
      self.postMessage({
        id,
        error: String(err?.stack ?? err?.message ?? err),
        ms: performance.now() - t0,
      });
    }
  };
}
