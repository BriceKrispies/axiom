/*
 * Vite dev config for the retro-fps @axiom/game hot-reload harness — one call to the
 * shared `axiomVitePreset`. The preset derives root, the @axiom/game SDK-source alias,
 * server.fs.allow, and the wasm-restart plugin from this file's own URL.
 */

import { defineConfig } from "vite";
import { axiomVitePreset } from "../../../packages/axiom-vite/preset.js";

export default defineConfig(axiomVitePreset(import.meta.url));
