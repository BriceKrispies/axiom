/*
 * render.ts — draws the entire Arena Forge frame to the single gameplay canvas.
 * It is a pure function of (match view, layout, ui state, combat playback frame):
 * it reads authoritative state and animation cues and paints them, but never
 * decides anything. Everything the spec requires to stay visible without opening
 * a panel — shop, hand, seven-slot warband, timer, gold, health, rank, current
 * opponent — is on screen at once, in an "arcane industrial forge arena" look
 * that intensifies with the player's arena stage. Canvas2D only: no WebGL needed.
 */

import type { LoadedContent } from "../sim/content/load.ts";
import type { CardDefinition } from "../sim/content/schema.ts";
import type { PlayerId } from "../sim/ids.ts";
import type { MatchState, PlayerState, UnitInstance } from "../sim/model.ts";
import type { CombatFrame } from "../presentation/combat-playback.ts";
import { PALETTE, STAGE_TREATMENT } from "./theme.ts";
import { type Rect, button, inRect, panel, rrPath, shade, text } from "./draw.ts";
import type { Layout } from "./layout.ts";
import { inspectRects } from "./layout.ts";
import type { UiState } from "./interaction.ts";
import { forgeUpgradeCost } from "../sim/tuning.ts";
import type { Rules } from "../sim/tuning.ts";

export interface RenderInput {
  readonly ctx: CanvasRenderingContext2D;
  readonly layout: Layout;
  readonly view: MatchState;
  readonly content: LoadedContent;
  readonly rules: Rules;
  readonly ui: UiState;
  readonly humanId: PlayerId;
  readonly combat: CombatFrame | null;
  readonly timerFrac: number;
  readonly opponentLabel: string;
  /** When true, the 3D figure layer draws the units; the 2D overlay draws only
   * slots, stat badges, and floaters (no unit plaques). */
  readonly figures3d?: boolean;
}

/** A compact attack/health badge for a slot whose figure is drawn in 3D. */
const drawStatBadge = (ctx: CanvasRenderingContext2D, r: Rect, attack: number, health: number, forged: boolean): void => {
  const bw = Math.min(r.w, 58);
  const badge: Rect = { x: r.x + (r.w - bw) / 2, y: r.y + r.h - 20, w: bw, h: 18 };
  panel(ctx, badge, "rgba(8,7,6,0.72)", forged ? PALETTE.brassLight : PALETTE.panelEdge, 5);
  text(ctx, `${attack}`, badge.x + 10, badge.y + 9, { size: 12, weight: 800, align: "center", color: PALETTE.ember });
  text(ctx, `${health}`, badge.x + bw - 10, badge.y + 9, { size: 12, weight: 800, align: "center", color: PALETTE.health });
  if (forged) text(ctx, "✦", badge.x + bw / 2, badge.y + 9, { size: 11, align: "center", color: PALETTE.brassLight });
};

const accentOf = (content: LoadedContent, def: CardDefinition): string => {
  const g = def.groups[0];
  return g === undefined ? PALETTE.brass : content.group(g).accent;
};

const drawCard = (ctx: CanvasRenderingContext2D, r: Rect, content: LoadedContent, def: CardDefinition, cost: number | null, dimmed: boolean): void => {
  const accent = accentOf(content, def);
  panel(ctx, r, dimmed ? "#171310" : PALETTE.panel, PALETTE.panelEdge, 7);
  // Group accent header strip (stamped schematic).
  ctx.save();
  rrPath(ctx, { x: r.x, y: r.y, w: r.w, h: Math.max(14, r.h * 0.2) }, 7);
  ctx.clip();
  ctx.fillStyle = accent;
  ctx.globalAlpha = 0.85;
  ctx.fillRect(r.x, r.y, r.w, r.h);
  ctx.restore();
  text(ctx, def.name.toUpperCase().slice(0, 12), r.x + r.w / 2, r.y + Math.max(9, r.h * 0.1), { size: Math.min(11, r.w / 8), weight: 800, align: "center", color: "#0c0a09" });
  // Tier pips.
  for (let i = 0; i < def.tier; i += 1) {
    ctx.fillStyle = PALETTE.brassLight;
    ctx.fillRect(r.x + 4 + i * 5, r.y + r.h * 0.28, 3, 3);
  }
  // Attack / health.
  text(ctx, `${def.baseAttack}`, r.x + 8, r.y + r.h - 12, { size: 15, weight: 800, color: PALETTE.ember });
  text(ctx, `${def.baseHealth}`, r.x + r.w - 8, r.y + r.h - 12, { size: 15, weight: 800, color: PALETTE.health, align: "right" });
  if (cost !== null) {
    ctx.beginPath();
    ctx.arc(r.x + r.w - 12, r.y + r.h * 0.42, 9, 0, Math.PI * 2);
    ctx.fillStyle = PALETTE.gold;
    ctx.fill();
    text(ctx, `${cost}`, r.x + r.w - 12, r.y + r.h * 0.42, { size: 11, weight: 800, align: "center", color: "#0c0a09" });
  }
};

const drawUnit = (
  ctx: CanvasRenderingContext2D,
  r: Rect,
  content: LoadedContent,
  cardId: string,
  attack: number,
  health: number,
  forged: boolean,
  opts: { maxHealth?: number; dead?: boolean; highlight?: string } = {},
): void => {
  const def = content.card(cardId);
  const accent = accentOf(content, def);
  ctx.save();
  if (opts.dead) ctx.globalAlpha = 0.28;
  panel(ctx, r, PALETTE.panel, forged ? PALETTE.brassLight : PALETTE.panelEdge, 7);
  ctx.save();
  rrPath(ctx, r, 7);
  ctx.clip();
  ctx.fillStyle = accent;
  ctx.globalAlpha = forged ? 0.4 : 0.22;
  ctx.fillRect(r.x, r.y, r.w, r.h);
  ctx.restore();
  if (opts.highlight) {
    ctx.lineWidth = 3;
    ctx.strokeStyle = opts.highlight;
    rrPath(ctx, r, 7);
    ctx.stroke();
  }
  text(ctx, def.name.toUpperCase().slice(0, 10), r.x + r.w / 2, r.y + 11, { size: Math.min(10, r.w / 9), weight: 800, align: "center", color: PALETTE.ink });
  if (forged) text(ctx, "✦", r.x + r.w / 2, r.y + r.h / 2, { size: 18, align: "center", color: PALETTE.brassLight });
  if (opts.maxHealth !== undefined) {
    const bar: Rect = { x: r.x + 6, y: r.y + r.h - 22, w: r.w - 12, h: 4 };
    panel(ctx, bar, "#3a1f1f", "#000", 2);
    const frac = Math.max(0, Math.min(1, health / Math.max(1, opts.maxHealth)));
    ctx.fillStyle = PALETTE.health;
    ctx.fillRect(bar.x, bar.y, bar.w * frac, bar.h);
  }
  text(ctx, `${attack}`, r.x + 7, r.y + r.h - 10, { size: 14, weight: 800, color: PALETTE.ember });
  text(ctx, `${health}`, r.x + r.w - 7, r.y + r.h - 10, { size: 14, weight: 800, color: PALETTE.health, align: "right" });
  ctx.restore();
};

const drawSlot = (ctx: CanvasRenderingContext2D, r: Rect, highlight: boolean): void => {
  ctx.save();
  ctx.setLineDash([5, 4]);
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = highlight ? PALETTE.brass : "#33291f";
  rrPath(ctx, r, 7);
  ctx.stroke();
  ctx.restore();
};

const drawHud = (input: RenderInput): void => {
  const { ctx, layout, view, humanId, timerFrac, opponentLabel } = input;
  const p = view.players[humanId] as PlayerState;
  panel(ctx, layout.hud, PALETTE.bg1, PALETTE.panelEdge, 0);
  const cy = layout.hud.h / 2;
  text(ctx, "♥", 14, cy, { size: 16, color: PALETTE.health });
  text(ctx, `${p.health}`, 30, cy, { size: 18, weight: 800, color: PALETTE.ink });
  text(ctx, "◆", 74, cy, { size: 14, color: PALETTE.gold });
  text(ctx, `${p.gold}`, 88, cy, { size: 18, weight: 800, color: PALETTE.gold });
  text(ctx, `RANK ${p.forgeRank}`, 128, cy, { size: 12, weight: 800, color: PALETTE.brassLight });
  text(ctx, `ROUND ${view.round}`, layout.w / 2, cy - 8, { size: 13, weight: 800, align: "center", color: PALETTE.ink });
  // Timer bar.
  const tb: Rect = { x: layout.w / 2 - 60, y: cy + 4, w: 120, h: 5 };
  panel(ctx, tb, "#2a221b", "#000", 2);
  ctx.fillStyle = timerFrac > 0.25 ? PALETTE.brass : PALETTE.emberHot;
  ctx.fillRect(tb.x, tb.y, tb.w * Math.max(0, Math.min(1, timerFrac)), tb.h);
  text(ctx, `VS ${opponentLabel}`, layout.sell.x - 12, cy, { size: 11, weight: 700, align: "right", color: PALETTE.inkDim });
  // Sell drop-zone.
  panel(ctx, layout.sell, "#2a1414", PALETTE.bad, 6);
  text(ctx, `SELL +${input.rules.sellValue}`, layout.sell.x + layout.sell.w / 2, layout.sell.y + layout.sell.h / 2, { size: 11, weight: 800, align: "center", color: PALETTE.bad });
  text(ctx, `#${p.eliminated ? p.placement : view.players.filter((q) => !q.eliminated).length} left`, layout.w / 2, cy + 15, { size: 9, weight: 700, align: "center", color: PALETTE.inkDim });
};

const drawShopPhase = (input: RenderInput): void => {
  const { ctx, layout, view, content, ui, humanId, rules } = input;
  const p = view.players[humanId] as PlayerState;
  const dragSlotHi = ui.drag !== null;
  // Warband. With 3D figures, draw only the slot + a stat badge (the figure stands
  // in the slot); otherwise draw the full 2D unit plaque.
  layout.warband.forEach((r, i) => {
    const u = p.warband[i] ?? null;
    if (u === null) {
      drawSlot(ctx, r, dragSlotHi);
    } else if (input.figures3d) {
      if (dragSlotHi) shade(ctx, r, PALETTE.brass, 0.08);
      drawStatBadge(ctx, r, u.attack, u.health, u.forged);
    } else {
      drawUnit(ctx, r, content, u.cardId, u.attack, u.health, u.forged);
    }
  });
  text(ctx, "WARBAND", layout.w / 2, layout.warband[0]!.y - 8, { size: 9, weight: 800, align: "center", color: PALETTE.inkDim });
  // Hand.
  layout.hand.forEach((r, i) => {
    const u = p.hand[i] as UnitInstance;
    drawUnit(ctx, r, content, u.cardId, u.attack, u.health, u.forged);
  });
  if (layout.hand.length > 0) text(ctx, "HAND", 14, layout.hand[0]!.y - 6, { size: 9, weight: 800, color: PALETTE.inkDim });
  // Shop.
  layout.shop.forEach((r, i) => {
    const slot = p.shop[i];
    if (slot === undefined) return;
    const def = content.card(slot.cardId);
    const cost = Math.max(0, def.cost - slot.discount);
    drawCard(ctx, r, content, def, cost, p.gold < cost);
  });
  // Buttons.
  const upCost = forgeUpgradeCost(rules, p.forgeRank);
  button(ctx, layout.buttons.reroll, "REROLL", { fill: "#241d16", edge: PALETTE.brass, text: PALETTE.brassLight, enabled: p.gold >= rules.rerollCost, sub: `${rules.rerollCost}◆` });
  button(ctx, layout.buttons.freeze, p.shopFrozen ? "UNFREEZE" : "FREEZE", { fill: p.shopFrozen ? "#16303a" : "#241d16", edge: PALETTE.steel, text: p.shopFrozen ? "#8fd0ff" : PALETTE.ink });
  button(ctx, layout.buttons.upgrade, "FORGE UP", { fill: "#2a1d12", edge: PALETTE.ember, text: PALETTE.brassLight, enabled: upCost !== null && p.gold >= upCost, sub: upCost === null ? "MAX" : `${upCost}◆` });
};

const drawCombatPhase = (input: RenderInput): void => {
  const { ctx, layout, combat, content, view, humanId } = input;
  text(ctx, "COMBAT", layout.w / 2, layout.hud.h + 12, { size: 12, weight: 800, align: "center", color: PALETTE.ember });
  if (combat === null) {
    return;
  }
  const iAmA = combat.units.some((u) => u.side === "a" && view.players[humanId]?.warband.some((w) => w?.instanceId === u.instanceId));
  const mySide = iAmA ? "a" : "b";
  const place = (units: typeof combat.units, row: readonly Rect[]): void => {
    for (const u of units) {
      const r = row[u.slot];
      if (r === undefined) continue;
      const hl = u.instanceId === combat.attacker ? PALETTE.ember : u.instanceId === combat.defender ? PALETTE.bad : undefined;
      if (input.figures3d) {
        // The 3D layer draws the combatant; the overlay shows a stat badge + floater.
        if (u.alive) drawStatBadge(ctx, r, u.attack, u.health, false);
      } else {
        drawUnit(ctx, r, content, u.cardId, u.attack, u.health, false, { maxHealth: u.maxHealth, dead: !u.alive, ...(hl ? { highlight: hl } : {}) });
      }
      const f = combat.floaters.find((fl) => fl.unitId === u.instanceId);
      if (f) text(ctx, f.text, r.x + r.w / 2, r.y - 6, { size: 15, weight: 800, align: "center", color: f.color });
    }
  };
  place(combat.units.filter((u) => u.side !== mySide), layout.enemy);
  place(combat.units.filter((u) => u.side === mySide), layout.warband);
  text(ctx, "ENEMY", layout.w / 2, layout.enemy[0]!.y - 6, { size: 9, weight: 800, align: "center", color: PALETTE.bad });
  text(ctx, "YOUR WARBAND", layout.w / 2, layout.warband[0]!.y - 6, { size: 9, weight: 800, align: "center", color: PALETTE.good });
};

const drawDragGhost = (input: RenderInput): void => {
  const { ctx, ui, view, content, humanId, layout } = input;
  if (ui.drag === null) {
    return;
  }
  const p = view.players[humanId] as PlayerState;
  const t = ui.drag.target;
  const r: Rect = { x: ui.drag.x - 34, y: ui.drag.y - 44, w: 68, h: 84 };
  // Highlight sell + empty slots.
  shade(ctx, layout.sell, PALETTE.bad, 0.25);
  layout.warband.forEach((s, i) => { if (p.warband[i] === null) shade(ctx, s, PALETTE.brass, 0.18); });
  ctx.save();
  ctx.globalAlpha = 0.9;
  if (t.kind === "shop") {
    const slot = p.shop[t.index];
    if (slot) drawCard(ctx, r, content, content.card(slot.cardId), null, false);
  } else {
    const u = t.kind === "hand" ? p.hand[t.index] : p.warband[t.index];
    if (u) drawUnit(ctx, r, content, u.cardId, u.attack, u.health, u.forged);
  }
  ctx.restore();
};

const drawInspect = (input: RenderInput): void => {
  const { ctx, ui, view, content, humanId } = input;
  if (ui.inspect === null) {
    return;
  }
  const p = view.players[humanId] as PlayerState;
  const t = ui.inspect;
  const slot = t.kind === "shop" ? p.shop[t.index] : null;
  const unit = t.kind === "hand" ? p.hand[t.index] : t.kind === "warband" ? p.warband[t.index] : null;
  const cardId = slot?.cardId ?? unit?.cardId;
  if (cardId === undefined) {
    return;
  }
  const def = content.card(cardId);
  shade(ctx, { x: 0, y: 0, w: input.layout.w, h: input.layout.h }, "#000", 0.55);
  const { panel: pr, action, close } = inspectRects(input.layout.w, input.layout.h);
  panel(ctx, pr, PALETTE.panel, accentOf(content, def), 10);
  text(ctx, def.name, pr.x + 16, pr.y + 22, { size: 16, weight: 800, color: PALETTE.brassLight });
  text(ctx, `TIER ${def.tier}  ${def.baseAttack}/${def.baseHealth}${unit?.forged ? "  ✦ FORGED" : ""}`, pr.x + 16, pr.y + 44, { size: 11, weight: 700, color: PALETTE.inkDim });
  const groups = def.groups.map((g) => content.group(g).name).join(" · ") || "Neutral";
  text(ctx, groups.toUpperCase(), pr.x + 16, pr.y + 62, { size: 10, weight: 700, color: PALETTE.steel });
  // Rules text, wrapped.
  const words = (unit?.forged ? `${def.rulesText}` : def.rulesText).split(" ");
  let line = "";
  let ly = pr.y + 88;
  for (const word of words) {
    const test = line + word + " ";
    if (ctx.measureText(test).width > pr.w - 34 && line !== "") {
      text(ctx, line, pr.x + 16, ly, { size: 12, weight: 600, color: PALETTE.ink });
      line = word + " ";
      ly += 18;
    } else {
      line = test;
    }
  }
  text(ctx, line, pr.x + 16, ly, { size: 12, weight: 600, color: PALETTE.ink });
  const label = t.kind === "shop" ? "BUY" : t.kind === "hand" ? "PLAY" : "SELL";
  button(ctx, action, label, { fill: "#2a1d12", edge: PALETTE.ember, text: PALETTE.brassLight });
  button(ctx, close, "✕", { fill: "#241d16", edge: PALETTE.panelEdge, text: PALETTE.ink });
  void inRect;
};

const drawResults = (input: RenderInput): void => {
  const { ctx, layout, view, humanId } = input;
  if (view.phase !== "match_complete") {
    return;
  }
  shade(ctx, { x: 0, y: 0, w: layout.w, h: layout.h }, "#000", 0.72);
  const winner = view.players.find((p) => p.placement === 1);
  const me = view.players[humanId] as PlayerState;
  const won = me.placement === 1;
  text(ctx, won ? "MASTERWORK VICTORY" : "TOURNAMENT OVER", layout.w / 2, layout.h / 2 - 40, { size: 26, weight: 800, align: "center", color: won ? PALETTE.gold : PALETTE.ember });
  text(ctx, `Champion: ${winner?.name ?? "?"}`, layout.w / 2, layout.h / 2 - 6, { size: 15, weight: 700, align: "center", color: PALETTE.ink });
  text(ctx, `You placed #${me.placement} of 8`, layout.w / 2, layout.h / 2 + 20, { size: 14, weight: 700, align: "center", color: PALETTE.inkDim });
  text(ctx, "Tap to forge a new run", layout.w / 2, layout.h / 2 + 54, { size: 13, weight: 700, align: "center", color: PALETTE.good });
};

/** Draw one full frame. */
export const renderFrame = (input: RenderInput): void => {
  const { ctx, layout, view, humanId } = input;
  const stage = STAGE_TREATMENT[(view.players[humanId] as PlayerState).presentationStage];
  // With the 3D figure layer active, the overlay MUST stay transparent over the
  // arena so the base scene shows through — clear, don't fill. Otherwise paint the
  // 2D forge backdrop (the no-3D fallback path).
  if (input.figures3d) {
    ctx.clearRect(0, 0, layout.w, layout.h);
  } else {
    ctx.fillStyle = PALETTE.bg0;
    ctx.fillRect(0, 0, layout.w, layout.h);
    ctx.fillStyle = stage.floor;
    ctx.fillRect(0, layout.hud.h, layout.w, layout.h - layout.hud.h);
    const grad = ctx.createRadialGradient(layout.w / 2, layout.h, 20, layout.w / 2, layout.h, layout.h);
    grad.addColorStop(0, stage.glow);
    grad.addColorStop(1, "rgba(0,0,0,0)");
    ctx.save();
    ctx.globalAlpha = stage.glowStrength;
    ctx.fillStyle = grad;
    ctx.fillRect(0, layout.hud.h, layout.w, layout.h - layout.hud.h);
    ctx.restore();
  }

  const combat = view.phase === "combat" || view.phase === "combat_prepare" || view.phase === "combat_resolve";
  if (combat) {
    drawCombatPhase(input);
  } else {
    drawShopPhase(input);
  }
  drawHud(input);
  drawDragGhost(input);
  drawInspect(input);
  drawResults(input);
};
