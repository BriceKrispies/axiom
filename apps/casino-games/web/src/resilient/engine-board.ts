/*
 * engine-board.ts — the TOP OF THE LADDER: the shipped Treasure Chest Pick,
 * rendered by @axiom/web-engine, laid OVER the form that already worked.
 *
 * REUSE, NOT REIMPLEMENTATION. This mounts `TREASURE_CHEST_PICK` — the same
 * definition the arcade shell mounts, the same beach, the same nine carved
 * chests, the same spiral-to-hero flight and latch-first reveal ritual. There
 * is no second chest game here, because a second chest game is a chest game
 * that drifts.
 *
 * THE BACKBONE IS UNTOUCHED. Three rules, and every line below obeys them:
 *
 *   1. The nine `<button type="submit" name="pick" value="N">` controls stay in
 *      the DOM, stay enabled, stay focusable and stay inside the `<form>`. They
 *      are MOVED (into a positioning wrapper that is still a descendant of the
 *      fieldset) and repositioned, never replaced, disabled or rebuilt. Form
 *      serialization does not care where in the form a control sits.
 *   2. The canvas is `pointer-events: none` — set inline, not in a stylesheet,
 *      because it is load-bearing rather than cosmetic. Every click, tap, focus
 *      ring and Enter keypress lands on the button, exactly as in the baseline.
 *   3. The game is driven ONLY by what the form tells it. Immediately after
 *      mounting, every keyboard action the game binds is UNBOUND
 *      (`bindAction(name, [])`), so the engine's window-level key listeners can
 *      no longer reach a selection. Without that, pressing Enter on the focused
 *      button would submit chest N through the form AND select the game's own
 *      internally-focused chest — two controls, two answers, one board. Now the
 *      only input the game ever sees is the pointer sample this file feeds it,
 *      at the projected position of the chest the form just submitted.
 *
 * ONE POST PATH. Nothing here posts, fetches, or decides. `reveal()` is handed
 * the answer the page's single transport already fetched, and plays it through
 * `InjectedChanceResultSource` — the chance engine's existing "an authority
 * committed this; you may only animate it" boundary.
 *
 * THE FLIGHT IS DRIVEN BY THE ANSWER, NOT BY THE CLICK. `reveal()` opens
 * `response.picked` — the chest the SERVER says was opened — which is not
 * always the chest the button named. A repeat POST in the same round REPLAYS
 * the recorded pick (a reload, a double submit), because an outcome once
 * revealed cannot reroll; that is the server's commitment rule, and a page that
 * had already launched the requested chest on click would then be showing a
 * chest number the panel underneath it contradicts. Predicting the authority is
 * how a render starts lying, so this one does not: it waits the same-origin
 * round trip the whole build is designed around and animates what came back.
 *
 * SILENT BY CONSTRUCTION. The mount runs at zero volume, so `casino-mount.ts`
 * emits no tones and no `AudioContext` is ever created. This page is a form; it
 * has no mute control, and shipping unmutable audio behind one would be a
 * worse failure than shipping none.
 */

import type { Tier } from "@axiom/web-engine";
import { rendererTier } from "@axiom/web-engine";
import type { CasinoHud, PresentationSettings, RunningCasinoGame } from "../chance-engine/registry/definition.ts";
import { InjectedChanceResultSource } from "../chance-engine/outcomes/result-source.ts";
import { CHEST_REWARD_TIERS } from "../chest-round/round.ts";
import { COMMON_ACTIONS } from "../games/casino-mount.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH } from "../presentation/cameras/picking.ts";
import { TREASURE_CHEST_PICK } from "../games/treasure-chest-pick/definition.ts";
import { chestPlacements, type ChestPlacement } from "./board-layout.ts";
import type { PickResponse } from "./contract.ts";
import { injectedOutcomeOf } from "./injected-outcome.ts";

/**
 * The presentation seed for the IDLE board — the chest dance, the crab, the
 * palm. Fixed, not drawn: this build owns no entropy (the server draws and
 * records the one seed that decides anything), and an idle wobble must be
 * incapable of correlating with an outcome anyway. Once a round commits, the
 * reveal runs on the injected `presentationSeed` instead.
 */
const IDLE_SEED = 1;

/** How long a press is held before it is released, in ms. Both gaps clear
 * several 60 Hz ticks, so the arm → press → release edges the choice fold reads
 * land on distinct ticks however the frame budget is behaving. */
const PRESS_MS = 60;
const RELEASE_MS = 130;

/** A hard cap on waiting for the reveal animation before the outcome panel is
 * written anyway. The answer is already in hand at this point; a stalled render
 * loop (a backgrounded tab: `requestAnimationFrame` does not fire there) must
 * cost the animation, never the result. */
const REVEAL_CAP_MS = 9000;

/** A mounted engine board. Presentation only — it posts nothing. */
export interface EngineBoard {
  /** The tier the renderer actually came up at (not the tier that was asked
   * for: the engine walks DOWN if a backend fails to construct). */
  readonly tier: Tier;
  /** Highlight the chest under the cursor or the keyboard focus, or nothing. */
  readonly hover: (index: number | null) => void;
  /**
   * Play the authoritative outcome: `response.picked` lifts and spirals into
   * its close-up, the latch falls, the lid swings, and the reward the server
   * committed rises out of it. Resolves once the chest has actually opened and
   * the celebration has begun (or the cap above expires).
   */
  readonly reveal: (response: PickResponse) => Promise<void>;
  readonly stop: () => void;
}

/** What the board needs from the page. */
export interface EngineBoardOptions {
  /** The `<fieldset>` the nine buttons live in. */
  readonly host: HTMLElement;
  readonly buttons: readonly HTMLButtonElement[];
  readonly view: Window;
}

/** Player settings for a page that has no settings panel: silent, and honest
 * about the reduced-motion preference the OS already stated. */
const settingsFor = (view: Window): PresentationSettings => {
  const reduced = ((): boolean => {
    try {
      return view.matchMedia("(prefers-reduced-motion: reduce)").matches;
    } catch {
      return false;
    }
  })();
  return {
    cameraShake: !reduced,
    highContrast: false,
    masterVolume: 0,
    particleScale: reduced ? 0.4 : 1,
    reducedMotion: reduced,
    sfxVolume: 0,
  };
};

/** The chest game's own config, with the reward ladder swapped for the one the
 * SERVER decides against. `targetWinRate` is inert on this path — the injected
 * source plans no population, because the authority already did. */
const boardConfig = (): ReturnType<typeof TREASURE_CHEST_PICK.defaultConfig> => ({
  ...TREASURE_CHEST_PICK.defaultConfig(),
  rewardTiers: CHEST_REWARD_TIERS,
});

/** Move a button onto its chest. Layout is written INLINE because it is derived
 * (percentages computed from the projection) and because `pointer-events` and
 * absolute placement are the invariant, not decoration. */
const placeButton = (button: HTMLButtonElement, placement: ChestPlacement): void => {
  button.style.position = "absolute";
  button.style.left = `${placement.leftPct}%`;
  button.style.top = `${placement.topPct}%`;
  button.style.width = `${placement.widthPct}%`;
  button.style.height = `${placement.heightPct}%`;
  button.style.margin = "0";
  button.classList.add("is-engine");
};

/**
 * Mount the engine-rendered board over `host`'s buttons, or return null if it
 * could not be done. Null is a normal answer, not an error: the caller demotes
 * one rung and tries the CSS 3D chests instead.
 *
 * Nothing in the page is disturbed until the game has actually mounted — the
 * canvas goes in, the game is constructed, and only THEN are the buttons moved.
 * A throw at any point before that leaves the document exactly as served.
 */
export const mountEngineBoard = (options: EngineBoardOptions): EngineBoard | null => {
  const { buttons, host, view } = options;
  const doc = host.ownerDocument;
  const placements = chestPlacements(buttons.length);
  if (placements.length !== buttons.length) {
    return null;
  }

  const stage = doc.createElement("div");
  stage.className = "resilient-board3d";

  const canvas = doc.createElement("canvas");
  canvas.className = "resilient-canvas";
  canvas.width = CANVAS_WIDTH;
  canvas.height = CANVAS_HEIGHT;
  canvas.setAttribute("aria-hidden", "true");
  // The load-bearing half of "the button stays the control".
  canvas.style.pointerEvents = "none";
  canvas.style.display = "block";
  canvas.style.width = "100%";
  canvas.style.height = "auto";
  stage.append(canvas);
  // In the document BEFORE the mount: the chest game attaches its stylized-water
  // overlay to the canvas's parent, and silently skips it when there is none.
  host.append(stage);

  const source = new InjectedChanceResultSource();
  let latestHud: CasinoHud | null = null;
  let revealed: (() => void) | null = null;

  const onHud = (hud: CasinoHud): void => {
    latestHud = hud;
    const opened = hud.phase === "celebrating" || hud.phase === "complete";
    if (opened && revealed !== null) {
      const done = revealed;
      revealed = null;
      done();
    }
  };

  const running = ((): RunningCasinoGame | null => {
    try {
      return TREASURE_CHEST_PICK.mount(canvas, {
        // `backend` is deliberately absent: undefined means "auto", which runs
        // the engine's probed ladder and honours `?render=`.
        config: boardConfig(),
        onHud,
        round: 1,
        seed: IDLE_SEED,
        settings: settingsFor(view),
        source,
      });
    } catch {
      return null;
    }
  })();

  if (running === null) {
    stage.remove();
    return null;
  }

  // Rule 3: the form is the only control. Unbind every action the shared set
  // declares (the chest game adds none of its own), so no key the engine hears
  // at the window can reach a selection.
  Object.keys(COMMON_ACTIONS).forEach((action) => running.input.bindAction(action, []));

  const byIndex = new Map(placements.map((placement) => [placement.index, placement] as const));
  buttons.forEach((button, index) => {
    const placement = byIndex.get(index);
    if (placement !== undefined) {
      placeButton(button, placement);
      stage.append(button);
    }
  });
  host.classList.add("is-engine-board");

  let picked = false;
  const point = (placement: ChestPlacement, down: boolean): void => {
    running.input.pointerEvent(placement.pickX, placement.pickY, down);
  };

  return {
    hover: (index: number | null): void => {
      const placement = index === null ? undefined : byIndex.get(index);
      if (picked) {
        return;
      }
      if (placement === undefined) {
        running.input.pointerClear();
        return;
      }
      point(placement, false);
    },

    reveal: (response: PickResponse): Promise<void> =>
      new Promise<void>((done) => {
        const placement = byIndex.get(response.picked);
        if (picked || placement === undefined) {
          done();
          return;
        }
        picked = true;
        revealed = done;
        // The answer goes in FIRST, so it is already waiting when the session
        // reaches its commitment point a beat later. The chest still flies the
        // full spiral — that beat is `commitPauseTicks`, not a poll interval —
        // and nothing about the outcome can change once it is in this map.
        source.supply(latestHud?.round ?? 1, injectedOutcomeOf(response));
        // Arm → press → release: the three pointer edges the shared choice fold
        // resolves a selection from, fed AT the projected anchor of the chest
        // the server named, so the chest that opens is that chest.
        point(placement, false);
        view.setTimeout(() => point(placement, true), PRESS_MS);
        view.setTimeout(() => point(placement, false), RELEASE_MS);
        view.setTimeout(() => {
          if (revealed !== null) {
            revealed = null;
            done();
          }
        }, REVEAL_CAP_MS);
      }),

    stop: (): void => {
      running.stop();
      stage.remove();
    },

    tier: rendererTier() ?? "canvas2d",
  };
};
