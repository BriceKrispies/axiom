import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseSource } from '../src/frontmatter.ts';
import { isSoundError } from '../src/errors.ts';

const OK = `+++
id = "ui-perfect"
duration_ms = 900
tail_ms = 200
channels = 1
bitrate_kbps = 128
+++

note("c5").s("triangle")
`;

function expectError(fn: () => unknown, code: string): void {
  try {
    fn();
    assert.fail('expected a SoundError');
  } catch (err) {
    assert.ok(isSoundError(err), `expected SoundError, got ${String(err)}`);
    assert.equal(err.code, code);
  }
}

test('parses valid front matter + body', () => {
  const p = parseSource(OK, 'ui-perfect');
  assert.deepEqual(p.config, {
    id: 'ui-perfect',
    durationMs: 900,
    tailMs: 200,
    channels: 1,
    bitrateKbps: 128,
    mode: 'offline',
  });
  assert.match(p.body, /note\("c5"\)/);
  assert.equal(p.bodyStartLine, 8);
});

test('body pastes into REPL: front matter is fully stripped', () => {
  const p = parseSource(OK, 'ui-perfect');
  assert.doesNotMatch(p.body, /\+\+\+/);
  assert.doesNotMatch(p.body, /duration_ms/);
});

test('rejects a missing opening delimiter', () => {
  expectError(() => parseSource('id = "x"\n', 'x'), 'INVALID_FRONT_MATTER');
});

test('rejects an unterminated front matter', () => {
  expectError(() => parseSource('+++\nid = "x"\n', 'x'), 'INVALID_FRONT_MATTER');
});

test('rejects an unknown field (typo cannot silently pass)', () => {
  const src = OK.replace('bitrate_kbps = 128', 'bitrate_kbps = 128\nbitrate = 128');
  expectError(() => parseSource(src, 'ui-perfect'), 'INVALID_FRONT_MATTER');
});

test('rejects a missing required field', () => {
  const src = OK.replace('channels = 1\n', '');
  expectError(() => parseSource(src, 'ui-perfect'), 'INVALID_FRONT_MATTER');
});

test('rejects an empty body', () => {
  const src = `+++\nid = "x"\nduration_ms = 100\ntail_ms = 0\nchannels = 1\nbitrate_kbps = 128\n+++\n\n`;
  expectError(() => parseSource(src, 'x'), 'INVALID_FRONT_MATTER');
});

test('rejects invalid TOML with a location', () => {
  const src = `+++\nid = =\n+++\n\nnote("c5")\n`;
  try {
    parseSource(src, 'x');
    assert.fail('expected error');
  } catch (err) {
    assert.ok(isSoundError(err));
    assert.equal(err.code, 'INVALID_FRONT_MATTER');
    assert.ok((err.details.line ?? 0) >= 2, 'line should map past the +++ delimiter');
  }
});
