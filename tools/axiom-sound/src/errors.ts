// Stable, machine-readable error codes plus the SoundError carrier.
//
// Every failure in the tool is a SoundError with one of these codes. `cli.ts`
// converts a thrown SoundError into a nonzero exit + a concise message (human)
// or a JSON envelope (`--json`). Codes are part of the tool's public contract
// (documented in README.md); do not rename them casually.

export type ErrorCode =
  | 'APP_NOT_FOUND'
  | 'APP_MANIFEST_NOT_FOUND'
  | 'INVALID_SOUND_ID'
  | 'INVALID_FRONT_MATTER'
  | 'DUPLICATE_SOUND_ID'
  | 'SOURCE_NOT_FOUND'
  | 'SOURCE_EXISTS'
  | 'STRUDEL_TRANSPILE_FAILED'
  | 'STRUDEL_EVALUATION_FAILED'
  | 'STRUDEL_PATTERN_INVALID'
  | 'NETWORK_ACCESS_ATTEMPTED'
  | 'RENDER_TIMEOUT'
  | 'RENDER_SILENT'
  | 'RENDER_CLIPPED'
  | 'RENDER_INVALID_PCM'
  | 'ENCODE_FAILED'
  | 'ENCODE_VALIDATION_FAILED'
  | 'MANIFEST_WRITE_FAILED'
  | 'USAGE';

/** Optional source location (1-based line/column) for source-mapped diagnostics. */
export interface SourceLocation {
  readonly file?: string;
  readonly line?: number;
  readonly column?: number;
}

export interface SoundErrorDetails extends SourceLocation {
  /** The sound id this error concerns, when applicable. */
  readonly id?: string;
  /** Extra, non-secret structured context surfaced in `--json` and `--verbose`. */
  readonly extra?: Record<string, unknown>;
  /** An underlying cause, shown only under `--verbose`. */
  readonly cause?: unknown;
}

export class SoundError extends Error {
  readonly code: ErrorCode;
  readonly details: SoundErrorDetails;

  constructor(code: ErrorCode, message: string, details: SoundErrorDetails = {}) {
    super(message);
    this.name = 'SoundError';
    this.code = code;
    this.details = details;
  }
}

export function isSoundError(value: unknown): value is SoundError {
  return value instanceof SoundError;
}
