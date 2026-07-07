/*
 * `axiomVitePreset` — the one-call Vite config for a hot-reloadable @axiom/game app, so
 * an app's `vite.config.ts` is three lines instead of the whole resolve/fs/plugin block:
 *
 *   import { defineConfig } from "vite";
 *   import { axiomVitePreset } from "../../../packages/axiom-vite/preset.js";
 *   export default defineConfig(axiomVitePreset(import.meta.url));
 *
 * It derives everything from the config file's own URL:
 *   - `root` is the app's `web/` dir (the config's directory);
 *   - `@axiom/game` resolves to the workspace SDK SOURCE (so SDK edits hot-reload too),
 *     found by walking up to the repo root (the dir holding `packages/axiom-game`);
 *   - `server.fs.allow` is widened to that repo root so Vite may serve the SDK source
 *     that lives outside the app's `web/` root;
 *   - the Axiom plugin forces a full reload on a `*.wasm` rebuild (possible ABI change).
 *
 * Repo tooling — outside the engine graph, no gate.
 */

import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { axiomHotReload } from "./index.js";

/** Walk up from `start` to the repo root — the first ancestor holding `packages/axiom-game/src/index.ts`. */
const findRepoRoot = (start) => {
  let dir = start;
  while (!existsSync(join(dir, "packages", "axiom-game", "src", "index.ts"))) {
    const parent = dirname(dir);
    if (parent === dir) {
      throw new Error(`axiomVitePreset: axiom repo root not found above ${start}`);
    }
    dir = parent;
  }
  return dir;
};

/**
 * Build the Vite config for a hot-reloadable @axiom/game app from its `vite.config.ts`
 * `import.meta.url`. `options.port` overrides the dev port (default 8080).
 *
 * @param {string} configUrl - the app `vite.config.ts`'s `import.meta.url`.
 * @param {{ port?: number }} [options]
 * @returns {import("vite").UserConfig}
 */
export const axiomVitePreset = (configUrl, options = {}) => {
  const webDir = dirname(fileURLToPath(configUrl));
  const repoRoot = findRepoRoot(webDir);
  return {
    plugins: [axiomHotReload()],
    resolve: {
      alias: {
        "@axiom/game": join(repoRoot, "packages", "axiom-game", "src", "index.ts"),
      },
    },
    root: webDir,
    server: {
      fs: { allow: [repoRoot] },
      port: options.port ?? 8080,
      strictPort: true,
    },
  };
};

export default axiomVitePreset;
