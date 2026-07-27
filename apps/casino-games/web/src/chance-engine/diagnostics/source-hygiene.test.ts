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

/**
 * The app's SHELL BOUNDARIES — the outermost entry points, and the only places
 * allowed to draw boundary entropy. Everything below a shell must be a pure
 * function of the seed the shell drew and recorded.
 *
 * There are three, because there are three front ends over the one chance
 * engine: `application/shell.ts` drives the engine-rendered canvas app,
 * `css3d/main.ts` is the entry point of the canvas-free CSS 3D build, and
 * `resilient/main.ts` is the entry point of the form-first build. None is
 * reachable from another. This list is the invariant, not an exemption — adding
 * an entry means declaring a new app entry point, and every other file in the
 * tree is still forbidden.
 *
 * `resilient/main.ts` is declared even though it draws NO entropy: its server
 * (`tools/axiom-chest-server`) owns the seed, because the server decides the
 * outcome for a zero-JavaScript form POST. Declaring it is what makes that fact
 * checkable — the test below fails the day someone gives the resilient shell a
 * seed of its own, and a reader can see at a glance that the app has exactly
 * three entry points.
 */
const SHELL_BOUNDARIES: readonly string[] = [
  join("application", "shell.ts"),
  join("css3d", "main.ts"),
  join("resilient", "main.ts"),
];

test("boundary entropy is read only at a shell boundary", () => {
  const offenders = tsFilesUnder(SRC_ROOT).filter(
    (file) =>
      readFileSync(file, "utf8").includes("crypto.getRandomValues") &&
      !SHELL_BOUNDARIES.some((shell) => file.endsWith(shell)),
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
