// Pins the assembled offline exporter (browser-entry.ts + the pinned @strudel/*
// and superdough versions) against real renders. This is the "prove the vendored
// exporter still works with the pinned Strudel version" guard the task requires:
// if a dependency bump breaks the offline render, this test fails loudly.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { RenderHarness } from '../src/render/harness.ts';
import { computeStats } from '../src/pcm.ts';
import { toRenderConfig } from '../src/config.ts';

const config = toRenderConfig({
  id: 'tone',
  duration_ms: 400,
  tail_ms: 100,
  channels: 1,
  bitrate_kbps: 128,
});
const BODY = 'note("c5 e5").s("triangle").attack(0.005).decay(0.1).sustain(0).release(0.15).gain(0.7)';

test('offline exporter renders audible, deterministic PCM', { timeout: 120_000 }, async () => {
  const harness = await RenderHarness.launch();
  try {
    const a = await harness.render(BODY, config, 1);
    const b = await harness.render(BODY, config, 1);

    const stats = computeStats(a);
    assert.ok(stats.rms > 0.01, `render should be audible (rms=${stats.rms})`);
    assert.ok(stats.peak < 1.0, 'render should not clip');
    assert.equal(a.sampleRate, 48000);
    assert.equal(a.channels.length, 1);

    // Determinism: two independent offline renders must be byte-identical.
    assert.equal(a.channels[0].length, b.channels[0].length);
    let maxDiff = 0;
    for (let i = 0; i < a.channels[0].length; i++) {
      maxDiff = Math.max(maxDiff, Math.abs(a.channels[0][i] - b.channels[0][i]));
    }
    assert.equal(maxDiff, 0, `renders should be identical (maxDiff=${maxDiff})`);
  } finally {
    await harness.dispose();
  }
});
