// The tool's gitignored cache for lossless WAV intermediates. Namespaced per app
// (by a hash of the app's absolute path) so `clean` can drop exactly one app's
// renders. Lives under the tool, never inside an app's assets/.

import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import type { AppDirs } from './appdir.ts';

const here = dirname(fileURLToPath(import.meta.url));
const TOOL_ROOT = join(here, '..');

/** Root of the whole sound cache: <tool>/.axiom-cache/sound. */
export const CACHE_ROOT = join(TOOL_ROOT, '.axiom-cache', 'sound');

/** Stable short key for an app, derived from its absolute root path. */
export function appKey(app: AppDirs): string {
  return createHash('sha256').update(app.root).digest('hex').slice(0, 16);
}

/** The cache directory holding one app's WAV intermediates. */
export function appCacheDir(app: AppDirs): string {
  return join(CACHE_ROOT, appKey(app));
}

/** Cache path for a sound's lossless WAV intermediate. */
export function wavCachePath(app: AppDirs, id: string): string {
  return join(appCacheDir(app), `${id}.wav`);
}
