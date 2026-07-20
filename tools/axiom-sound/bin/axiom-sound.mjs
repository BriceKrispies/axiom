#!/usr/bin/env node
// Thin CLI entry point for the `axiom-sound` tool.
//
// The real implementation lives in TypeScript under `src/`. Node >= 24 runs
// `.ts` directly via native type-stripping (process.features.typescript), the
// same mechanism the repo's TS packages rely on for `node --test`. This shim
// exists so `bin`/npm scripts have a stable `.mjs` entry that never needs a
// build step (there is no `dist/`).
import { main } from '../src/cli.ts';

main(process.argv.slice(2)).then(
  (code) => {
    process.exitCode = code;
  },
  (err) => {
    // A truly unexpected throw (not one of our structured SoundErrors, which
    // main() catches and converts to an exit code). Print concisely to stderr.
    process.stderr.write(`axiom-sound: fatal: ${err?.stack ?? err}\n`);
    process.exitCode = 70; // EX_SOFTWARE
  },
);
