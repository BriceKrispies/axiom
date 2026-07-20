import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { assertInsideApp, assertStemMatchesId, assertValidId, isValidId } from '../src/ids.ts';
import { isSoundError } from '../src/errors.ts';

test('accepts kebab-case ids', () => {
  for (const id of ['a', 'ui-perfect', 'x1', 'a-b-c', 'hit9']) {
    assert.ok(isValidId(id), id);
  }
});

test('rejects non-kebab ids', () => {
  for (const id of ['A', 'ui_perfect', '-x', 'x-', 'a--b', 'wave!', 'spa ce', '']) {
    assert.ok(!isValidId(id), id);
  }
});

test('assertValidId throws INVALID_SOUND_ID', () => {
  try {
    assertValidId('Bad_Id');
    assert.fail('expected error');
  } catch (err) {
    assert.ok(isSoundError(err) && err.code === 'INVALID_SOUND_ID');
  }
});

test('assertStemMatchesId enforces stem == id', () => {
  assertStemMatchesId('foo', 'foo');
  try {
    assertStemMatchesId('foo', 'bar');
    assert.fail('expected error');
  } catch (err) {
    assert.ok(isSoundError(err) && err.code === 'INVALID_SOUND_ID');
  }
});

test('assertInsideApp accepts paths within the app', () => {
  const root = mkdtempSync(join(tmpdir(), 'axsnd-ids-'));
  try {
    const p = assertInsideApp(root, join(root, 'sounds', 'a.strudel'), 'source');
    assert.ok(p.startsWith(root));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('assertInsideApp rejects .. traversal', () => {
  const root = mkdtempSync(join(tmpdir(), 'axsnd-ids-'));
  try {
    assertInsideApp(root, join(root, '..', 'escape.strudel'), 'source');
    assert.fail('expected traversal rejection');
  } catch (err) {
    assert.ok(isSoundError(err) && err.code === 'INVALID_SOUND_ID');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
