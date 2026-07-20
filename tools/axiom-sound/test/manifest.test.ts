import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  readManifest,
  serializeManifest,
  withEntry,
  withoutEntries,
  writeManifestIfChanged,
  type AssetEntry,
  type Manifest,
} from '../src/manifest.ts';

function entry(id: string): AssetEntry {
  return {
    path: `audio/${id}.mp3`,
    mimeType: 'audio/mpeg',
    durationMs: 600,
    channels: 1,
    sampleRate: 48000,
    bitrateKbps: 128,
    sha256: `sha-${id}`,
    sourceSha256: `src-${id}`,
  };
}

test('serializes asset ids in deterministic sorted order', () => {
  let m: Manifest = readManifest('/does/not/exist');
  m = withEntry(withEntry(withEntry(m, 'zeta', entry('zeta')), 'alpha', entry('alpha')), 'mid', entry('mid'));
  const text = serializeManifest(m);
  const order = [...text.matchAll(/"(alpha|mid|zeta)":/g)].map((x) => x[1]);
  assert.deepEqual(order, ['alpha', 'mid', 'zeta']);
});

test('paths are relative, no absolute paths or timestamps', () => {
  const text = serializeManifest(withEntry(readManifest('/none'), 'a', entry('a')));
  assert.match(text, /"path": "audio\/a\.mp3"/);
  assert.doesNotMatch(text, /[A-Za-z]:\\|\/tmp\/|timestamp|generatedAt/);
});

test('preserves unrelated existing entries', () => {
  const dir = mkdtempSync(join(tmpdir(), 'axsnd-man-'));
  const path = join(dir, 'manifest.json');
  try {
    writeManifestIfChanged(path, withEntry(readManifest(path), 'keep', entry('keep')));
    const reloaded = readManifest(path);
    const updated = withEntry(reloaded, 'add', entry('add'));
    writeManifestIfChanged(path, updated);
    const final = readManifest(path);
    assert.ok(final.assets.keep);
    assert.ok(final.assets.add);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('does not rewrite when serialized content is unchanged', () => {
  const dir = mkdtempSync(join(tmpdir(), 'axsnd-man-'));
  const path = join(dir, 'manifest.json');
  try {
    const m = withEntry(readManifest(path), 'a', entry('a'));
    assert.equal(writeManifestIfChanged(path, m), true);
    const mtime1 = statSync(path).mtimeMs;
    assert.equal(writeManifestIfChanged(path, m), false, 'unchanged content should not rewrite');
    assert.equal(statSync(path).mtimeMs, mtime1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('withoutEntries removes only the named ids', () => {
  let m = withEntry(withEntry(readManifest('/none'), 'a', entry('a')), 'b', entry('b'));
  m = withoutEntries(m, ['a']);
  assert.ok(!m.assets.a);
  assert.ok(m.assets.b);
});

test('reading a corrupt manifest throws MANIFEST_WRITE_FAILED', () => {
  const dir = mkdtempSync(join(tmpdir(), 'axsnd-man-'));
  const path = join(dir, 'manifest.json');
  try {
    writeFileSync(path, '{ not json');
    assert.throws(() => readManifest(path), /MANIFEST_WRITE_FAILED|not valid JSON/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
