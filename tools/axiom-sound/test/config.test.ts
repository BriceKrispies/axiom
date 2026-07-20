import { test } from 'node:test';
import assert from 'node:assert/strict';
import { encoderSignature, toRenderConfig, totalMs, totalSeconds } from '../src/config.ts';
import { isSoundError } from '../src/errors.ts';

function base(): Record<string, unknown> {
  return { id: 'x', duration_ms: 500, tail_ms: 100, channels: 1, bitrate_kbps: 128 };
}

function expectBad(fields: Record<string, unknown>): void {
  try {
    toRenderConfig(fields);
    assert.fail('expected INVALID_FRONT_MATTER');
  } catch (err) {
    assert.ok(isSoundError(err) && err.code === 'INVALID_FRONT_MATTER');
  }
}

test('accepts valid config', () => {
  const c = toRenderConfig(base());
  assert.equal(c.durationMs, 500);
  assert.equal(totalMs(c), 600);
  assert.equal(totalSeconds(c), 0.6);
});

test('rejects non-integer numeric fields', () => {
  expectBad({ ...base(), duration_ms: 500.5 });
});

test('rejects duration <= 0 and > 60s', () => {
  expectBad({ ...base(), duration_ms: 0 });
  expectBad({ ...base(), duration_ms: 60_001 });
});

test('rejects negative or over-long tail', () => {
  expectBad({ ...base(), tail_ms: -1 });
  expectBad({ ...base(), tail_ms: 30_001 });
});

test('rejects channels other than 1 or 2', () => {
  expectBad({ ...base(), channels: 0 });
  expectBad({ ...base(), channels: 3 });
  assert.equal(toRenderConfig({ ...base(), channels: 2 }).channels, 2);
});

test('rejects a bitrate outside the allowlist', () => {
  expectBad({ ...base(), bitrate_kbps: 130 });
  for (const b of [96, 128, 160, 192, 256, 320]) {
    assert.equal(toRenderConfig({ ...base(), bitrate_kbps: b }).bitrateKbps, b);
  }
});

test('render mode defaults to offline and accepts realtime', () => {
  assert.equal(toRenderConfig(base()).mode, 'offline');
  assert.equal(toRenderConfig({ ...base(), render: 'offline' }).mode, 'offline');
  assert.equal(toRenderConfig({ ...base(), render: 'realtime' }).mode, 'realtime');
});

test('rejects an unknown render mode', () => {
  expectBad({ ...base(), render: 'live' });
  expectBad({ ...base(), render: 42 });
});

test('encoder signature reflects settings that change bytes', () => {
  const a = encoderSignature(toRenderConfig(base()));
  const b = encoderSignature(toRenderConfig({ ...base(), bitrate_kbps: 320 }));
  assert.notEqual(a, b);
  assert.match(a, /codec=mp3/);
  assert.match(a, /sample_rate=48000/);
});
