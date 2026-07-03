// Cartridge loader — the workspace as a dev *console* that loads any `games/`
// cartridge you're working on into a live runtime viewport.
//
// It reads a games manifest (the cartridges the workspace hosts, derived from
// `games/`), renders a game-selector, and on selection loads the chosen game's
// OWN wasm bundle straight into a canvas and calls its entry (e.g. `retro_fps_start`)
// — no embedded document, and no Rust runtime dependency: the workspace crate
// stays `kernel` + `runtime` portable, each game bringing its own engine through
// its bundle, so the same cartridge the gallery showcases loads here unchanged.
//
// This host is mounted ONCE, outside the reducer-driven `#workspace-root` (which
// `dom_mount` clears on every dispatch), so a running game survives shell
// re-renders.

interface Cartridge {
  readonly id: string;
  readonly title: string;
  readonly bundle: string;
  readonly entry: string;
  readonly canvas: string;
}

export async function mountCartridgeHost(
  host: HTMLElement,
  manifestUrl: string,
): Promise<void> {
  const cartridges: readonly Cartridge[] = await fetch(manifestUrl)
    .then((r) => r.json() as Promise<readonly Cartridge[]>)
    .catch(() => []);

  const bar = document.createElement("div");
  bar.className = "ws-cartridge-bar";

  const heading = document.createElement("span");
  heading.className = "ws-cartridge-heading";
  heading.textContent = "RUNTIME CONSOLE";

  const label = document.createElement("label");
  label.className = "ws-cartridge-label";
  label.textContent = "game (games/ cartridge):";

  const select = document.createElement("select");
  select.className = "ws-cartridge-select";
  const none = document.createElement("option");
  none.value = "";
  none.textContent = cartridges.length
    ? "— select a cartridge —"
    : "— no cartridges found —";
  select.append(none);
  for (const c of cartridges) {
    const option = document.createElement("option");
    option.value = c.id;
    option.textContent = c.title;
    select.append(option);
  }

  const status = document.createElement("span");
  status.className = "ws-cartridge-status";
  status.textContent = `${cartridges.length} cartridge(s) in games/`;

  bar.append(heading, label, select, status);

  const stage = document.createElement("div");
  stage.className = "ws-cartridge-stage";
  const idle = document.createElement("div");
  idle.className = "ws-cartridge-idle";
  idle.textContent =
    "Select a cartridge to load it live in the workspace viewport.";
  stage.append(idle);

  host.append(bar, stage);

  let loading = false;
  select.addEventListener("change", () => {
    const cartridge = cartridges.find((x) => x.id === select.value);
    if (cartridge === undefined || loading) {
      return;
    }
    loading = true;
    void bootCartridge(cartridge, stage, status).finally(() => {
      loading = false;
    });
  });
}

async function bootCartridge(
  cartridge: Cartridge,
  stage: HTMLElement,
  status: HTMLElement,
): Promise<void> {
  status.textContent = `loading ${cartridge.title}…`;
  stage.replaceChildren();
  const canvas = document.createElement("canvas");
  canvas.id = cartridge.canvas;
  canvas.width = 960;
  canvas.height = 600;
  canvas.className = "ws-cartridge-canvas";
  stage.append(canvas);
  try {
    const mod = (await import(cartridge.bundle)) as {
      default: () => Promise<unknown>;
      [entry: string]: unknown;
    };
    await mod.default();
    (mod[cartridge.entry] as () => void)();
    status.textContent = `running ${cartridge.title}`;
  } catch (error) {
    status.textContent = `failed to load ${cartridge.title}: ${String(error)}`;
  }
}
