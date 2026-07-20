// Version identity that feeds the source hash (hash.ts).
//
// The source hash must invalidate whenever ANYTHING that could change the
// rendered bytes changes: the source text, the parsed config, the pinned
// Strudel package versions, the tool version, the renderer version, and the
// encoder settings. This module owns the version half of that identity.
//
// The Strudel versions are read from our own package.json `dependencies` so
// the hash tracks the exact pins without a second place to update.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const pkgPath = join(here, '..', 'package.json');

interface PackageJson {
  readonly version?: string;
  readonly dependencies?: Record<string, string>;
}

const pkg: PackageJson = JSON.parse(readFileSync(pkgPath, 'utf8')) as PackageJson;

/** The tool's own semantic version (from package.json). */
export const TOOL_VERSION = pkg.version ?? '0.0.0';

/**
 * The renderer contract version. Bump this by hand whenever the offline render
 * pipeline changes in a way that alters output bytes for unchanged inputs
 * (e.g. cps default, scheduling, sample-rate handling). This is independent of
 * the Strudel package versions.
 */
export const RENDERER_VERSION = '2';

const STRUDEL_PACKAGES = [
  '@strudel/core',
  '@strudel/mini',
  '@strudel/tonal',
  '@strudel/transpiler',
  '@strudel/webaudio',
  'superdough',
] as const;

/** The exact pinned version of every Strudel package, sorted, for the hash. */
export const STRUDEL_VERSIONS: ReadonlyArray<readonly [string, string]> = STRUDEL_PACKAGES.map(
  (name) => [name, pkg.dependencies?.[name] ?? 'unknown'] as const,
).sort((a, b) => a[0].localeCompare(b[0]));
