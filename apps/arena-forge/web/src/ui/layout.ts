/*
 * layout.ts — the responsive, mobile-first layout solver. Given the live canvas
 * size and the current phase, it computes hit-testable rects for the HUD, the
 * seven warband slots, the enemy row (combat), the hand, the shop cards, the
 * action buttons, and the sell drop-zone. Designed landscape-first from an
 * 844×390 viewport and scaled up for larger mobile and desktop; every touch
 * target (buttons, cards, slots) is kept >= 44 CSS px in its smallest dimension.
 */

import type { Rect } from "./draw.ts";
import { WARBAND_SLOTS } from "../sim/model.ts";

export interface Layout {
  readonly w: number;
  readonly h: number;
  readonly hud: Rect;
  readonly sell: Rect;
  readonly warband: readonly Rect[];
  readonly enemy: readonly Rect[];
  readonly hand: readonly Rect[];
  readonly shop: readonly Rect[];
  readonly buttons: { readonly reroll: Rect; readonly freeze: Rect; readonly upgrade: Rect };
  readonly slotW: number;
  readonly slotH: number;
}

const clamp = (v: number, lo: number, hi: number): number => Math.max(lo, Math.min(hi, v));

/** Lay a row of `n` cells of width `cw`, height `ch`, centered in `[x0, x0+width]`. */
const row = (n: number, x0: number, width: number, y: number, cw: number, ch: number, gap: number): Rect[] => {
  const total = n * cw + (n - 1) * gap;
  const start = x0 + Math.max(0, (width - total) / 2);
  return Array.from({ length: n }, (_, i) => ({ x: start + i * (cw + gap), y, w: cw, h: ch }));
};

export const computeLayout = (w: number, h: number, combat: boolean, shopSize: number, handCount: number): Layout => {
  const pad = clamp(w * 0.014, 8, 20);
  const hudH = clamp(h * 0.12, 42, 58);
  const gap = clamp(w * 0.008, 5, 10);
  const slotGap = clamp(w * 0.007, 4, 9);

  const boardW = w - pad * 2;
  const slotW = clamp((boardW - (WARBAND_SLOTS - 1) * slotGap) / WARBAND_SLOTS, 44, 150);
  const slotH = clamp(slotW * 0.98, 60, 150);

  const sell: Rect = { x: w - pad - clamp(w * 0.12, 74, 130), y: 4, w: clamp(w * 0.12, 74, 130), h: hudH - 8 };
  const hud: Rect = { x: 0, y: 0, w, h: hudH };

  const enemyY = hudH + gap;
  const enemy = row(WARBAND_SLOTS, pad, boardW, enemyY, slotW, slotH * 0.9, slotGap);

  // Warband band sits below the enemy row (combat) or centered (shop).
  const warbandY = combat ? enemyY + slotH * 0.9 + gap * 2 : hudH + gap + clamp(h * 0.06, 8, 40);
  const warband = row(WARBAND_SLOTS, pad, boardW, warbandY, slotW, slotH, slotGap);

  // Bottom cluster: shop cards on the left, a vertical button stack on the right.
  const btnW = clamp(w * 0.14, 84, 150);
  const shopBottom = h - pad;
  const shopH = clamp(h * 0.30, 96, 220);
  const shopY = shopBottom - shopH;
  const shopAreaW = w - pad * 2 - btnW - gap;
  const cardW = clamp((shopAreaW - (shopSize - 1) * slotGap) / Math.max(1, shopSize), 60, 130);
  const shop = row(shopSize, pad, shopAreaW, shopY, cardW, shopH, slotGap);

  const btnX = w - pad - btnW;
  const btnH = clamp((shopH - gap * 2) / 3, 44, 70);
  const buttons = {
    reroll: { x: btnX, y: shopY, w: btnW, h: btnH },
    freeze: { x: btnX, y: shopY + btnH + gap, w: btnW, h: btnH },
    upgrade: { x: btnX, y: shopY + (btnH + gap) * 2, w: btnW, h: btnH },
  };

  // Hand: a compact row between warband and shop.
  const handY = warbandY + slotH + gap;
  const handH = clamp(shopY - handY - gap, 40, 90);
  const handW = clamp(slotW * 0.9, 44, 120);
  const hand = handCount > 0 ? row(handCount, pad, boardW, handY, handW, handH, slotGap) : [];

  return { w, h, hud, sell, warband, enemy, hand, shop, buttons, slotW, slotH };
};

/** The inspect/detail overlay rects (panel + a contextual action + close). */
export const inspectRects = (w: number, h: number): { panel: Rect; action: Rect; close: Rect } => {
  const pw = clamp(w * 0.42, 300, 460);
  const ph = clamp(h * 0.7, 220, 340);
  const panel: Rect = { x: (w - pw) / 2, y: (h - ph) / 2, w: pw, h: ph };
  const action: Rect = { x: panel.x + 16, y: panel.y + ph - 56, w: pw - 32, h: 46 };
  const close: Rect = { x: panel.x + pw - 44, y: panel.y + 8, w: 36, h: 36 };
  return { panel, action, close };
};
