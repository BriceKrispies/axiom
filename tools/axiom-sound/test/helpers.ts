// Shared test utilities: hermetic temp-app fixtures and stdout/stderr capture.

import { cpSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { resolveApp } from '../src/appdir.ts';
import { appCacheDir } from '../src/cache.ts';
import { Reporter } from '../src/output.ts';

const here = dirname(fileURLToPath(import.meta.url));

/** The committed read-only fixture app (never built in place). */
export const FIXTURE_APP = join(here, 'fixtures', 'app');

export interface TempApp {
  /** Absolute root of the writable temp copy (contains app.toml + sounds/). */
  readonly root: string;
  /** Remove the temp copy and this app's tool cache. */
  cleanup(): void;
}

/** Copy the fixture app into a fresh temp dir so tests can build into it. */
export function makeTempApp(): TempApp {
  const base = mkdtempSync(join(tmpdir(), 'axsnd-'));
  const root = join(base, 'app');
  cpSync(FIXTURE_APP, root, { recursive: true });
  return {
    root,
    cleanup(): void {
      try {
        const cache = appCacheDir(resolveApp(root));
        rmSync(cache, { recursive: true, force: true });
      } catch {
        /* app already gone */
      }
      rmSync(base, { recursive: true, force: true });
    },
  };
}

/** Author or overwrite a `.strudel` source in a temp app. */
export function writeSound(root: string, id: string, contents: string): void {
  writeFileSync(join(root, 'sounds', `${id}.strudel`), contents);
}

/** A well-formed source with the given id and body, for cache/invalidation tests. */
export function tone(id: string, body: string, extra: Record<string, number> = {}): string {
  const front = {
    duration_ms: 500,
    tail_ms: 100,
    channels: 1,
    bitrate_kbps: 128,
    ...extra,
  };
  const lines = Object.entries(front).map(([k, v]) => `${k} = ${v}`);
  return `+++\nid = "${id}"\n${lines.join('\n')}\n+++\n\n${body}\n`;
}

export interface CommandRun<T> {
  readonly stdout: string;
  readonly stderr: string;
  readonly value: T;
}

/**
 * Run a command with a JSON-mode collecting Reporter, returning its exit code
 * and captured output. No global stdout/stderr swap (which would corrupt the
 * test runner's own output) — the Reporter writes into local buffers.
 */
export async function runCommand<T>(
  fn: (reporter: Reporter) => Promise<T> | T,
): Promise<CommandRun<T>> {
  const { reporter, stdout, stderr } = Reporter.collecting({ json: true, verbose: false });
  const value = await fn(reporter);
  return { value, stdout: stdout(), stderr: stderr() };
}
