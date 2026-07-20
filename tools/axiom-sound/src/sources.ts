// Discovery of authored `.strudel` sources within an app, plus duplicate-id
// detection. The per-file parse/validate/check/build work lives in the command
// modules; this module only locates sources and guards global id uniqueness.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { SoundError } from './errors.ts';
import { assertInsideApp, assertValidId } from './ids.ts';
import { parseSource } from './frontmatter.ts';
import { sourcePath, type AppDirs } from './appdir.ts';

export interface SourceRef {
  /** The filename stem (also the expected id). */
  readonly stem: string;
  /** Absolute path to the `.strudel` file. */
  readonly path: string;
}

const EXT = '.strudel';

/** List all `.strudel` sources in the app, sorted by stem. Empty if none. */
export function listSources(app: AppDirs): SourceRef[] {
  if (!existsSync(app.soundsDir)) {
    return [];
  }
  return readdirSync(app.soundsDir)
    .filter((name) => name.endsWith(EXT))
    .map((name) => ({ stem: basename(name, EXT), path: join(app.soundsDir, name) }))
    .sort((a, b) => a.stem.localeCompare(b.stem));
}

/**
 * Resolve `--name <id>` to a single source ref, validating the id shape and
 * that the resolved path stays inside the app. Throws SOURCE_NOT_FOUND if the
 * file does not exist.
 */
export function resolveNamed(app: AppDirs, id: string): SourceRef {
  assertValidId(id);
  const path = assertInsideApp(app.root, sourcePath(app, id), 'source');
  if (!existsSync(path)) {
    throw new SoundError('SOURCE_NOT_FOUND', `no source at ${path}`, { id });
  }
  return { stem: id, path };
}

/** Read a source file's raw contents. */
export function readSource(ref: SourceRef): string {
  return readFileSync(ref.path, 'utf8');
}

/**
 * Detect two sources declaring the same front-matter `id`. Files whose front
 * matter cannot be parsed are skipped here (their own check will report the
 * INVALID_FRONT_MATTER error). Throws DUPLICATE_SOUND_ID on any collision.
 */
export function assertNoDuplicateIds(refs: readonly SourceRef[]): void {
  const byId = new Map<string, string[]>();
  for (const ref of refs) {
    let id: string;
    try {
      id = parseSource(readFileSync(ref.path, 'utf8'), ref.stem).config.id;
    } catch {
      continue;
    }
    const stems = byId.get(id) ?? [];
    stems.push(ref.stem);
    byId.set(id, stems);
  }
  for (const [id, stems] of byId) {
    if (stems.length > 1) {
      throw new SoundError(
        'DUPLICATE_SOUND_ID',
        `duplicate sound id \`${id}\` declared by: ${stems.sort().join(', ')}`,
        { id, extra: { stems } },
      );
    }
  }
}
