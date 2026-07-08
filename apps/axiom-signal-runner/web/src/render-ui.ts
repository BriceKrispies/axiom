/*
 * The screen-space HUD: the reference's clean white rounded panels with dark
 * outlines and simple procedural icons — the objective panel + checklist, the centre
 * timer, the storm status, the right-side minimap, the controls legend, the
 * speed/charge readout, the four ability cards, and the win/lose banner. Every value
 * comes from the pure `Hud` model (hud.ts); this file only draws. Monospace text is
 * the engine's built-in font (the SDK's only draw2d font).
 */

import * as P from "./palette.ts";
import { type Hud, formatTimer } from "./hud.ts";
import { WIDTH } from "./constants.ts";
import { box, diamond, disc, label, panel, poly, seg, textWidth } from "./draw.ts";
import type { AbilityKind } from "./types.ts";
import type { Frame } from "@axiom/game";
import type { State } from "./types.ts";

const L_PANEL = 100;
const L_ICON = 102;
const L_TEXT = 104;
const R = 14; // panel corner radius

const pnl = (frame: Frame, x: number, y: number, w: number, h: number): void => {
  panel(frame, x, y, w, h, R, P.PANEL, P.PANEL_EDGE, L_PANEL, 3);
};

/** A small rounded key-cap with a centred glyph. */
const keycap = (frame: Frame, x: number, y: number, w: number, text: string): void => {
  panel(frame, x, y, w, 26, 6, P.PANEL, P.PANEL_INK, L_ICON, 2);
  label(frame, text, x + w / 2, y + 6, 15, P.PANEL_INK, L_TEXT, "center");
};

// ── Icons ───────────────────────────────────────────────────────────────────

const relayIcon = (frame: Frame, cx: number, cy: number): void => {
  seg(frame, { x: cx - 12, y: cy + 14 }, { x: cx, y: cy - 12 }, P.PANEL_INK, 3, L_ICON);
  seg(frame, { x: cx + 12, y: cy + 14 }, { x: cx, y: cy - 12 }, P.PANEL_INK, 3, L_ICON);
  seg(frame, { x: cx - 8, y: cy + 5 }, { x: cx + 8, y: cy + 5 }, P.PANEL_INK, 3, L_ICON);
  diamond(frame, cx, cy - 14, 6, 8, P.SHARD, L_ICON, P.PANEL_INK, 2);
};

const stormIcon = (frame: Frame, cx: number, cy: number): void => {
  disc(frame, cx - 8, cy, 9, P.PANEL_MUTE, L_ICON);
  disc(frame, cx + 6, cy - 2, 11, P.PANEL_MUTE, L_ICON);
  disc(frame, cx + 2, cy + 4, 9, P.PANEL_MUTE, L_ICON);
  poly(frame, [{ x: cx, y: cy + 2 }, { x: cx + 8, y: cy + 2 }, { x: cx + 1, y: cy + 16 }, { x: cx + 5, y: cy + 6 }, { x: cx - 3, y: cy + 6 }], P.PLATE, L_TEXT, P.PLATE_EDGE, 1);
};

const abilityIcon = (frame: Frame, kind: AbilityKind, cx: number, cy: number, ink: typeof P.PANEL_INK): void => {
  if (kind === "boost") {
    for (const dy of [4, -4]) {
      poly(frame, [{ x: cx - 12, y: cy + dy + 6 }, { x: cx, y: cy + dy - 6 }, { x: cx + 12, y: cy + dy + 6 }, { x: cx, y: cy + dy }], ink, L_ICON);
    }
  } else if (kind === "shield") {
    poly(frame, [{ x: cx, y: cy - 12 }, { x: cx + 12, y: cy - 6 }, { x: cx + 9, y: cy + 12 }, { x: cx, y: cy + 15 }, { x: cx - 9, y: cy + 12 }, { x: cx - 12, y: cy - 6 }], ink, L_ICON);
  } else if (kind === "pulse") {
    seg(frame, { x: cx - 14, y: cy }, { x: cx - 6, y: cy }, ink, 3, L_ICON);
    seg(frame, { x: cx - 6, y: cy }, { x: cx - 2, y: cy - 12 }, ink, 3, L_ICON);
    seg(frame, { x: cx - 2, y: cy - 12 }, { x: cx + 3, y: cy + 12 }, ink, 3, L_ICON);
    seg(frame, { x: cx + 3, y: cy + 12 }, { x: cx + 7, y: cy }, ink, 3, L_ICON);
    seg(frame, { x: cx + 7, y: cy }, { x: cx + 14, y: cy }, ink, 3, L_ICON);
  } else {
    for (const s of [-1, 1]) {
      seg(frame, { x: cx + s * 6, y: cy - 2 }, { x: cx + s * 15, y: cy - 8 }, ink, 3, L_ICON);
    }
    disc(frame, cx, cy, 8, ink, L_ICON);
    disc(frame, cx, cy, 3.5, P.DRONE_CORE, L_ICON);
  }
};

// ── Panels ───────────────────────────────────────────────────────────────────

const drawObjective = (frame: Frame, hud: Hud): void => {
  pnl(frame, 24, 24, 322, 60);
  relayIcon(frame, 58, 54);
  label(frame, hud.objectiveTitle, 86, 40, 27, hud.beaconReady ? P.READY : P.PANEL_INK, L_TEXT);

  pnl(frame, 24, 96, 360, 116);
  const rows: [typeof P.SHARD, string, string, boolean][] = [
    [P.SHARD, "Collect Signal Shards", `${hud.shards} / ${hud.shardGoal}`, hud.shards >= hud.shardGoal],
    [P.PLATE, "Activate Pressure Plates", `${hud.plates} / ${hud.plateGoal}`, hud.plates >= hud.plateGoal],
    [P.SEG_OFF, "Restore the Beacon", "", hud.beaconRestored],
  ];
  rows.forEach(([color, text, count, done], i) => {
    const y = 118 + i * 32;
    diamond(frame, 48, y + 8, 9, 12, done ? P.alpha(color, 0.4) : color, L_ICON, P.PANEL_INK, 2);
    label(frame, text, 68, y, 18, done ? P.PANEL_MUTE : P.PANEL_INK, L_TEXT);
    label(frame, count, 368, y, 18, P.PANEL_INK, L_TEXT, "right");
  });
};

const drawTimer = (frame: Frame, hud: Hud): void => {
  const w = 200;
  const x = (WIDTH - w) / 2;
  pnl(frame, x, 24, w, 60);
  label(frame, hud.timer, x + w / 2, 38, 38, P.PANEL_INK, L_TEXT, "center");
};

const drawStorm = (frame: Frame, hud: Hud): void => {
  const w = 250;
  const x = WIDTH - 24 - w;
  pnl(frame, x, 24, w, 60);
  stormIcon(frame, x + 34, 52);
  label(frame, hud.stormLabel, x + 66, 40, 24, hud.stormIntensity > 0.66 ? P.STORM : P.PANEL_INK, L_TEXT);
};

const drawMinimap = (frame: Frame, hud: Hud): void => {
  const w = 150;
  const h = 250;
  const x = WIDTH - 24 - w;
  const y = 100;
  pnl(frame, x, y, w, h);
  const cx = x + w / 2;
  const topY = y + 22;
  const botY = y + h - 22;
  const routeX = (t: number): number => cx + Math.sin(t * 9) * w * 0.16;
  const routeY = (t: number): number => botY - t * (botY - topY);
  const line: { x: number; y: number }[] = [];
  for (let k = 0; k <= 20; k += 1) {
    line.push({ x: routeX(k / 20), y: routeY(k / 20) });
  }
  frame.path(line, { closed: false, fill: [0, 0, 0, 0], layer: L_ICON, stroke: P.PANEL_MUTE, strokeWidth: 2.5 });
  for (const n of hud.nodes) {
    const color = n.kind === "shard" ? P.SHARD : n.kind === "plate" ? P.PLATE : P.STORM;
    const size = n.kind === "beacon" ? 8 : 5;
    diamond(frame, routeX(n.t), routeY(n.t), size, size + 2, n.done ? P.alpha(color, 0.35) : color, L_TEXT, P.PANEL_INK, 1.5);
  }
  const sy = routeY(hud.stormProgress);
  diamond(frame, routeX(hud.stormProgress), sy, 7, 9, P.STORM, L_TEXT, P.PANEL_INK, 1.5);
  const py = routeY(hud.progress);
  poly(frame, [{ x: routeX(hud.progress), y: py - 8 }, { x: routeX(hud.progress) + 7, y: py + 6 }, { x: routeX(hud.progress) - 7, y: py + 6 }], P.SLED_GLOW, L_TEXT, P.PANEL_INK, 1.5);
};

const drawControls = (frame: Frame): void => {
  const x = 24;
  const y = 508;
  pnl(frame, x, y, 250, 150);
  keycap(frame, x + 20, y + 22, 26, "A");
  keycap(frame, x + 50, y + 22, 26, "D");
  label(frame, "STEER", x + 90, y + 26, 18, P.PANEL_INK, L_TEXT);
  keycap(frame, x + 20, y + 62, 62, "SHIFT");
  label(frame, "BRAKE", x + 90, y + 66, 18, P.PANEL_INK, L_TEXT);
  panel(frame, x + 22, y + 100, 22, 30, 8, P.PANEL, P.PANEL_INK, L_ICON, 2);
  seg(frame, { x: x + 33, y: y + 100 }, { x: x + 33, y: y + 114 }, P.PANEL_INK, 2, L_TEXT);
  label(frame, "DRAG", x + 90, y + 106, 18, P.PANEL_INK, L_TEXT);
};

const drawSpeedCharge = (frame: Frame, hud: Hud): void => {
  const x = 24;
  const y = 672;
  const w = 320;
  pnl(frame, x, y, w, 104);
  label(frame, String(hud.speedKmh), x + 20, y + 20, 46, P.PANEL_INK, L_TEXT);
  label(frame, "KM/H", x + 22, y + 70, 16, P.PANEL_MUTE, L_TEXT);
  label(frame, "CHARGE", x + 132, y + 22, 16, P.PANEL_MUTE, L_TEXT);
  const segW = 22;
  const segGap = 6;
  for (let i = 0; i < hud.chargeSegments; i += 1) {
    const on = i < hud.chargeFilled;
    box(frame, x + 132 + i * (segW + segGap), y + 48, segW, 30, on ? P.SEG_ON : P.SEG_OFF, L_ICON, P.PANEL_INK, 2);
  }
};

const drawAbilities = (frame: Frame, hud: Hud): void => {
  const cw = 92;
  const ch = 104;
  const gap = 12;
  const total = hud.abilities.length * cw + (hud.abilities.length - 1) * gap;
  const startX = WIDTH - 24 - total;
  const y = 672;
  hud.abilities.forEach((card, i) => {
    const x = startX + i * (cw + gap);
    const edge = card.active ? P.READY : P.PANEL_EDGE;
    panel(frame, x, y, cw, ch, R, P.PANEL, edge, L_PANEL, card.active ? 4 : 3);
    const ink = card.ready || card.active ? P.PANEL_INK : P.SEG_OFF;
    abilityIcon(frame, card.kind, x + cw / 2, y + 40, ink);
    label(frame, card.label, x + cw / 2, y + ch - 26, 15, ink, L_TEXT, "center");
    // A cooldown sweep dims the card from the bottom.
    if (card.cooldown > 0) {
      box(frame, x + 3, y + ch - 3 - (ch - 6) * card.cooldown, cw - 6, (ch - 6) * card.cooldown, P.alpha(P.PANEL_MUTE, 0.35), L_ICON);
    }
  });
};

const LOSE_TITLE: Record<NonNullable<State["loseReason"]>, string> = {
  crashed: "SIGNAL LOST",
  fell: "SIGNAL LOST",
  storm: "STORM OVERRAN THE RELAY",
  time: "STORM OVERRAN THE RELAY",
};

const LOSE_SUB: Record<NonNullable<State["loseReason"]>, string> = {
  crashed: "the courier crashed out",
  fell: "the courier left the path",
  storm: "the storm wall caught you",
  time: "the storm timer ran out",
};

const drawBanner = (frame: Frame, state: State, hud: Hud): void => {
  if (state.phase === "run") {
    if (hud.beaconReady) {
      label(frame, "PRESS ENTER TO ACTIVATE RELAY", WIDTH / 2, 120, 24, P.READY, L_TEXT, "center");
    }
    return;
  }
  box(frame, 0, 0, WIDTH, 800, P.alpha(P.PANEL_INK, 0.5), 120);
  const won = state.phase === "win";
  const title = won ? "SIGNAL RESTORED" : LOSE_TITLE[state.loseReason ?? "crashed"];
  const sub = won ? "the beacon is lit" : LOSE_SUB[state.loseReason ?? "crashed"];
  const pw = 620;
  const px = (WIDTH - pw) / 2;
  panel(frame, px, 280, pw, 240, 20, P.PANEL, P.PANEL_EDGE, 121, 4);
  label(frame, title, WIDTH / 2, 312, 40, won ? P.READY : P.STORM, 122, "center");
  label(frame, sub, WIDTH / 2, 366, 18, P.PANEL_MUTE, 122, "center");
  const stats = `TIME ${formatTimer(state.elapsed)}    SHARDS ${hud.shards}/${hud.shardGoal}    CRASHES ${hud.crashes}`;
  label(frame, stats, WIDTH / 2, 410, 18, P.PANEL_INK, 122, "center");
  label(frame, won ? "Press Enter to Run Again" : "Press Enter to Retry", WIDTH / 2, 462, 20, P.PANEL_INK, 122, "center");
  void textWidth;
};

/** Render the entire HUD for `state`. */
export const renderUi = (frame: Frame, state: State, hud: Hud): void => {
  drawObjective(frame, hud);
  drawTimer(frame, hud);
  drawStorm(frame, hud);
  drawMinimap(frame, hud);
  drawControls(frame);
  drawSpeedCharge(frame, hud);
  drawAbilities(frame, hud);
  drawBanner(frame, state, hud);
};
