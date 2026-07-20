// Parse the `.strudel` source format: `+++` TOML front matter, then raw Strudel
// body. The body after the front matter is ordinary Strudel code that pastes
// into the Strudel REPL unchanged (once the `+++...+++` block is removed).

import { parse as parseToml, TomlError } from 'smol-toml';
import { SoundError } from './errors.ts';
import { KNOWN_FIELDS, REQUIRED_FIELDS, toRenderConfig, type RenderConfig } from './config.ts';

const DELIMITER = '+++';

export interface ParsedSource {
  readonly config: RenderConfig;
  /** The Strudel code after the front matter (verbatim, trailing newline kept). */
  readonly body: string;
  /** 1-based line on which the body starts, for source-mapped diagnostics. */
  readonly bodyStartLine: number;
}

/**
 * Split a raw `.strudel` file into its TOML front matter and Strudel body,
 * validate the front matter, and return the RenderConfig + body. Throws
 * SoundError(INVALID_FRONT_MATTER) on any structural or field error.
 *
 * `idHint` (the filename stem) is only used to attach an id to errors raised
 * before the `id` field is read; it is not authoritative.
 */
export function parseSource(raw: string, idHint = ''): ParsedSource {
  const normalized = raw.replace(/^﻿/, '');
  const lines = normalized.split(/\r?\n/);

  if (lines[0]?.trim() !== DELIMITER) {
    throw new SoundError(
      'INVALID_FRONT_MATTER',
      `source must begin with a \`${DELIMITER}\` front-matter delimiter on line 1`,
      { id: idHint, line: 1 },
    );
  }

  const closingIndex = lines.findIndex((line, i) => i > 0 && line.trim() === DELIMITER);
  if (closingIndex === -1) {
    throw new SoundError(
      'INVALID_FRONT_MATTER',
      `unterminated front matter: missing closing \`${DELIMITER}\` delimiter`,
      { id: idHint },
    );
  }

  const tomlText = lines.slice(1, closingIndex).join('\n');
  const body = lines.slice(closingIndex + 1).join('\n');
  const bodyStartLine = closingIndex + 2;

  let fields: Record<string, unknown>;
  try {
    fields = parseToml(tomlText) as Record<string, unknown>;
  } catch (err) {
    // smol-toml reports 1-based line/column within the TOML slice. The TOML
    // starts on source line 2 (line 1 is the opening `+++`), so add 1 to the
    // line; the column is already 1-based and maps directly.
    const loc =
      err instanceof TomlError && typeof err.line === 'number'
        ? { line: err.line + 1, column: err.column }
        : {};
    throw new SoundError('INVALID_FRONT_MATTER', `front-matter TOML is invalid: ${messageOf(err)}`, {
      id: idHint,
      ...loc,
    });
  }

  const unknown = Object.keys(fields).filter(
    (key) => !KNOWN_FIELDS.includes(key as (typeof KNOWN_FIELDS)[number]),
  );
  if (unknown.length > 0) {
    throw new SoundError(
      'INVALID_FRONT_MATTER',
      `unknown front-matter field(s): ${unknown.join(', ')} (allowed: ${KNOWN_FIELDS.join(', ')})`,
      { id: idHint },
    );
  }

  const missing = REQUIRED_FIELDS.filter((key) => !(key in fields));
  if (missing.length > 0) {
    throw new SoundError(
      'INVALID_FRONT_MATTER',
      `missing required front-matter field(s): ${missing.join(', ')}`,
      { id: idHint },
    );
  }

  if (body.trim() === '') {
    throw new SoundError('INVALID_FRONT_MATTER', 'source body (Strudel code) is empty', {
      id: idHint,
    });
  }

  const config = toRenderConfig(fields);
  return { config, body, bodyStartLine };
}

function messageOf(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
