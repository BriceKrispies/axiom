/*
 * The browser boot harness for Home Run! — the host / platform edge. It boots the
 * shared pure-TypeScript engine (`@axiom/web-engine` — WebGL2 renderer, fixed-step
 * loop, input, WebAudio) with no external SDK and no wasm. It wires the pieces
 * together (renderer → input → game → loop) and adds the DOM HUD: score / pitch /
 * homers / streak / speed readouts,
 * the bat-load meter, the big center outcome text (HOME RUN! + confetti), the
 * ready and round-over overlays, and a tap-to-swing touch pad for mobile — all
 * driven from the game's `readHud()`.
 *
 * URL affordances (dev + deterministic screenshots): ?seed=N picks the round seed,
 * ?shot=N freezes the simulation after exactly N ticks, ?auto=1 starts the round
 * unattended, ?swingAt=N scripts one deterministic full-power swing, and
 * ?backend=canvas2d|webgl2 forces a render backend.
 *
 * The two dev-server couplings (the versioned hot-reload import and the `/events`
 * SSE channel) are the anchors the single-file packager rewrites for the static
 * gallery build — keep them verbatim.
 */

import { type BackendChoice, type Game, type RunningGame, runGame } from "@axiom/web-engine";
import type { Hud } from "./game.ts";
import { SUN_NOON_MS, SUN_START_MS } from "./view.ts";
import type { HomeRunSession } from "./session.ts";
import { HOME_RUN_CINEMATIC_TUNING } from "./cinematic-constants.ts";

const CANVAS_ID = "axiom-canvas";

interface Feedback {
  readonly kind: string;
  readonly text: string;
  readonly big: boolean;
}

/** The dynamically-imported pure game module (kept as a `/dist/game.js` import so
 * the single-file packager's hot-reload anchor stays verbatim). */
interface GameModule {
  readonly game: Game<HomeRunSession>;
  readonly readHud: (state: HomeRunSession) => Hud;
}

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

const boot_ = async (): Promise<void> => {
  const canvas = el(CANVAS_ID) as HTMLCanvasElement;
  const message = el("message");
  const confetti = el("confetti");
  const ready = el("ready");
  const over = el("over");
  const loadMeter = el("load-meter");
  const loadFill = el("load-fill");
  const hudEl = el("hud");
  const letterboxTop = el("letterbox-top");
  const letterboxBottom = el("letterbox-bottom");
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

  const updateHud = (hud: Hud, events: readonly Feedback[]): void => {
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

    // Home-run cinematic: letterbox bars (height alone animates — the canvas
    // itself never resizes) and the HUD dimming while the cinematic owns the
    // wide framing.
    const barPct = hud.letterboxProgress * HOME_RUN_CINEMATIC_TUNING.letterboxScreenFraction * 100;
    letterboxTop.style.height = `${barPct}%`;
    letterboxBottom.style.height = `${barPct}%`;
    hudEl.classList.toggle("cinematic-hidden", !hud.hudVisible);

    for (const fb of events) {
      if (fb.text.length > 0 && fb.kind !== "release") {
        showMessage(fb);
      }
      if (fb.kind === "homer") {
        popConfetti();
      }
    }
  };

  // Touch pad: left/right hold zones + a tap-to-swing button (mobile parity). Each
  // synthesizes the SAME key events the keyboard feeds, so no game-specific plumbing
  // is needed — the pure `update` sees identical input from touch and from keys.
  const wirePad = (running: RunningGame<HomeRunSession>): void => {
    const key = (code: string, down: boolean): void => {
      running.input.keyEvent(code, down);
    };
    const hold = (id: string, code: string): void => {
      const node = el(id);
      node.addEventListener("pointerdown", (e: PointerEvent): void => {
        node.setPointerCapture?.(e.pointerId);
        key(code, true);
      });
      node.addEventListener("pointerup", (): void => key(code, false));
      node.addEventListener("pointercancel", (): void => key(code, false));
    };
    // Buttons are labeled in SCREEN direction; ArrowLeft/Right map through the same
    // negation the keyboard does, so pad-left moves the batter screen-left.
    hold("pad-left", "ArrowLeft");
    hold("pad-right", "ArrowRight");
    // The swing button taps Space: a press edge this tick, released just after.
    el("pad-swing").addEventListener("pointerdown", (): void => {
      key("Space", true);
      globalThis.setTimeout((): void => key("Space", false), 40);
    });
  };

  // Backend selection: WebGL2 with an automatic Canvas2D software fallback;
  // `?backend=canvas2d` (or `?backend=webgl2`) forces one, the repo convention.
  const requested = new URLSearchParams(location.search).get("backend");
  const choice: BackendChoice = requested === "canvas2d" || requested === "webgl2" ? requested : "auto";

  let running: RunningGame<HomeRunSession> | undefined;
  const load = async (version: number): Promise<void> => {
    running?.stop();
    const mod = (await import(`/dist/game.js?v=${version}`)) as GameModule;

    // URL-driven dev/screenshot affordances (all deterministic).
    const params = new URLSearchParams(location.search);
    const num = (key: string): number | undefined => {
      const raw = params.get(key);
      return raw === null ? undefined : Number(raw);
    };
    const autoStart = params.get("auto") === "1";
    const swingAt = num("swingAt") ?? -1;
    const pinned = params.has("shot");
    // The sun's wall-clock timeMs: pinned to high noon under a ?shot freeze so
    // screenshots stay deterministic, otherwise a slow crawl from mid-morning.
    const nowMs = (): number => (pinned ? SUN_NOON_MS : SUN_START_MS + performance.now());

    // Buffer this-tick feedback across the frame's ticks, flush to the DOM HUD once.
    let events: Feedback[] = [];

    running = runGame(canvas, mod.game, {
      backend: choice,
      freezeAtTick: num("shot"),
      now: nowMs,
      onFrame: (state): void => {
        updateHud(mod.readHud(state), events);
        events = [];
      },
      onTick: (state): void => {
        events = [...events, ...state.tickEvents];
      },
      seed: num("seed"),
      // Scripted deterministic input for screenshots: start the round, swing once.
      script: (tick, input): void => {
        if (autoStart && tick === 2) {
          input.keyEvent("Enter", true);
        }
        if (autoStart && tick === 3) {
          input.keyEvent("Enter", false);
        }
        if (swingAt >= 0 && tick === swingAt) {
          input.keyEvent("Space", true);
        }
        if (swingAt >= 0 && tick === swingAt + 1) {
          input.keyEvent("Space", false);
        }
      },
    });
    wirePad(running);
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
