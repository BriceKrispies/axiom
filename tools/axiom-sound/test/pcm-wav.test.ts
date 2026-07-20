import { test } from 'node:test';
import assert from 'node:assert/strict';
import { encodeWav, type RenderedPcm } from '../src/wav.ts';
import { computeStats, validatePcm } from '../src/pcm.ts';
import { toRenderConfig } from '../src/config.ts';
import { isSoundError } from '../src/errors.ts';

const SR = 48000;

function config(channels: 1 | 2 = 1): ReturnType<typeof toRenderConfig> {
  return toRenderConfig({
    id: 'x',
    duration_ms: 100,
    tail_ms: 0,
    channels,
    bitrate_kbps: 128,
  });
}

/** A steady tone of `frames` samples at amplitude `amp`. */
function tone(frames: number, amp: number, channels = 1): RenderedPcm {
  const make = (): Float32Array => {
    const d = new Float32Array(frames);
    for (let i = 0; i < frames; i++) {
      d[i] = amp * Math.sin((2 * Math.PI * 440 * i) / SR);
    }
    return d;
  };
  return { channels: Array.from({ length: channels }, make), sampleRate: SR };
}

test('encodeWav produces a 44-byte header + 16-bit interleaved data', () => {
  const frames = 4800; // 100ms
  const wav = encodeWav(tone(frames, 0.5));
  assert.equal(wav.toString('ascii', 0, 4), 'RIFF');
  assert.equal(wav.toString('ascii', 8, 12), 'WAVE');
  assert.equal(wav.readUInt16LE(22), 1); // channels
  assert.equal(wav.readUInt32LE(24), SR); // sample rate
  assert.equal(wav.readUInt16LE(34), 16); // bits per sample
  assert.equal(wav.length, 44 + frames * 2);
});

test('validatePcm accepts an audible in-spec render', () => {
  const stats = validatePcm(tone(4800, 0.5), config());
  assert.ok(stats.rms > 0.001);
  assert.equal(stats.channels, 1);
});

test('rejects an empty render', () => {
  expectCode(() => validatePcm({ channels: [new Float32Array(0)], sampleRate: SR }, config()), 'RENDER_INVALID_PCM');
});

test('rejects a silent render', () => {
  expectCode(() => validatePcm(tone(4800, 0), config()), 'RENDER_SILENT');
});

test('rejects a clipping render and reports the peak', () => {
  try {
    validatePcm(tone(4800, 1.5), config());
    assert.fail('expected clip');
  } catch (err) {
    assert.ok(isSoundError(err) && err.code === 'RENDER_CLIPPED');
    assert.ok((err.details.extra?.peak as number) >= 1.0);
  }
});

test('rejects non-finite samples', () => {
  const bad = tone(4800, 0.5);
  bad.channels[0][10] = Number.NaN;
  expectCode(() => validatePcm(bad, config()), 'RENDER_INVALID_PCM');
});

test('rejects a wrong channel count', () => {
  expectCode(() => validatePcm(tone(4800, 0.5, 2), config(1)), 'RENDER_INVALID_PCM');
});

test('rejects a duration outside tolerance', () => {
  // 200ms of audio for a 100ms config
  expectCode(() => validatePcm(tone(9600, 0.5), config()), 'RENDER_INVALID_PCM');
});

test('computeStats measures peak and rms', () => {
  const s = computeStats(tone(4800, 0.8));
  assert.ok(s.peak > 0.75 && s.peak <= 0.81);
  assert.ok(s.rms > 0 && s.rms < s.peak);
});

function expectCode(fn: () => unknown, code: string): void {
  try {
    fn();
    assert.fail(`expected ${code}`);
  } catch (err) {
    assert.ok(isSoundError(err), `expected SoundError, got ${String(err)}`);
    assert.equal(err.code, code);
  }
}
