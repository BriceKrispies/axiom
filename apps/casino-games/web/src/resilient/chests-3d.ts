/*
 * chests-3d.ts — TIER 4: the CSS 3D chest, dropped inside the form button.
 *
 * REUSE, NOT REIMPLEMENTATION. `buildChest` is the CSS 3D build's chest, taken
 * verbatim from `../css3d/scene/chest.ts` — the same 13-element transform tree,
 * the same hinged lid, the same gradients-instead-of-geometry plank seams, and
 * the same `styles/css3d.css` rules. Writing a second chest for this page would
 * have been the shortcut: two chests that must be kept looking alike is one
 * chest more than the engine needs.
 *
 * THE BUTTON STAYS THE CONTROL. The 3D scene is appended INSIDE each existing
 * `<button type="submit">` and marked `pointer-events: none`, so the browser
 * still hit-tests, focuses, keyboard-activates and submits the very same
 * element the baseline uses. Nothing about the form changes; only what is
 * painted on top of it does. That is the whole progressive-enhancement rule
 * applied to geometry: decorate the working thing, never replace it.
 *
 * A per-button `perspective` (see `.resilient-stage`) means nine independent
 * cameras rather than one shared world, which is what lets the chests sit in an
 * ordinary CSS grid instead of a hand-placed 3D board.
 */

import { buildChest, type ChestView } from "../css3d/scene/chest.ts";

/** One enhanced chest: the button it decorates and the solid inside it. */
export interface ChestDecoration {
  readonly button: HTMLButtonElement;
  readonly view: ChestView;
}

/**
 * Decorate every button with a 3D chest. `brandAt` marks which slot carries the
 * nameplate (the centre chest, as on the CSS 3D board).
 */
export const decorateChests = (buttons: readonly HTMLButtonElement[], brandAt: number, brand: string): readonly ChestDecoration[] =>
  buttons.map((button, index) => {
    const stage = document.createElement("div");
    stage.className = "resilient-stage";
    stage.setAttribute("aria-hidden", "true");

    const world = document.createElement("div");
    world.className = "resilient-world";

    const view = buildChest(0, 0, index === brandAt ? brand : null);
    view.pose(0, 0, 0);
    world.append(view.el);
    stage.append(world);
    button.append(stage);
    button.classList.add("is-3d");
    return { button, view };
  });

/**
 * The idle bob, driven by the round's AMBIENT stream on the server side of the
 * fence — here it is a plain time-and-index phase, because this page never sees
 * the round's seed before a pick and must not invent one. Nine style writes a
 * frame, and only while nothing is open.
 */
export const runIdle = (chests: readonly ChestDecoration[], view: Window, isFrozen: () => boolean): void => {
  const start = view.performance.now();
  const tick = (now: number): void => {
    const t = (now - start) / 1000;
    const frozen = isFrozen();
    chests.forEach((chest, index) => {
      const phase = index * 0.7;
      const alive = frozen ? 0 : 1;
      chest.view.pose(Math.sin(t * 1.15 + phase) * 3.2 * alive, 0, Math.sin(t * 0.85 + phase) * 1.1 * alive);
    });
    view.requestAnimationFrame(tick);
  };
  view.requestAnimationFrame(tick);
};
