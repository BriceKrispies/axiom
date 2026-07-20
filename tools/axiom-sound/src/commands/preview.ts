// `axiom-sound preview --app <app> --name <id>`: build the asset if stale, then
// open the generated MP3 with the OS default player. No custom GUI; the `.strudel`
// file stays the editing surface.

import { existsSync } from 'node:fs';
import { assetPath, type AppDirs } from '../appdir.ts';
import { SoundError } from '../errors.ts';
import { loadSound } from '../pipeline.ts';
import { openWithOs } from '../launch.ts';
import { readManifest, withEntry, writeManifestIfChanged } from '../manifest.ts';
import { resolveNamed } from '../sources.ts';
import { withHarness } from '../render/harness.ts';
import { renderEncodePublish } from './build.ts';
import type { Reporter } from '../output.ts';
import type { CommonOptions, NameOption } from '../options.ts';

export async function runPreview(
  app: AppDirs,
  reporter: Reporter,
  opts: CommonOptions & NameOption,
): Promise<number> {
  if (!opts.name) {
    throw new SoundError('USAGE', '`preview` requires --name <sound-id>');
  }
  const ref = resolveNamed(app, opts.name);
  const loaded = loadSound(app, ref);
  const id = loaded.config.id;
  const mp3 = assetPath(app, id);

  const manifest = readManifest(app.manifestPath);
  const entry = manifest.assets[id];
  const fresh = Boolean(entry) && entry.sourceSha256 === loaded.sourceHash && existsSync(mp3);

  if (!fresh) {
    reporter.info(`  building ${id}…`);
    const built = await withHarness((harness) => renderEncodePublish(app, harness, loaded, mp3));
    writeManifestIfChanged(app.manifestPath, withEntry(manifest, id, built));
  }

  reporter.human(`opening ${mp3}`);
  reporter.result({ ok: true, id, asset: mp3, rebuilt: !fresh });
  openWithOs(mp3, (message) => reporter.info(`axiom-sound: ${message}`));
  return 0;
}
