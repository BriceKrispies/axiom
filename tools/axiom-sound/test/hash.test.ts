import { test } from 'node:test';
import assert from 'node:assert/strict';
import { sourceSha256 } from '../src/hash.ts';
import { toRenderConfig } from '../src/config.ts';

const config = toRenderConfig({
  id: 'x',
  duration_ms: 500,
  tail_ms: 100,
  channels: 1,
  bitrate_kbps: 128,
});
const raw = '+++\nid = "x"\n+++\nnote("c5")\n';

test('source hash is deterministic and 64 hex chars', () => {
  const a = sourceSha256(raw, config);
  const b = sourceSha256(raw, config);
  assert.equal(a, b);
  assert.match(a, /^[0-9a-f]{64}$/);
});

test('changing the source body changes the hash', () => {
  const a = sourceSha256(raw, config);
  const b = sourceSha256(raw.replace('c5', 'e5'), config);
  assert.notEqual(a, b);
});

test('changing config (bitrate) changes the hash even for identical source', () => {
  const a = sourceSha256(raw, config);
  const other = toRenderConfig({
    id: 'x',
    duration_ms: 500,
    tail_ms: 100,
    channels: 1,
    bitrate_kbps: 320,
  });
  assert.notEqual(a, sourceSha256(raw, other));
});

test('changing the render mode changes the hash (cache invalidates)', () => {
  const offline = sourceSha256(raw, config);
  const realtime = sourceSha256(
    raw,
    toRenderConfig({ id: 'x', duration_ms: 500, tail_ms: 100, channels: 1, bitrate_kbps: 128, render: 'realtime' }),
  );
  assert.notEqual(offline, realtime);
});
