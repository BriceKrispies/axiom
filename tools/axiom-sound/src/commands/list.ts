// `axiom-sound list --app <app>`: a stable, sorted inventory of every source —
// id, paths, source status, build status, duration, channels, source hash. No
// browser; purely filesystem + manifest inspection.

import { existsSync } from 'node:fs';
import { relative } from 'node:path';
import { assetPath, manifestAssetPath, type AppDirs } from '../appdir.ts';
import { totalMs } from '../config.ts';
import { isSoundError } from '../errors.ts';
import { loadSound } from '../pipeline.ts';
import { readManifest } from '../manifest.ts';
import { listSources } from '../sources.ts';
import type { Reporter } from '../output.ts';
import type { CommonOptions } from '../options.ts';

type SourceStatus = 'ok' | 'invalid';
type BuildStatus = 'built' | 'stale' | 'unbuilt';

export interface ListRow {
  readonly id: string;
  readonly source: string;
  readonly asset: string;
  readonly sourceStatus: SourceStatus;
  readonly buildStatus: BuildStatus;
  readonly durationMs: number | null;
  readonly channels: number | null;
  readonly sourceHash: string | null;
}

export function runList(app: AppDirs, reporter: Reporter, _opts: CommonOptions): number {
  const manifest = readManifest(app.manifestPath);
  const rows: ListRow[] = listSources(app).map((ref) => {
    const source = relative(app.root, ref.path);
    const asset = manifestAssetPath(ref.stem);
    try {
      const loaded = loadSound(app, ref);
      const id = loaded.config.id;
      const entry = manifest.assets[id];
      const built = Boolean(entry) && existsSync(assetPath(app, id));
      const buildStatus: BuildStatus = !built
        ? 'unbuilt'
        : entry.sourceSha256 === loaded.sourceHash
          ? 'built'
          : 'stale';
      return {
        id,
        source,
        asset,
        sourceStatus: 'ok',
        buildStatus,
        durationMs: totalMs(loaded.config),
        channels: loaded.config.channels,
        sourceHash: loaded.sourceHash,
      };
    } catch (err) {
      const code = isSoundError(err) ? err.code : 'INTERNAL';
      return {
        id: ref.stem,
        source,
        asset,
        sourceStatus: 'invalid',
        buildStatus: 'unbuilt',
        durationMs: null,
        channels: null,
        sourceHash: null,
        // Surface the reason on stderr for humans; JSON keeps the row minimal.
        ...logInvalid(reporter, ref.stem, code),
      };
    }
  });

  reporter.result({ ok: true, sounds: rows });
  for (const row of rows) {
    reporter.human(formatRow(row));
  }
  if (rows.length === 0) {
    reporter.human('(no sounds)');
  }
  return 0;
}

function logInvalid(reporter: Reporter, stem: string, code: string): Record<string, never> {
  reporter.info(`  ${stem}: source invalid [${code}]`);
  return {};
}

function formatRow(row: ListRow): string {
  const dur = row.durationMs === null ? '   -  ' : `${String(row.durationMs).padStart(5)}ms`;
  const ch = row.channels === null ? '-' : String(row.channels);
  const hash = row.sourceHash ? row.sourceHash.slice(0, 12) : '-'.repeat(12);
  return [
    row.id.padEnd(20),
    row.sourceStatus.padEnd(7),
    row.buildStatus.padEnd(7),
    dur,
    `${ch}ch`,
    hash,
    row.source,
  ].join('  ');
}
