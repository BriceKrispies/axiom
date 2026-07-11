#!/usr/bin/env node
/*
 * Package the TypeScript-only Home Run! app into ONE self-contained `index.html`
 * with everything embedded — no server, no /pkg, no /vendor, no /dist. Open the
 * produced file directly (file://) and it runs.
 *
 * Repo tooling — NOT engine spine. Zero engine dependencies; drives esbuild
 * (fetched on demand) purely as a bundler. A sibling of
 * `package_heat_check_singlefile.mjs` (same 3D `boot`+`present3d` path, same
 * three harness transforms), differing only in the app directory + default output.
 *
 * What gets embedded, and how:
 *   1. The whole ES-module graph — the compiled app (`web/dist/*.js`), the
 *      `@axiom/game` SDK (`packages/axiom-game/dist/*`), and the wasm-bindgen
 *      glue (`.../pkg/axiom_game_runtime.js`) — is bundled by esbuild into ONE
 *      ES module. The server-only specifier roots (`/vendor/axiom-game/*`,
 *      `@axiom/game`, `/pkg/...`, and the harness's hot-reload `/dist/game.js`)
 *      are mapped to real files by a resolver plugin.
 *   2. The `axiom_game_runtime_bg.wasm` is gzip-compressed and base64'd into the
 *      bundle as a virtual module; at boot the page inflates it with the browser's
 *      `DecompressionStream('gzip')` and hands the bytes straight to the
 *      wasm-bindgen `init({ module_or_path })` seam — so NOTHING is ever fetched.
 *   3. The harness's dev-server couplings are neutralized for the static build.
 *   4. The bundled module + the page chrome from `web/index.html` (minus its
 *      import map and external <script>) are written into one HTML file.
 *
 * Run:  node scripts/package_home_run_singlefile.mjs [outfile]
 * Out:  dist/home-run.html  (default), or the path given.
 */

import { spawnSync } from "node:child_process";
import { gzipSync } from "node:zlib";
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const APP_DIR = join(REPO_ROOT, "apps", "axiom-home-run");
const APP_DIST = join(APP_DIR, "web", "dist");
const SDK_DIST = join(REPO_ROOT, "packages", "axiom-game", "dist");
const PKG_DIR = join(REPO_ROOT, "apps", "axiom-game-runtime", "web", "pkg");
const INDEX_HTML = join(APP_DIR, "web", "index.html");
const WASM_FILE = join(PKG_DIR, "axiom_game_runtime_bg.wasm");
const GLUE_FILE = join(PKG_DIR, "axiom_game_runtime.js");

const outArg = process.argv[2];
const OUT_FILE = outArg ? join(process.cwd(), outArg) : join(REPO_ROOT, "dist", "home-run.html");

// Bundle readability. The default MINIMIZES size (collapses whitespace + safe
// syntax minification) but KEEPS every identifier name intact — no mangling.
// `AXIOM_READABLE=1` turns off all minification for fully-formatted output.
const READABLE = process.env.AXIOM_READABLE === "1";

// ---------------------------------------------------------------------------
// 1. Static-build transform of the compiled harness.
//    The harness (`web/dist/harness.js`) is the dev host edge; it assumes the
//    dev server (versioned hot-reload import + SSE). For a static single file we
//    rewrite exactly three couplings and leave every other line untouched.
// ---------------------------------------------------------------------------
const rawHarness = readFileSync(join(APP_DIST, "harness.js"), "utf8");

const transforms = [
  {
    what: "feed the embedded wasm bytes to init instead of fetching",
    find: "await initWasm();",
    repl:
      "{ const __b = await __axiomWasmBytes(); const __t = performance.now(); await initWasm({ module_or_path: __b }); console.log(`axiom: wasm instantiate ${(performance.now() - __t).toFixed(0)}ms`); }",
  },
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
// Prepend the wasm-bytes loader import (a virtual module the plugin serves).
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
  const t0 = performance.now();
  const gz = await (await fetch("data:application/octet-stream;base64," + B64)).arrayBuffer();
  const t1 = performance.now();
  const stream = new Blob([gz]).stream().pipeThrough(new DecompressionStream("gzip"));
  const wasm = new Uint8Array(await new Response(stream).arrayBuffer());
  const t2 = performance.now();
  console.log(\`axiom: wasm decode \${(t1 - t0).toFixed(0)}ms, gunzip \${(t2 - t1).toFixed(0)}ms\`);
  return wasm;
}
`;

// ---------------------------------------------------------------------------
// 3. esbuild: bundle the transformed harness + all virtual roots into one ESM.
// ---------------------------------------------------------------------------
const work = mkdtempSync(join(tmpdir(), "axiom-homerun-"));
try {
  writeFileSync(join(work, "package.json"), JSON.stringify({ name: "axiom-singlefile-build", private: true, type: "module" }));
  writeFileSync(join(work, "harness.entry.js"), harness);
  writeFileSync(join(work, "wasm-loader.js"), wasmLoaderSource);

  const driver = `
import { build } from "esbuild";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const APP_DIST = ${JSON.stringify(APP_DIST)};
const SDK_DIST = ${JSON.stringify(SDK_DIST)};
const GLUE_FILE = ${JSON.stringify(GLUE_FILE)};
const WASM_LOADER = ${JSON.stringify(join(work, "wasm-loader.js"))};
const ENTRY = ${JSON.stringify(join(work, "harness.entry.js"))};

const resolver = {
  name: "axiom-virtual-roots",
  setup(b) {
    b.onResolve({ filter: /^virtual:axiom-wasm$/ }, () => ({ path: WASM_LOADER }));
    b.onResolve({ filter: /^@axiom\\/game$/ }, () => ({ path: join(SDK_DIST, "index.js") }));
    b.onResolve({ filter: /^\\/vendor\\/axiom-game\\// }, (a) => ({
      path: join(SDK_DIST, a.path.replace("/vendor/axiom-game/", "")),
    }));
    b.onResolve({ filter: /^\\/pkg\\// }, () => ({ path: GLUE_FILE }));
    b.onResolve({ filter: /^\\/dist\\// }, (a) => ({
      path: join(APP_DIST, a.path.replace("/dist/", "")),
    }));
    // The relocated harness entry keeps its own relative imports (e.g. "./constants.js").
    // Those can't resolve from the temp dir, but every app module is a flat sibling in
    // the app dist — so map the ENTRY's relatives there. App-internal and SDK-internal
    // relatives (importer already inside those dirs) resolve normally.
    b.onResolve({ filter: /^\\.\\// }, (a) => {
      if (a.importer.startsWith(APP_DIST) || a.importer.startsWith(SDK_DIST)) {
        return undefined;
      }
      return { path: join(APP_DIST, a.path.replace(/^\\.\\//, "")) };
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
