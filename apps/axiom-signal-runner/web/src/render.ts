/*
 * The top-level frame composition. It fixes the draw order the brief calls for —
 * backdrop → mountains → storm → path → world objects → player → effects → UI — by
 * calling the world, player, and UI renderers in sequence (draw2d then layer-sorts
 * within that submission order). Pure presentation over a `State`; no game logic.
 */

import { type Hud, buildHud } from "./hud.ts";
import { renderPlayer, renderPulse } from "./render-player.ts";
import type { Frame } from "@axiom/game";
import type { State } from "./types.ts";
import { renderUi } from "./render-ui.ts";
import { renderWorld } from "./render-world.ts";

/** Draw one full frame of Signal Runner for `state`. */
export const renderGame = (frame: Frame, state: State): void => {
  const hud: Hud = buildHud(state);
  renderWorld(frame, state);
  renderPulse(frame, state);
  renderPlayer(frame, state);
  renderUi(frame, state, hud);
};
