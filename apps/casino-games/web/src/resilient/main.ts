/*
 * main.ts — SHELL BOUNDARY of the resilient build: the only file that knows the
 * page exists, and the only one that climbs the ladder.
 *
 * ONE LADDER, TOP TO BOTTOM, ON ONE PAGE:
 *
 *     webgpu → webgl2 → webgl1 → canvas2d → css3d → form
 *
 * The bottom rung is the document itself — a real `<form method="POST">` with
 * nine submit buttons that decides a round with zero JavaScript and zero CSS.
 * Everything this file does is layered OVER that, and the form is never
 * removed, disabled, rebuilt or replaced: the engine's canvas is inserted
 * BEHIND the buttons with `pointer-events: none`, and the CSS 3D chests are
 * built INSIDE them. The failure mode of every line below is "the page behaves
 * like the baseline", which is a good failure mode and the reason the build is
 * shaped this way.
 *
 * WHERE EACH RUNG COMES FROM. The render tier is `@axiom/web-engine`'s
 * `detectTier()` — a real probed cascade that PAINTS a known pattern on each
 * rung and classifies the pixels, so a context that exists but renders nothing
 * is rejected. `?render=<tier>` forces any rung (the engine reads it; this page
 * does not re-parse it). `css3d` and above are drawn; the form is what is left
 * when nothing above it mounted.
 *
 * THE ONE HARD PART is still the native fallback. Intercepting a submit means
 * calling `preventDefault()` BEFORE we know whether the post will work — the
 * answer is asynchronous and the event is not. So the shell always prevents,
 * tries the transports, and on failure re-submits the form itself with the
 * pressed chest carried in a hidden input. (`form.submit()` does not include
 * the pressed button's name/value; nothing does, unless you put it there.) The
 * player sees a native navigation to a server-rendered page — the baseline — a
 * moment later than they would have, and nothing else differs.
 *
 * ONE POST PATH AT EVERY RUNG. The engine-rendered board posts nothing: it is
 * handed the answer this file already fetched. There is exactly one
 * `POST /api/pick` in this build, and it is below.
 *
 * DRAWS NO ENTROPY. Unlike the app's other two shells, this one owns no seed:
 * the server draws and records it, because the server is what decides an
 * outcome here. `crypto.getRandomValues` appears nowhere in this build.
 */

import type { DetectionReport } from "@axiom/web-engine";
import { detectTier } from "@axiom/web-engine";
import { decorateChests, runIdle, type ChestDecoration } from "./chests-3d.ts";
import { PICK_ENDPOINT, PICK_FIELD, parsePick, type PickResponse } from "./contract.ts";
import { mountEngineBoard, type EngineBoard } from "./engine-board.ts";
import { describeOutcome } from "./outcome.ts";
import { FORM_TIER, chooseTier, demoteRender, postsInPlace, probeCss, rungFor, type PageTier } from "./tier.ts";
import { postInPlace } from "./transport.ts";

/** The centre chest carries the nameplate, as on the CSS 3D board. */
const BRAND_SLOT = 4;
const BRAND = "ACME";

declare global {
  interface Window {
    __axiomTier?: PageTier;
    __axiomReady?: boolean;
    __renderProbe?: DetectionReport;
  }
}

/**
 * Publish the tier three ways: on `window` for an in-page assertion, as a data
 * attribute so CSS and a screenshot can see it, and by `postMessage` so a
 * harness iframe learns it without reaching across a document boundary.
 *
 * It is published more than once on purpose — the tier is a claim about what
 * WORKS, and an enhancement that fails to mount, or a transport that fails at
 * request time, falsifies the claim made at load. A harness that only ever
 * hears the optimistic first value is being misled, so every downgrade is
 * announced too.
 */
const publishTier = (view: Window, tier: PageTier): void => {
  view.__axiomTier = tier;
  view.document.documentElement.dataset["axiomTier"] = tier;
  try {
    view.parent.postMessage({ axiomTier: tier }, "*");
  } catch {
    // A harness may not exist, or may be cross-origin with a hostile policy.
    // Neither is a reason for the game to stop working.
  }
};

/** Run the engine's probed ladder. It is capped and self-contained, but it is
 * still the largest piece of foreign code this page runs before it has a
 * picture, so a throw there costs the render tier and nothing else. */
const detectSafely = async (): Promise<DetectionReport | null> => {
  try {
    return await detectTier();
  } catch {
    return null;
  }
};

const boot = async (): Promise<void> => {
  const doc = document;
  const form = doc.getElementById("pick-form") as HTMLFormElement | null;
  const fieldset = doc.getElementById("chest-board");
  const outcomeEl = doc.getElementById("outcome");
  const againForm = doc.getElementById("new-form") as HTMLFormElement | null;
  if (form === null || fieldset === null || outcomeEl === null || againForm === null) return;

  const buttons = [...form.querySelectorAll<HTMLButtonElement>("button.resilient-chest")];

  // ── the ladder ────────────────────────────────────────────────────────────
  // Published as `form` first and upgraded once something has actually mounted:
  // at this instant the page IS the served document, and saying anything better
  // would be a promise rather than a report.
  let tier: PageTier = FORM_TIER;
  publishTier(window, tier);

  const detection = await detectSafely();
  if (detection !== null) {
    window.__renderProbe = detection;
  }
  // No report at all means the probe itself died; `css3d` is the engine's own
  // terminal rung and needs no drawing context, so it is the honest floor.
  let candidate = chooseTier({ cssApplied: probeCss(window), renderTier: detection?.tier ?? "css3d" });

  let board: EngineBoard | null = null;
  let decorations: readonly ChestDecoration[] = [];

  // Walk DOWN from the candidate rung until one mounts. Every attempt is
  // wrapped: an enhancement that throws costs its rung, never the page.
  while (rungFor(candidate) !== "none" && board === null && decorations.length === 0) {
    try {
      if (rungFor(candidate) === "engine") {
        board = mountEngineBoard({ buttons, host: fieldset, view: window });
      } else {
        decorations = decorateChests(buttons, BRAND_SLOT, BRAND);
      }
    } catch {
      board = null;
      decorations = [];
    }
    // The engine reports the rung it actually came up at, which can be lower
    // than the one it was asked for (a backend that failed to construct).
    tier = board !== null ? board.tier : decorations.length > 0 ? candidate : FORM_TIER;
    candidate = demoteRender(candidate);
  }
  publishTier(window, tier);

  let opened: number | null = null;
  if (decorations.length > 0) runIdle(decorations, window, () => opened !== null);

  // ── remember which chest was pressed
  //
  // `SubmitEvent.submitter` is the right answer and is not everywhere, so the
  // press is recorded on `click`, which fires first and has existed forever.
  let pressed: number | null = null;
  buttons.forEach((button, index) => {
    button.addEventListener("click", () => {
      pressed = parsePick(button.value, buttons.length);
    });
    // Mirror the DOM's own hover/focus into the rendered board, so the chest
    // the browser thinks you are on is the chest that lights up. Read-only —
    // it can arm a highlight, never a selection.
    button.addEventListener("pointerenter", () => board?.hover(index));
    button.addEventListener("focus", () => board?.hover(index));
    button.addEventListener("pointerleave", () => board?.hover(null));
    button.addEventListener("blur", () => board?.hover(null));
  });

  const showOutcome = (response: PickResponse): void => {
    const copy = describeOutcome(response);
    outcomeEl.textContent = "";

    const headline = doc.createElement("h2");
    headline.className = `resilient-headline ${copy.won ? "is-win" : "is-loss"}`;
    headline.textContent = copy.headline;

    const detail = doc.createElement("p");
    detail.className = "resilient-prize";
    detail.textContent = copy.detail;

    const board3 = doc.createElement("ul");
    board3.className = "resilient-board-list";
    copy.board.forEach((line) => {
      const item = doc.createElement("li");
      item.textContent = line;
      board3.append(item);
    });

    const facts = doc.createElement("p");
    facts.className = "resilient-sub";
    facts.textContent = copy.facts;

    outcomeEl.append(headline, detail, board3, facts);
    outcomeEl.hidden = false;
    againForm.hidden = false;

    opened = response.picked;
    buttons.forEach((button, index) => {
      button.disabled = true;
      button.classList.toggle("is-open", index === response.picked);
      button.classList.toggle("is-win", index === response.picked && response.won);
    });
    const chest = decorations[response.picked];
    if (chest !== undefined) {
      chest.view.open(1);
      // A loss shows an OPEN, EMPTY chest. The css3d build puts a dim marker in
      // the hole; here the outcome panel right below already says "empty", and a
      // gold orb under that sentence reads as a prize you did not get.
      window.setTimeout(() => chest.view.setPrize(response.won ? "★" : null, response.won), 260);
    }
  };

  /** Hand the pick back to the browser: append what `form.submit()` would
   * otherwise drop, then navigate. This is the baseline, reached late. */
  const submitNatively = (index: number): void => {
    const carried = doc.createElement("input");
    carried.type = "hidden";
    carried.name = PICK_FIELD;
    carried.value = String(index);
    form.append(carried);
    form.submit();
  };

  form.addEventListener("submit", (event) => {
    const index = pressed;
    if (!postsInPlace(tier) || index === null) return; // let the browser do it
    event.preventDefault();

    void postInPlace(PICK_ENDPOINT, { [PICK_FIELD]: index }, {
      fetchImpl: (window as { fetch?: unknown }).fetch,
      xhrCtor: (window as { XMLHttpRequest?: unknown }).XMLHttpRequest,
    }).then(async (outcome) => {
      if (outcome.kind === "ok") {
        // Play the answer on the chest the SERVER says was opened — which is
        // not always the one just pressed: a repeat POST in the same round
        // replays the recorded pick. Then let the chest finish opening before
        // the panel spells out what was in it. With no rendered board there is
        // nothing to wait for.
        await (board?.reveal(outcome.body) ?? Promise.resolve());
        showOutcome(outcome.body);
        return;
      }
      // Both transports refused. Say so — a harness watching the tier is the
      // only diagnostic available in the browser this targets — and fall all
      // the way back to the form the page shipped with.
      tier = FORM_TIER;
      publishTier(window, tier);
      submitNatively(index);
    });
  });

  window.__axiomReady = true;
  publishTier(window, tier);
};

void boot();
