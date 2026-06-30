#!/usr/bin/env node
/*
 * Axiom TypeScript co-location gate.
 *
 * Enforces: every source file `src/<path>/<name>.ts` has a sibling unit test
 * `src/<path>/<name>.test.ts`. This is the structural guarantee behind the 100%
 * coverage law for the TS SDKs (packages/axiom-client, packages/axiom-game):
 * node's `--test` coverage only reports files that some test actually IMPORTS,
 * so an untested file is silently INVISIBLE — not failing. Requiring a
 * co-located test per source file forces every file into the test graph (hence
 * the coverage report), so the 100% gate actually bites.
 *
 * Test-tier files (`*.test.ts`, `*.testkit.ts`) need no sibling of their own.
 * Platform-edge files listed in `<pkg>/test-exempt.json` (browser-only code
 * verified via the Playwright path, never under node) are exempt from BOTH this
 * check and the coverage gate — the SAME single list drives both (see
 * scripts/ts-coverage.mjs), so a file can never be quietly dropped from coverage
 * without also being declared exempt here, in the open.
 *
 * Usage: node scripts/ts-colocation-check.mjs <packageDir>
 */

import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, relative, dirname, basename } from "node:path";

const pkgDir = process.argv[2];
if (!pkgDir) {
  console.error("usage: node scripts/ts-colocation-check.mjs <packageDir>");
  process.exit(2);
}

const srcDir = join(pkgDir, "src");
const exemptPath = join(pkgDir, "test-exempt.json");
const exempt = new Set(
  existsSync(exemptPath) ? JSON.parse(readFileSync(exemptPath, "utf8")) : [],
);

function walk(dir) {
  const out = [];
  for (const ent of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, ent.name);
    if (ent.isDirectory()) {
      out.push(...walk(p));
    } else if (ent.name.endsWith(".ts")) {
      out.push(p);
    }
  }
  return out;
}

const isTestTier = (f) => f.endsWith(".test.ts") || f.endsWith(".testkit.ts");
const relOf = (f) => relative(srcDir, f).split("\\").join("/");
const siblingTest = (f) => join(dirname(f), `${basename(f, ".ts")}.test.ts`);

const all = walk(srcDir);

// 1. Every non-test, non-exempt source file needs a co-located <name>.test.ts.
const missing = all
  .filter((f) => !isTestTier(f))
  .filter((f) => !exempt.has(relOf(f)))
  .filter((f) => !existsSync(siblingTest(f)))
  .map(relOf);

// 2. Every exempt entry must name a real source file (no rotting exemptions).
const staleExempt = [...exempt].filter((rel) => !existsSync(join(srcDir, rel)));

// 3. An exempt file must NOT also carry a test: if it is testable under node it
//    does not belong on the platform-edge exemption list. Keeps the list honest.
const exemptButTested = [...exempt].filter((rel) =>
  existsSync(siblingTest(join(srcDir, rel))),
);

const problems = [];
if (missing.length) {
  problems.push(
    `source files missing a co-located <name>.test.ts:\n  ${missing.join("\n  ")}`,
  );
}
if (staleExempt.length) {
  problems.push(
    `test-exempt.json names files that do not exist:\n  ${staleExempt.join("\n  ")}`,
  );
}
if (exemptButTested.length) {
  problems.push(
    `exempt files that DO have a test (remove them from test-exempt.json):\n  ${exemptButTested.join("\n  ")}`,
  );
}

if (problems.length) {
  console.error(`co-location gate FAILED for ${pkgDir}:\n\n${problems.join("\n\n")}`);
  process.exit(1);
}

const counted = all.filter((f) => !isTestTier(f)).length - exempt.size;
console.log(
  `co-location gate OK for ${pkgDir}: ${counted} source files, each with a sibling test (${exempt.size} platform-edge files exempt).`,
);
