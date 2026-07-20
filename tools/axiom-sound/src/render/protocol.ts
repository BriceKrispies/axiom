// The narrow message contract between the Node harness and the in-browser
// Strudel entry. Everything crossing `page.evaluate` must be JSON-serializable,
// so PCM is carried as base64 of little-endian Float32 bytes.

/** The pipeline phase a failure occurred in (drives the Node error code). */
export type Phase = 'transpile' | 'evaluate' | 'pattern' | 'query';

export interface Diagnostic {
  readonly phase: Phase;
  readonly message: string;
  /** 1-based line within the Strudel body, when the error carries a location. */
  readonly line?: number;
  /** 1-based column, when available. */
  readonly column?: number;
}

export interface CheckRequest {
  readonly code: string;
  readonly seconds: number;
  readonly cps: number;
}

export interface RenderRequest extends CheckRequest {
  readonly channels: number;
  readonly sampleRate: number;
}

export interface CheckResult {
  readonly ok: boolean;
  readonly hapCount?: number;
  readonly diagnostic?: Diagnostic;
}

export interface RenderResult {
  readonly ok: boolean;
  readonly diagnostic?: Diagnostic;
  /** One base64 Float32-LE blob per channel. */
  readonly channelsB64?: readonly string[];
  readonly sampleRate?: number;
  readonly frames?: number;
}
