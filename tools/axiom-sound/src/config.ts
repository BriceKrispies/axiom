// Render configuration: the parsed, validated front-matter fields plus the
// fixed engine constants they combine with. Owns all bound/allowlist checks.

import { SoundError } from './errors.ts';

/** Fixed render sample rate (Hz). WAV intermediate and MP3 output both use it. */
export const SAMPLE_RATE = 48_000;

/**
 * Cycles-per-second used to drive the pattern query and superdough scheduling.
 * 0.5 is Strudel's REPL default (one cycle = 2 seconds), so a `.strudel` body
 * behaves identically when pasted into the Strudel REPL. Part of the render
 * contract (RENDERER_VERSION) — changing it changes output for unchanged input.
 */
export const CPS = 0.5;

/** Supported constant MP3 bitrates (kbps). Template default is 128. */
export const SUPPORTED_BITRATES = [96, 128, 160, 192, 256, 320] as const;

/** Upper bound on authored duration (ms) — a conservative sound-effect ceiling. */
export const MAX_DURATION_MS = 60_000;
/** Upper bound on effect tail (ms) — release/echo/reverb settle time. */
export const MAX_TAIL_MS = 30_000;

/** Front-matter keys that every source must declare. */
export const REQUIRED_FIELDS = ['id', 'duration_ms', 'tail_ms', 'channels', 'bitrate_kbps'] as const;

/** Optional front-matter keys (each has a default when absent). */
export const OPTIONAL_FIELDS = ['render'] as const;

/** The exact set of allowed front-matter keys; any other key is rejected. */
export const KNOWN_FIELDS = [...REQUIRED_FIELDS, ...OPTIONAL_FIELDS] as const;

/**
 * The render pipeline for a sound. `offline` (default) renders deterministically
 * through an OfflineAudioContext. `realtime` plays the pattern through a live
 * AudioContext in wall-clock time and captures the master mix — non-deterministic,
 * but the realtime audio thread flushes denormals to zero, so it renders effects
 * (a distorted signal into `.room()` reverb) that stall the offline path. Opt in
 * only when a sound genuinely needs it.
 */
export const RENDER_MODES = ['offline', 'realtime'] as const;
export type RenderMode = (typeof RENDER_MODES)[number];

export interface RenderConfig {
  readonly id: string;
  readonly durationMs: number;
  readonly tailMs: number;
  readonly channels: 1 | 2;
  readonly bitrateKbps: number;
  readonly mode: RenderMode;
}

/** Total render length in milliseconds (authored duration + effect tail). */
export function totalMs(config: RenderConfig): number {
  return config.durationMs + config.tailMs;
}

/** Total render length in seconds. */
export function totalSeconds(config: RenderConfig): number {
  return totalMs(config) / 1000;
}

function requireInteger(value: unknown, field: string, id: string): number {
  const ok = typeof value === 'number' && Number.isInteger(value);
  return ok
    ? (value as number)
    : (() => {
        throw new SoundError('INVALID_FRONT_MATTER', `field \`${field}\` must be an integer`, { id });
      })();
}

/**
 * Validate an already-parsed front-matter object (unknown-field rejection has
 * already happened in frontmatter.ts) into a RenderConfig. Throws SoundError
 * with INVALID_FRONT_MATTER on any bound/allowlist violation.
 */
export function toRenderConfig(fields: Record<string, unknown>): RenderConfig {
  const id = typeof fields.id === 'string' ? fields.id : '';
  const durationMs = requireInteger(fields.duration_ms, 'duration_ms', id);
  const tailMs = requireInteger(fields.tail_ms, 'tail_ms', id);
  const channels = requireInteger(fields.channels, 'channels', id);
  const bitrateKbps = requireInteger(fields.bitrate_kbps, 'bitrate_kbps', id);
  const mode = requireMode(fields.render, id);

  if (durationMs <= 0 || durationMs > MAX_DURATION_MS) {
    throw new SoundError(
      'INVALID_FRONT_MATTER',
      `\`duration_ms\` must be in (0, ${MAX_DURATION_MS}]; got ${durationMs}`,
      { id },
    );
  }
  if (tailMs < 0 || tailMs > MAX_TAIL_MS) {
    throw new SoundError(
      'INVALID_FRONT_MATTER',
      `\`tail_ms\` must be in [0, ${MAX_TAIL_MS}]; got ${tailMs}`,
      { id },
    );
  }
  if (channels !== 1 && channels !== 2) {
    throw new SoundError('INVALID_FRONT_MATTER', `\`channels\` must be 1 or 2; got ${channels}`, {
      id,
    });
  }
  if (!SUPPORTED_BITRATES.includes(bitrateKbps as (typeof SUPPORTED_BITRATES)[number])) {
    throw new SoundError(
      'INVALID_FRONT_MATTER',
      `\`bitrate_kbps\` must be one of ${SUPPORTED_BITRATES.join(', ')}; got ${bitrateKbps}`,
      { id },
    );
  }

  return { id, durationMs, tailMs, channels, bitrateKbps, mode };
}

/** Validate the optional `render` field into a RenderMode (defaults to offline). */
function requireMode(value: unknown, id: string): RenderMode {
  const mode = value === undefined ? 'offline' : value;
  return RENDER_MODES.includes(mode as RenderMode)
    ? (mode as RenderMode)
    : (() => {
        throw new SoundError(
          'INVALID_FRONT_MATTER',
          `\`render\` must be one of ${RENDER_MODES.join(', ')}; got ${JSON.stringify(value)}`,
          { id },
        );
      })();
}

/**
 * A stable, canonical string describing the encoder settings that affect output
 * bytes. Feeds the source hash so an encoder-setting change invalidates caches.
 */
export function encoderSignature(config: RenderConfig): string {
  return [
    'codec=mp3',
    'cbr',
    `bitrate_kbps=${config.bitrateKbps}`,
    `channels=${config.channels}`,
    `sample_rate=${SAMPLE_RATE}`,
    'strip_metadata=1',
    'bitexact=1',
  ].join(';');
}
