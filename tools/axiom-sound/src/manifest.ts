// The generated audio manifest: <app>/assets/audio/manifest.json.
//
// Shape (schemaVersion 1):
//   {
//     "schemaVersion": 1,
//     "generatedBy": "axiom-sound",
//     "assets": {
//       "<id>": { path, mimeType, durationMs, channels, sampleRate,
//                 bitrateKbps, sha256, sourceSha256 }
//     }
//   }
//
// Keys are written in deterministic sorted order; paths are relative to the
// app's assets/ directory; no absolute paths and no timestamps. Unrelated
// existing entries are preserved. The file is written atomically and is NOT
// rewritten when the serialized bytes are unchanged.

import { existsSync, readFileSync } from 'node:fs';
import { atomicWrite } from './atomicwrite.ts';
import { SoundError } from './errors.ts';

export const SCHEMA_VERSION = 1;
export const GENERATED_BY = 'axiom-sound';

export interface AssetEntry {
  readonly path: string;
  readonly mimeType: 'audio/mpeg';
  readonly durationMs: number;
  readonly channels: number;
  readonly sampleRate: number;
  readonly bitrateKbps: number;
  readonly sha256: string;
  readonly sourceSha256: string;
}

export interface Manifest {
  readonly schemaVersion: number;
  readonly generatedBy: string;
  readonly assets: Record<string, AssetEntry>;
}

/** Read the manifest at `path`, or an empty manifest if it does not exist. */
export function readManifest(path: string): Manifest {
  if (!existsSync(path)) {
    return { schemaVersion: SCHEMA_VERSION, generatedBy: GENERATED_BY, assets: {} };
  }
  try {
    const parsed = JSON.parse(readFileSync(path, 'utf8')) as Partial<Manifest>;
    return {
      schemaVersion: parsed.schemaVersion ?? SCHEMA_VERSION,
      generatedBy: parsed.generatedBy ?? GENERATED_BY,
      assets: parsed.assets ?? {},
    };
  } catch (err) {
    throw new SoundError('MANIFEST_WRITE_FAILED', `existing manifest is not valid JSON: ${path}`, {
      cause: err,
    });
  }
}

/** Return a new manifest with `entry` set for `id` (does not mutate `base`). */
export function withEntry(base: Manifest, id: string, entry: AssetEntry): Manifest {
  return {
    schemaVersion: SCHEMA_VERSION,
    generatedBy: GENERATED_BY,
    assets: { ...base.assets, [id]: entry },
  };
}

/** Return a new manifest with `ids` removed (does not mutate `base`). */
export function withoutEntries(base: Manifest, ids: readonly string[]): Manifest {
  const remove = new Set(ids);
  const assets: Record<string, AssetEntry> = {};
  for (const key of Object.keys(base.assets)) {
    if (!remove.has(key)) {
      assets[key] = base.assets[key];
    }
  }
  return { schemaVersion: SCHEMA_VERSION, generatedBy: GENERATED_BY, assets };
}

/**
 * Serialize a manifest deterministically: assets sorted by id, and each entry's
 * fields in a fixed order. Trailing newline for clean diffs.
 */
export function serializeManifest(manifest: Manifest): string {
  const sortedIds = Object.keys(manifest.assets).sort((a, b) => a.localeCompare(b));
  const assets: Record<string, AssetEntry> = {};
  for (const id of sortedIds) {
    const e = manifest.assets[id];
    assets[id] = {
      path: e.path,
      mimeType: e.mimeType,
      durationMs: e.durationMs,
      channels: e.channels,
      sampleRate: e.sampleRate,
      bitrateKbps: e.bitrateKbps,
      sha256: e.sha256,
      sourceSha256: e.sourceSha256,
    };
  }
  const ordered: Manifest = {
    schemaVersion: manifest.schemaVersion,
    generatedBy: manifest.generatedBy,
    assets,
  };
  return `${JSON.stringify(ordered, null, 2)}\n`;
}

/**
 * Write `manifest` to `path` atomically, but only if the serialized content
 * differs from what is already on disk. Returns true if a write happened.
 */
export function writeManifestIfChanged(path: string, manifest: Manifest): boolean {
  const next = serializeManifest(manifest);
  if (existsSync(path)) {
    const current = readFileSync(path, 'utf8');
    if (current === next) {
      return false;
    }
  }
  try {
    atomicWrite(path, next);
  } catch (err) {
    throw new SoundError('MANIFEST_WRITE_FAILED', `failed to write manifest: ${path}`, {
      cause: err,
    });
  }
  return true;
}
