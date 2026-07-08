/*
 * The player: a small red hooded courier riding a glowing hover sled, drawn from
 * behind near the lower-centre of the screen. Deliberately NOT a humanoid rig —
 * just procedural draw2d shapes (hood, cape trapezoid, emblem, little legs, board
 * disc, skid + motion lines) with simple pose offsets: a turn-lean shear, a gentle
 * bob, and a fluttering cape hem. All derived deterministically from the runner's
 * state so a screenshot at a given tick is reproducible.
 */

import * as P from "./palette.ts";
import { HEIGHT, WIDTH } from "./constants.ts";
import { box, disc, poly, seg } from "./draw.ts";
import type { Frame, Vec2 } from "@axiom/game";
import type { State } from "./types.ts";

const PLAYER_Y = HEIGHT * 0.8;
const SWING = WIDTH * 0.26;
const SCALE = 1.05;

const L_MOTION = 56;
const L_GLOW = 57;
const L_DISC = 58;
const L_BODY = 60;

/** Screen-x of the courier from the runner's clamped lateral offset. */
const playerX = (state: State): number => {
  const width = 250;
  const t = Math.max(-1.4, Math.min(1.4, state.runner.lateral / width));
  return WIDTH / 2 + t * SWING;
};

/** Render the courier + sled + motion for `state`. */
export const renderPlayer = (frame: Frame, state: State): void => {
  const r = state.runner;
  const px = playerX(state);
  const bob = Math.sin(state.tick * 0.18) * 3;
  const py = PLAYER_Y + bob;
  const sc = SCALE;
  const lean = Math.max(-1, Math.min(1, r.lean));
  const flutter = Math.sin(state.tick * 0.3);
  // A crash flash blinks the whole courier.
  const flash = r.invulnTicks > 0 && Math.floor(state.tick / 4) % 2 === 0;
  const bodyA = flash ? 0.35 : 1;

  // Shear a point by the turn-lean: higher points lean further.
  const tilt = (x: number, y: number): Vec2 => ({ x: x + lean * (py - y) * 0.22, y });

  // Motion lines streaming out behind the sled.
  const streak = 40 + Math.min(60, r.speed * 0.24);
  for (let i = -2; i <= 2; i += 1) {
    const sx = px + i * 12 * sc;
    seg(frame, { x: sx, y: py + 20 * sc }, { x: sx - lean * 16, y: py + 20 * sc + streak }, P.SKID, 2.5, L_MOTION, 0.6);
  }

  // Hover glow + board disc.
  disc(frame, px, py + 20 * sc, 60 * sc, P.alpha(P.SLED_GLOW, 0.35), L_GLOW);
  frame.ellipse({ x: px, y: py + 18 * sc }, { rx: 62 * sc, ry: 18 * sc }, { fill: P.SLED, layer: L_DISC, stroke: P.OUTLINE, strokeWidth: 3 });
  frame.ellipse({ x: px, y: py + 15 * sc }, { rx: 46 * sc, ry: 11 * sc }, { fill: P.alpha(P.SLED_GLOW, 0.5), layer: L_DISC });

  // Little legs peeking below the cape.
  for (const s of [-1, 1]) {
    box(frame, px + s * 12 * sc - 4 * sc, py - 8 * sc, 8 * sc, 18 * sc, P.SKIN, L_BODY, undefined, 0, bodyA);
  }

  // Cape / poncho: a fluttering red trapezoid from the shoulders to a wide hem.
  const shoulderY = py - 92 * sc;
  const hemY = py - 4 * sc;
  const cape: Vec2[] = [
    tilt(px - 20 * sc, shoulderY),
    tilt(px + 20 * sc, shoulderY),
    tilt(px + 40 * sc + flutter * 4, hemY),
    tilt(px + 14 * sc, hemY + 6 + flutter * 3),
    tilt(px - 14 * sc, hemY + 6 - flutter * 3),
    tilt(px - 40 * sc - flutter * 4, hemY),
  ];
  poly(frame, cape, P.CAPE, L_BODY, P.HOOD_DARK, 2.5, bodyA);

  // The gold emblem (three stacked chevrons) on the courier's back.
  const emY = py - 58 * sc;
  for (let i = 0; i < 3; i += 1) {
    const y = emY + i * 9 * sc;
    const wCh = (12 - i * 2) * sc;
    poly(
      frame,
      [tilt(px - wCh, y), tilt(px, y - 8 * sc), tilt(px + wCh, y), tilt(px, y - 3 * sc)],
      P.CAPE_GLYPH,
      L_BODY,
      undefined,
      0,
      bodyA,
    );
  }

  // Hood: a rounded red cowl with a dark face opening, tilted with the lean.
  const headTop = py - 150 * sc;
  const hood: Vec2[] = [
    tilt(px, headTop),
    tilt(px + 24 * sc, py - 118 * sc),
    tilt(px + 22 * sc, shoulderY + 4 * sc),
    tilt(px - 22 * sc, shoulderY + 4 * sc),
    tilt(px - 24 * sc, py - 118 * sc),
  ];
  poly(frame, hood, P.HOOD, L_BODY, P.HOOD_DARK, 2.5, bodyA);
  disc(frame, tilt(px, py - 120 * sc).x, py - 120 * sc, 12 * sc, P.SKIN, L_BODY, undefined, 0, bodyA);

  // A shield bubble when the shield ability is up.
  if (r.shieldTicks > 0) {
    disc(frame, px, py - 66 * sc, 78 * sc, P.alpha(P.SLED_GLOW, 0.14), L_BODY);
    disc(frame, px, py - 66 * sc, 78 * sc, P.alpha(P.SLED_GLOW, 0), L_BODY, P.SLED_GLOW, 2, 0.7);
  }
};

/** Draw an expanding pulse ring from the courier (called while a pulse is fresh). */
export const renderPulse = (frame: Frame, state: State): void => {
  const cd = state.ability.pulseCd;
  if (cd === 0) {
    return;
  }
  const px = playerX(state);
  const py = PLAYER_Y;
  const age = 1 - cd / 60;
  disc(frame, px, py - 40, 40 + age * 320, P.alpha(P.SLED_GLOW, (1 - age) * 0.5), 62, P.SLED_GLOW, 3, (1 - age) * 0.8);
};
