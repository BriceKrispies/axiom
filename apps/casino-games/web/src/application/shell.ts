/*
 * shell.ts — the application shell: screen switching (catalog ↔ game),
 * mounting/stopping games through the registry, the game chrome (instruction,
 * result banner, round controls), pointer normalization into the fixed
 * 960×600 logical canvas space, synthetic key presses for the DOM buttons and
 * touch, the settings panel, and the dev diagnostics drawer (?debug=1 only).
 *
 * The shell is the app's impure edge: the one place DOM, localStorage, and
 * URL parameters exist. Seeds enter here (boundary entropy or ?seed) and are
 * passed into deterministic session logic — never drawn inside games.
 */

import type { BackendChoice, InputState } from "@axiom/web-engine";
import type { CasinoGameConfig } from "../chance-engine/configuration/schema.ts";
import type { CasinoHud, CasinoGameDefinition, RunningCasinoGame } from "../chance-engine/registry/definition.ts";
import type { CasinoGameRegistry } from "../chance-engine/registry/registry.ts";
import { SeededChanceResultSource } from "../chance-engine/outcomes/result-source.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH } from "../presentation/cameras/picking.ts";
import { RARITY_CSS } from "../presentation/rewards/tiers.ts";
import { storedConfigOf } from "./config-store.ts";
import type { PlayerSettings } from "./settings.ts";
import { loadSettings, resolveSettings, saveSettings } from "./settings.ts";
import { buildWorkbench } from "../workbench/workbench.ts";
import { buildCatalog } from "../catalog/catalog.ts";
import { paintGlyphBadge } from "../catalog/thumbnails.ts";

interface UrlBoot {
  readonly game: string | null;
  readonly seed: number | null;
  readonly shot: number | null;
  readonly backend: BackendChoice | undefined;
  readonly debug: boolean;
  readonly presses: readonly { readonly code: string; readonly at: number }[];
  readonly workbench: boolean;
}

const parseUrl = (): UrlBoot => {
  const params = new URLSearchParams(location.search);
  const seedText = params.get("seed");
  const shotText = params.get("shot");
  const backendText = params.get("backend");
  const presses = (params.get("press") ?? "")
    .split(",")
    .map((token) => token.trim())
    .filter((token) => token.includes("@"))
    .map((token) => {
      const [code, at] = token.split("@");
      return { at: Number(at), code: code as string };
    });
  return {
    backend: backendText === "canvas2d" || backendText === "webgl2" ? backendText : undefined,
    debug: params.get("debug") === "1",
    game: params.get("game"),
    presses,
    seed: seedText === null ? null : Number(seedText) >>> 0,
    shot: shotText === null ? null : Number(shotText),
    workbench: params.get("workbench") === "1",
  };
};

/** The outermost-boundary entropy read: one unpredictable 32-bit seed,
 * immediately recorded and passed into deterministic session logic. */
const boundarySeed = (): number => {
  const words = new Uint32Array(1);
  crypto.getRandomValues(words);
  return (words[0] as number) >>> 0;
};

const el = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

/** Press a synthetic key code into a running game's input (held ~3 ticks so
 * the fixed-step snapshot reliably sees the edge — see the end-zone lesson). */
const pressSynthetic = (input: InputState, code: string): void => {
  input.keyEvent(code, true);
  setTimeout(() => input.keyEvent(code, false), 60);
};

export const bootShell = (registry: CasinoGameRegistry): void => {
  const url = parseUrl();
  const rootSeed = url.seed ?? boundarySeed();
  let settings: PlayerSettings = loadSettings();

  const canvas = el<HTMLCanvasElement>("axiom-canvas");
  const catalogSection = el<HTMLElement>("catalog");
  const gameSection = el<HTMLElement>("game-screen");
  const workbenchHost = el<HTMLElement>("workbench");
  const settingsHost = el<HTMLElement>("settings");
  const diagnosticsHost = el<HTMLElement>("diagnostics");
  const instruction = el<HTMLElement>("instruction");
  const banner = el<HTMLElement>("result-banner");
  const title = el<HTMLElement>("game-title");
  const seedLabel = el<HTMLElement>("seed-label");

  let running: RunningCasinoGame | null = null;
  let activeGameId: string | null = null;
  let detachPointer: (() => void) | null = null;
  let lastHudKey = "";

  const applyRootSettings = (): void => {
    document.documentElement.dataset["contrast"] = settings.highContrast ? "high" : "normal";
    document.documentElement.dataset["textScale"] = settings.textScale;
    const pressed = String(settings.muted);
    el<HTMLElement>("btn-mute").setAttribute("aria-pressed", pressed);
    el<HTMLElement>("btn-game-mute").setAttribute("aria-pressed", pressed);
    el<HTMLElement>("btn-mute").textContent = settings.muted ? "🔇" : "🔊";
    el<HTMLElement>("btn-game-mute").textContent = settings.muted ? "🔇" : "🔊";
  };

  /** Re-feed canvas pointer events in LOGICAL canvas coordinates. Registered
   * after the engine's own listener, so its normalized sample wins the tick. */
  const attachPointerNormalizer = (input: InputState): (() => void) => {
    const normalize = (event: PointerEvent): void => {
      const scaleX = CANVAS_WIDTH / canvas.clientWidth;
      const scaleY = CANVAS_HEIGHT / canvas.clientHeight;
      input.pointerEvent(event.offsetX * scaleX, event.offsetY * scaleY, event.buttons !== 0);
    };
    canvas.addEventListener("pointerdown", normalize);
    canvas.addEventListener("pointermove", normalize);
    canvas.addEventListener("pointerup", normalize);
    return (): void => {
      canvas.removeEventListener("pointerdown", normalize);
      canvas.removeEventListener("pointermove", normalize);
      canvas.removeEventListener("pointerup", normalize);
    };
  };

  const onHud = (hud: CasinoHud): void => {
    const key = `${hud.phase}|${hud.instruction}|${hud.resultText ?? ""}|${hud.inputLocked}|${hud.round}`;
    if (key === lastHudKey) {
      return;
    }
    lastHudKey = key;
    instruction.textContent = hud.instruction;
    if (hud.resultText !== null) {
      banner.textContent = hud.resultText;
      banner.className = `show ${hud.win === true ? `win-${hud.rarity ?? "common"}` : "loss"}`;
      if (hud.rarity !== null) {
        banner.style.color = RARITY_CSS[hud.rarity];
      } else {
        banner.style.removeProperty("color");
      }
    } else {
      banner.className = "";
    }
    for (const id of ["btn-new-round", "btn-replay"]) {
      el<HTMLButtonElement>(id).disabled = hud.inputLocked;
    }
    seedLabel.textContent = `seed ${hud.audit.seedOrRoundId} · round ${hud.round}`;
    if (url.debug) {
      diagnosticsHost.textContent = JSON.stringify(
        { hud: { phase: hud.phase, rarity: hud.rarity, round: hud.round, tierId: hud.tierId, win: hud.win } , audit: hud.audit },
        null,
        1,
      );
    }
  };

  const stopGame = (): void => {
    running?.stop();
    running = null;
    detachPointer?.();
    detachPointer = null;
    activeGameId = null;
    lastHudKey = "";
    delete document.body.dataset["activeGame"];
  };

  const showScreen = (screen: "catalog" | "game"): void => {
    catalogSection.style.display = screen === "catalog" ? "" : "none";
    gameSection.classList.toggle("active", screen === "game");
    if (screen === "catalog") {
      workbenchHost.classList.remove("active");
    }
  };

  const playGame = (gameId: string, configOverride?: CasinoGameConfig<unknown>, seedOverride?: number | null): void => {
    const definition = registry.get(gameId);
    stopGame();
    const config = configOverride ?? storedConfigOf(definition);
    const seed = (seedOverride ?? rootSeed) >>> 0;
    title.textContent = definition.displayName;
    document.body.dataset["activeGame"] = gameId;
    paintGlyphBadge(el<HTMLCanvasElement>("game-marquee-glyph"), definition.thumbnail);
    showScreen("game");
    const script =
      url.presses.length === 0
        ? undefined
        : (tick: number, input: InputState): void => {
            for (const press of url.presses) {
              if (tick === press.at) {
                input.keyEvent(press.code, true);
              }
              if (tick === press.at + 3) {
                input.keyEvent(press.code, false);
              }
            }
          };
    running = definition.mount(canvas, {
      backend: url.backend,
      config,
      freezeAtTick: url.shot ?? undefined,
      onHud,
      pinnedNowMs: url.shot === null ? undefined : 120_000,
      round: 1,
      script,
      seed,
      settings: resolveSettings(settings, config.reducedMotion),
      source: new SeededChanceResultSource(seed),
    });
    detachPointer = attachPointerNormalizer(running.input);
    activeGameId = gameId;
  };

  // ── workbench ───────────────────────────────────────────────────
  const workbench = buildWorkbench(workbenchHost, {
    onClose: (): void => workbenchHost.classList.remove("active"),
    onPreview: (gameId, config, seed): void => {
      workbenchHost.classList.remove("active");
      playGame(gameId, config, seed);
    },
  });

  // ── settings panel ──────────────────────────────────────────────
  const renderSettings = (): void => {
    settingsHost.replaceChildren();
    const heading = document.createElement("h2");
    heading.textContent = "Operator Panel";
    settingsHost.append(heading);

    const plate = (legendText: string): HTMLFieldSetElement => {
      const fieldset = document.createElement("fieldset");
      const legend = document.createElement("legend");
      legend.textContent = legendText;
      fieldset.append(legend);
      settingsHost.append(fieldset);
      return fieldset;
    };

    const patch = (changes: Partial<PlayerSettings>): void => {
      settings = { ...settings, ...changes };
      saveSettings(settings);
      applyRootSettings();
      renderSettings();
      if (activeGameId !== null) {
        playGame(activeGameId);
      }
    };

    const slider = (label: string, value: number, apply: (v: number) => void): HTMLElement => {
      const row = document.createElement("div");
      row.className = "row";
      const l = document.createElement("label");
      l.textContent = label;
      const input = document.createElement("input");
      input.type = "range";
      input.min = "0";
      input.max = "1";
      input.step = "0.05";
      input.value = String(value);
      input.addEventListener("change", () => apply(Number(input.value)));
      row.append(l, input);
      return row;
    };
    const toggle = (label: string, value: boolean, apply: (v: boolean) => void): HTMLElement => {
      const row = document.createElement("div");
      row.className = "row";
      const l = document.createElement("label");
      l.textContent = label;
      const input = document.createElement("input");
      input.type = "checkbox";
      input.checked = value;
      input.addEventListener("change", () => apply(input.checked));
      row.append(l, input);
      return row;
    };
    const select = <T extends string>(label: string, value: T, options: readonly T[], apply: (v: T) => void): HTMLElement => {
      const row = document.createElement("div");
      row.className = "row";
      const l = document.createElement("label");
      l.textContent = label;
      const sel = document.createElement("select");
      for (const option of options) {
        const node = document.createElement("option");
        node.value = option;
        node.textContent = option;
        node.selected = option === value;
        sel.append(node);
      }
      sel.addEventListener("change", () => apply(sel.value as T));
      row.append(l, sel);
      return row;
    };

    plate("Audio — speaker grille").append(
      slider("Master volume", settings.masterVolume, (v) => patch({ masterVolume: v })),
      slider("Sound effects volume", settings.sfxVolume, (v) => patch({ sfxVolume: v })),
      toggle("Mute all sound", settings.muted, (v) => patch({ muted: v })),
    );
    plate("Motion & display").append(
      select("Reduced motion", settings.reducedMotion, ["system", "on", "off"], (v) => patch({ reducedMotion: v })),
      select("Particle density", settings.particleDensity, ["full", "low"], (v) => patch({ particleDensity: v })),
      toggle("Camera shake", settings.cameraShake, (v) => patch({ cameraShake: v })),
    );
    plate("Accessibility calibration").append(
      toggle("High-contrast UI", settings.highContrast, (v) => patch({ highContrast: v })),
      select("Text scale", settings.textScale, ["normal", "large"], (v) => patch({ textScale: v })),
    );

    const actions = document.createElement("div");
    actions.className = "actions";
    const close = document.createElement("button");
    close.textContent = "Close panel";
    close.className = "close";
    close.addEventListener("click", () => settingsHost.classList.remove("active"));
    actions.append(close);
    settingsHost.append(actions);
  };
  renderSettings();

  // ── catalog ─────────────────────────────────────────────────────
  buildCatalog(registry, el("filters"), el("cards"), {
    onConfigure: (gameId): void => workbench.open(registry.get(gameId)),
    onPlay: (gameId): void => playGame(gameId),
  });

  // ── chrome wiring ───────────────────────────────────────────────
  el("btn-back").addEventListener("click", () => {
    stopGame();
    showScreen("catalog");
  });
  el("btn-new-round").addEventListener("click", () => {
    if (running !== null) {
      pressSynthetic(running.input, "Synthetic:NewRound");
    }
  });
  el("btn-replay").addEventListener("click", () => {
    if (running !== null) {
      pressSynthetic(running.input, "Synthetic:Replay");
    }
  });
  el("btn-configure").addEventListener("click", () => {
    if (activeGameId !== null) {
      workbench.open(registry.get(activeGameId));
    }
  });
  const toggleMute = (): void => {
    settings = { ...settings, muted: !settings.muted };
    saveSettings(settings);
    applyRootSettings();
    if (activeGameId !== null) {
      playGame(activeGameId);
    }
  };
  el("btn-mute").addEventListener("click", toggleMute);
  el("btn-game-mute").addEventListener("click", toggleMute);
  const toggleSettings = (): void => {
    settingsHost.classList.toggle("active");
  };
  el("btn-settings").addEventListener("click", toggleSettings);
  el("btn-game-settings").addEventListener("click", toggleSettings);

  if (url.debug) {
    diagnosticsHost.classList.add("active");
  }
  applyRootSettings();

  // ── boot route ──────────────────────────────────────────────────
  if (url.game !== null && registry.has(url.game)) {
    if (url.workbench) {
      showScreen("catalog");
      workbench.open(registry.get(url.game));
    } else {
      playGame(url.game);
    }
  } else {
    showScreen("catalog");
  }
};
