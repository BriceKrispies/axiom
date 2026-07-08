#!/usr/bin/env node
/*
 * Package the TypeScript-only Signal Runner app into ONE self-contained `index.html`
 * with everything embedded — no server, no /pkg, no /vendor, no /dist. Open the
 * produced file directly (file://) and it runs. It is the sibling of
 * `package_soccer_penalty_singlefile.mjs`; the only differences are the app dir and
 * that Signal Runner is a 2D `bootHotApp` app whose harness needs exactly ONE
 * static-build transform (feed the embedded wasm bytes to `initWasm`) instead of
 * soccer's three dev-server couplings.
 *
 * Repo tooling — NOT engine spine. Drives esbuild (fetched on demand) as a bundler.
 *
 * What gets embedded, and how:
 *   1. The whole ES-module graph — the compiled app (`web/dist/*.js`), the
 *      `@axiom/game` SDK (`packages/axiom-game/dist/*`), and the wasm-bindgen glue
 *      (`.../pkg/axiom_game_runtime.js`) — is bundled by esbuild into ONE ES module.
 *      The server-only specifier roots (`@axiom/game`, `/pkg/...`, `/dist/...`) are
 *      mapped to real files by a resolver plugin.
 *   2. The `axiom_game_runtime_bg.wasm` is gzip-compressed and base64'd into the
 *      bundle as a virtual module; at boot the page inflates it with the browser's
 *      `DecompressionStream('gzip')` and hands the bytes to the wasm-bindgen
 *      `init({ module_or_path })` seam — so NOTHING is ever fetched.
 *   3. The harness's one dev-server coupling (`loadWasm = () => initWasm()`) is
 *      rewritten to feed the embedded bytes.
 *   4. The bundled module + the page chrome from `web/index.html` (minus its import
 *      map + external <script>) are written into one HTML file.
 *
 * Run:  node scripts/package_signal_runner_singlefile.mjs [outfile]
 * Out:  dist/signal-runner.html  (default), or the path given.
 */

import { spawnSync } from "node:child_process";
import { gzipSync } from "node:zlib";
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const APP_DIR = join(REPO_ROOT, "apps", "axiom-signal-runner");
const APP_DIST = join(APP_DIR, "web", "dist");
const SDK_DIST = join(REPO_ROOT, "packages", "axiom-game", "dist");
const PKG_DIR = join(REPO_ROOT, "apps", "axiom-game-runtime", "web", "pkg");
const INDEX_HTML = join(APP_DIR, "web", "index.html");
const WASM_FILE = join(PKG_DIR, "axiom_game_runtime_bg.wasm");
const GLUE_FILE = join(PKG_DIR, "axiom_game_runtime.js");

const outArg = process.argv[2];
const OUT_FILE = outArg ? join(process.cwd(), outArg) : join(REPO_ROOT, "dist", "signal-runner.html");

// Keep every identifier name intact (no mangling) so the one inlined <script> stays
// legible; `AXIOM_READABLE=1` turns off all minification.
const READABLE = process.env.AXIOM_READABLE === "1";

// ---------------------------------------------------------------------------
// 1. Static-build transform of the compiled harness: feed the embedded wasm
//    bytes to init instead of fetching them. The harness deliberately exposes a
//    single-line anchor for exactly this.
// ---------------------------------------------------------------------------
const rawHarness = readFileSync(join(APP_DIST, "harness.js"), "utf8");

const ANCHOR = "const loadWasm = () => initWasm();";
const REPLACEMENT =
  "const loadWasm = async () => { const __b = await __axiomWasmBytes(); const __t = performance.now(); const __r = await initWasm({ module_or_path: __b }); console.log(`axiom: wasm instantiate ${(performance.now() - __t).toFixed(0)}ms`); return __r; };";

if (!rawHarness.includes(ANCHOR)) {
  console.error(`FATAL: harness static-build anchor not found\n  looked for: ${ANCHOR}`);
  process.exit(1);
}
let harness = rawHarness.replace(ANCHOR, REPLACEMENT);
harness = `import { bytes as __axiomWasmBytes } from "virtual:axiom-wasm";\n${harness}`;

// ---------------------------------------------------------------------------
// 2. Gzip + base64 the wasm; build the virtual loader module.
// ---------------------------------------------------------------------------
const wasmRaw = readFileSync(WASM_FILE);
const wasmGz = gzipSync(wasmRaw, { level: 9 });
const wasmB64 = wasmGz.toString("base64");
console.log(
  `wasm: ${(wasmRaw.length / 1e6).toFixed(2)} MB raw → ${(wasmGz.length / 1e6).toFixed(2)} MB gzip → ${(wasmB64.length / 1e6).toFixed(2)} MB base64`,
);

const wasmLoaderSource = `
const B64 = ${JSON.stringify(wasmB64)};
export async function bytes() {
  const gz = await (await fetch("data:application/octet-stream;base64," + B64)).arrayBuffer();
  const stream = new Blob([gz]).stream().pipeThrough(new DecompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}
`;

// ---------------------------------------------------------------------------
// 3. esbuild: bundle the transformed harness + all virtual roots into one ESM.
// ---------------------------------------------------------------------------
const work = mkdtempSync(join(tmpdir(), "axiom-signalrunner-"));
try {
  writeFileSync(join(work, "package.json"), JSON.stringify({ name: "axiom-singlefile-build", private: true, type: "module" }));
  writeFileSync(join(work, "harness.entry.js"), harness);
  writeFileSync(join(work, "wasm-loader.js"), wasmLoaderSource);

  const driver = `
import { build } from "esbuild";

const APP_DIST = ${JSON.stringify(APP_DIST)};
const SDK_DIST = ${JSON.stringify(SDK_DIST)};
const GLUE_FILE = ${JSON.stringify(GLUE_FILE)};
const WASM_LOADER = ${JSON.stringify(join(work, "wasm-loader.js"))};
const ENTRY = ${JSON.stringify(join(work, "harness.entry.js"))};
import { join } from "node:path";

const resolver = {
  name: "axiom-virtual-roots",
  setup(b) {
    b.onResolve({ filter: /^virtual:axiom-wasm$/ }, () => ({ path: WASM_LOADER }));
    // The relocated entry imports its manifest as "./app.js"; anchor it at the app dist.
    b.onResolve({ filter: /^\\.\\/app\\.js$/ }, () => ({ path: join(APP_DIST, "app.js") }));
    b.onResolve({ filter: /^@axiom\\/game$/ }, () => ({ path: join(SDK_DIST, "index.js") }));
    b.onResolve({ filter: /^\\/pkg\\// }, () => ({ path: GLUE_FILE }));
    b.onResolve({ filter: /^\\/dist\\// }, (a) => ({ path: join(APP_DIST, a.path.replace("/dist/", "")) }));
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
  // 4. Fold the bundle into the page.
  // -------------------------------------------------------------------------
  let html = readFileSync(INDEX_HTML, "utf8");
  html = html
    .replace(/<script type="importmap">[\s\S]*?<\/script>\s*/, "")
    .replace(/<!--\s*The @axiom\/game[\s\S]*?-->\s*/, "")
    .replace(
      /<script type="module" src="\/dist\/harness\.js"><\/script>/,
      `<script type="module">\n${bundleJs}\n</script>`,
    );

  mkdirSync(dirname(OUT_FILE), { recursive: true });
  writeFileSync(OUT_FILE, html);
  const sizeMb = (Buffer.byteLength(html) / 1e6).toFixed(2);
  console.log(`\nwrote ${OUT_FILE}  (${sizeMb} MB, self-contained)`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
