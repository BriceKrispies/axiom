/**
 * THE BAKER REGISTRY — which pure seed-to-bytes jobs the bakery can run.
 *
 * This is a composition-root file, next to main.js, for the same reason main.js
 * is the only place that knows the subsystem list: `src/core/` is shared
 * substrate and does not import subsystems (see ARCHITECTURE.md), so the pool in
 * `src/core/bakery.js` is generic and the recipes are wired in from here.
 *
 * EVERY ENTRY MUST BE PURE. A baker takes a structured-cloneable payload —
 * numbers, strings, typed arrays — and returns typed arrays and plain data. It
 * may not touch THREE, the DOM, WebGL, the engine context, or module-level
 * mutable state, because it has to produce identical output on a worker thread
 * and on the main thread. That constraint is what makes the fallback in
 * bakery.js safe, and it is why each of these modules is split away from the
 * THREE-facing half of its subsystem (atlas.js / atlasbake.js is the pattern).
 *
 * RANDOMNESS. A payload carries a `seed` (one `u32`), never an `Rng`. The caller
 * draws it from its own stream with the same single `u32()` its `rng.fork()` was
 * already making, so streams advance exactly as before and the pixel gate stays
 * meaningful.
 *
 * This module is imported by BOTH the main thread (for the synchronous
 * fallback) and bakers.worker.js. Anything it pulls in is parsed in every
 * worker, so keep the import graph to the pure halves.
 */

import { paintParticleAtlas, paintDecalAtlas, paintBrass } from './fx/atlasbake.js';
import { bakeSoldierSets } from './ai/bake.js';

export const BAKERS = {
  'fx:particle-atlas': paintParticleAtlas,
  'fx:decal-atlas': paintDecalAtlas,
  'fx:brass': paintBrass,
  'ai:soldier-sets': bakeSoldierSets,
};
