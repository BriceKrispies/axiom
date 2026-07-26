/*
 * main.ts — LAYER 4 of the CSS3D build: the shell that binds rules to elements.
 *
 * It owns the only impure things in the build: the seed drawn once at the app
 * boundary, DOM events, and a requestAnimationFrame idle loop. Everything it
 * decides it asks `game/round.ts` (the real chance engine); everything it draws
 * it asks `scene/` (CSS 3D solids). No canvas is created anywhere in this app.
 *
 * PICKING IS FREE. The engine build has to raycast a pointer into the scene to
 * find which chest was clicked. Here the chests ARE elements, so a chest click
 * is a plain DOM `click` listener — the browser already hit-tests 3D-transformed
 * elements for us. That is the one place where the DOM renderer is not merely
 * competitive with the canvas one but strictly simpler.
 *
 * THE FRAME BUDGET. Only the nine chest wrappers are re-posed per frame (nine
 * style writes); the ~40-element diorama is static and the lid/prize animations
 * are CSS transitions running on the compositor. That is what keeps this at
 * 60fps where a faithful DOM port of the full engine scene sits at ~5.
 */

import { buildDiorama } from "./scene/diorama.ts";
import { buildChest, CHEST, type ChestView } from "./scene/chest.ts";
import { CHEST_COUNT, type PickResult, type Round, startRound } from "./game/round.ts";

const COLS = 3;
const SPACING_X = 146;
const SPACING_Y = 136;
/** Ticks (at 60Hz) the lid takes to finish opening before the prize shows. */
const REVEAL_DELAY_MS = 260;

const el = <T extends HTMLElement>(id: string): T => {
  const found = document.getElementById(id);
  if (found === null) throw new Error(`css3d: missing #${id}`);
  return found as T;
};

/** One seed, drawn once at the outermost boundary and recorded — exactly the
 * discipline the source shell follows. `?seed=N` pins it for reproducible runs. */
const initialSeed = (): number => {
  const fromUrl = new URLSearchParams(location.search).get("seed");
  if (fromUrl !== null && Number.isFinite(Number(fromUrl))) return Number(fromUrl) >>> 0;
  const buf = new Uint32Array(1);
  crypto.getRandomValues(buf);
  return (buf[0] ?? 1) >>> 0;
};

interface State {
  round: Round;
  seed: number;
  roundNumber: number;
  winRate: number;
  focused: number;
  picked: number | null;
  result: PickResult | null;
}

const boot = (): void => {
  const world = el("world");
  const banner = el("banner");
  const hudSeed = el("hud-seed");
  const hudPop = el("hud-pop");
  const hudResult = el("hud-result");
  const rate = el<HTMLInputElement>("rate");
  const rateOut = el("rate-out");

  world.append(buildDiorama());

  // ── the nine chests
  const chests: ChestView[] = [];
  const board = document.createElement("div");
  board.className = "s board";
  for (let i = 0; i < CHEST_COUNT; i += 1) {
    const col = i % COLS;
    const row = Math.floor(i / COLS);
    const slotX = (col - 1) * SPACING_X;
    const slotY = (row - 1) * SPACING_Y;
    const chest = buildChest(slotX, slotY, i === 4 ? "ACME" : null);
    chest.el.dataset["index"] = String(i);
    chest.el.setAttribute("role", "button");
    chest.el.setAttribute("tabindex", "-1");
    chest.el.setAttribute("aria-label", `Chest ${i + 1}`);
    chests.push(chest);
    board.append(chest.el);
  }
  world.append(board);

  const seed = initialSeed();
  const state: State = {
    focused: 4,
    picked: null,
    result: null,
    round: startRound(seed, 1, 0.44),
    roundNumber: 1,
    seed,
    winRate: 0.44,
  };

  const syncHud = (): void => {
    hudSeed.textContent = `seed ${state.seed} · round ${state.roundNumber}`;
    hudPop.textContent = `${state.round.winnerCount} of ${CHEST_COUNT} chests hold a prize`;
    rateOut.textContent = `${Math.round(state.winRate * 100)}%`;
    const issues = state.round.issues;
    hudPop.classList.toggle("is-bad", issues.length > 0);
  };

  const syncChests = (): void => {
    chests.forEach((chest, i) => {
      chest.setFocused(state.picked === null && i === state.focused);
      chest.setDimmed(state.picked !== null && i !== state.picked);
    });
  };

  const pick = (index: number): void => {
    if (state.picked !== null) return;
    state.picked = index;
    const result = state.round.reveal(index);
    state.result = result;
    chests[index]?.open(1);
    syncChests();
    banner.className = result.won ? "is-shown is-win" : "is-shown is-loss";
    banner.textContent = result.won ? `${result.tier?.label} — ${result.label}!` : "Empty — try another round";
    hudResult.textContent = result.won ? `WIN · ${result.tier?.rarity} · ${result.label}` : "LOSS · empty chest";
    window.setTimeout(() => {
      chests[index]?.setPrize(result.won ? "★" : "·", result.won);
    }, REVEAL_DELAY_MS);
  };

  const newRound = (sameSeed: boolean): void => {
    state.roundNumber = sameSeed ? state.roundNumber : state.roundNumber + 1;
    state.round = startRound(state.seed, state.roundNumber, state.winRate);
    state.picked = null;
    state.result = null;
    chests.forEach((chest) => {
      chest.open(0);
      chest.setPrize(null, false);
    });
    banner.className = "";
    banner.textContent = "";
    hudResult.textContent = "—";
    syncChests();
    syncHud();
  };

  // ── input: a chest is an element, so picking is a plain click listener
  board.addEventListener("click", (event) => {
    const target = (event.target as HTMLElement).closest<HTMLElement>(".chest");
    const index = target?.dataset["index"];
    if (index !== undefined) pick(Number(index));
  });
  board.addEventListener("pointermove", (event) => {
    if (state.picked !== null) return;
    const target = (event.target as HTMLElement).closest<HTMLElement>(".chest");
    const index = target?.dataset["index"];
    if (index !== undefined) {
      state.focused = Number(index);
      syncChests();
    }
  });

  const STEP: Readonly<Record<string, number>> = {
    ArrowDown: COLS,
    ArrowLeft: -1,
    ArrowRight: 1,
    ArrowUp: -COLS,
  };
  window.addEventListener("keydown", (event) => {
    const delta = STEP[event.code];
    if (delta !== undefined) {
      event.preventDefault();
      state.focused = Math.max(0, Math.min(CHEST_COUNT - 1, state.focused + delta));
      syncChests();
      return;
    }
    if (event.code === "Enter" || event.code === "Space") {
      event.preventDefault();
      if (state.picked === null) pick(state.focused);
      else newRound(false);
    }
  });

  el("btn-new").addEventListener("click", () => newRound(false));
  el("btn-replay").addEventListener("click", () => newRound(true));
  rate.addEventListener("input", () => {
    state.winRate = Number(rate.value) / 100;
    newRound(true);
  });

  // ── idle loop: nine style writes a frame, nothing else
  const start = performance.now();
  let frames = 0;
  let fpsMark = start;
  const hudFps = el("hud-fps");
  const tick = (now: number): void => {
    const t = (now - start) / 1000;
    chests.forEach((chest, i) => {
      const phase = state.round.ambient(i) * Math.PI * 2;
      const alive = state.picked === null || state.picked === i ? 1 : 0.25;
      const bob = Math.sin(t * 1.15 + phase) * 3.2 * alive;
      const lift = (state.picked === i ? 26 : 0) + (state.picked === null && state.focused === i ? 9 : 0);
      const twist = Math.sin(t * 0.85 + phase) * 1.1 * alive;
      chest.pose(bob, lift, twist);
    });
    frames += 1;
    if (now - fpsMark > 500) {
      hudFps.textContent = `${Math.round((frames * 1000) / (now - fpsMark))} fps · ${document.querySelectorAll("#world i").length} elements`;
      frames = 0;
      fpsMark = now;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  // Keep the board centred on the chest grid regardless of chest depth.
  board.style.transform = `translate3d(0px,${(CHEST.d * 0.1).toFixed(1)}px,2px)`;
  syncChests();
  syncHud();
};

boot();
