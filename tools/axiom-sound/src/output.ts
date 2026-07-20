// Output routing. Two modes:
//   human (default): human-readable lines on stdout; diagnostics on stderr.
//   --json:          ONLY JSON on stdout (the machine result / error envelope);
//                    ALL diagnostics on stderr.
// --verbose adds stacks / underlying causes to error output in both modes.

import { isSoundError, type SoundError } from './errors.ts';

export interface OutputOptions {
  readonly json: boolean;
  readonly verbose: boolean;
}

/** Where a Reporter writes. Injectable so tests can collect output without
 *  touching the global process streams (which the test runner also uses). */
export interface Sinks {
  out(text: string): void;
  err(text: string): void;
}

const PROCESS_SINKS: Sinks = {
  out: (text) => void process.stdout.write(text),
  err: (text) => void process.stderr.write(text),
};

export class Reporter {
  readonly #opts: OutputOptions;
  readonly #sinks: Sinks;

  constructor(opts: OutputOptions, sinks: Sinks = PROCESS_SINKS) {
    this.#opts = opts;
    this.#sinks = sinks;
  }

  /** Progress / diagnostics. Always stderr, so JSON stdout stays clean. */
  info(message: string): void {
    this.#sinks.err(`${message}\n`);
  }

  /** Human-facing content. stdout in human mode; suppressed in JSON mode. */
  human(message: string): void {
    if (!this.#opts.json) {
      this.#sinks.out(`${message}\n`);
    }
  }

  /** The machine result. stdout in JSON mode; suppressed in human mode. */
  result(payload: unknown): void {
    if (this.#opts.json) {
      this.#sinks.out(`${JSON.stringify(payload, null, 2)}\n`);
    }
  }

  /** Report a failure. Returns the process exit code to use. */
  error(err: unknown): number {
    const envelope = toEnvelope(err, this.#opts.verbose);
    if (this.#opts.json) {
      this.#sinks.out(`${JSON.stringify({ ok: false, error: envelope }, null, 2)}\n`);
    } else {
      this.#sinks.err(`axiom-sound: error [${envelope.code}] ${envelope.message}\n`);
      if (envelope.location) {
        this.#sinks.err(`  at ${envelope.location}\n`);
      }
      if (this.#opts.verbose && envelope.stack) {
        this.#sinks.err(`${envelope.stack}\n`);
      }
    }
    return 1;
  }

  /** Create a Reporter that collects into arrays, with accessors for the text. */
  static collecting(opts: OutputOptions): {
    reporter: Reporter;
    stdout(): string;
    stderr(): string;
  } {
    const out: string[] = [];
    const err: string[] = [];
    const reporter = new Reporter(opts, {
      out: (t) => void out.push(t),
      err: (t) => void err.push(t),
    });
    return { reporter, stdout: () => out.join(''), stderr: () => err.join('') };
  }
}

interface ErrorEnvelope {
  readonly code: string;
  readonly message: string;
  readonly id?: string;
  readonly location?: string;
  readonly line?: number;
  readonly column?: number;
  readonly file?: string;
  readonly extra?: Record<string, unknown>;
  readonly stack?: string;
}

function toEnvelope(err: unknown, verbose: boolean): ErrorEnvelope {
  if (isSoundError(err)) {
    const e: SoundError = err;
    const loc = locationString(e.details.file, e.details.line, e.details.column);
    return {
      code: e.code,
      message: e.message,
      id: e.details.id,
      location: loc,
      line: e.details.line,
      column: e.details.column,
      file: e.details.file,
      extra: e.details.extra,
      stack: verbose ? causeStack(e) : undefined,
    };
  }
  const message = err instanceof Error ? err.message : String(err);
  return { code: 'INTERNAL', message, stack: verbose && err instanceof Error ? err.stack : undefined };
}

function locationString(file?: string, line?: number, column?: number): string | undefined {
  if (!file && line === undefined) {
    return undefined;
  }
  const parts = [file ?? '<source>'];
  if (line !== undefined) {
    parts.push(String(line));
  }
  if (column !== undefined) {
    parts.push(String(column));
  }
  return parts.join(':');
}

function causeStack(err: SoundError): string | undefined {
  const own = err.stack ?? '';
  const cause = err.details.cause;
  const causeText = cause instanceof Error ? `\nCaused by: ${cause.stack ?? cause.message}` : '';
  return `${own}${causeText}`;
}
