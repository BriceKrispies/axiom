/*
 * Vite dev config for the @axiom/game hot-reload harness (hot-reload architecture §7.1).
 * Vite owns file watching, TS/ESM transform, the module graph, cache busting, and the
 * HMR transport; the harness accepts `./game.ts` edits via `import.meta.hot.accept` and
 * reconciles them into the LIVE engine. Repo tooling — outside the engine graph.
 *
 *   - `root` is this `web/` dir; the entry is `index.html` → `/src/harness.ts`.
 *   - `@axiom/game` resolves to the workspace SDK SOURCE, so SDK edits hot-reload too
 *     (no build step) — this replaces the old `index.html` import map.
 *   - `/pkg/*` (the wasm-bindgen build) is served from `web/pkg`; `server.fs.allow`
 *     is widened to the repo root so Vite may serve the SDK src that lives outside `web/`.
 *   - the Axiom plugin forces a full reload on a `*.wasm` rebuild (possible ABI change).
 */

import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import { axiomHotReload } from "../../../packages/axiom-vite/index.js";

const here = (relative: string): string => fileURLToPath(new URL(relative, import.meta.url));

export default defineConfig({
  plugins: [axiomHotReload()],
  resolve: {
    alias: {
      "@axiom/game": here("../../../packages/axiom-game/src/index.ts"),
    },
  },
  root: here("."),
  server: {
    fs: {
      // Allow serving the SDK source, which lives outside this web/ root.
      allow: [here("../../../")],
    },
    port: 8080,
    strictPort: true,
  },
});
