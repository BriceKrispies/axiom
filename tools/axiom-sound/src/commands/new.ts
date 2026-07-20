// `axiom-sound new --app <app> --name <id>`: scaffold a new source with valid
// front matter and a tiny synthesized placeholder tone. Never overwrites.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { sourcePath, type AppDirs } from '../appdir.ts';
import { SoundError } from '../errors.ts';
import { assertInsideApp, assertValidId } from '../ids.ts';
import { parseSource } from '../frontmatter.ts';
import type { Reporter } from '../output.ts';
import type { CommonOptions } from '../options.ts';

const here = dirname(fileURLToPath(import.meta.url));
const TEMPLATE_PATH = join(here, '..', '..', 'templates', 'new-sound.strudel');

export function runNew(
  app: AppDirs,
  id: string | undefined,
  reporter: Reporter,
  _opts: CommonOptions,
): number {
  if (!id) {
    throw new SoundError('USAGE', '`new` requires --name <sound-id>');
  }
  assertValidId(id);
  const dest = assertInsideApp(app.root, sourcePath(app, id), 'source');

  if (existsSync(dest)) {
    throw new SoundError('SOURCE_EXISTS', `refusing to overwrite existing source: ${dest}`, { id });
  }

  const template = readFileSync(TEMPLATE_PATH, 'utf8').replaceAll('__ID__', id);
  // Fail fast if the template + id do not parse (guards against a broken template).
  parseSource(template, id);

  mkdirSync(dirname(dest), { recursive: true });
  writeFileSync(dest, template, { flag: 'wx' });

  reporter.human(`created ${dest}`);
  reporter.result({ ok: true, id, source: dest });
  return 0;
}
