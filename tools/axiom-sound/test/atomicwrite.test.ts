import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  existsSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { atomicWrite } from '../src/atomicwrite.ts';

test('writes bytes and creates parent directories', () => {
  const dir = mkdtempSync(join(tmpdir(), 'axsnd-aw-'));
  try {
    const dest = join(dir, 'nested', 'deep', 'file.bin');
    atomicWrite(dest, new Uint8Array([1, 2, 3]));
    assert.deepEqual([...readFileSync(dest)], [1, 2, 3]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('overwrites an existing regular file atomically', () => {
  const dir = mkdtempSync(join(tmpdir(), 'axsnd-aw-'));
  try {
    const dest = join(dir, 'f.txt');
    writeFileSync(dest, 'old');
    atomicWrite(dest, 'new');
    assert.equal(readFileSync(dest, 'utf8'), 'new');
    // no leftover temp files
    assert.deepEqual(
      readdirSync(dir).filter((n) => n.includes('.tmp-')),
      [],
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('refuses to write through a symlink destination', (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'axsnd-aw-'));
  try {
    const real = join(dir, 'real.txt');
    const link = join(dir, 'link.txt');
    writeFileSync(real, 'protected');
    try {
      symlinkSync(real, link);
    } catch {
      // Symlink creation can be denied (e.g. Windows without privilege); skip.
      t.skip('symlink creation not permitted here');
      return;
    }
    assert.throws(() => atomicWrite(link, 'attack'), /symlink/);
    assert.equal(readFileSync(real, 'utf8'), 'protected');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
