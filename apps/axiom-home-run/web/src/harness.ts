/*
 * The browser boot harness for Home Run! — the host / platform edge (NOT engine
 * spine), so it lives in the app `web/` dir, outside the branchless + coverage
 * gates, and uses ordinary control flow. `createGame()` mints the per-game registry
 * the author module (`game.ts`) registers its `onFixedUpdate` into; the SDK's
 * `boot()` builds the deterministic loop over the wasm bridge, wires DOM input, and
 * presents the authored stadium every frame. This harness adds the DOM HUD: score /
 * pitch / homers / streak / speed readouts, the bat-load meter, the big center
 * outcome text (HOME RUN! + confetti), the ready and round-over overlays, and a
 * tap-to-swing touch pad for mobile — all driven from the game's `readHud()`.
 *
 * URL affordances (dev + deterministic screenshots): ?seed=N picks the round seed,
 * ?shot=N freezes the simulation after exactly N ticks, ?auto=1 starts the round
 * unattended, ?swingAt=N scripts one deterministic full-power swing.
 *
 * The three dev-server couplings (the wasm init call, the versioned hot-reload
 * import, and the `/events` SSE channel) are the anchors the single-file packager
 * rewrites for the static gallery build — keep them verbatim.
 */

import { boot } from "/vendor/axiom-game/boot.js";
import { createGame, onRender } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";

const FIXED_HZ = 60;
const SEED = 1n;
const NANOS_PER_SECOND = 1_000_000_000;
const FIXED_STEP_NANOS = Math.round(NANOS_PER_SECOND / FIXED_HZ);
const MAX_STEPS_PER_FRAME = 8;
const MAX_INSTANCES = 4096;
const CANVAS_ID = "axiom-canvas";

interface Feedback {
  readonly kind: string;
  readonly text: string;
  readonly big: boolean;
}

interface PitchResult {
  readonly outcome: string;
  readonly points: number;
  readonly distance: number;
  readonly mph: number;
  readonly caught: boolean;
}

/** The HUD snapshot the game module exposes each frame. */
interface Hud {
  readonly phase: "ready" | "windup" | "pitch" | "flight" | "result" | "over";
  readonly score: number;
  readonly pitchNumber: number;
  readonly pitchCount: number;
  readonly homers: number;
  readonly streak: number;
  readonly multiplier: number;
  readonly bestDistance: number;
  readonly lastMph: number;
  readonly lastPitchName: string;
  readonly readiness: number;
  readonly ready: boolean;
  readonly results: readonly PitchResult[];
  readonly events: readonly Feedback[];
}

interface GameModule {
  readonly readHud: () => Hud;
  readonly setPad: (moveX: number, swingTap: boolean) => void;
  readonly configure: (opts: {
    seed?: number;
    freezeAt?: number;
    autoStart?: boolean;
    swingAt?: number;
  }) => void;
}

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

// The Canvas2D software backend logs a verbose stats line EVERY frame (~60/sec) —
// drop just that per-frame line at the platform edge; one-time engine logs still show.
const passthroughLog = console.log.bind(console);
console.log = (...args: unknown[]): void => {
  const first = args[0];
  if (typeof first === "string" && first.startsWith("axiom-canvas2d:")) {
    return;
  }
  passthroughLog(...args);
};

const boot_ = async (): Promise<void> => {
  const canvas = el(CANVAS_ID) as HTMLCanvasElement;
  const message = el("message");
  const confetti = el("confetti");
  const ready = el("ready");
  const over = el("over");
  const loadMeter = el("load-meter");
  const loadFill = el("load-fill");
  const fields = {
    best: el("best"),
    homers: el("homers"),
    mph: el("mph"),
    overBest: el("over-best"),
    overHomers: el("over-homers"),
    overScore: el("over-score"),
    pitch: el("pitch"),
    score: el("score"),
    streak: el("streak"),
  };

  await initWasm();

  // Center outcome text: one message at a time, keyed by outcome kind for styling.
  // Under a ?shot=N freeze the message is pinned, so screenshots are deterministic.
  const pinMessages = new URLSearchParams(location.search).has("shot");
  let messageTimer = 0;
  const showMessage = (fb: Feedback): void => {
    message.textContent = fb.text;
    message.className = `show ${fb.kind}${fb.big ? " big" : ""}`;
    globalThis.clearTimeout(messageTimer);
    if (!pinMessages) {
      messageTimer = globalThis.setTimeout((): void => {
        message.className = "";
      }, fb.big ? 2100 : 1200);
    }
  };

  // A burst of DOM confetti for home runs (presentation only). Under a ?shot
  // freeze the bits are laid out statically (no wall-clock animation), so the
  // celebration frame is deterministic.
  const popConfetti = (): void => {
    confetti.innerHTML = "";
    for (let i = 0; i < 36; i += 1) {
      const bit = document.createElement("div");
      bit.className = "bit";
      bit.style.left = `${8 + ((i * 37) % 84)}%`;
      bit.style.background = ["#ffd23d", "#ff6a5e", "#6ecbff", "#7fffa8", "#ff9de2"][i % 5]!;
      if (pinMessages) {
        bit.style.animation = "none";
        bit.style.top = `${(i * 53) % 78}%`;
        bit.style.transform = `rotate(${(i * 97) % 360}deg)`;
      } else {
        bit.style.animationDelay = `${(i % 9) * 0.07}s`;
        bit.style.animationDuration = `${1.3 + (i % 5) * 0.18}s`;
      }
      confetti.append(bit);
    }
    if (!pinMessages) {
      globalThis.setTimeout((): void => {
        confetti.innerHTML = "";
      }, 2600);
    }
  };

  const updateHud = (hud: Hud): void => {
    fields.score.textContent = String(hud.score);
    fields.pitch.textContent = `${hud.pitchNumber}/${hud.pitchCount}`;
    fields.homers.textContent = String(hud.homers);
    fields.streak.innerHTML = `${hud.multiplier}&times;`;
    fields.streak.classList.toggle("up", hud.streak > 1);
    fields.mph.textContent = hud.lastMph > 0 ? `${hud.lastMph} MPH ${hud.lastPitchName}` : "—";
    fields.best.textContent = hud.bestDistance > 0 ? `${hud.bestDistance}m` : "—";

    // The ready meter: visible while the batter re-winds (the swing cooldown),
    // fading away once he's wound and ready. Hidden on the ready/over screens.
    const live = hud.phase !== "ready" && hud.phase !== "over";
    loadMeter.classList.toggle("on", live && !hud.ready);
    loadFill.style.width = `${Math.round(hud.readiness * 100)}%`;
    loadFill.classList.toggle("full", hud.ready);

    ready.classList.toggle("show", hud.phase === "ready");
    const done = hud.phase === "over";
    over.classList.toggle("show", done);
    if (done) {
      fields.overScore.textContent = String(hud.score);
      fields.overHomers.textContent = String(hud.homers);
      fields.overBest.textContent = hud.bestDistance > 0 ? `${hud.bestDistance}m` : "—";
    }

    for (const fb of hud.events) {
      if (fb.text.length > 0 && fb.kind !== "release") {
        showMessage(fb);
      }
      if (fb.kind === "homer") {
        popConfetti();
      }
    }
  };

  // Touch pad: left/right hold zones + a tap-to-swing button (mobile parity).
  const wirePad = (mod: GameModule): void => {
    let moveX = 0;
    const hold = (id: string, down: () => void, up: () => void): void => {
      const node = el(id);
      node.addEventListener("pointerdown", (e: PointerEvent): void => {
        node.setPointerCapture?.(e.pointerId);
        down();
      });
      node.addEventListener("pointerup", up);
      node.addEventListener("pointercancel", up);
    };
    // Buttons are labeled in SCREEN direction; setPad expects screen sign (game.ts negates).
    hold("pad-left", () => { moveX = -1; mod.setPad(moveX, false); }, () => { moveX = 0; mod.setPad(moveX, false); });
    hold("pad-right", () => { moveX = 1; mod.setPad(moveX, false); }, () => { moveX = 0; mod.setPad(moveX, false); });
    // The swing button queues one press edge per tap.
    el("pad-swing").addEventListener("pointerdown", (): void => mod.setPad(moveX, true));
  };

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: CANVAS_ID });
    const mod = (await import(`/dist/game.js?v=${version}`)) as GameModule;

    // URL-driven dev/screenshot affordances (all deterministic).
    const params = new URLSearchParams(location.search);
    const num = (key: string): number | undefined => {
      const raw = params.get(key);
      return raw === null ? undefined : Number(raw);
    };
    mod.configure({
      autoStart: params.get("auto") === "1",
      freezeAt: num("shot"),
      seed: num("seed"),
      swingAt: num("swingAt"),
    });
    wirePad(mod);

    onRender((): void => updateHud(mod.readHud()));

    app.start();
    // frameLocked: one sim tick per displayed frame, so the first frame builds the
    // whole scene (registering every material) BEFORE the 3D surface binds.
    teardown = boot(game as unknown as Parameters<typeof boot>[0], app, {
      canvas,
      frameLocked: true,
      present3d: { maxInstances: MAX_INSTANCES },
    });
  };

  await load(0);

  const isDev = location.hostname === "localhost" || location.hostname === "127.0.0.1";
  if (isDev) {
    const events = new EventSource("/events");
    events.addEventListener("reload", (event: MessageEvent<string>): void => {
      void load(Number(event.data));
    });
  }
};

void boot_();
