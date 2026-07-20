// App-directory resolution and the canonical sound/asset layout.
//
// An Axiom app is identified by its existing `app.toml` manifest. Authored
// Strudel sources live under `<app>/sounds/<id>.strudel`; generated runtime
// assets under `<app>/assets/audio/<id>.mp3` with `<app>/assets/audio/manifest.json`.
// Source files deliberately never live under `assets/` — they must not ship as
// runtime assets.

import { existsSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { SoundError } from './errors.ts';

export interface AppDirs {
  /** Absolute app root (the directory containing app.toml). */
  readonly root: string;
  /** Absolute `<app>/sounds` (authored `.strudel` sources). */
  readonly soundsDir: string;
  /** Absolute `<app>/assets` (runtime asset root). */
  readonly assetsDir: string;
  /** Absolute `<app>/assets/audio` (generated MP3 + manifest). */
  readonly audioDir: string;
  /** Absolute `<app>/assets/audio/manifest.json`. */
  readonly manifestPath: string;
}

/**
 * Resolve `--app <path>` into the app's directory layout, requiring that the
 * path exists and contains an `app.toml`. Throws APP_NOT_FOUND or
 * APP_MANIFEST_NOT_FOUND — never silently accepts an arbitrary directory.
 */
export function resolveApp(appPath: string): AppDirs {
  const root = resolve(appPath);

  if (!existsSync(root) || !statSync(root).isDirectory()) {
    throw new SoundError('APP_NOT_FOUND', `app path is not an existing directory: ${root}`, {
      extra: { appPath },
    });
  }

  const manifest = join(root, 'app.toml');
  if (!existsSync(manifest)) {
    throw new SoundError(
      'APP_MANIFEST_NOT_FOUND',
      `not an Axiom app: no app.toml at ${manifest}`,
      { extra: { appPath: root } },
    );
  }

  return {
    root,
    soundsDir: join(root, 'sounds'),
    assetsDir: join(root, 'assets'),
    audioDir: join(root, 'assets', 'audio'),
    manifestPath: join(root, 'assets', 'audio', 'manifest.json'),
  };
}

/** Absolute path to the authored source for `id`. */
export function sourcePath(app: AppDirs, id: string): string {
  return join(app.soundsDir, `${id}.strudel`);
}

/** Absolute path to the generated MP3 asset for `id`. */
export function assetPath(app: AppDirs, id: string): string {
  return join(app.audioDir, `${id}.mp3`);
}

/** The manifest-relative asset path for `id` (relative to `assets/`). */
export function manifestAssetPath(id: string): string {
  return `audio/${id}.mp3`;
}
