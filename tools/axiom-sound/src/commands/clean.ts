// `axiom-sound clean --app <app>`: remove generated MP3s, the generated manifest
// entries, and this app's cached WAV renders. Never touches authored `.strudel`
// sources.

import { existsSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { type AppDirs } from '../appdir.ts';
import { appCacheDir } from '../cache.ts';
import { readManifest } from '../manifest.ts';
import { assertInsideApp } from '../ids.ts';
import type { Reporter } from '../output.ts';
import type { CommonOptions } from '../options.ts';

export function runClean(app: AppDirs, reporter: Reporter, _opts: CommonOptions): number {
  const manifest = readManifest(app.manifestPath);
  const removed: string[] = [];

  // Remove every MP3 the manifest generated (paths are relative to assets/).
  for (const id of Object.keys(manifest.assets).sort()) {
    const rel = manifest.assets[id].path;
    const abs = assertInsideApp(app.assetsDir, join(app.assetsDir, rel), 'asset');
    if (existsSync(abs)) {
      rmSync(abs, { force: true });
      removed.push(rel);
      reporter.info(`  removed asset ${rel}`);
    }
  }

  // Remove the generated manifest itself (it is entirely tool-owned).
  let manifestRemoved = false;
  if (existsSync(app.manifestPath)) {
    rmSync(app.manifestPath, { force: true });
    manifestRemoved = true;
    reporter.info('  removed manifest.json');
  }

  // Remove this app's cached WAV renders.
  const cache = appCacheDir(app);
  let cacheRemoved = false;
  if (existsSync(cache)) {
    rmSync(cache, { recursive: true, force: true });
    cacheRemoved = true;
    reporter.info('  removed cache');
  }

  reporter.result({ ok: true, removedAssets: removed, manifestRemoved, cacheRemoved });
  reporter.human(
    `cleaned: ${removed.length} asset(s)` +
      `${manifestRemoved ? ', manifest' : ''}${cacheRemoved ? ', cache' : ''}`,
  );
  return 0;
}
