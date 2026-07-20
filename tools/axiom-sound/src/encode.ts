// MP3 encoding + validation + atomic publication.
//
// Given a validated WAV intermediate, encode a constant-bitrate MP3 with the
// pinned local FFmpeg (never a PATH executable), strip all metadata, validate
// the result with FFprobe, and only then atomically rename it into the app's
// assets/audio/. A failure at any step leaves the previous asset untouched and
// removes the temp file.

import { spawnSync } from 'node:child_process';
import { renameSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { SoundError } from './errors.ts';
import { SAMPLE_RATE, totalSeconds, type RenderConfig } from './config.ts';
import { atomicWrite } from './atomicwrite.ts';

const require = createRequire(import.meta.url);
// Both packages ship pinned native binaries. ffmpeg-static's CJS export is the
// binary path (string | null); ffprobe-static exposes `{ path }`.
const ffmpegPath = require('ffmpeg-static') as string | null;
const ffprobePath: string = (require('ffprobe-static') as { path: string }).path;

if (!ffmpegPath) {
  throw new SoundError('ENCODE_FAILED', 'ffmpeg-static did not resolve a binary path');
}
const FFMPEG: string = ffmpegPath;

/** Tolerance (seconds) when validating the encoded MP3 duration. */
const DURATION_TOLERANCE_S = 0.15;

export interface EncodeResult {
  readonly outPath: string;
  readonly durationMs: number;
  readonly channels: number;
  readonly sampleRate: number;
  readonly bitrateKbps: number;
}

/**
 * Encode `wavPath` to MP3 and publish it atomically at `destPath`. Throws
 * ENCODE_FAILED / ENCODE_VALIDATION_FAILED on failure without leaving a partial
 * destination. Returns the probed properties of the published file.
 */
export function encodeMp3(wavPath: string, destPath: string, config: RenderConfig): EncodeResult {
  const tmp = join(dirname(destPath), `.${config.id}.mp3.tmp-${process.pid}`);

  const args = [
    '-hide_banner',
    '-nostdin',
    '-y',
    '-i',
    wavPath,
    '-c:a',
    'libmp3lame',
    '-b:a',
    `${config.bitrateKbps}k`,
    '-ac',
    String(config.channels),
    '-ar',
    String(SAMPLE_RATE),
    // Constant bitrate: disable the bit reservoir and VBR.
    '-abr',
    '0',
    // Strip metadata and non-reproducible headers.
    '-map_metadata',
    '-1',
    '-id3v2_version',
    '0',
    '-write_xing',
    '0',
    '-flags',
    '+bitexact',
    '-fflags',
    '+bitexact',
    '-f',
    'mp3',
    tmp,
  ];

  const run = spawnSync(FFMPEG, args, { encoding: 'utf8' });
  if (run.status !== 0) {
    rmSync(tmp, { force: true });
    throw new SoundError('ENCODE_FAILED', `ffmpeg failed (exit ${run.status ?? 'signal'})`, {
      id: config.id,
      extra: { stderr: tailLines(run.stderr, 12) },
    });
  }

  let probed: Omit<EncodeResult, 'outPath'>;
  try {
    probed = probeAndValidate(tmp, config);
  } catch (err) {
    rmSync(tmp, { force: true });
    throw err;
  }

  // Atomic publish: refuse a symlinked dest, then rename over the target.
  try {
    // atomicWrite is for byte payloads; for the already-written temp file we do
    // the equivalent rename ourselves (same directory, so it is atomic).
    renameSync(tmp, destPath);
  } catch (err) {
    rmSync(tmp, { force: true });
    throw new SoundError('ENCODE_FAILED', `failed to publish asset: ${destPath}`, {
      id: config.id,
      cause: err,
    });
  }
  return { ...probed, outPath: destPath };
}

interface FfprobeStream {
  readonly codec_name?: string;
  readonly channels?: number;
  readonly sample_rate?: string;
  readonly bit_rate?: string;
}
interface FfprobeOutput {
  readonly streams?: readonly FfprobeStream[];
  readonly format?: { readonly duration?: string };
}

function probeAndValidate(path: string, config: RenderConfig): Omit<EncodeResult, 'outPath'> {
  const run = spawnSync(
    ffprobePath,
    [
      '-hide_banner',
      '-v',
      'error',
      '-select_streams',
      'a:0',
      '-show_entries',
      'stream=codec_name,channels,sample_rate,bit_rate:format=duration',
      '-of',
      'json',
      path,
    ],
    { encoding: 'utf8' },
  );
  if (run.status !== 0) {
    throw new SoundError('ENCODE_VALIDATION_FAILED', `ffprobe failed (exit ${run.status ?? 'signal'})`, {
      id: config.id,
      extra: { stderr: tailLines(run.stderr, 8) },
    });
  }

  let json: FfprobeOutput;
  try {
    json = JSON.parse(run.stdout) as FfprobeOutput;
  } catch (err) {
    throw new SoundError('ENCODE_VALIDATION_FAILED', 'ffprobe returned invalid JSON', {
      id: config.id,
      cause: err,
    });
  }

  const stream = json.streams?.[0];
  const fail = (message: string): never => {
    throw new SoundError('ENCODE_VALIDATION_FAILED', message, { id: config.id });
  };
  const expectedS = totalSeconds(config);

  if (!stream || stream.codec_name !== 'mp3') {
    return fail(`encoded file is not MP3 (codec: ${stream?.codec_name ?? 'none'})`);
  }
  if (stream.channels !== config.channels) {
    return fail(`encoded channels ${stream.channels} != expected ${config.channels}`);
  }
  const sampleRate = Number(stream.sample_rate ?? 0);
  if (sampleRate !== SAMPLE_RATE) {
    return fail(`encoded sample rate ${sampleRate} != expected ${SAMPLE_RATE}`);
  }
  const durationS = Number(json.format?.duration ?? 0);
  if (!(durationS > 0) || Math.abs(durationS - expectedS) > DURATION_TOLERANCE_S) {
    return fail(
      `encoded duration ${durationS.toFixed(3)}s differs from expected ${expectedS.toFixed(3)}s`,
    );
  }

  return {
    durationMs: Math.round(durationS * 1000),
    channels: stream.channels,
    sampleRate,
    bitrateKbps: config.bitrateKbps,
  };
}

/** Write a validated WAV buffer to the cache path atomically. */
export function writeWavIntermediate(wavPath: string, bytes: Uint8Array): void {
  atomicWrite(wavPath, bytes);
}

function tailLines(text: string | null, n: number): string {
  return (text ?? '').split(/\r?\n/).filter(Boolean).slice(-n).join('\n');
}
