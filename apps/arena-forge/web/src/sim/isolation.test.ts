import { strict as assert } from "node:assert";
import { test } from "node:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/*
 * A structural guard: the simulation (`src/sim/`) must remain DOM-free and
 * renderer-free so it runs unchanged in a browser client, a Node server, a test
 * runner, or a replay tool. Visual quality settings therefore cannot alter
 * simulation behavior — the simulation has no rendering input at all.
 */

const simDir = fileURLToPath(new URL(".", import.meta.url));

const walk = (dir: string): string[] => {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...walk(full));
    } else if (full.endsWith(".ts") && !full.endsWith(".test.ts")) {
      out.push(full);
    }
  }
  return out;
};

const FORBIDDEN = [
  /@axiom\/web-engine/,
  /\bdocument\./,
  /\bwindow\./,
  /requestAnimationFrame/,
  /HTMLCanvas/,
  /CanvasRenderingContext/,
  /getElementById/,
];

test("the simulation imports no DOM, renderer, or engine-host module", () => {
  const files = walk(simDir);
  assert.ok(files.length > 10, "expected to scan the whole sim tree");
  for (const file of files) {
    const src = readFileSync(file, "utf8");
    for (const pattern of FORBIDDEN) {
      assert.ok(!pattern.test(src), `${file} references forbidden token ${pattern}`);
    }
  }
});
