// Shared option shapes passed from the CLI to command handlers.

export interface CommonOptions {
  readonly json: boolean;
  readonly verbose: boolean;
}

export interface NameOption {
  /** A single sound id, or undefined to act on every sound in the app. */
  readonly name?: string;
}

export interface BuildOptions extends CommonOptions, NameOption {
  /** Bypass the source-hash cache and rebuild unconditionally. */
  readonly force: boolean;
}
