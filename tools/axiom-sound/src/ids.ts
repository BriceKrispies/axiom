// Sound-id validation and in-app path containment (traversal + symlink guard).

import { realpathSync } from 'node:fs';
import { relative, resolve, sep } from 'node:path';
import { SoundError } from './errors.ts';

/** Lowercase ASCII letters, digits, and single hyphens between segments. */
export const ID_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** True if `id` is a syntactically valid kebab-case sound id. */
export function isValidId(id: string): boolean {
  return ID_PATTERN.test(id);
}

/** Throw INVALID_SOUND_ID unless `id` is valid kebab-case. */
export function assertValidId(id: string): void {
  if (!isValidId(id)) {
    throw new SoundError(
      'INVALID_SOUND_ID',
      `sound id \`${id}\` must be lowercase ASCII letters, digits, and hyphens (kebab-case)`,
      { id },
    );
  }
}

/** Throw INVALID_SOUND_ID unless the `.strudel` filename stem equals `id`. */
export function assertStemMatchesId(stem: string, id: string): void {
  if (stem !== id) {
    throw new SoundError(
      'INVALID_SOUND_ID',
      `front-matter id \`${id}\` must equal the filename stem \`${stem}\``,
      { id },
    );
  }
}

/**
 * Resolve `candidate` and assert it stays inside `root` — rejecting `..`
 * traversal, absolute escapes, and symlinks that point outside the app. Returns
 * the resolved absolute path. `label` names the path for the error message.
 *
 * Symlink handling: we realpath the nearest existing ancestor of `candidate`
 * (the candidate itself may not exist yet, e.g. a not-yet-written asset) and
 * confirm that real ancestor is still within the realpath of `root`.
 */
export function assertInsideApp(root: string, candidate: string, label: string): string {
  const absRoot = resolve(root);
  const abs = resolve(absRoot, candidate);

  const withinLexically = (parent: string, child: string): boolean => {
    const rel = relative(parent, child);
    return rel === '' || (!rel.startsWith('..') && !rel.startsWith(`..${sep}`) && !isAbsoluteRel(rel));
  };

  if (!withinLexically(absRoot, abs)) {
    throw new SoundError('INVALID_SOUND_ID', `${label} path escapes the app directory: ${abs}`);
  }

  const realRoot = realpathSafe(absRoot);
  const realExistingAncestor = realpathSafe(nearestExistingAncestor(abs));
  if (!withinLexically(realRoot, realExistingAncestor)) {
    throw new SoundError(
      'INVALID_SOUND_ID',
      `${label} path resolves (via a symlink) outside the app directory: ${abs}`,
    );
  }

  return abs;
}

function isAbsoluteRel(rel: string): boolean {
  // On Windows, relative() can return a drive-letter path when parent/child are
  // on different drives; treat any drive-qualified result as "outside".
  return /^[a-zA-Z]:/.test(rel);
}

function nearestExistingAncestor(p: string): string {
  let current = p;
  for (;;) {
    try {
      realpathSync(current);
      return current;
    } catch {
      const parent = resolve(current, '..');
      if (parent === current) {
        return current;
      }
      current = parent;
    }
  }
}

function realpathSafe(p: string): string {
  try {
    return realpathSync(p);
  } catch {
    return resolve(p);
  }
}
