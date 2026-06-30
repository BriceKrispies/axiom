#!/usr/bin/env node
/*
 * Axiom @axiom/game hot-reload dev server (spike).
 *
 * Repo tooling — NOT engine spine. Zero dependencies (Node built-ins only).
 *
 * It does three things, which together are the whole dev-UX loop:
 *   1. Serves the harness page + the @axiom/game SDK build as native ES modules
 *      (the SDK at /vendor/axiom-game/ out of packages/axiom-game/dist).
 *   2. Watches the author's TypeScript (apps/axiom-game-runtime/web/src) and
 *      recompiles it with tsgo on every save — no bundler, no WASM rebuild.
 *   3. Pushes a `reload` event over Server-Sent Events after each successful
 *      compile; the harness re-imports the author module in place, leaving the
 *      live WASM engine untouched.
 *
 * Run:  node scripts/axiom_dev_server.mjs   (then open http://localhost:8080)
 * Env:  AXIOM_DEV_PORT to change the port.
 */

import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { watch } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize, sep } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
// Which app's web/ dir to serve + hot-reload. Defaults to the 2D hot-reload
// harness; set AXIOM_DEV_APP=axiom-doom-ts-browser for the TS-only 3D DOOM app.
// Both are authored purely in TypeScript over @axiom/game and drive the SAME
// axiom-game-runtime wasm, so /pkg is served from the one canonical build below.
const APP = process.env.AXIOM_DEV_APP ?? "axiom-game-runtime";
const WEB_ROOT = join(REPO_ROOT, "apps", APP, "web");
const SRC_DIR = join(WEB_ROOT, "src");
const TSCONFIG = join(WEB_ROOT, "tsconfig.json");
const VENDOR_DIR = join(REPO_ROOT, "packages", "axiom-game", "dist");
// The shared wasm engine: built once into the game-runtime app's web/pkg and
// served at /pkg for whichever app is active (the runtime is game-agnostic).
const PKG_DIR = join(REPO_ROOT, "apps", "axiom-game-runtime", "web", "pkg");
const TSGO = join(
  REPO_ROOT,
  "packages",
  "axiom-game",
  "node_modules",
  ".bin",
  process.platform === "win32" ? "tsgo.cmd" : "tsgo",
);
const PORT = Number(process.env.AXIOM_DEV_PORT ?? 8080);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".wasm": "application/wasm",
  ".map": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml",
};

/** Open SSE connections to notify on reload. */
const clients = new Set();
/** Bumped after each successful compile; the cache-bust version the client imports. */
let version = Date.now();

const broadcast = () => {
  for (const res of clients) {
    res.write(`event: reload\ndata: ${version}\n\n`);
  }
};

/** Run tsgo once over the web tsconfig. Resolves to the exit code. */
const compile = () =>
  new Promise((resolve) => {
    const started = Date.now();
    const child = spawn(TSGO, ["-p", TSCONFIG], { cwd: WEB_ROOT, shell: true });
    let stderr = "";
    child.stdout.on("data", (d) => process.stdout.write(d));
    child.stderr.on("data", (d) => {
      stderr += d;
      process.stderr.write(d);
    });
    child.on("close", (code) => {
      const ms = Date.now() - started;
      if (code === 0) {
        console.log(`  tsgo ok in ${ms}ms`);
      } else {
        console.error(`  tsgo FAILED (exit ${code})${stderr ? "" : " — see output above"}`);
      }
      resolve(code ?? 1);
    });
    child.on("error", (err) => {
      console.error(`  tsgo could not start: ${err.message}`);
      resolve(1);
    });
  });

/** Serve a file from `root`, guarding against path traversal. */
const serveFile = async (root, relPath, res) => {
  const safe = normalize(relPath).replace(/^(\.\.[/\\])+/, "");
  const full = join(root, safe);
  if (!full.startsWith(root.endsWith(sep) ? root : root + sep) && full !== root) {
    res.writeHead(403).end("forbidden");
    return;
  }
  try {
    const info = await stat(full);
    const target = info.isDirectory() ? join(full, "index.html") : full;
    const body = await readFile(target);
    res.writeHead(200, {
      "Content-Type": MIME[extname(target)] ?? "application/octet-stream",
      "Cache-Control": "no-store",
    });
    res.end(body);
  } catch {
    res.writeHead(404, { "Content-Type": "text/plain" }).end(`404 ${relPath}`);
  }
};

const server = createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
  const path = decodeURIComponent(url.pathname);

  // The reload stream.
  if (path === "/events") {
    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-store",
      Connection: "keep-alive",
    });
    res.write(`retry: 1000\n: connected\n\n`);
    clients.add(res);
    req.on("close", () => clients.delete(res));
    return;
  }

  // The SDK build, served as native ES modules.
  if (path.startsWith("/vendor/axiom-game/")) {
    await serveFile(VENDOR_DIR, path.slice("/vendor/axiom-game/".length), res);
    return;
  }

  // The shared wasm engine build (game-agnostic), served from its canonical pkg/.
  if (path.startsWith("/pkg/")) {
    await serveFile(PKG_DIR, path.slice("/pkg/".length), res);
    return;
  }

  // Everything else: the active app's web/ dir (index.html, /dist/*.js, /assets/*).
  await serveFile(WEB_ROOT, path === "/" ? "index.html" : path, res);
});

const debounce = (fn, ms) => {
  let timer;
  return () => {
    clearTimeout(timer);
    timer = setTimeout(fn, ms);
  };
};

const rebuild = debounce(async () => {
  console.log("change detected → recompiling…");
  const code = await compile();
  if (code === 0) {
    version = Date.now();
    broadcast();
    console.log(`  reloaded ${clients.size} client(s)\n`);
  } else {
    console.log("  not reloaded (compile failed)\n");
  }
}, 120);

const main = async () => {
  console.log("Axiom @axiom/game dev server\n  initial compile…");
  await compile();
  watch(SRC_DIR, { recursive: true }, (_event, file) => {
    if (file && file.endsWith(".ts")) {
      rebuild();
    }
  });
  server.listen(PORT, () => {
    console.log(`\n  serving  http://localhost:${PORT}  (app: ${APP})`);
    console.log(`  watching ${SRC_DIR}`);
    console.log(`  edit apps/${APP}/web/src/game.ts and save\n`);
  });
};

void main();
