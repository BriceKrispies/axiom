/*
 * source-hygiene.test.ts — repository-level fairness hygiene: no gameplay
 * file may call `Math.random()` (all randomness flows through the named
 * deterministic streams), and only the shell boundary may read boundary
 * entropy (`crypto.getRandomValues`).
 */

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const SRC_ROOT = join(import.meta.dirname, "..", "..");

const tsFilesUnder = (dir: string): readonly string[] =>
  readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      return tsFilesUnder(path);
    }
    return entry.name.endsWith(".ts") && !entry.name.endsWith(".test.ts") ? [path] : [];
  });

/** Strip // line comments and /* block comments so documentation that NAMES
 * the banned call (e.g. "never calls Math.random()") cannot trip the scan. */
const withoutComments = (source: string): string =>
  source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");

test("no gameplay file calls Math.random()", () => {
  const offenders = tsFilesUnder(SRC_ROOT).filter((file) => /Math\.random\s*\(/.test(withoutComments(readFileSync(file, "utf8"))));
  assert.deepEqual(offenders, []);
});

test("boundary entropy is read only at the shell boundary", () => {
  const offenders = tsFilesUnder(SRC_ROOT).filter(
    (file) => readFileSync(file, "utf8").includes("crypto.getRandomValues") && !file.endsWith(join("application", "shell.ts")),
  );
  assert.deepEqual(offenders, []);
});

test("chance-engine and games never reach into the DOM", () => {
  const scopes = ["chance-engine", "games", "presentation"].map((d) => join(SRC_ROOT, d));
  const offenders = scopes
    .flatMap((dir) => tsFilesUnder(dir))
    .filter((file) => !file.endsWith("casino-mount.ts"))
    .filter((file) => /document\.|localStorage|getElementById/.test(readFileSync(file, "utf8")));
  assert.deepEqual(offenders, []);
});
