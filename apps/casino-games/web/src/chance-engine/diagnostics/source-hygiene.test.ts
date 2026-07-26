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
 * There are two, because there are two front ends over the one chance engine:
 * `application/shell.ts` drives the engine-rendered canvas app, and
 * `css3d/main.ts` is the entry point of the canvas-free CSS 3D build. Both draw
 * one seed at startup and record it; neither is reachable from the other. This
 * list is the invariant, not an exemption — adding an entry means declaring a
 * new app entry point, and every other file in the tree is still forbidden.
 */
const SHELL_BOUNDARIES: readonly string[] = [join("application", "shell.ts"), join("css3d", "main.ts")];

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
