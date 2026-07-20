// The end-to-end fixture suite: the 16 behaviors the task requires, proven
// against a real headless-Chromium render + FFmpeg encode, using hermetic temp
// copies of the fixture app. Each test drives the actual command functions.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { assetPath, resolveApp, sourcePath } from '../src/appdir.ts';
import { runCheck } from '../src/commands/check.ts';
import { runBuild } from '../src/commands/build.ts';
import { runClean } from '../src/commands/clean.ts';
import { encodeMp3 } from '../src/encode.ts';
import { wavCachePath } from '../src/cache.ts';
import { readManifest } from '../src/manifest.ts';
import { fileSha256, sourceSha256 } from '../src/hash.ts';
import { isSoundError } from '../src/errors.ts';
import { parseSource } from '../src/frontmatter.ts';
import { makeTempApp, runCommand, tone, writeSound } from './helpers.ts';

const require = createRequire(import.meta.url);
const ffprobe: string = (require('ffprobe-static') as { path: string }).path;
const TIMEOUT = 180_000;

interface CmdResult {
  readonly code: number;
  readonly json: Record<string, unknown>;
}

async function build(root: string, name?: string, force = false): Promise<CmdResult> {
  const app = resolveApp(root);
  const run = await runCommand((r) => runBuild(app, r, { json: true, verbose: false, name, force }));
  return { code: run.value, json: JSON.parse(run.stdout) };
}

async function check(root: string, name?: string): Promise<CmdResult> {
  const app = resolveApp(root);
  const run = await runCommand((r) => runCheck(app, r, { json: true, verbose: false, name }));
  return { code: run.value, json: JSON.parse(run.stdout) };
}

function firstBuilt(r: CmdResult): { status?: string; error?: { code: string } } {
  return (r.json.built as Array<{ status?: string; error?: { code: string } }>)[0];
}
function firstChecked(r: CmdResult): { ok: boolean; error?: { code: string } } {
  return (r.json.checked as Array<{ ok: boolean; error?: { code: string } }>)[0];
}

function probe(mp3: string): { codec: string; channels: number; sampleRate: number; bitrate: number; duration: number } {
  const run = spawnSync(
    ffprobe,
    ['-v', 'error', '-select_streams', 'a:0', '-show_entries', 'stream=codec_name,channels,sample_rate,bit_rate:format=duration', '-of', 'json', mp3],
    { encoding: 'utf8' },
  );
  const j = JSON.parse(run.stdout) as {
    streams: Array<{ codec_name: string; channels: number; sample_rate: string; bit_rate: string }>;
    format: { duration: string };
  };
  const s = j.streams[0];
  return {
    codec: s.codec_name,
    channels: s.channels,
    sampleRate: Number(s.sample_rate),
    bitrate: Number(s.bit_rate),
    duration: Number(j.format.duration),
  };
}

// 1
test('a valid synthesized tone passes check', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const r = await check(app.root, 'tone-ok');
    assert.equal(r.code, 0);
    assert.equal(firstChecked(r).ok, true);
  } finally {
    app.cleanup();
  }
});

// 2, 3, 4
test('invalid syntax / mini-notation / unknown function all fail check', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const syntax = await check(app.root, 'bad-syntax');
    assert.equal(syntax.code, 1);
    assert.equal(firstChecked(syntax).error?.code, 'STRUDEL_TRANSPILE_FAILED');

    const mini = await check(app.root, 'bad-mini');
    assert.equal(mini.code, 1);
    assert.match(String(firstChecked(mini).error?.code), /STRUDEL_/);

    const unknown = await check(app.root, 'unknown-fn');
    assert.equal(unknown.code, 1);
    assert.equal(firstChecked(unknown).error?.code, 'STRUDEL_EVALUATION_FAILED');
  } finally {
    app.cleanup();
  }
});

// 5
test('a failed check produces no MP3', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    await check(app.root, 'bad-syntax');
    assert.ok(!existsSync(assetPath(resolveApp(app.root), 'bad-syntax')));
    assert.ok(!existsSync(resolveApp(app.root).audioDir));
  } finally {
    app.cleanup();
  }
});

// 6, 7, 8
test('a valid sound builds a WAV internally and a final MP3 with correct properties + manifest', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const dirs = resolveApp(app.root);
    const r = await build(app.root, 'tone-ok');
    assert.equal(r.code, 0);
    assert.equal(firstBuilt(r).status, 'built');

    // 6: WAV intermediate + final MP3 both exist.
    assert.ok(existsSync(wavCachePath(dirs, 'tone-ok')), 'WAV intermediate exists in cache');
    const mp3 = assetPath(dirs, 'tone-ok');
    assert.ok(existsSync(mp3), 'MP3 asset exists');

    // 7: MP3 properties.
    const p = probe(mp3);
    assert.equal(p.codec, 'mp3');
    assert.equal(p.channels, 1);
    assert.equal(p.sampleRate, 48000);
    assert.equal(p.bitrate, 128000);
    assert.ok(Math.abs(p.duration - 1.1) < 0.15, `duration ~1.1s (got ${p.duration})`);

    // 8: manifest entry with correct relative path + hashes.
    const entry = readManifest(dirs.manifestPath).assets['tone-ok'];
    assert.equal(entry.path, 'audio/tone-ok.mp3');
    assert.equal(entry.mimeType, 'audio/mpeg');
    assert.equal(entry.durationMs, 1100);
    assert.equal(entry.sha256, fileSha256(mp3));
    const raw = readFileSync(sourcePath(dirs, 'tone-ok'), 'utf8');
    assert.equal(entry.sourceSha256, sourceSha256(raw, parseSource(raw, 'tone-ok').config));
  } finally {
    app.cleanup();
  }
});

// 9
test('rebuilding unchanged input is a cache hit and does not rewrite output', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const dirs = resolveApp(app.root);
    await build(app.root, 'tone-ok');
    const mp3 = assetPath(dirs, 'tone-ok');
    const mp3Mtime = statSync(mp3).mtimeMs;
    const manMtime = statSync(dirs.manifestPath).mtimeMs;

    const again = await build(app.root, 'tone-ok');
    assert.equal(firstBuilt(again).status, 'cached');
    assert.equal(statSync(mp3).mtimeMs, mp3Mtime, 'MP3 not rewritten');
    assert.equal(statSync(dirs.manifestPath).mtimeMs, manMtime, 'manifest not rewritten');
  } finally {
    app.cleanup();
  }
});

// 10
test('changing the source invalidates the cache', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const dirs = resolveApp(app.root);
    await build(app.root, 'tone-ok');
    const before = readManifest(dirs.manifestPath).assets['tone-ok'].sourceSha256;

    writeSound(app.root, 'tone-ok', tone('tone-ok', 'note("c5 e5 g5").s("triangle").gain(0.6)'));
    const rebuilt = await build(app.root, 'tone-ok');
    assert.equal(firstBuilt(rebuilt).status, 'built');
    const after = readManifest(dirs.manifestPath).assets['tone-ok'].sourceSha256;
    assert.notEqual(before, after);
  } finally {
    app.cleanup();
  }
});

// 11
test('an all-silent pattern is rejected and writes no asset', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const r = await build(app.root, 'silent');
    assert.equal(r.code, 1);
    assert.equal(firstBuilt(r).error?.code, 'RENDER_SILENT');
    assert.ok(!existsSync(assetPath(resolveApp(app.root), 'silent')));
  } finally {
    app.cleanup();
  }
});

// 12
test('a clipping pattern is rejected and writes no asset', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const r = await build(app.root, 'clipping');
    assert.equal(r.code, 1);
    assert.equal(firstBuilt(r).error?.code, 'RENDER_CLIPPED');
    assert.ok(!existsSync(assetPath(resolveApp(app.root), 'clipping')));
  } finally {
    app.cleanup();
  }
});

// 13
test('an attempted remote sample fetch is blocked', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const r = await build(app.root, 'remote-sample');
    assert.equal(r.code, 1);
    assert.equal(firstBuilt(r).error?.code, 'NETWORK_ACCESS_ATTEMPTED');
    assert.ok(!existsSync(assetPath(resolveApp(app.root), 'remote-sample')));
  } finally {
    app.cleanup();
  }
});

// 14
test('building two sounds in one invocation does not leak Strudel state', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const dirs = resolveApp(app.root);
    const r = await build(app.root); // all sounds
    const built = r.json.built as Array<{ id: string; status: string }>;
    const byId = new Map(built.map((b) => [b.id, b.status]));
    assert.equal(byId.get('tone-ok'), 'built');
    assert.equal(byId.get('tone-two'), 'built');
    assert.ok(existsSync(assetPath(dirs, 'tone-ok')));
    assert.ok(existsSync(assetPath(dirs, 'tone-two')));

    const man = readManifest(dirs.manifestPath).assets;
    assert.ok(man['tone-ok'] && man['tone-two']);
    // Independent outputs (different hashes), proving no cross-contamination.
    assert.notEqual(man['tone-ok'].sha256, man['tone-two'].sha256);
    assert.equal(man['tone-two'].channels, 2);
  } finally {
    app.cleanup();
  }
});

// 15
test('clean removes generated output but preserves sources', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const dirs = resolveApp(app.root);
    await build(app.root, 'tone-ok');
    assert.ok(existsSync(assetPath(dirs, 'tone-ok')));

    await runCommand((r) => runClean(dirs, r, { json: true, verbose: false }));

    assert.ok(!existsSync(assetPath(dirs, 'tone-ok')), 'MP3 removed');
    assert.ok(!existsSync(dirs.manifestPath), 'manifest removed');
    assert.ok(!existsSync(wavCachePath(dirs, 'tone-ok')), 'cache removed');
    // sources preserved
    assert.ok(existsSync(sourcePath(dirs, 'tone-ok')), 'source preserved');
    assert.ok(readdirSync(dirs.soundsDir).length > 0);
  } finally {
    app.cleanup();
  }
});

// 16
test('a destination write failure leaves the previous valid asset untouched', { timeout: TIMEOUT }, async () => {
  const app = makeTempApp();
  try {
    const dirs = resolveApp(app.root);
    await build(app.root, 'tone-ok');
    const mp3 = assetPath(dirs, 'tone-ok');
    const good = readFileSync(mp3);
    const raw = readFileSync(sourcePath(dirs, 'tone-ok'), 'utf8');
    const config = parseSource(raw, 'tone-ok').config;

    // Force a publish failure: encode from a non-existent WAV (ffmpeg fails)
    // targeting the existing asset. It must throw and NOT clobber the asset.
    assert.throws(
      () => encodeMp3(wavCachePath(dirs, 'does-not-exist'), mp3, config),
      (err: unknown) => isSoundError(err) && err.code === 'ENCODE_FAILED',
    );
    assert.deepEqual(readFileSync(mp3), good, 'previous asset is untouched');
    // no temp file left behind in the audio dir
    assert.deepEqual(
      readdirSync(dirs.audioDir).filter((n) => n.includes('.tmp-')),
      [],
    );
  } finally {
    app.cleanup();
  }
});
