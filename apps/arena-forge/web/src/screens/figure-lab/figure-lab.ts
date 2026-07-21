/*
 * figure-lab.ts — the Figure Lab: the internal art-direction / procedural /
 * inspection workspace over the REAL Arena Forge figure systems. It renders the
 * selected card's real generated figure on a standardized forge stage (drag to
 * rotate), a scrollable catalog of every collectible + token (grouped, filtered
 * by the real group data), a top bar, and an inspector showing the card's
 * procedural inputs + its group's real visual-language summary. Nothing here is
 * mocked or gallery-only: it uses `figureForCard`, `FigureInstance`, the arena
 * scene, and `languageFor`. It never starts or advances a gameplay match.
 *
 * This is the Lab core; animation playback, the full modifier controls, comparison
 * / silhouette / game-context views, and validation diagnostics build on top of it.
 */

import { rendererBackendName, renderScene, setCamera3D } from "@axiom/web-engine";
import type { LoadedContent } from "../../sim/content/load.ts";
import type { QualityTier } from "../../figures/parts.ts";
import type { RootFrame } from "../../figures/compose.ts";
import { add, normalize, quatFromEulerXyz, scale, vec3 } from "../../figures/vec3.ts";
import { FigureInstance } from "../../figures/scene/figure-instance.ts";
import { buildArena } from "../../figures/scene/arena-scene.ts";
import { figureForCard } from "../../figures/registry.ts";
import { figureSeed } from "../../figures/variation.ts";
import { languageFor } from "../../figures/languages/index.ts";
import { type Rect, button, inRect, panel, text } from "../../ui/draw.ts";
import { hexToRgba } from "../../figures/languages/index.ts";
import { PALETTE } from "../../ui/theme.ts";
import { type Screen, type ScreenNav, resetEngineScene } from "../screen.ts";
import { type CatalogEntry, type LabGroup, GROUP_LABEL, LAB_GROUPS, buildCatalog, filterCatalog, groupOfCard } from "./catalog.ts";

export class FigureLabScreen implements Screen {
  private readonly quality: QualityTier;
  private readonly catalog: CatalogEntry[];
  private group: LabGroup = "ironbound";
  private filtered: CatalogEntry[] = [];
  private selected = 0;
  private forged = false;
  private figure: FigureInstance | null = null;
  private tick = 0;
  private yaw = 0.5;
  private zoom = 1.5; // camera distance multiplier; larger = zoomed out
  private panX = 0;
  private panY = 0.35; // default lift so the base/feet are in frame
  private dragMode: "move" | "spin" = "move";
  private dragging = false;
  private lastX = 0;
  private lastY = 0;
  private scroll = 0;
  private galleryScrolling = false;
  private galleryMoved = false;

  // Hit rects (recomputed each render).
  private back: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private forgedBtn: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private resetBtn: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private chips: Rect[] = [];
  private items: Rect[] = [];
  private galleryRect: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private stageRect: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private stageTool: Rect = { x: 0, y: 0, w: 0, h: 0 };
  // Portrait / narrow reflow: the catalog + inspector collapse into a bottom
  // tabbed tray while the stage keeps the top; landscape keeps the three columns.
  private portrait = false;
  private tab: "gallery" | "info" = "gallery";
  private tabGallery: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private tabInfo: Rect = { x: 0, y: 0, w: 0, h: 0 };

  public constructor(private readonly content: LoadedContent, private readonly nav: ScreenNav) {
    this.quality = rendererBackendName() === "WebGL2" ? "med" : "low";
    this.catalog = buildCatalog(content);
  }

  public enter(): void {
    this.filtered = filterCatalog(this.catalog, this.group);
    this.selected = 0;
    this.forged = false;
    this.rebuild();
  }

  public exit(): void {
    resetEngineScene();
    this.figure = null;
  }

  private current(): CatalogEntry | undefined {
    return this.filtered[this.selected];
  }

  private rebuild(): void {
    resetEngineScene();
    buildArena("workshop");
    const entry = this.current();
    this.figure = entry ? new FigureInstance(this.content, entry.card.id, this.forged, this.quality) : null;
    this.tick = 0;
  }

  private selectGroup(group: LabGroup): void {
    if (group === this.group) {
      return;
    }
    this.group = group;
    this.filtered = filterCatalog(this.catalog, group);
    this.selected = 0;
    this.scroll = 0;
    this.rebuild();
  }

  private select(index: number): void {
    if (index < 0 || index >= this.filtered.length || index === this.selected) {
      return;
    }
    this.selected = index;
    this.rebuild();
  }

  public update(): void {
    this.tick += 1;
  }

  public renderScene3D(): void {
    // A stable three-quarter inspection camera whose DISTANCE is the zoom control
    // (wheel + pinch). Camera-only — it never touches figure generation. The figure
    // is panned in X/Y (drag) around a slightly low look-at so the base stays framed.
    const target = vec3(0, 0.8, 0);
    const dir = normalize(vec3(0, 0.32, 1));
    const dist = Math.max(1.6, 4.4 * this.zoom);
    setCamera3D({ position: add(target, scale(dir, dist)), target, fovY: 0.72, near: 0.1, far: 140 });
    if (this.figure !== null) {
      const bob = Math.sin(this.tick * 0.045) * 0.02;
      const root: RootFrame = { position: vec3(this.panX, bob + this.panY, 0), rotation: quatFromEulerXyz(0, this.yaw, 0), scale: 1.0 };
      this.figure.pose(root);
    }
    renderScene();
  }

  /** The MOVE/SPIN toggle drawn at the stage's top-left corner (single control so
   * one-finger drag either pans X/Y or rotates). */
  private drawStageTool(ctx: CanvasRenderingContext2D): void {
    this.stageTool = { x: this.stageRect.x + 6, y: this.stageRect.y + 6, w: 74, h: 26 };
    const move = this.dragMode === "move";
    button(ctx, this.stageTool, move ? "✥ MOVE" : "⟳ SPIN", { fill: "#1a1f26", edge: move ? PALETTE.brass : PALETTE.steel, text: PALETTE.ink });
  }

  private applyZoom(factor: number): void {
    this.zoom = Math.max(0.7, Math.min(3.6, this.zoom * factor));
  }

  /** Desktop wheel zoom (positive delta = zoom out). */
  public onWheel(delta: number): void {
    this.applyZoom(delta > 0 ? 1.12 : 0.89);
  }

  /** Mobile pinch zoom: `factor` = current/previous finger distance (spread > 1). */
  public onPinch(factor: number): void {
    if (factor > 0) {
      this.applyZoom(1 / factor);
    }
  }

  public render(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    ctx.clearRect(0, 0, w, h);
    // Reflow, don't just shrink: below ~640px wide (phone portrait / narrow) the
    // catalog + inspector collapse into a bottom tab tray so nothing overlaps.
    this.portrait = w < 640;
    if (this.portrait) {
      this.renderPortrait(ctx, w, h);
    } else {
      this.renderLandscape(ctx, w, h);
    }
  }

  private renderLandscape(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    const barH = 40;
    const galleryW = Math.min(200, w * 0.26);
    const inspectorW = Math.min(230, w * 0.28);
    this.stageRect = { x: galleryW, y: barH, w: w - galleryW - inspectorW, h: h - barH };
    this.tabGallery = { x: 0, y: 0, w: 0, h: 0 };
    this.tabInfo = { x: 0, y: 0, w: 0, h: 0 };

    this.drawTopBar(ctx, w, barH, false);
    this.drawGroupNav(ctx, barH, w, false);
    this.drawGallery(ctx, { x: 0, y: barH + 30, w: galleryW, h: h - barH - 30 });
    this.drawInspector(ctx, { x: w - inspectorW, y: barH + 30, w: inspectorW, h: h - barH - 30 });
    this.drawStageTool(ctx);
  }

  private renderPortrait(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    const barH = 36;
    const navH = 26;
    this.drawTopBar(ctx, w, barH, true);
    this.drawGroupNav(ctx, barH, w, true);

    const trayH = Math.max(240, Math.round(h * 0.46));
    const trayTop = h - trayH;
    // The stage occupies the full-width upper region; the 3D figure renders behind
    // the transparent overlay, so this is just the drag-to-rotate hit region.
    this.stageRect = { x: 0, y: barH + navH, w, h: trayTop - (barH + navH) };

    // Tray: a card-name header, a Gallery/Info tab bar, then the active panel.
    const headerH = 20;
    const tabH = 28;
    panel(ctx, { x: 0, y: trayTop, w, h: trayH }, PALETTE.bg1, PALETTE.panelEdge, 0);
    const entry = this.current();
    const label = entry ? `${entry.card.name}${this.forged ? "  ✦ FORGED" : ""}   ·   T${entry.card.tier} ${entry.card.baseAttack}/${entry.card.baseHealth}` : "—";
    text(ctx, label, 10, trayTop + headerH / 2 + 2, { size: 11, weight: 800, color: PALETTE.brassLight });
    text(ctx, `Q:${this.quality}`, w - 10, trayTop + headerH / 2 + 2, { size: 10, weight: 700, align: "right", color: PALETTE.steel });

    const tabY = trayTop + headerH;
    this.tabGallery = { x: 4, y: tabY, w: w / 2 - 6, h: tabH };
    this.tabInfo = { x: w / 2 + 2, y: tabY, w: w / 2 - 6, h: tabH };
    button(ctx, this.tabGallery, `GALLERY (${this.filtered.length})`, { fill: this.tab === "gallery" ? "#2a2118" : "#161310", edge: this.tab === "gallery" ? PALETTE.brassLight : PALETTE.panelEdge, text: this.tab === "gallery" ? PALETTE.brassLight : PALETTE.inkDim });
    button(ctx, this.tabInfo, "INFO", { fill: this.tab === "info" ? "#2a2118" : "#161310", edge: this.tab === "info" ? PALETTE.brassLight : PALETTE.panelEdge, text: this.tab === "info" ? PALETTE.brassLight : PALETTE.inkDim });

    const panelArea: Rect = { x: 0, y: tabY + tabH + 4, w, h: h - (tabY + tabH + 4) };
    if (this.tab === "gallery") {
      this.drawGallery(ctx, panelArea);
    } else {
      this.galleryRect = { x: 0, y: 0, w: 0, h: 0 };
      this.drawInspector(ctx, panelArea);
    }
    this.drawStageTool(ctx);
  }

  private drawTopBar(ctx: CanvasRenderingContext2D, w: number, barH: number, compact: boolean): void {
    panel(ctx, { x: 0, y: 0, w, h: barH }, PALETTE.bg1, PALETTE.panelEdge, 0);
    this.back = { x: 6, y: 5, w: compact ? 50 : 60, h: barH - 10 };
    button(ctx, this.back, "◂", { fill: "#241d16", edge: PALETTE.brass, text: PALETTE.brassLight });
    text(ctx, "FIGURE LAB", this.back.x + this.back.w + 10, barH / 2, { size: compact ? 12 : 14, weight: 800, color: PALETTE.brassLight });
    // The full "group · card" label lives in the top bar only when there's room
    // (landscape); in portrait it moves to the tray header (no overlap).
    if (!compact) {
      const entry = this.current();
      const label = entry ? `${GROUP_LABEL[this.group]} · ${entry.card.name}${this.forged ? " ✦" : ""}` : GROUP_LABEL[this.group];
      text(ctx, label, 176, barH / 2, { size: 11, weight: 700, color: PALETTE.ink });
      text(ctx, `Q:${this.quality}`, w - 190, barH / 2, { size: 10, weight: 700, color: PALETTE.steel });
    }
    const btnW = compact ? 62 : 72;
    this.forgedBtn = { x: w - btnW * 2 - 10, y: 5, w: btnW, h: barH - 10 };
    button(ctx, this.forgedBtn, this.forged ? "FORGED" : "NORMAL", { fill: this.forged ? "#2a1d12" : "#1a1f26", edge: this.forged ? PALETTE.ember : PALETTE.steel, text: this.forged ? PALETTE.brassLight : PALETTE.ink });
    this.resetBtn = { x: w - btnW - 6, y: 5, w: btnW, h: barH - 10 };
    button(ctx, this.resetBtn, "RESET", { fill: "#241d16", edge: PALETTE.panelEdge, text: PALETTE.ink });
  }

  private drawGroupNav(ctx: CanvasRenderingContext2D, barH: number, w: number, abbrev: boolean): void {
    const cw = w / LAB_GROUPS.length;
    this.chips = LAB_GROUPS.map((g, i) => {
      const r: Rect = { x: i * cw, y: barH, w: cw - 2, h: abbrev ? 26 : 28 };
      const active = g === this.group;
      panel(ctx, r, active ? "#2a2118" : "#161310", active ? PALETTE.brassLight : PALETTE.panelEdge, 4);
      const name = abbrev ? GROUP_LABEL[g].slice(0, 5).toUpperCase() : GROUP_LABEL[g].toUpperCase();
      text(ctx, name, r.x + r.w / 2, r.y + r.h / 2, { size: abbrev ? 8 : 9, weight: 800, align: "center", color: active ? PALETTE.brassLight : PALETTE.inkDim });
      return r;
    });
  }

  private drawGallery(ctx: CanvasRenderingContext2D, area: Rect): void {
    this.galleryRect = area;
    panel(ctx, area, "rgba(10,9,8,0.82)", PALETTE.panelEdge, 0);
    const itemH = 34;
    const maxScroll = Math.max(0, this.filtered.length * itemH - area.h + 6);
    this.scroll = Math.max(0, Math.min(maxScroll, this.scroll));
    this.items = [];
    ctx.save();
    ctx.beginPath();
    ctx.rect(area.x, area.y, area.w, area.h);
    ctx.clip();
    this.filtered.forEach((entry, i) => {
      const r: Rect = { x: area.x + 4, y: area.y + 4 + i * itemH - this.scroll, w: area.w - 8, h: itemH - 4 };
      this.items.push(r);
      if (r.y + r.h < area.y || r.y > area.y + area.h) {
        return; // culled (offscreen) — not drawn
      }
      const active = i === this.selected;
      const accent = entry.group === "neutral" ? hexToRgba("#b8934a") : hexToRgba(this.content.group(entry.group).accent);
      panel(ctx, r, active ? "#2a2118" : "#17130f", active ? PALETTE.brassLight : PALETTE.panelEdge, 4);
      // Group accent tab on the left edge.
      ctx.fillStyle = `rgba(${Math.round(accent[0] * 255)},${Math.round(accent[1] * 255)},${Math.round(accent[2] * 255)},1)`;
      ctx.fillRect(r.x, r.y, 3, r.h);
      text(ctx, entry.card.name, r.x + 8, r.y + 11, { size: 10, weight: 700, color: active ? PALETTE.brassLight : PALETTE.ink });
      text(ctx, `T${entry.card.tier} · ${entry.card.baseAttack}/${entry.card.baseHealth}${entry.token ? " · token" : ""}`, r.x + 8, r.y + 22, { size: 9, weight: 600, color: PALETTE.inkDim });
    });
    ctx.restore();
  }

  private drawInspector(ctx: CanvasRenderingContext2D, area: Rect): void {
    panel(ctx, area, "rgba(10,9,8,0.82)", PALETTE.panelEdge, 0);
    const entry = this.current();
    if (entry === undefined) {
      return;
    }
    const def = figureForCard(this.content, entry.card.id);
    const lang = languageFor(def.language);
    const seed = figureSeed(entry.card.id, def.seedSalt);
    let y = area.y + 16;
    const line = (label: string, value: string, color: string = PALETTE.ink): void => {
      text(ctx, label, area.x + 8, y, { size: 9, weight: 700, color: PALETTE.inkDim });
      text(ctx, value, area.x + area.w - 8, y, { size: 9, weight: 700, align: "right", color });
      y += 15;
    };
    text(ctx, entry.card.name.toUpperCase(), area.x + 8, y, { size: 11, weight: 800, color: PALETTE.brassLight });
    y += 18;
    line("card id", entry.card.id.slice(0, 20));
    line("group", GROUP_LABEL[groupOfCard(entry.card) as LabGroup] ?? "Neutral");
    line("body plan / language", def.language);
    line("silhouette", def.silhouette);
    line("tier", `${entry.card.tier}`);
    line("attack / health", `${entry.card.baseAttack} / ${entry.card.baseHealth}`);
    line("canonical seed", `0x${seed.toString(16)}`);
    line("part count", `${this.figure?.partCount ?? 0}`, (this.figure?.partCount ?? 0) <= 28 ? PALETTE.good : PALETTE.bad);
    line("forged", this.forged ? "yes" : "no");
    y += 6;
    text(ctx, "GROUP VISUAL LANGUAGE", area.x + 8, y, { size: 9, weight: 800, color: PALETTE.ember });
    y += 15;
    line("joint style", lang.jointStyle);
    line("primitives", lang.preferredPrimitives.slice(0, 3).join(","));
    line("default anim", lang.defaultAnimation);
    line("accent", lang.groupColorHex || "—");
    y += 6;
    text(ctx, "drag stage to rotate", area.x + 8, area.y + area.h - 10, { size: 9, weight: 600, color: PALETTE.inkDim });
  }

  public onPointerDown(x: number, y: number): void {
    if (inRect(this.back, x, y)) {
      this.nav.goto("main_menu");
      return;
    }
    if (inRect(this.forgedBtn, x, y)) {
      this.forged = !this.forged;
      this.rebuild();
      return;
    }
    if (inRect(this.resetBtn, x, y)) {
      this.forged = false;
      this.yaw = 0.5;
      this.zoom = 1.5;
      this.panX = 0;
      this.panY = 0.35;
      this.dragMode = "move";
      this.rebuild();
      return;
    }
    if (inRect(this.stageTool, x, y)) {
      this.dragMode = this.dragMode === "move" ? "spin" : "move";
      return;
    }
    const chip = this.chips.findIndex((r) => inRect(r, x, y));
    if (chip >= 0) {
      this.selectGroup(LAB_GROUPS[chip] as LabGroup);
      return;
    }
    // Portrait tray tabs (zero-sized in landscape, so these never match there).
    if (inRect(this.tabGallery, x, y)) {
      this.tab = "gallery";
      return;
    }
    if (inRect(this.tabInfo, x, y)) {
      this.tab = "info";
      return;
    }
    if (inRect(this.galleryRect, x, y)) {
      this.galleryScrolling = true;
      this.galleryMoved = false;
      this.lastX = y;
      return;
    }
    if (inRect(this.stageRect, x, y)) {
      this.dragging = true;
      this.lastX = x;
      this.lastY = y;
    }
  }

  public onPointerMove(x: number, y: number): void {
    if (this.galleryScrolling) {
      const dy = y - this.lastX;
      if (Math.abs(dy) > 4) {
        this.galleryMoved = true;
      }
      this.scroll -= dy;
      this.lastX = y;
      return;
    }
    if (this.dragging) {
      const dx = x - this.lastX;
      const dy = y - this.lastY;
      if (this.dragMode === "spin") {
        this.yaw += dx * 0.012;
      } else {
        // Pan the figure in X/Y; scale by zoom so it feels consistent at any distance.
        const k = 0.006 * this.zoom;
        this.panX = Math.max(-3, Math.min(3, this.panX + dx * k));
        this.panY = Math.max(-1.5, Math.min(3, this.panY - dy * k));
      }
      this.lastX = x;
      this.lastY = y;
    }
  }

  public onPointerUp(x: number, y: number): void {
    if (this.galleryScrolling) {
      // A tap (not a scroll drag) selects the item under the pointer.
      if (!this.galleryMoved) {
        const idx = this.items.findIndex((r) => inRect(r, x, y));
        if (idx >= 0) {
          this.select(idx);
        }
      }
      this.galleryScrolling = false;
      return;
    }
    this.dragging = false;
  }

  // ── dev/test hooks (for the app shell + capture harness) ─────────────────────
  public debugSelect(group: LabGroup, cardId: string): void {
    this.selectGroup(group);
    const idx = this.filtered.findIndex((e) => e.card.id === cardId);
    if (idx >= 0) {
      this.select(idx);
    }
  }

  public debugSetForged(forged: boolean): void {
    if (forged !== this.forged) {
      this.forged = forged;
      this.rebuild();
    }
  }

  public debugZoom(factor: number): void {
    this.applyZoom(factor);
  }

  public debugInfo(): { group: string; card: string; forged: boolean; parts: number; count: number; zoom: number } {
    const entry = this.current();
    return { group: this.group, card: entry?.card.id ?? "", forged: this.forged, parts: this.figure?.partCount ?? 0, count: this.filtered.length, zoom: Math.round(this.zoom * 100) / 100 };
  }
}
