// Node side of the harness: bundle the in-browser Strudel entry with esbuild
// (once per process) and wrap it in a self-contained HTML page. The bundle is
// inlined as a <script>, so the page needs no sub-resource requests — the only
// network the browser would ever initiate is a blocked, recorded escape.

import esbuild from 'esbuild';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const OFFLINE_ENTRY = join(here, 'browser-entry.ts');
const REALTIME_ENTRY = join(here, 'realtime-entry.ts');
const TOOL_ROOT = join(here, '..', '..');

const bundleCache = new Map<string, string>();

async function bundleEntry(entry: string): Promise<string> {
  const cached = bundleCache.get(entry);
  if (cached !== undefined) {
    return cached;
  }
  const result = await esbuild.build({
    entryPoints: [entry],
    absWorkingDir: TOOL_ROOT,
    bundle: true,
    format: 'iife',
    platform: 'browser',
    target: 'es2022',
    write: false,
    logLevel: 'silent',
  });
  const js = result.outputFiles[0].text;
  bundleCache.set(entry, js);
  return js;
}

/** Build (and cache) the offline render browser bundle JS. */
export function buildBundle(): Promise<string> {
  return bundleEntry(OFFLINE_ENTRY);
}

/** Build (and cache) the realtime capture browser bundle JS. */
export function buildRealtimeBundle(): Promise<string> {
  return bundleEntry(REALTIME_ENTRY);
}

/** The fixed in-memory page URL the harness serves and navigates to. */
export const PAGE_URL = 'https://axiom-sound.local/';

/** Wrap the bundle in a minimal self-contained HTML document. */
export function pageHtml(bundleJs: string): string {
  return `<!doctype html><html><head><meta charset="utf-8"><title>axiom-sound render</title><script>${bundleJs}</script></head><body></body></html>`;
}
