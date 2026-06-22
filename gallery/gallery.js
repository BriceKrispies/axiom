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
  {
    id: "stress-cubes",
    title: "Stress (N cubes)",
    blurb: "A field of N spinning cubes — a live load test you can watch.",
    desc:
      "The browser-visible counterpart to the engine's CPU pipeline benchmark: " +
      "a grid of N independently-spinning cube renderables on the same " +
      "scene → render → WebGPU path. Pick a cube count below and watch the FPS " +
      "fall as N climbs — the frame-rate collapse is the deterministic pipeline " +
      "cost made visible. Purely visual — no input.",
    dir: "stress-cubes",
    jsModule: "axiom_stress_cubes_browser",
    canvasId: "axiom-stress-canvas",
    buttons: [],
    // Declares a cube-count control bar + FPS readout, and that `start` takes a
    // cube count (read from `?cubes=`, default 2000).
    cubeStress: true,
  },
  {
    id: "growth",
    title: "Growth (walkable terrain)",
    blurb: "Generate a planet, pick a spot on its map, and walk the procedural terrain in first person.",
    desc:
      "A procedural-terrain world viewer on the engine: configure and generate a " +
      "planet, descend onto a land spot from the overworld map, then walk its " +
      "streamed LOD terrain. Desktop: click the canvas to capture the mouse, WASD/" +
      "arrows to move, mouse to look, Esc to release. (WebGPU; desktop-oriented.)",
    // Growth is self-hosted: its multi-screen flow (config form → overworld map →
    // descend → first-person view) doesn't fit the shared single-canvas shell, so
    // its card links to its own page (copied into dist/growth/ by the assembler).
    page: "growth/index.html",
  },
  {
    id: "roomed-puzzle",
    title: "Roomed Puzzle",
    blurb: "Leave ghosts of your past runs on the buttons, then walk the live block through the doors they open.",
    desc:
      "A deterministic top-down grid puzzle on the engine. Walk a block one cell " +
      "at a time (WASD / arrows); press Q to freeze the current run into a ghost " +
      "that replays your exact path on a fixed 0.5s step, and R to restart. " +
      "Ghosts are solid and hold buttons open — so the way through a locked door " +
      "is to leave a ghost on the button and walk the live block through. " +
      "Includes an in-browser level editor + playtest with TOML import/export.",
    // Self-hosted: the editor/playtest flow (canvas + TOML textarea + validation
    // panel) owns its own page, copied into dist/roomed-puzzle/ by the assembler.
    page: "roomed-puzzle/index.html",
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

// The cube-count presets offered by the stress demo's control bar.
const CUBE_PRESETS = [100, 500, 1000, 2000, 5000, 10000, 25000];

// Read the requested cube count from `?cubes=`, defaulting to 2000 and never
// below 1 (matching the Rust `start` clamp).
function readCubeCount(params) {
  return Math.max(1, parseInt(params.get("cubes") ?? "2000", 10) || 2000);
}

// Mount the stress demo's control bar: cube-count presets that reload the page
// with `?cubes=N` applied, plus a live FPS / frame-time readout. The FPS counter
// is an independent requestAnimationFrame loop on the main thread, so it
// measures the true delivered frame rate — when a frame's CPU+GPU work overruns
// the vsync budget, this drops with it.
function mountCubeBar(host, demo, current) {
  const bar = document.createElement("div");
  bar.className = "cubebar";
  const label = document.createElement("span");
  label.className = "cubebar-label";
  label.textContent = "cubes:";
  bar.appendChild(label);
  for (const n of CUBE_PRESETS) {
    const a = document.createElement("a");
    const q = new URLSearchParams({ id: demo.id, cubes: String(n) });
    a.href = "?" + q.toString();
    a.textContent = n.toLocaleString();
    if (n === current) a.className = "active";
    bar.appendChild(a);
  }
  const fps = document.createElement("span");
  fps.className = "fps";
  fps.textContent = "fps: —";
  bar.appendChild(fps);
  host.appendChild(bar);

  let last = performance.now();
  let acc = 0;
  let frames = 0;
  const loop = (now) => {
    acc += now - last;
    last = now;
    frames += 1;
    if (acc >= 500) {
      const value = (frames * 1000) / acc;
      fps.textContent = `fps: ${value.toFixed(1)}  (${(acc / frames).toFixed(1)} ms)`;
      acc = 0;
      frames = 0;
    }
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
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
  // Self-hosted demos own their page; the shared shell just forwards to it.
  if (demo.page) {
    location.replace(`./${demo.page}`);
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

  const cubeCount = demo.cubeStress ? readCubeCount(params) : null;
  if (demo.cubeStress) {
    mountCubeBar(document.getElementById("controls"), demo, cubeCount);
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
    if (cubeCount != null) {
      mod.start(cubeCount);
    } else {
      mod.start();
    }

    if (demo.cubeStress) {
      setStatus(
        status,
        `Engine started — rendering ${cubeCount.toLocaleString()} spinning ` +
          "cubes. Pick a cube count above and watch the FPS.",
        "ok",
      );
    } else if (demo.needsRelay && !relay) {
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
