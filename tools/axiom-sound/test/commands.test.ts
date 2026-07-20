import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { resolveApp } from '../src/appdir.ts';
import { parseSource } from '../src/frontmatter.ts';
import { assertNoDuplicateIds, listSources } from '../src/sources.ts';
import { runNew } from '../src/commands/new.ts';
import { runList } from '../src/commands/list.ts';
import { main } from '../src/cli.ts';
import { isSoundError } from '../src/errors.ts';
import { Reporter } from '../src/output.ts';
import { makeTempApp, runCommand, tone, writeSound } from './helpers.ts';

test('resolveApp rejects a missing directory', () => {
  try {
    resolveApp(join(tmpdir(), 'axsnd-nope-does-not-exist'));
    assert.fail('expected APP_NOT_FOUND');
  } catch (err) {
    assert.ok(isSoundError(err) && err.code === 'APP_NOT_FOUND');
  }
});

test('resolveApp rejects a directory without app.toml', () => {
  const dir = mkdtempSync(join(tmpdir(), 'axsnd-noapp-'));
  try {
    resolveApp(dir);
    assert.fail('expected APP_MANIFEST_NOT_FOUND');
  } catch (err) {
    assert.ok(isSoundError(err) && err.code === 'APP_MANIFEST_NOT_FOUND');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('new scaffolds a valid, parseable source and never overwrites', async () => {
  const app = makeTempApp();
  try {
    const dirs = resolveApp(app.root);
    const r = await runCommand((rep) => runNew(dirs, 'ui-hover', rep, { json: true, verbose: false }));
    assert.equal(r.value, 0);
    const path = join(dirs.soundsDir, 'ui-hover.strudel');
    assert.ok(existsSync(path));
    // The scaffold parses and its id matches the filename.
    assert.equal(parseSource(readFileSync(path, 'utf8'), 'ui-hover').config.id, 'ui-hover');

    // Second attempt refuses to overwrite.
    try {
      await runCommand((rep) => runNew(dirs, 'ui-hover', rep, { json: true, verbose: false }));
      assert.fail('expected SOURCE_EXISTS');
    } catch (err) {
      assert.ok(isSoundError(err) && err.code === 'SOURCE_EXISTS');
    }
  } finally {
    app.cleanup();
  }
});

test('new rejects an invalid id', () => {
  const app = makeTempApp();
  try {
    const dirs = resolveApp(app.root);
    const silent = new Reporter({ json: true, verbose: false }, { out() {}, err() {} });
    assert.throws(
      () => runNew(dirs, 'Bad_Id', silent, { json: true, verbose: false }),
      (err: unknown) => isSoundError(err) && err.code === 'INVALID_SOUND_ID',
    );
  } finally {
    app.cleanup();
  }
});

test('duplicate declared ids are rejected', () => {
  const app = makeTempApp();
  try {
    writeSound(app.root, 'first', tone('dup', 'note("c5").s("triangle")'));
    writeSound(app.root, 'second', tone('dup', 'note("e5").s("triangle")'));
    try {
      assertNoDuplicateIds(listSources(resolveApp(app.root)));
      assert.fail('expected DUPLICATE_SOUND_ID');
    } catch (err) {
      assert.ok(isSoundError(err) && err.code === 'DUPLICATE_SOUND_ID');
    }
  } finally {
    app.cleanup();
  }
});

test('list reports a stable, sorted inventory with statuses', async () => {
  const app = makeTempApp();
  try {
    const dirs = resolveApp(app.root);
    const r = await runCommand((rep) => Promise.resolve(runList(dirs, rep, { json: true, verbose: false })));
    const sounds = (JSON.parse(r.stdout).sounds as Array<{ id: string; buildStatus: string; sourceStatus: string }>);
    const ids = sounds.map((s) => s.id);
    assert.deepEqual([...ids].sort(), ids, 'ids are sorted');
    const ok = sounds.find((s) => s.id === 'tone-ok');
    assert.equal(ok?.sourceStatus, 'ok');
    assert.equal(ok?.buildStatus, 'unbuilt');
    // bad-syntax parses front matter fine (only the body is invalid) -> ok source.
    const bad = sounds.find((s) => s.id === 'bad-syntax');
    assert.equal(bad?.sourceStatus, 'ok');
  } finally {
    app.cleanup();
  }
});

test('cli prints usage with no command and fails cleanly on a bad one', async () => {
  const help = await runCaptured(['--json']);
  assert.equal(help.code, 0);

  const unknown = await runCaptured(['frobnicate', '--app', '.', '--json']);
  assert.equal(unknown.code, 1);
  assert.match(unknown.stdout, /USAGE|unknown command/);

  const noApp = await runCaptured(['check', '--json']);
  assert.equal(noApp.code, 1);
  assert.match(noApp.stdout, /--app/);
});

async function runCaptured(argv: string[]): Promise<{ code: number; stdout: string }> {
  const out: string[] = [];
  const code = await main(argv, { out: (t) => void out.push(t), err: () => {} });
  return { code, stdout: out.join('') };
}
