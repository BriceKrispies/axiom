#!/usr/bin/env node
/*
 * Axiom TypeScript coverage gate runner.
 *
 * Runs node:test with built-in coverage at 100% lines/branches/functions over
 * the co-located unit tests (`src/**\/*.test.ts`). The set of files exempt from
 * coverage is read from `<pkg>/test-exempt.json` — the SAME single list the
 * co-location gate reads (scripts/ts-colocation-check.mjs) — so the two gates can
 * never disagree about what is exempt. Test-tier files (`*.test.ts`,
 * `*.testkit.ts`) are excluded as "not the code under test".
 *
 * Usage: node scripts/ts-coverage.mjs <packageDir>
 */

import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const pkgDir = process.argv[2] ?? ".";
const exemptPath = join(pkgDir, "test-exempt.json");
const exempt = existsSync(exemptPath)
  ? JSON.parse(readFileSync(exemptPath, "utf8"))
  : [];

// Globs use both the top-level and nested forms so a file directly under src/
// (e.g. src/byte-reader.test.ts) is matched as reliably as a nested one.
const excludeGlobs = [
  "src/**/*.test.ts",
  "src/*.test.ts",
  "src/**/*.testkit.ts",
  "src/*.testkit.ts",
  ...exempt.map((rel) => `src/${rel}`),
];

const args = [
  "--test",
  "--experimental-test-coverage",
  "--test-coverage-lines=100",
  "--test-coverage-branches=100",
  "--test-coverage-functions=100",
  ...excludeGlobs.map((g) => `--test-coverage-exclude=${g}`),
  "src/**/*.test.ts",
];

const res = spawnSync(process.execPath, args, { cwd: pkgDir, stdio: "inherit" });
process.exit(res.status ?? 1);
