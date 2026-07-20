// Shared per-sound loading used by check/build/list: read → parse front matter
// → validate id/stem → confirm the asset path stays inside the app → compute the
// source hash. One place so every command validates identically.

import { assetPath, type AppDirs } from './appdir.ts';
import { parseSource, type ParsedSource } from './frontmatter.ts';
import { assertInsideApp, assertStemMatchesId, assertValidId } from './ids.ts';
import { sourceSha256 } from './hash.ts';
import { readSource, type SourceRef } from './sources.ts';
import type { RenderConfig } from './config.ts';

export interface LoadedSound {
  readonly ref: SourceRef;
  readonly raw: string;
  readonly parsed: ParsedSource;
  readonly config: RenderConfig;
  readonly sourceHash: string;
}

/** Read and fully validate one source, ready to check or build. Throws SoundError. */
export function loadSound(app: AppDirs, ref: SourceRef): LoadedSound {
  const raw = readSource(ref);
  const parsed = parseSource(raw, ref.stem);
  const config = parsed.config;
  assertValidId(config.id);
  assertStemMatchesId(ref.stem, config.id);
  // Confirm the destination the build would write stays inside the app.
  assertInsideApp(app.root, assetPath(app, config.id), 'asset');
  return { ref, raw, parsed, config, sourceHash: sourceSha256(raw, config) };
}
