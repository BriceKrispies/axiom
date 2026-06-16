// Axiom demo gallery — repo tooling (NOT part of the engine dependency graph;
// same status as the Makefile and scripts/). This module owns the demo manifest
// and the shared per-demo boot shell. It is plain ES modules served statically;
// it imports nothing from the engine.
//
// Each demo's wasm bundle is produced by `wasm-bindgen --target web` exactly as
// `make demo-build` / `make netplay-build` do, and dropped at
// `./<dir>/pkg/<jsModule>.js` + `_bg.wasm` by the deploy workflow.

import { renderKeypad } from "./keypad.js";

// The gallery manifest. `canvasId` MUST match each app's Rust surface id
// (`with_surface_id(...)`), and `buttons` declares the on-screen keypad — empty
// means the demo takes no input, so no keypad is shown.
export const DEMOS = [
  {
    id: "rotating-cube",
    title: "Rotating Cube",
    blurb: "Three deterministic shaded cubes spinning on different axes.",
    desc:
      "The engine's browser-visible vertical slice: pure scene description on " +
      "App::new()…run(), rendered through WebGPU. Purely visual — no input.",
    dir: "rotating-cube",
    jsModule: "axiom_demo_rotating_cube_browser",
    canvasId: "axiom-cube-canvas",
    buttons: [],
  },
  {
    id: "netplay",
    title: "Lockstep Multiplayer",
    blurb: "Move your cube with the D-pad; only signed inputs cross the wire.",
    desc:
      "Deterministic-lockstep netplay. Open this in two browsers pointed at " +
      "the same relay — each controls its own cube and the two stay identical " +
      "by determinism. Red = player 1, blue = player 2.",
    dir: "netplay",
    jsModule: "axiom_netplay_browser",
    canvasId: "axiom-netplay-canvas",
    buttons: [
      { key: "ArrowUp", label: "▲", pos: "up" },
      { key: "ArrowLeft", label: "◀", pos: "left" },
      { key: "ArrowRight", label: "▶", pos: "right" },
      { key: "ArrowDown", label: "▼", pos: "down" },
    ],
    needsRelay: true,
  },
  {
    id: "doom",
    title: "DOOM (first-person)",
    blurb: "Stalk a cube-walled level and shoot the cubes — built on just the engine.",
    desc:
      "A DOOM-style first-person shooter on nothing but the engine: the level " +
      "is scaled cube instances, the camera is the engine's first-person " +
      "controller, and enemies are chasing cube players. Desktop: click to look " +
      "(mouse), WASD to move, click to fire. Touch: ◀ ▶ turn, ▲ ▼ move, FIRE.",
    dir: "doom",
    jsModule: "axiom_doom_browser",
    canvasId: "axiom-doom-canvas",
    buttons: [
      { key: "ArrowUp", label: "▲", pos: "up" },
      { key: "ArrowLeft", label: "◀", pos: "left" },
      { key: "ArrowRight", label: "▶", pos: "right" },
      { key: "ArrowDown", label: "▼", pos: "down" },
      { key: " ", label: "FIRE", pos: "fire" },
    ],
  },
];

/** Look a demo up by its `id`, or `null` when unknown. */
export function demoById(id) {
  return DEMOS.find((d) => d.id === id) || null;
}

const setStatus = (el, msg, cls) => {
  el.textContent = msg;
  el.className = "status" + (cls ? " " + cls : "");
};

// Mount the netplay relay bar: a tiny form to point the demo at a hosted relay
// (the static deploy has none of its own). Reloads with `?relay=<url>` applied.
function mountRelayBar(host, demo, currentRelay) {
  const bar = document.createElement("form");
  bar.className = "relaybar";
  const input = document.createElement("input");
  input.type = "text";
  input.placeholder = "wss://your-relay.example:443";
  input.value = currentRelay || "";
  input.setAttribute("aria-label", "Relay URL");
  const apply = document.createElement("button");
  apply.type = "submit";
  apply.textContent = "Connect";
  bar.append(input, apply);
  bar.addEventListener("submit", (e) => {
    e.preventDefault();
    const url = input.value.trim();
    const q = new URLSearchParams({ id: demo.id });
    if (url) q.set("relay", url);
    location.search = "?" + q.toString();
  });
  host.appendChild(bar);
}

/**
 * Boot the demo named by `?id=` into the page. Mirrors the per-app index.html
 * boot logic (cache-bust, dynamic import, init({module_or_path}), start()), but
 * data-driven so one shell serves every demo and renders the right keypad.
 */
export async function bootDemo() {
  const params = new URLSearchParams(location.search);
  const stage = document.getElementById("stage");
  const keypad = document.getElementById("keypad");
  const status = document.getElementById("status");
  const titleEl = document.getElementById("demo-title");

  const demo = demoById(params.get("id"));
  if (!demo) {
    setStatus(status, "Unknown demo. Return to the gallery to pick one.", "err");
    return;
  }
  titleEl.textContent = demo.title;
  document.title = "Axiom — " + demo.title;

  // The canvas the engine binds its surface to; id must match the Rust app.
  const canvas = document.createElement("canvas");
  canvas.id = demo.canvasId;
  canvas.width = 800;
  canvas.height = 600;
  stage.appendChild(canvas);

  if (demo.buttons.length > 0) {
    renderKeypad(keypad, demo.buttons);
  }

  const relay = params.get("relay");
  if (demo.needsRelay) {
    mountRelayBar(document.getElementById("controls"), demo, relay);
  }

  if (!("gpu" in navigator)) {
    setStatus(
      status,
      "WebGPU is not available in this browser. Use a recent Chrome/Edge, " +
        "Android Chrome, or iOS Safari 18.2+.",
      "err",
    );
    return;
  }

  try {
    // Cache-bust the JS glue and the wasm binary on every load (the dev/static
    // server may send no cache headers), matching the per-app pages.
    const v = Date.now();
    const mod = await import(`./${demo.dir}/pkg/${demo.jsModule}.js?v=${v}`);
    const wasmUrl = new URL(
      `./${demo.dir}/pkg/${demo.jsModule}_bg.wasm?v=${v}`,
      import.meta.url,
    );
    await mod.default({ module_or_path: wasmUrl });
    mod.start();

    if (demo.needsRelay && !relay) {
      setStatus(
        status,
        "Engine started. No relay set — enter a wss:// relay above (or run " +
          "one locally with `make relay`) and open this page in a second " +
          "browser to play. Use the D-pad to move your cube.",
        "warn",
      );
    } else if (demo.buttons.length > 0) {
      setStatus(status, "Engine started. Use the D-pad to move your cube.", "ok");
    } else {
      setStatus(status, "Engine started.", "ok");
    }
  } catch (e) {
    setStatus(status, "Startup failed: " + e, "err");
    throw e;
  }
}
