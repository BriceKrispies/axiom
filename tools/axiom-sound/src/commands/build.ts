// `axiom-sound build --app <app> [--name <id>] [--force]`: run the full check
// pipeline, then render → validate PCM → encode MP3 → atomically publish +
// update the manifest. Skips a sound whose source hash already matches its
// built output (unless --force). Exits nonzero if any sound fails.

import { existsSync, mkdirSync } from 'node:fs';
import { relative } from 'node:path';
import { assetPath, manifestAssetPath, type AppDirs } from '../appdir.ts';
import { SAMPLE_RATE, totalMs } from '../config.ts';
import { isSoundError } from '../errors.ts';
import { fileSha256 } from '../hash.ts';
import { loadSound, type LoadedSound } from '../pipeline.ts';
import { validatePcm } from '../pcm.ts';
import { encodeWav } from '../wav.ts';
import { encodeMp3, writeWavIntermediate } from '../encode.ts';
import { wavCachePath } from '../cache.ts';
import {
  readManifest,
  withEntry,
  writeManifestIfChanged,
  type AssetEntry,
  type Manifest,
} from '../manifest.ts';
import { assertNoDuplicateIds, listSources, type SourceRef } from '../sources.ts';
import { RenderHarness, withHarness } from '../render/harness.ts';
import { selectRefs } from './check.ts';
import type { Reporter } from '../output.ts';
import type { BuildOptions } from '../options.ts';

type BuildStatus = 'built' | 'cached' | 'failed';

export interface BuildOutcome {
  readonly id: string;
  readonly source: string;
  readonly status: BuildStatus;
  readonly asset?: string;
  readonly error?: { code: string; message: string };
}

export async function runBuild(
  app: AppDirs,
  reporter: Reporter,
  opts: BuildOptions,
): Promise<number> {
  const refs = selectRefs(app, opts.name);
  if (refs.length === 0) {
    reporter.human('no sounds to build');
    reporter.result({ ok: true, built: [] });
    return 0;
  }

  assertNoDuplicateIds(listSources(app));

  const outcomes = await withHarness((harness) => buildAll(app, refs, harness, reporter, opts));
  const failed = outcomes.filter((o) => o.status === 'failed');

  reporter.result({ ok: failed.length === 0, built: outcomes });
  reporter.human(summaryLine(outcomes));
  return failed.length === 0 ? 0 : 1;
}

async function buildAll(
  app: AppDirs,
  refs: readonly SourceRef[],
  harness: RenderHarness,
  reporter: Reporter,
  opts: BuildOptions,
): Promise<BuildOutcome[]> {
  let manifest = readManifest(app.manifestPath);
  const outcomes: BuildOutcome[] = [];

  for (const ref of refs) {
    const source = relative(app.root, ref.path);
    try {
      const loaded = loadSound(app, ref);
      const id = loaded.config.id;
      const mp3 = assetPath(app, id);

      if (!opts.force && isCached(manifest, id, loaded, mp3)) {
        reporter.info(`  cached ${id}`);
        outcomes.push({ id, source, status: 'cached', asset: manifestAssetPath(id) });
        continue;
      }

      const entry = await renderEncodePublish(app, harness, loaded, mp3);
      manifest = withEntry(manifest, id, entry);
      writeManifestIfChanged(app.manifestPath, manifest);
      reporter.info(`  built  ${id}`);
      outcomes.push({ id, source, status: 'built', asset: manifestAssetPath(id) });
    } catch (err) {
      const e = isSoundError(err)
        ? { code: err.code, message: err.message }
        : { code: 'INTERNAL', message: String(err) };
      reporter.info(`  FAIL   ${ref.stem}  [${e.code}] ${e.message}`);
      outcomes.push({ id: ref.stem, source, status: 'failed', error: e });
    }
  }

  return outcomes;
}

/**
 * Render one loaded sound, validate its PCM, encode + atomically publish the
 * MP3, and return its manifest entry. Shared by `build` and `preview`. Never
 * touches the manifest file itself — the caller owns that.
 */
export async function renderEncodePublish(
  app: AppDirs,
  harness: RenderHarness,
  loaded: LoadedSound,
  mp3: string,
): Promise<AssetEntry> {
  const { config } = loaded;
  const pcm =
    config.mode === 'realtime'
      ? await harness.renderRealtime(loaded.parsed.body, config, loaded.parsed.bodyStartLine)
      : await harness.render(loaded.parsed.body, config, loaded.parsed.bodyStartLine);
  validatePcm(pcm, config);

  const wavPath = wavCachePath(app, config.id);
  writeWavIntermediate(wavPath, encodeWav(pcm));

  mkdirSync(app.audioDir, { recursive: true });
  encodeMp3(wavPath, mp3, config);

  return {
    path: manifestAssetPath(config.id),
    mimeType: 'audio/mpeg',
    durationMs: totalMs(config),
    channels: config.channels,
    sampleRate: SAMPLE_RATE,
    bitrateKbps: config.bitrateKbps,
    sha256: fileSha256(mp3),
    sourceSha256: loaded.sourceHash,
  };
}

function isCached(manifest: Manifest, id: string, loaded: LoadedSound, mp3: string): boolean {
  const existing = manifest.assets[id];
  return Boolean(existing) && existing.sourceSha256 === loaded.sourceHash && existsSync(mp3);
}

function summaryLine(outcomes: readonly BuildOutcome[]): string {
  const built = outcomes.filter((o) => o.status === 'built').length;
  const cached = outcomes.filter((o) => o.status === 'cached').length;
  const failed = outcomes.filter((o) => o.status === 'failed').length;
  const head = failed === 0 ? 'ok' : 'FAILED';
  return `${head}: ${built} built, ${cached} cached, ${failed} failed`;
}
