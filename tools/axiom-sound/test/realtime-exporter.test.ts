// Pins the realtime capture path (realtime-entry.ts): a live AudioContext played
// in wall-clock time and captured. Its reason to exist is effects whose decaying
// tails generate denormals — a distorted signal into `.room()` reverb — which
// stall the OfflineAudioContext render thread but render fine in realtime (which
// flushes denormals). Realtime is non-deterministic, so this asserts audible,
// non-clipping, correctly-shaped PCM rather than byte-equality.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { RenderHarness } from '../src/render/harness.ts';
import { computeStats } from '../src/pcm.ts';
import { toRenderConfig } from '../src/config.ts';

const config = toRenderConfig({
  id: 'rt',
  duration_ms: 500,
  tail_ms: 200,
  channels: 2,
  bitrate_kbps: 128,
  render: 'realtime',
});
const BODY = 'note("c3 e3").s("sawtooth").distort("4:.3").room(.2).gain(0.5)';

test('realtime capture renders audible, non-clipping PCM (distort + room)', { timeout: 60_000 }, async () => {
  const harness = await RenderHarness.launch();
  try {
    const pcm = await harness.renderRealtime(BODY, config, 1);
    const stats = computeStats(pcm);
    assert.ok(stats.rms > 0.001, `render should be audible (rms=${stats.rms})`);
    assert.ok(stats.peak < 1.0, `render should not clip (peak=${stats.peak})`);
    assert.equal(pcm.sampleRate, 48000);
    assert.equal(pcm.channels.length, 2);
    // duration_ms + tail_ms = 700 ms of audio at 48 kHz, within a few blocks.
    assert.ok(
      Math.abs(pcm.channels[0].length - 0.7 * 48000) < 2000,
      `captured length ~0.7s (got ${pcm.channels[0].length})`,
    );
  } finally {
    await harness.dispose();
  }
});
