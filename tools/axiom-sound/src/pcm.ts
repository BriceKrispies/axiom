// PCM validation, run on the rendered Float32 channels before encoding. A
// render that fails any check is rejected with a specific SoundError so a bad
// sound never reaches the app's assets/ directory.

import { SoundError } from './errors.ts';
import { SAMPLE_RATE, totalSeconds, type RenderConfig } from './config.ts';
import type { RenderedPcm } from './wav.ts';

/**
 * RMS below this (linear, full-scale 1.0) is treated as silence. -60 dBFS is a
 * conservative floor: audible sound sits well above it, while a pattern that
 * renders to numerical dust (or gain(0)) sits below.
 */
export const SILENCE_RMS_THRESHOLD = 0.001; // ~ -60 dBFS

/** Peak at or above this (linear) is treated as clipping. */
export const CLIP_THRESHOLD = 1.0;

/** Allowed deviation between requested and actual render duration. */
export const DURATION_TOLERANCE_MS = 20;

export interface PcmStats {
  readonly peak: number;
  readonly rms: number;
  readonly frames: number;
  readonly channels: number;
  readonly sampleRate: number;
  readonly durationMs: number;
}

/** Compute peak/RMS/shape stats over all channels. */
export function computeStats(pcm: RenderedPcm): PcmStats {
  const channels = pcm.channels.length;
  const frames = channels === 0 ? 0 : pcm.channels[0].length;
  let peak = 0;
  let sumSq = 0;
  let count = 0;
  for (const data of pcm.channels) {
    for (let i = 0; i < data.length; i++) {
      const v = data[i];
      const a = Math.abs(v);
      if (a > peak) {
        peak = a;
      }
      sumSq += v * v;
      count += 1;
    }
  }
  const rms = count === 0 ? 0 : Math.sqrt(sumSq / count);
  return {
    peak,
    rms,
    frames,
    channels,
    sampleRate: pcm.sampleRate,
    durationMs: pcm.sampleRate === 0 ? 0 : (frames / pcm.sampleRate) * 1000,
  };
}

/**
 * Validate rendered PCM against `config`. Throws:
 *   RENDER_INVALID_PCM  — empty, wrong channel count / sample rate / duration,
 *                         or non-finite samples
 *   RENDER_SILENT       — RMS below the silence floor
 *   RENDER_CLIPPED      — peak at/above full scale (reports the measured peak)
 * Returns the computed stats on success.
 */
export function validatePcm(pcm: RenderedPcm, config: RenderConfig): PcmStats {
  const id = config.id;

  if (pcm.channels.length === 0 || pcm.channels[0].length === 0) {
    throw new SoundError('RENDER_INVALID_PCM', 'render produced no PCM samples', { id });
  }
  if (pcm.channels.length !== config.channels) {
    throw new SoundError(
      'RENDER_INVALID_PCM',
      `render produced ${pcm.channels.length} channel(s); expected ${config.channels}`,
      { id },
    );
  }
  if (pcm.sampleRate !== SAMPLE_RATE) {
    throw new SoundError(
      'RENDER_INVALID_PCM',
      `render sample rate ${pcm.sampleRate} Hz != expected ${SAMPLE_RATE} Hz`,
      { id },
    );
  }

  const frames = pcm.channels[0].length;
  for (const data of pcm.channels) {
    if (data.length !== frames) {
      throw new SoundError('RENDER_INVALID_PCM', 'channels have mismatched lengths', { id });
    }
    for (let i = 0; i < data.length; i++) {
      if (!Number.isFinite(data[i])) {
        throw new SoundError('RENDER_INVALID_PCM', 'render contains non-finite samples', { id });
      }
    }
  }

  const stats = computeStats(pcm);

  const expectedMs = totalSeconds(config) * 1000;
  if (Math.abs(stats.durationMs - expectedMs) > DURATION_TOLERANCE_MS) {
    throw new SoundError(
      'RENDER_INVALID_PCM',
      `render duration ${stats.durationMs.toFixed(1)}ms differs from expected ${expectedMs.toFixed(
        1,
      )}ms by more than ${DURATION_TOLERANCE_MS}ms`,
      { id, extra: { actualMs: stats.durationMs, expectedMs } },
    );
  }

  if (stats.peak >= CLIP_THRESHOLD) {
    throw new SoundError(
      'RENDER_CLIPPED',
      `render clips: peak ${stats.peak.toFixed(4)} >= ${CLIP_THRESHOLD.toFixed(1)} full scale`,
      { id, extra: { peak: stats.peak } },
    );
  }

  if (stats.rms < SILENCE_RMS_THRESHOLD) {
    throw new SoundError(
      'RENDER_SILENT',
      `render is silent: RMS ${stats.rms.toExponential(2)} < ${SILENCE_RMS_THRESHOLD} (${'~ -60 dBFS'})`,
      { id, extra: { rms: stats.rms } },
    );
  }

  return stats;
}
