// Axiom demo gallery — backend-comparison triptych. Repo tooling (NOT part of the
// engine dependency graph; same status as gallery.js and the Makefile). Plain ES
// modules served statically.
//
// It renders ONE shared-shell demo three times, side by side, each pinned to a
// different render backend — WebGPU, WebGL2, and the Canvas2D software rasterizer
// — so you can watch the same scene through each path at once. Each pane is an
// `<iframe>` loading the ordinary per-demo page in embed mode with the backend
// pinned by URL:
//
//     demo.html?id=<demo>&backend=webgpu|webgl2|canvas2d&embed=1
//
// The engine (axiom-windowing) reads `?backend=` itself and binds exactly that
// backend (see `backend_preference` / `select_backend`), so no per-demo wiring is
// needed — the panes differ only by that one query param.
//
// A single transparent "mirror" overlay sits on top of the three panes and is the
// sole input authority: it captures the keyboard (the on-screen keypad, physical
// keys) and the pointer (one pointer-lock over all three, mouse-move deltas, mouse
// buttons) and fans every event out as a synthetic event into all three iframes.
// Because the demos are deterministic and tick-driven, identical input keeps the
// three renders in lockstep — you drive one, you drive all three. (Each pane runs
// its own requestAnimationFrame loop, so continuous demos can sit a frame or two
// out of phase; this is a visual backend comparison, not a frame-locked replay.)

import { DEMOS, demoById } from "./gallery.js";
import { renderKeypad } from "./keypad.js";

// The three backends, in the order the engine's own cascade prefers them.
const BACKENDS = [
  { id: "webgpu", name: "WebGPU", note: "GPU · primary" },
  { id: "webgl2", name: "WebGL2", note: "GPU · fallback" },
  { id: "canvas2d", name: "Canvas2D", note: "software rasterizer" },
];

// The game keys the demos consume that would otherwise scroll the page; we
// preventDefault them on the parent so mirroring never fights the browser.
const SCROLL_KEYS = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", " "]);

const setStatus = (el, msg, cls) => {
  el.textContent = msg;
  el.className = "status" + (cls ? " " + cls : "");
};

// Build the iframe src for one pane: the ordinary demo page, in embed mode, with
// the backend pinned. Preserves every incoming param (e.g. `cubes`, `relay`) so
// the panes match the triptych's own configuration.
function buildPaneSrc(demo, backendId, params) {
  const q = new URLSearchParams(params);
  q.set("id", demo.id);
  q.set("backend", backendId);
  q.set("embed", "1");
  return `./demo.html?${q.toString()}`;
}

// Forward a keyboard event into one iframe as a fresh event constructed in that
// iframe's realm (so its listeners' `instanceof` checks pass). Best-effort: a
// not-yet-loaded frame is skipped.
function forwardKey(frame, type, src) {
  const w = frame.contentWindow;
  if (!w || !w.KeyboardEvent) return;
  w.dispatchEvent(
    new w.KeyboardEvent(type, {
      key: src.key,
      code: src.code,
      keyCode: src.keyCode,
      which: src.which,
      location: src.location,
      repeat: src.repeat,
      shiftKey: src.shiftKey,
      ctrlKey: src.ctrlKey,
      altKey: src.altKey,
      metaKey: src.metaKey,
      bubbles: true,
      cancelable: true,
    }),
  );
}

// Forward a mouse event (move deltas / buttons) into one iframe's window.
function forwardMouse(frame, type, src) {
  const w = frame.contentWindow;
  if (!w || !w.MouseEvent) return;
  w.dispatchEvent(
    new w.MouseEvent(type, {
      button: src.button,
      buttons: src.buttons,
      movementX: src.movementX,
      movementY: src.movementY,
      bubbles: true,
      cancelable: true,
      view: w,
    }),
  );
}

// Wire the single input authority. `overlay` is the transparent capture layer
// over the panes; `frames` are the three demo iframes. Keyboard mirrors always;
// the mouse mirrors only while the overlay holds the pointer lock (so ambient
// movement can't spin a camera when you're not "playing"), and the lock-engaging
// click is swallowed rather than fired.
function installMirror(overlay, frames) {
  const isLocked = () => document.pointerLockElement === overlay;

  const onKey = (e) => {
    if (SCROLL_KEYS.has(e.key)) e.preventDefault();
    for (const f of frames) forwardKey(f, e.type, e);
  };
  window.addEventListener("keydown", onKey, { passive: false });
  window.addEventListener("keyup", onKey, { passive: false });

  overlay.addEventListener("mousedown", (e) => {
    e.preventDefault();
    // First click engages the shared pointer lock; it must not also fire.
    if (!isLocked()) {
      overlay.requestPointerLock();
      return;
    }
    for (const f of frames) forwardMouse(f, "mousedown", e);
  });
  document.addEventListener("mousemove", (e) => {
    if (!isLocked()) return;
    for (const f of frames) forwardMouse(f, "mousemove", e);
  });
  document.addEventListener("mouseup", (e) => {
    if (!isLocked()) return;
    for (const f of frames) forwardMouse(f, "mouseup", e);
  });
  document.addEventListener("pointerlockchange", () => {
    overlay.classList.toggle("locked", isLocked());
  });
  // Keep keyboard focus in the parent document (never inside an iframe) so the
  // window key listeners above always see the keystrokes.
  overlay.tabIndex = 0;
  overlay.focus();
}

// Parent-level cube-count presets for the stress demo: reload the whole triptych
// (all three panes) with `?cubes=N`.
const CUBE_PRESETS = [100, 500, 1000, 2000, 5000, 10000, 25000];

function mountCubeBar(host, demo, current) {
  const bar = document.createElement("div");
  bar.className = "cubebar";
  const label = document.createElement("span");
  label.className = "cubebar-label";
  label.textContent = "cubes:";
  bar.appendChild(label);
  for (const n of CUBE_PRESETS) {
    const a = document.createElement("a");
    a.href = `./triptych.html?id=${encodeURIComponent(demo.id)}&cubes=${n}`;
    a.textContent = n.toLocaleString();
    if (n === current) a.className = "active";
    bar.appendChild(a);
  }
  host.appendChild(bar);
}

// Parent-level relay bar for the netplay demo: reload the triptych pointed at a
// relay. (All three panes then connect as the same client — the point here is to
// compare the render backends, not to play a real two-party match.)
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
 * Boot the backend-comparison triptych for the demo named by `?id=`.
 */
export function bootTriptych() {
  const params = new URLSearchParams(location.search);
  const panesHost = document.getElementById("panes");
  const controls = document.getElementById("controls");
  const keypad = document.getElementById("keypad");
  const status = document.getElementById("status");
  const titleEl = document.getElementById("demo-title");

  const demo = demoById(params.get("id"));
  if (!demo) {
    setStatus(status, "Unknown demo. Return to the gallery to pick one.", "err");
    return;
  }
  // Self-hosted demos own bespoke multi-screen pages that don't fit the shared
  // single-canvas embed; the triptych only wraps shared-shell demos. Fall back to
  // the demo's own page.
  if (demo.page) {
    location.replace(`./${demo.page}`);
    return;
  }
  titleEl.textContent = demo.title + " — backends";
  document.title = "Axiom — " + demo.title + " (backends)";

  // Three panes, each an embedded demo pinned to one backend.
  const frames = [];
  for (const b of BACKENDS) {
    const pane = document.createElement("div");
    pane.className = "pane";

    const label = document.createElement("div");
    label.className = "pane-label";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = b.name;
    const note = document.createElement("span");
    note.textContent = b.note;
    label.append(name, note);

    const frame = document.createElement("iframe");
    frame.className = "pane-frame";
    frame.setAttribute("scrolling", "no");
    frame.title = `${demo.title} — ${b.name}`;
    frame.src = buildPaneSrc(demo, b.id, params);

    pane.append(label, frame);
    panesHost.appendChild(pane);
    frames.push(frame);
  }

  // The single input authority over all three panes.
  const overlay = document.createElement("div");
  overlay.className = "mirror-overlay";
  const hint = document.createElement("div");
  hint.className = "mirror-hint";
  hint.textContent = demo.buttons.length
    ? "Click to capture the mouse · keyboard, touch & mouse mirror to all three panes · Esc releases"
    : "This demo takes no input — the three panes render the same scene on each backend";
  overlay.appendChild(hint);
  panesHost.appendChild(overlay);
  installMirror(overlay, frames);

  // Parent-owned chrome: one keypad + control bar drives all three panes.
  if (demo.buttons.length > 0) {
    renderKeypad(keypad, demo.buttons);
  }
  if (demo.cubeStress) {
    const current = Math.max(1, parseInt(params.get("cubes") ?? "2000", 10) || 2000);
    mountCubeBar(controls, demo, current);
  }
  if (demo.needsRelay) {
    mountRelayBar(controls, demo, params.get("relay"));
  }

  setStatus(
    status,
    "Rendering the same scene through WebGPU · WebGL2 · Canvas2D. A pane stays " +
      "black if its backend isn't available in this browser (e.g. WebGPU in " +
      "Firefox/Safari) — its console logs why.",
    "ok",
  );
}
