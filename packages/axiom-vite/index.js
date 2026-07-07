/*
 * The Axiom Vite plugin (repo tooling — outside the engine graph; not a layer,
 * module, or app). It adds the two HMR behaviours Vite cannot infer on its own for
 * an `@axiom/game` app (hot-reload architecture §7.3):
 *
 *   1. WASM restart — a rebuilt `*_bg.wasm` may change the exported ABI, so an
 *      in-place `hot_patch` is unsafe. When a watched wasm file changes, the plugin
 *      forces a full page reload (the harness then reconstructs the `WasmGame`),
 *      rather than letting Vite try to hot-swap around a new binary.
 *
 *   2. Sound/asset passthrough is left to Vite's static serving; the plugin only
 *      owns the wasm-restart signal today. Manifest validation + the custom
 *      `axiom:engine-restart` event are a documented extension point for when the
 *      classifier needs to escalate from the server side.
 *
 * It is intentionally tiny and dependency-free (a plain Vite plugin object).
 */

/**
 * @param {{ wasmDir?: string }} [options] - `wasmDir` names the folder whose `*.wasm`
 *   changes trigger a full reload (defaults to any path containing `/pkg/`).
 * @returns {import("vite").Plugin}
 */
export const axiomHotReload = (options = {}) => {
  const marker = options.wasmDir ?? "/pkg/";
  return {
    name: "axiom-hot-reload",
    handleHotUpdate(ctx) {
      const isWasm = ctx.file.endsWith(".wasm") || ctx.file.includes(marker);
      if (isWasm) {
        // A new wasm binary may have a new ABI — the running engine cannot be patched
        // around it, so reload the whole page (the harness rebuilds the WasmGame).
        ctx.server.ws.send({ type: "full-reload", path: "*" });
        return [];
      }
      return ctx.modules;
    },
  };
};

export default axiomHotReload;
