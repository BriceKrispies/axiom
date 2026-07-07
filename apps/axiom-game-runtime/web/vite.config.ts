/*
 * Vite dev config for the @axiom/game hot-reload harness — one call to the shared
 * `axiomVitePreset` (hot-reload architecture §7.1). The preset derives `root`, the
 * `@axiom/game` → SDK-source alias, `server.fs.allow`, and the wasm-restart plugin from
 * this file's own URL, so a new hot-reloadable app's config is exactly these three lines.
 */

import { defineConfig } from "vite";
import { axiomVitePreset } from "../../../packages/axiom-vite/preset.js";

export default defineConfig(axiomVitePreset(import.meta.url));
