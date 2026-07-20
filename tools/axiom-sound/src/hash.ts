// Content hashing for the cache and manifest.
//
// `sourceSha256` is the cache key. It must change whenever anything that could
// change the rendered output changes: the complete `.strudel` source, the
// parsed render config, the exact pinned Strudel versions, the tool version,
// the renderer version, and the encoder settings. `fileSha256` is the plain
// hash of the encoded MP3, recorded in the manifest.

import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { encoderSignature, SAMPLE_RATE, CPS, type RenderConfig } from './config.ts';
import { RENDERER_VERSION, STRUDEL_VERSIONS, TOOL_VERSION } from './versions.ts';

/**
 * The cache/source hash for one sound. `rawSource` is the complete, unmodified
 * `.strudel` file contents. Deterministic and independent of machine or path.
 */
export function sourceSha256(rawSource: string, config: RenderConfig): string {
  const canonicalConfig = JSON.stringify({
    id: config.id,
    durationMs: config.durationMs,
    tailMs: config.tailMs,
    channels: config.channels,
    bitrateKbps: config.bitrateKbps,
    mode: config.mode,
    sampleRate: SAMPLE_RATE,
    cps: CPS,
  });
  const versions = STRUDEL_VERSIONS.map(([name, version]) => `${name}@${version}`).join(',');

  const h = createHash('sha256');
  h.update('axiom-sound\0');
  h.update(`tool=${TOOL_VERSION}\0`);
  h.update(`renderer=${RENDERER_VERSION}\0`);
  h.update(`strudel=${versions}\0`);
  h.update(`encoder=${encoderSignature(config)}\0`);
  h.update(`config=${canonicalConfig}\0`);
  h.update('source=\0');
  h.update(rawSource);
  return h.digest('hex');
}

/** SHA-256 hex of a byte buffer. */
export function bytesSha256(bytes: Uint8Array): string {
  return createHash('sha256').update(bytes).digest('hex');
}

/** SHA-256 hex of a file's contents. */
export function fileSha256(path: string): string {
  return bytesSha256(readFileSync(path));
}
