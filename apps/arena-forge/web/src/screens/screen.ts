/*
 * screen.ts — the application screen contract. Arena Forge has exactly three
 * top-level screens — `main_menu`, `gameplay`, `figure_lab` — and the app shell
 * (`../game.ts`) owns which one is active, routing the harness's per-frame
 * update/render/pointer callbacks to it. Screens never navigate directly; they
 * ask the shell via `ScreenNav.goto`, so transitions stay centralized (no
 * scattered booleans). Each screen has an explicit lifecycle: `enter` builds its
 * 3D scene + state, `exit` releases per-screen engine resources. The DATA figure
 * prototype cache (`figureForCard`) is module-level and survives screen switches;
 * only the engine's GPU scene is rebuilt per screen.
 */

import { clearScene } from "@axiom/web-engine";
import { resetMeshCache } from "../figures/primitives.ts";
import { resetMaterialCache } from "../figures/scene/materials.ts";

export type ScreenState = "main_menu" | "gameplay" | "figure_lab";

/** How a screen requests navigation from the shell. */
export interface ScreenNav {
  goto(state: ScreenState): void;
}

/** One top-level application screen. */
export interface Screen {
  /** Build the 3D scene and screen state. */
  enter(): void;
  /** One fixed simulation tick (may be a no-op for static screens). */
  update(): void;
  /** Draw the base 3D canvas (then `render` draws the 2D overlay). */
  renderScene3D(): void;
  /** Draw the 2D overlay in CSS pixels. */
  render(ctx: CanvasRenderingContext2D, w: number, h: number): void;
  onPointerDown(x: number, y: number): void;
  onPointerMove(x: number, y: number): void;
  onPointerUp(x: number, y: number): void;
  /** Desktop wheel zoom (optional; positive delta = zoom out). */
  onWheel?(deltaY: number): void;
  /** Mobile pinch zoom (optional; `factor` = current/previous finger distance). */
  onPinch?(factor: number): void;
  /** A keyboard key (optional; a printable character, or a named key such as
   * `Backspace`/`Escape`/`Enter`). The platform edge filters modifiers out. */
  onKey?(key: string): void;
  /** Release per-screen engine resources (nodes/materials/meshes). */
  exit(): void;
}

/** Drop the shared engine scene + its mesh/material caches. Called on screen
 * enter to start from a clean, bounded GPU state. The pure-data figure prototype
 * cache is NOT touched. */
export const resetEngineScene = (): void => {
  clearScene();
  resetMeshCache();
  resetMaterialCache();
};

export const performanceNow = (): number => (typeof performance !== "undefined" ? performance.now() : 0);
