/*
 * hud.ts — the pure HUD model the harness renders into the DOM overlay each frame.
 * SDK-free: it reads only the session's public accessors and returns plain data, so
 * the presentation layer (DOM in harness.ts, seven-segment digits in scene.ts) has
 * a single source of truth to draw from.
 */

import type { SwipeBasketballSession } from "./session.ts";

/** How recent a make still triggers the score-pop flourish (ticks). */
const SCORE_POP_TICKS = 42;

/** The immutable HUD snapshot for one frame. */
export interface HudModel {
  readonly title: string;
  readonly instruction: string;
  readonly score: number;
  readonly shots: number;
  /** True briefly after a made basket, so the DOM can pop the score. */
  readonly scorePop: boolean;
}

/** Build the HUD model from the current session state. */
export const readHudModel = (session: SwipeBasketballSession): HudModel => ({
  instruction: session.holding ? "Swipe up & release!" : "Drag a ball, swipe up, release",
  score: session.score,
  scorePop: session.ticksSinceScore < SCORE_POP_TICKS,
  shots: session.shots,
  title: "Swipe Basketball",
});
