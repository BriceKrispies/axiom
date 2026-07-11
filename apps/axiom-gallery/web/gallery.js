// Axiom demo gallery — repo tooling (NOT part of the engine dependency graph;
// same status as the Makefile and scripts/). This module owns the demo manifest
// and the shared per-demo boot shell. It is plain ES modules served statically;
// it imports nothing from the engine.
//
// All demos are merged into ONE crate (apps/axiom-gallery), packaged by
// scripts/package_gallery.py (`make gallery`) into a SINGLE capability-detecting
// loader (`axiom-loader.js`, at the dist root) over a wasm fast-path plus a wasm2js
// fallback for browsers with no WebAssembly. The shell loads that one loader once
// and calls the demo's namespaced entry: `import("./axiom-loader.js")` ->
// `default()` -> `<demo>_start()`. Self-hosted demos own their page and import the
// same loader as `../axiom-loader.js`.

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
    startFn: "rotating_cube_start",
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
    startFn: "netplay_start",
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
    id: "retro-fps",
    title: "retro FPS (first-person)",
    blurb: "Stalk a cube-walled level and shoot the cubes — built on just the engine.",
    desc:
      "A retro FPS-style first-person shooter on nothing but the engine: the level " +
      "is scaled cube instances, the camera is the engine's first-person " +
      "controller, and enemies are chasing cube players. Desktop: click to look " +
      "(mouse), WASD to move, click to fire. Touch: ◀ ▶ turn, ▲ ▼ move, FIRE.",
    dir: "retro-fps",
    jsModule: "axiom_retro_fps_browser",
    startFn: "retro_fps_start",
    canvasId: "axiom-retro-fps-canvas",
    buttons: [
      { key: "ArrowUp", label: "▲", pos: "up" },
      { key: "ArrowLeft", label: "◀", pos: "left" },
      { key: "ArrowRight", label: "▶", pos: "right" },
      { key: "ArrowDown", label: "▼", pos: "down" },
      { key: " ", label: "FIRE", pos: "fire" },
    ],
  },
  {
    id: "soccer-penalty-kick",
    title: "Soccer Penalty",
    blurb: "Take penalty kicks against a diving keeper — aim, charge power, and shoot across five rounds.",
    desc:
      "A retro 32-bit penalty shootout on the engine: a fixed-camera stadium diorama " +
      "with a run-up-and-strike kicker, a diving goalie with real save volumes, " +
      "physics-arc ball flight, and goal / save / miss / post scoring over a five-round " +
      "session. Aim with ←/→ or A/D, set height with ↑/↓ or W/S, hold Space/K to charge " +
      "power and release to shoot, Enter to continue between rounds, R to reset.",
    // Self-hosted like growth/quintet/etc.: its page is a COMMITTED file under
    // web/soccer-penalty-kick/ that package_gallery copies verbatim into dist/. But
    // this game runs on its own @axiom/game SDK + axiom-game-runtime wasm (not the
    // gallery bundle), so that page is a SINGLE self-contained HTML (wasm + SDK + app
    // inlined) — regenerate it with `make gallery-soccer` after editing the app.
    page: "soccer-penalty-kick/index.html",
  },
  {
    id: "signal-runner",
    title: "Signal Runner",
    blurb: "A downhill signal courier: steer a hover-sled down a mountain ruin, grab shards, beat the storm.",
    desc:
      "A third-person downhill traversal game authored purely in TypeScript on the engine's " +
      "2D draw2d surface: a hooded courier rides a hover-sled down a procedurally generated, " +
      "winding mountain-ruin path in a flat-shaded low-poly world. Steer with A/D or ←/→, hold " +
      "SHIFT to brake into turns, collect 20 cyan signal shards, trip 3 pressure plates, and dodge " +
      "rocks, fallen columns, and drone hazards. Spend charge on BOOST (Space/1), SHIELD (2), " +
      "PULSE (3), and a helper DRONE (4). Restore the final beacon with ENTER before the purple " +
      "storm wall — a 2:30 countdown — overruns the relay. Fully deterministic from its seed.",
    // Self-hosted single-file page (its own @axiom/game SDK + axiom-game-runtime wasm,
    // 2D draw2d present path), COMMITTED under web/signal-runner/ and copied verbatim
    // into dist/ by package_gallery — regenerate with `make gallery-signal-runner`.
    page: "signal-runner/index.html",
  },
  {
    id: "swipe-basketball",
    title: "Swipe Basketball",
    blurb: "An arcade basketball machine: drag a ball, swipe up, and release to arc it into the hoop.",
    desc:
      "A first-person arcade basketball cabinet authored purely in TypeScript on the engine's " +
      "3D scene surface: a fixed camera facing a procedurally-built machine — sloped return ramp, " +
      "side rails, backboard, a real torus rim, hanging net, and a seven-segment scoreboard, with " +
      "orange seam-lined basketballs racked in the foreground. Drag a ball with mouse or touch, " +
      "swipe upward, and release: the swipe becomes a 3D throw and the ball is then fully physics-" +
      "simulated — bouncing off the rim, backboard, rails and ramp with real restitution. A clean " +
      "downward pass through the hoop scores once; misses rattle out or roll back down the ramp. " +
      "Press R to reset. Deterministic under fixed-step replay.",
    // Self-hosted single-file page (its own @axiom/game SDK + axiom-game-runtime wasm,
    // 3D present path), COMMITTED under web/swipe-basketball/ and copied verbatim into
    // dist/ by package_gallery — regenerate with `make gallery-swipe-basketball`.
    page: "swipe-basketball/index.html",
  },
  {
    id: "home-run",
    title: "Home Run!",
    blurb: "An arcade batting contest on a toy diamond: load the bat, read the pitch, clear the blue wall.",
    desc:
      "A toy-tabletop arcade baseball batting game authored purely in TypeScript on the engine's " +
      "3D scene surface — a fixed elevated camera behind home plate frames a compact striped diamond " +
      "with brown base paths, white foul lines, a pitching machine on the mound, blue stadium walls, " +
      "and nine red toy fielders wandering their own patrol circles. Ten pitches per round from a " +
      "deterministic seeded sequence — slow balls, sinkers, heaters, risers, inside and outside looks, " +
      "each telegraphed by the machine's compression. A/D shift the batter inside the box; holding " +
      "SPACE winds the spring-loaded bat (fast at first, resisting toward full load) and RELEASING " +
      "snaps it forward. Contact is resolved from the real spatial sweep of bat vs ball — position " +
      "along the barrel, timing angle, and vertical offset decide exit speed, spray, and loft — so " +
      "mistimed swings foul off, jam, top grounders, or pop up, while a square, well-positioned strike " +
      "clears the wall for HOME RUN! (500 + distance, consecutive homers multiply). Fielders converge " +
      "on reachable landing points and rob weak hits. Deterministic under fixed-step replay.",
    // Self-hosted single-file page (its own @axiom/game SDK + axiom-game-runtime wasm,
    // 3D present path), COMMITTED under web/home-run/ and copied verbatim into
    // dist/ by package_gallery — regenerate with `make gallery-home-run`.
    page: "home-run/index.html",
  },
  {
    id: "minimal-3v3",
    title: "Minimal 3v3 Basketball",
    blurb: "A deliberately minimal 3-on-3 half-court game: move, pass, rise, and release at the apex.",
    desc:
      "A minimally-legible 3D half-court basketball game authored purely in TypeScript on the " +
      "engine's 3D scene surface: a procedural court (key, arc, backboard, real torus rim + net) and " +
      "six box-and-sphere players. You control the blue ball handler — a third-person camera follows " +
      "behind, aimed at the hoop. WASD moves, Q/E pass to the left/right wing (control transfers with " +
      "the ball), and SPACE gathers into a jump: release at the apex for the best odds. Shot success " +
      "is deterministic — timing, distance, and defender contest all matter, and PERFECT never " +
      "guarantees. Three red defenders shade the handler, protect the lane, and rise for contest " +
      "jumps; a steal, interception, make, or miss freezes play, shows the result, and resets with " +
      "you in possession. Press R to reset. Deterministic under fixed-step replay.",
    // Self-hosted single-file page (its own @axiom/game SDK + axiom-game-runtime wasm,
    // 3D present path), COMMITTED under web/minimal-3v3/ and copied verbatim into
    // dist/ by package_gallery — regenerate with `make gallery-minimal-3v3`.
    page: "minimal-3v3/index.html",
  },
  {
    id: "three-point",
    title: "Three-Point Shootout",
    blurb: "A first-person three-point rack contest: ride the rise and release at the top — or swipe the ball up on touch.",
    desc:
      "A first-person 3D three-point contest in the spirit of Wii Sports rack shooting — a FULLY " +
      "SELF-CONTAINED pure-TypeScript app that ships its own engine (WebGL2 forward renderer, " +
      "fixed-step loop, pointer-lock/touch input, WebAudio synth) with no SDK and no wasm; the " +
      "whole game is one 70 KB page. Fifteen shots from three spots " +
      "around a procedurally-built arc — left wing, top of the key, right wing — five balls per " +
      "rack with a golden fifth ball. Every shot is ONE continuous motion that never waits: the " +
      "moment you release, the next ball is dealt off its rack slot into your hands while the " +
      "last shots are still in the air (several fly at once, scored in shot order). Holding " +
      "SPACE rises into the shot and releasing launches at that exact instant — the shot meter " +
      "tracks the rise and its ideal window. Early is short, the ideal window swishes, late " +
      "clangs off the glass. On touch, drag to look and swipe up from the " +
      "held ball to shoot — flick strength is your release, sideways flick steers, with the " +
      "same smoothed-gesture model as Swipe Basketball. The camera is exclusively player-driven " +
      "(the game never touches your view), so skill is your aim plus release timing. The ball " +
      "is a genuinely simulated projectile (deterministic fixed-step integrator — gravity, " +
      "backspin, restitution) that can swish, rattle off a rim whose colliders match the " +
      "visible torus exactly, bank off the glass, or airball. Baskets are confirmed by a " +
      "two-plane downward-crossing detector; streaks compound (3, 6, 9, 12…) and a miss resets " +
      "them. A broadcast-style glide carries you between racks; after ball 15 the buzzer shows " +
      "your line. Press R to run it back. Deterministic under fixed-step replay, with a " +
      "headless agent driver that plays full games in Node.",
    // Self-hosted single-file page (its own @axiom/game SDK + axiom-game-runtime wasm,
    // 3D present path), COMMITTED under web/three-point/ and copied verbatim into
    // dist/ by package_gallery — regenerate with `make gallery-three-point`.
    page: "three-point/index.html",
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
    startFn: "stress_start",
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
    id: "generia",
    title: "Generia (forest)",
    blurb: "Walk an Axiom-rendered procedural forest in first person — the fall-forest game, ported onto the engine.",
    desc:
      "A first-person walk through a procedural forest rendered with the engine's " +
      "GPU forest pipeline (terrain, trees, foliage, ground clutter, fog). Click the " +
      "canvas to capture the mouse, WASD/arrows to move, mouse to look, Esc to release. " +
      "The port foundation for the fall-forest game (streaming, props, discoveries, " +
      "and world modes land in later phases).",
    // Self-hosted: its own first-person canvas page (copied into dist/generia/).
    page: "generia/index.html",
  },
  {
    id: "zanzoban",
    title: "Zanzoban",
    blurb: "Leave ghosts of your past runs on the buttons, then walk the live block through the doors they open.",
    desc:
      "A deterministic top-down grid puzzle on the engine. Walk a block one cell " +
      "at a time (WASD / arrows); press Q to freeze the current run into a ghost " +
      "that replays your exact path on a fixed 0.5s step, and R to restart. " +
      "Ghosts are solid and hold buttons open — so the way through a locked door " +
      "is to leave a ghost on the button and walk the live block through. " +
      "Includes an in-browser level editor + playtest with TOML import/export.",
    // Self-hosted: the editor/playtest flow (canvas + TOML textarea + validation
    // panel) owns its own page, copied into dist/zanzoban/ by the assembler.
    page: "zanzoban/index.html",
  },
  {
    id: "quintet",
    title: "Quintet",
    blurb: "Drag 5-cell blocks onto a 10×10 board and fill rows and columns to clear them for score.",
    desc:
      "A deterministic block-breaking placement game on the engine. Drag the " +
      "generated quintet (a 5-cell polyomino) from the side panel onto the 10×10 " +
      "board; fill any whole row or column to clear it, and clear several lines at " +
      "once for bonus points. Every offered piece is a real orthogonally-connected " +
      "pentomino guaranteed to fit somewhere — generation is seeded from the board, " +
      "score, and move count, so a given state always yields the same next piece. " +
      "When nothing fits, the board reports a stuck state and you press Reset.",
    // Self-hosted: the drag-and-drop canvas game owns its own page, copied into
    // dist/quintet/ by the assembler.
    page: "quintet/index.html",
  },
  {
    id: "physics-crucible",
    title: "Physics Crucible",
    blurb: "A live six-station physics proving room: watch bodies fall, bounce, and pile — then kick them and watch it re-settle.",
    desc:
      "A hostile test chamber for the engine's deterministic rigid-body physics, " +
      "driven entirely through its public PhysicsApi and simulated live. Six stations " +
      "sit in a grid: Body Bay (static / dynamic / kinematic / disabled bodies), " +
      "Contact Bay (sphere/plane, sphere/sphere, sphere/box, box/plane contacts), " +
      "Material Bay (a restitution bounce ladder), Query Bay (raycast + overlap-" +
      "sphere), Stress Bay (a deterministic sphere pile), and Replay Bay (a second " +
      "hidden world kept byte-identical to prove same-input determinism). The camera " +
      "orbits while the physics plays out; colour encodes each body's role and " +
      "markers show contacts. ▲ / Space / K kick every dynamic body upward so the " +
      "pile scatters and re-settles; ▼ / R reset and re-drop. The room loops on its " +
      "own. (WebGPU, with a WebGL2 / Canvas2D fallback.)",
    dir: "physics-crucible",
    jsModule: "axiom_physics_crucible",
    startFn: "physics_start",
    canvasId: "axiom-physics-crucible-canvas",
    buttons: [
      { key: "ArrowUp", label: "KICK", pos: "up" },
      { key: "ArrowDown", label: "RESET", pos: "down" },
    ],
  },
  {
    id: "gravix",
    title: "Gravix",
    blurb: "Roll a physics marble across procedurally-generated floating platform courses — over ramps, across jump gaps — collecting coins to the finish pad.",
    desc:
      "A marble-roll platformer on the engine's deterministic rigid-body physics. " +
      "Steer with camera-relative roll torque (W A S D): the contact-point friction " +
      "converts spin into real forward rolling, so the marble carries momentum. " +
      "Space jumps when grounded, Shift brakes, and the arrow keys orbit the camera. " +
      "Every course is procedurally generated from its level index — a winding grid " +
      "path with turns, tilted ramps (oriented-box collision), jump gaps, and hovering " +
      "coins — so each level replays identically. Reach the finish pad to advance; three " +
      "falls end the run (press R to restart). (WebGPU, with a WebGL2 / Canvas2D fallback.)",
    dir: "gravix",
    startFn: "gravix_start",
    canvasId: "axiom-gravix-canvas",
    buttons: [
      { key: "w", label: "▲", pos: "up" },
      { key: "a", label: "◀", pos: "left" },
      { key: "d", label: "▶", pos: "right" },
      { key: "s", label: "▼", pos: "down" },
      { key: " ", label: "JUMP", pos: "fire" },
    ],
  },
  {
    id: "sports-physics-lab",
    title: "Sports Physics Lab",
    blurb: "A first-person procedural sports arena: walk the field, pick up four kinds of sports balls (and the practice dummy), and toss them with real physics.",
    desc:
      "The foundational interactive sports primitive lab. A procedurally generated " +
      "60×90 practice field (markings baked in code) enclosed by bouncy walls; a " +
      "lineup of four procedural balls — soccer, football, bowling, baseball — each " +
      "a real rigid body with its own mass, bounce, and friction; and a T-pose " +
      "humanoid practice dummy. Click the canvas to capture the mouse and look " +
      "around; W A S D walk; left click picks up what the reticle targets and " +
      "tosses what you hold (heavier objects throw slower); right click sets it " +
      "down gently; V or the mouse wheel zooms out to third person to see your own " +
      "procedural body; R resets the lineup. Everything visible is generated at " +
      "runtime — no imported assets. (WebGPU, with a WebGL2 / Canvas2D fallback.)",
    dir: "sports-physics-lab",
    startFn: "sports_physics_lab_start",
    canvasId: "axiom-sports-physics-lab-canvas",
    buttons: [
      { key: "w", label: "▲", pos: "up" },
      { key: "a", label: "◀", pos: "left" },
      { key: "d", label: "▶", pos: "right" },
      { key: "s", label: "▼", pos: "down" },
      { key: "v", label: "VIEW", pos: "fire" },
    ],
  },
  {
    id: "harness",
    title: "Debug Overlay",
    blurb: "A backquote-toggled developer debug overlay + command console for the engine's browser surface.",
    desc:
      "Developer tooling on the engine surface: a debug overlay with live frame / " +
      "fps / backend read-outs and a tiny command console (help, overlay.*, " +
      "diagnostics.snapshot, backend.report, …). Press the backquote key (`) to " +
      "toggle it; Shift / Ctrl / Alt + backquote cycle density, pin, or focus the " +
      "console. Values come from a replaceable stub provider for now. The same " +
      "overlay also rides on top of the other demos in this gallery — open any of " +
      "them and press backquote.",
    // Self-hosted: the harness owns its page (a split light/dark canvas + the
    // overlay), copied into dist/harness/ by the assembler.
    page: "harness/index.html",
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

// True when the browser can create a WebGL2 context — the engine's fallback
// render path when WebGPU (navigator.gpu) is absent. Probed off a throwaway
// canvas so the shell's boot gate reflects the engine's real capability rather
// than assuming WebGPU is the only path.
function hasWebgl2() {
  try {
    return !!document.createElement("canvas").getContext("webgl2");
  } catch {
    return false;
  }
}

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

// The gallery is ONE wasm bundle, so every consumer on a page — the demo AND the
// debug overlay that rides on top of it — MUST share ONE wasm instance. The
// wasm-bindgen glue (`axiom_gallery_bg.js`) holds a single module-level `wasm`
// binding; importing the loader twice (e.g. with distinct `?v=` cache-busts loads
// two loader modules over the one shared glue) inits two wasm instances whose
// second `__wbg_set_wasm(...)` OVERWRITES that shared binding — hijacking the
// already-running demo onto the wrong instance, so its live loop reads a foreign
// linear memory and crashes ("TextDecoder: encoded data not valid", "memory access
// out of bounds", "table index out of bounds"). We therefore import + init the
// loader exactly ONCE per page and hand the same module to every caller.
let enginePromise = null;
function loadEngine() {
  if (enginePromise === null) {
    // Cache-bust ONCE per page load (the dev/static server may send no cache
    // headers) and share that URL, so every caller resolves the SAME module.
    const v = Date.now();
    enginePromise = import(`./axiom-loader.js?v=${v}`).then(async (mod) => {
      await mod.default();
      return mod;
    });
  }
  return enginePromise;
}

// Mount the backquote-toggled debug overlay on top of the current demo, on the
// SAME shared wasm instance the demo runs on (see `loadEngine`). Fire-and-forget:
// failures here (e.g. a gallery built without the harness entry) only warn — they
// never block or break the demo. The overlay shows its own stub diagnostics; it
// does not read the demo's engine state.
async function mountDebugOverlay() {
  try {
    const mod = await loadEngine();
    mod.harness_start();
  } catch (e) {
    console.warn("[gallery] debug overlay unavailable:", e);
  }
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

  // Mount the developer debug overlay over this shared-shell demo (press ` to
  // open it). Fire-and-forget so it never blocks or breaks the demo — it even
  // mounts when the demo itself can't start (e.g. no WebGPU available).
  mountDebugOverlay();

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

  // The engine selects a render backend at runtime: WebGPU → WebGL2 → Canvas2d
  // (and `?backend=canvas2d` forces the software rasterizer, which needs no GPU
  // at all). So the shell must only refuse to boot when there is genuinely NO
  // path — not when WebGPU alone is missing. A WebGPU-only gate wrongly blocked
  // WebGL2-capable browsers (e.g. WebKit, which exposes WebGL2 but no
  // navigator.gpu) and even blocked the forced-canvas2d path. Let the engine
  // pick the backend; it logs its own `axiom: FATAL — no render backend` if all
  // of them fail.
  const forcedCanvas2d = params.get("backend") === "canvas2d";
  if (!forcedCanvas2d && !("gpu" in navigator) && !hasWebgl2()) {
    setStatus(
      status,
      "No WebGPU or WebGL2 support in this browser. Use a recent Chrome/Edge, " +
        "Firefox, Android Chrome, or iOS Safari 18.2+.",
      "err",
    );
    return;
  }

  try {
    // Boot through the ONE packaged capability-detecting loader (it picks the wasm
    // fast-path or the wasm2js fallback itself) via the shared `loadEngine`, so the
    // demo and the debug overlay run on the SAME single wasm instance, then call the
    // demo's namespaced entry (`<demo>_start`).
    const mod = await loadEngine();
    if (cubeCount != null) {
      mod[demo.startFn](cubeCount);
    } else {
      mod[demo.startFn]();
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
