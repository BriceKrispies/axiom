/*
 * Vite dev config for the retro-fps @axiom/game hot-reload harness (mirror of the
 * game-runtime app's config; hot-reload architecture §7.1). Repo tooling — outside the
 * engine graph. `@axiom/game` resolves to the workspace SDK source (so SDK edits
 * hot-reload too), `/pkg/*` (the shared wasm) is served from `web/pkg`, and the Axiom
 * plugin forces a full reload on a `*.wasm` rebuild (possible ABI change).
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
      allow: [here("../../../")],
    },
    port: 8080,
    strictPort: true,
  },
});
