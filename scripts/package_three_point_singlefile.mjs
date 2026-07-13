#!/usr/bin/env node
/*
 * Package the FULLY SELF-CONTAINED Three-Point Shootout app into ONE
 * `index.html`. The game ships its own pure-TypeScript engine (WebGL2 renderer,
 * fixed-step loop, input, WebAudio) under `web/src/engine/` — there is no SDK,
 * no wasm, and nothing to fetch: esbuild bundles the compiled app
 * (`web/dist/*.js`) into a single ES module and it is inlined into the page
 * chrome from `web/index.html`. Open the produced file directly (file://) and
 * it runs.
 *
 * Repo tooling — NOT engine spine. Zero engine dependencies; drives esbuild
 * (fetched on demand) purely as a bundler.
 *
 * The harness's two dev-server couplings are neutralized for the static build:
 * the versioned hot-reload import becomes a static specifier, and the `/events`
 * SSE channel becomes a stub.
 *
 * Run:  node scripts/package_three_point_singlefile.mjs [outfile]
 * Out:  dist/three-point.html  (default), or the path given.
 */

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const APP_DIR = join(REPO_ROOT, "apps", "axiom-three-point");
const APP_DIST = join(APP_DIR, "web", "dist");
const INDEX_HTML = join(APP_DIR, "web", "index.html");

const outArg = process.argv[2];
const OUT_FILE = outArg ? join(process.cwd(), outArg) : join(REPO_ROOT, "dist", "three-point.html");

// Bundle readability. The default MINIMIZES size (collapses whitespace + safe
// syntax minification) but KEEPS every identifier name intact — no mangling.
// `AXIOM_READABLE=1` turns off all minification for fully-formatted output.
const READABLE = process.env.AXIOM_READABLE === "1";

// ---------------------------------------------------------------------------
// 1. Static-build transform of the compiled harness: rewrite exactly the two
//    dev-server couplings and leave every other line untouched.
// ---------------------------------------------------------------------------
const rawHarness = readFileSync(join(APP_DIST, "harness.js"), "utf8");

const transforms = [
  {
    what: "make the hot-reload import a static specifier esbuild can bundle",
    find: "(await import(__rewriteRelativeImportExtension(`/dist/game.js?v=${version}`)))",
    repl: '(await import("/dist/game.js"))',
  },
  {
    what: "neutralize the dev-server SSE hot-reload channel",
    find: 'new EventSource("/events")',
    repl: "({ addEventListener() { } })",
  },
];

let harness = rawHarness;
for (const t of transforms) {
  if (!harness.includes(t.find)) {
    console.error(`FATAL: harness transform anchor not found — ${t.what}\n  looked for: ${t.find}`);
    process.exit(1);
  }
  harness = harness.replace(t.find, t.repl);
}

// ---------------------------------------------------------------------------
// 2. esbuild: bundle the transformed harness + the app dist into one ESM.
// ---------------------------------------------------------------------------
const work = mkdtempSync(join(tmpdir(), "axiom-threepoint-"));
try {
  writeFileSync(join(work, "package.json"), JSON.stringify({ name: "axiom-singlefile-build", private: true, type: "module" }));
  writeFileSync(join(work, "harness.entry.js"), harness);

  const driver = `
import { build } from "esbuild";
import { join } from "node:path";

const APP_DIST = ${JSON.stringify(APP_DIST)};
const ENTRY = ${JSON.stringify(join(work, "harness.entry.js"))};
const ENGINE_ENTRY = ${JSON.stringify(join(REPO_ROOT, "packages", "axiom-web-engine", "src", "index.ts"))};

const resolver = {
  name: "axiom-virtual-roots",
  setup(b) {
    // The shared pure-TS engine package: esbuild bundles its TypeScript source
    // directly into the single file (no wasm, no separate build step).
    b.onResolve({ filter: /^@axiom\\/web-engine$/ }, () => ({ path: ENGINE_ENTRY }));
    b.onResolve({ filter: /^\\/dist\\// }, (a) => ({
      path: join(APP_DIST, a.path.replace("/dist/", "")),
    }));
    // The relocated harness ENTRY keeps its own relative imports (e.g.
    // "./constants.js"). Those can't resolve from the temp dir, so ONLY the
    // ENTRY's relatives are mapped into the app dist. Everything else — app-dist
    // internals AND the engine package's own internal relatives ("./store.ts") —
    // resolves normally where it already lives.
    b.onResolve({ filter: /^\\.\\.?\\// }, (a) => {
      if (a.importer === ENTRY) {
        return { path: join(APP_DIST, a.path.replace(/^\\.\\//, "")) };
      }
      return undefined;
    });
  },
};

const READABLE = ${JSON.stringify(READABLE)};
const result = await build({
  entryPoints: [ENTRY],
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  legalComments: "none",
  minifyWhitespace: !READABLE,
  minifySyntax: !READABLE,
  minifyIdentifiers: false,
  write: false,
  plugins: [resolver],
});
process.stdout.write(result.outputFiles[0].text);
`;
  writeFileSync(join(work, "build.mjs"), driver);

  console.log("installing esbuild (one-off, into a temp dir)…");
  const inst = spawnSync(
    process.platform === "win32" ? "npm.cmd" : "npm",
    ["install", "--no-save", "--no-fund", "--no-audit", "esbuild@0.24.0"],
    { cwd: work, encoding: "utf8", shell: process.platform === "win32" },
  );
  if (inst.status !== 0) {
    console.error("FATAL: npm install esbuild failed\n", inst.stderr || inst.stdout);
    process.exit(1);
  }
  const bundled = spawnSync(process.execPath, [join(work, "build.mjs")], {
    cwd: work,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  if (bundled.status !== 0) {
    console.error("FATAL: esbuild bundle failed\n", bundled.stderr);
    process.exit(1);
  }
  const bundleJs = bundled.stdout;

  // -------------------------------------------------------------------------
  // 3. Fold the bundle into the page.
  // -------------------------------------------------------------------------
  let html = readFileSync(INDEX_HTML, "utf8");
  html = html.replace(
    /<script type="module" src="\/dist\/harness\.js"><\/script>/,
    `<script type="module">\n${bundleJs}\n</script>`,
  );

  mkdirSync(dirname(OUT_FILE), { recursive: true });
  writeFileSync(OUT_FILE, html);
  const sizeKb = (Buffer.byteLength(html) / 1e3).toFixed(0);
  console.log(`\nwrote ${OUT_FILE}  (${sizeKb} KB, self-contained — no SDK, no wasm)`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
