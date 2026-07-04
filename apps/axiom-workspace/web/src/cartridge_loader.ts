// Cartridge loader — the workspace as a dev *console* that loads any engine app
// you're working on into a live runtime viewport.
//
// It reads a console manifest (the apps the workspace hosts — the `games/`
// cartridges AND the gallery's showcase apps, all built into the one gallery
// bundle) and renders an app-selector. On selection it either:
//   * boots an **inline** app straight into a canvas by importing the bundle and
//     calling its entry (e.g. `retro_fps_start`) — no embedded document, no Rust
//     runtime dependency: the workspace crate stays `kernel` + `runtime`
//     portable, each app bringing its own engine through its bundle; or
//   * opens a **page** app (a self-hosted multi-screen demo — growth, zanzoban,
//     the debug-overlay harness — whose own DOM does not fit a bare canvas) in a
//     new tab. The workspace shell embeds no nested browsing contexts (see
//     tests/architecture.rs); a multi-screen app is launched, not embedded.
//
// Because this is the developer console (not the public showcase), it opts every
// inline app into the engine's dev overlays by setting `window.__axiom_dev_tools`
// before boot — that is what makes the frame-scrubber overlay appear here while
// it stays off in the gallery.
//
// This host is mounted ONCE, outside the reducer-driven `#workspace-root` (which
// `dom_mount` clears on every dispatch), so a running app survives shell
// re-renders.

interface Cartridge {
  readonly id: string;
  readonly title: string;
  readonly kind: "inline" | "page";
  // Present for `kind: "inline"`: the wasm bundle to import, the entry to call,
  // and the canvas id the app binds its surface to.
  readonly bundle?: string;
  readonly entry?: string;
  readonly canvas?: string;
  // Present for `kind: "page"`: the self-hosted page to open.
  readonly page?: string;
  // When true, the app is an engine-`App` 3D demo that can be rendered through
  // all three backends at once (the no-frame backend-compare tool).
  readonly compare?: boolean;
}

// Mark this document as a developer console so the engine's dev overlays (the
// frame scrubber) start visible here. The public gallery never sets this, so the
// same app bundle keeps the scrubber hidden there. Set once, up front.
function enableDevTools(): void {
  (globalThis as unknown as { __axiom_dev_tools?: boolean }).__axiom_dev_tools = true;
}

// The gallery bundle is ONE wasm-bindgen module: importing it and calling its
// `default()` init more than once re-runs the init and clobbers the shared `wasm`
// binding, hijacking any already-running app onto a second instance (the exact
// hazard gallery.js documents). So import + init each bundle EXACTLY ONCE per
// page and hand the same module to every caller (inline boot AND backend compare).
type EngineModule = {
  default: () => Promise<unknown>;
  [entry: string]: unknown;
};
const engineModules = new Map<string, Promise<EngineModule>>();
function loadEngine(bundle: string): Promise<EngineModule> {
  const cached = engineModules.get(bundle);
  if (cached !== undefined) {
    return cached;
  }
  const pending = import(bundle).then(async (mod) => {
    const engine = mod as EngineModule;
    await engine.default();
    return engine;
  });
  engineModules.set(bundle, pending);
  return pending;
}

export async function mountCartridgeHost(
  host: HTMLElement,
  manifestUrl: string,
): Promise<void> {
  enableDevTools();

  // `no-store`: always read the CURRENT manifest, never a cached one — a stale
  // cached manifest could route an app through the wrong (`page`) branch and pop
  // a new window instead of booting it inline.
  const cartridges: readonly Cartridge[] = await fetch(manifestUrl, { cache: "no-store" })
    .then((r) => r.json() as Promise<readonly Cartridge[]>)
    .catch(() => []);

  const bar = document.createElement("div");
  bar.className = "ws-cartridge-bar";

  const heading = document.createElement("span");
  heading.className = "ws-cartridge-heading";
  heading.textContent = "RUNTIME CONSOLE";

  const label = document.createElement("label");
  label.className = "ws-cartridge-label";
  label.textContent = "app:";

  const select = document.createElement("select");
  select.className = "ws-cartridge-select";
  const none = document.createElement("option");
  none.value = "";
  none.textContent = cartridges.length
    ? "— select an app —"
    : "— no apps found —";
  select.append(none);
  for (const c of cartridges) {
    const option = document.createElement("option");
    option.value = c.id;
    option.textContent = c.title;
    select.append(option);
  }

  // Backend-compare button: shown only while a comparable app is selected. It
  // renders the same app three ways at once (WebGPU / WebGL2 / Canvas2D), from
  // one wasm instance and one sim — the no-frame successor to the old triptych.
  const compareBtn = document.createElement("button");
  compareBtn.className = "ws-cartridge-compare";
  compareBtn.type = "button";
  compareBtn.textContent = "Compare backends";
  compareBtn.hidden = true;

  const status = document.createElement("span");
  status.className = "ws-cartridge-status";
  status.textContent = `${cartridges.length} app(s) available`;

  bar.append(heading, label, select, compareBtn, status);

  const stage = document.createElement("div");
  stage.className = "ws-cartridge-stage";
  const idle = document.createElement("div");
  idle.className = "ws-cartridge-idle";
  idle.textContent =
    "Select an app to load it live in the workspace viewport.";
  stage.append(idle);

  host.append(bar, stage);

  let loading = false;
  let selected: Cartridge | undefined;
  select.addEventListener("change", () => {
    const cartridge = cartridges.find((x) => x.id === select.value);
    selected = cartridge;
    compareBtn.hidden = cartridge?.compare !== true;
    if (cartridge === undefined || loading) {
      return;
    }
    loading = true;
    void bootCartridge(cartridge, stage, status).finally(() => {
      loading = false;
    });
  });

  compareBtn.addEventListener("click", () => {
    if (selected === undefined || selected.compare !== true || loading) {
      return;
    }
    loading = true;
    void bootCompare(selected, stage, status).finally(() => {
      loading = false;
    });
  });
}

async function bootCartridge(
  cartridge: Cartridge,
  stage: HTMLElement,
  status: HTMLElement,
): Promise<void> {
  // A self-hosted multi-screen app: launch it in a new tab (no nested browsing
  // context — the shell bans them). The console keeps running.
  if (cartridge.kind === "page") {
    const page = cartridge.page ?? "";
    window.open(page, "_blank", "noopener");
    status.textContent = `opened ${cartridge.title} in a new tab`;
    return;
  }

  status.textContent = `loading ${cartridge.title}…`;
  stage.replaceChildren();
  const canvas = document.createElement("canvas");
  canvas.id = cartridge.canvas ?? "";
  canvas.width = 960;
  canvas.height = 600;
  canvas.className = "ws-cartridge-canvas";
  stage.append(canvas);
  try {
    const mod = await loadEngine(cartridge.bundle ?? "");
    (mod[cartridge.entry ?? ""] as () => void)();
    status.textContent = `running ${cartridge.title}`;
  } catch (error) {
    status.textContent = `failed to load ${cartridge.title}: ${String(error)}`;
  }
}

// The three backend panes, in the engine cascade's own preference order. Each id
// is the surface the matching pane's presenter binds to.
const COMPARE_PANES: readonly { readonly id: string; readonly label: string }[] = [
  { id: "ws-compare-webgpu", label: "WebGPU · GPU primary" },
  { id: "ws-compare-webgl2", label: "WebGL2 · GPU fallback" },
  { id: "ws-compare-canvas2d", label: "Canvas2D · software" },
];

async function bootCompare(
  cartridge: Cartridge,
  stage: HTMLElement,
  status: HTMLElement,
): Promise<void> {
  status.textContent = `comparing ${cartridge.title} across backends…`;
  stage.replaceChildren();
  const grid = document.createElement("div");
  grid.className = "ws-compare-grid";
  for (const pane of COMPARE_PANES) {
    const cell = document.createElement("div");
    cell.className = "ws-compare-pane";
    const label = document.createElement("div");
    label.className = "ws-compare-label";
    label.textContent = pane.label;
    const canvas = document.createElement("canvas");
    canvas.id = pane.id;
    canvas.width = 400;
    canvas.height = 300;
    canvas.className = "ws-compare-canvas";
    cell.append(label, canvas);
    grid.append(cell);
  }
  stage.append(grid);
  try {
    const mod = await loadEngine(cartridge.bundle ?? "");
    const compare = mod["compare_start"] as (
      demoId: string,
      a: string,
      b: string,
      c: string,
    ) => void;
    compare(cartridge.id, COMPARE_PANES[0].id, COMPARE_PANES[1].id, COMPARE_PANES[2].id);
    status.textContent = `comparing ${cartridge.title} — WebGPU · WebGL2 · Canvas2D`;
  } catch (error) {
    status.textContent = `failed to compare ${cartridge.title}: ${String(error)}`;
  }
}
