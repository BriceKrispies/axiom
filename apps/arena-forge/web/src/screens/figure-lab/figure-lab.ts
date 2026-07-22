/*
 * figure-lab.ts — the Figure Lab: the internal art-direction / procedural /
 * inspection workspace over the REAL Arena Forge figure systems. It is a GALLERY:
 * every card's real generated figure is spawned once and rendered AT ONCE as a
 * wrapping grid of live 3D miniatures — grouped by tribe by default — each
 * turning slowly about the Y axis (a turntable spin). The grid is searchable, sortable, filterable,
 * and its icons are resizeable; selecting one opens the inspector with that card's
 * procedural inputs and its group's real visual-language summary. Nothing here is
 * mocked or gallery-only: it uses `figureForCard`, `FigureInstance`, and
 * `languageFor`. It never starts or advances a gameplay match.
 *
 * The engine scene is FLAT and SINGLE-CAMERA (no viewports, no render targets), so
 * "many icons" is one scene with one camera: the grid's screen rects are mapped
 * exactly into the camera's world plane, which is why the 2D captions land on the
 * 3D miniatures. Off-screen figures are parked, never despawned.
 */

import { rendererBackendName, renderScene, setCamera3D } from "@axiom/web-engine";
import type { LoadedContent } from "../../sim/content/load.ts";
import type { QualityTier } from "../../figures/parts.ts";
import type { RootFrame } from "../../figures/compose.ts";
import { type Vec3, quatFromAxisAngle, rotateVec, sub, vec3 } from "../../figures/vec3.ts";
import { FigureInstance } from "../../figures/scene/figure-instance.ts";
import { buildGalleryStage } from "../../figures/scene/arena-scene.ts";
import { figureForCard } from "../../figures/registry.ts";
import { figureSeed } from "../../figures/variation.ts";
import { languageFor } from "../../figures/languages/index.ts";
import { type Rect, button, inRect, panel, text } from "../../ui/draw.ts";
import { hexToRgba } from "../../figures/languages/index.ts";
import { PALETTE } from "../../ui/theme.ts";
import { type Screen, type ScreenNav, resetEngineScene } from "../screen.ts";
import { type CatalogEntry, type LabGroup, type SortMode, GROUP_LABEL, LAB_GROUPS, SORT_LABEL, SORT_MODES, buildCatalog, groupOfCard, queryCatalog } from "./catalog.ts";
import { type GalleryLayout, CAPTION_H, HEADER_H, clampIcon, layoutGallery } from "./gallery-layout.ts";

/** Screen pixels per world unit at the gallery plane (fixes the camera scale). */
const PX_PER_UNIT = 96;
const FOV_Y = 0.5;
/** Radians per tick of the slow turntable spin (~14°/s at 60Hz). */
const SPIN_PER_TICK = 0.004;
/** Fraction of an icon cell the figure's bounding height fills. Held well under 1
 * so the full head-to-pedestal silhouette (and any raised banner/spike) reads
 * inside the cell with a clear margin band — the reference frames each miniature
 * as a roomy portrait rather than packing it edge-to-edge and cropping the
 * feet/banner against the cell rim. */
const FIT = 0.7;
/** Fraction of a cell left as floor below a figure's feet. The reference stands
 * every miniature ON the cell floor (a "figures on a shelf" read), spending the
 * spare room as headroom above the helmet — rather than bbox-centring the figure
 * so it floats mid-cell with a dead band beneath the feet. Grounding to a shared
 * baseline also lines the whole row's feet up, however tall each figure is. */
const GROUND_MARGIN = 0.08;
const Y_AXIS: Vec3 = vec3(0, 1, 0);

export class FigureLabScreen implements Screen {
  private readonly quality: QualityTier;
  private readonly catalog: CatalogEntry[];
  private readonly figures = new Map<string, FigureInstance>();
  private group: LabGroup = "all";
  private sort: SortMode = "tribe";
  private search = "";
  private searchFocused = false;
  private entries: CatalogEntry[] = [];
  private layout: GalleryLayout = { columns: 1, cells: [], headers: [], contentHeight: 0 };
  private icon = 108;
  private selected: string | null = null;
  private forged = false;
  private spinning = true;
  private tick = 0;
  private scroll = 0;
  private viewW = 0;
  private viewH = 0;
  private portrait = false;
  private sheet = false;

  // Drag state: gallery flick-scroll vs. slider drag vs. a tap that selects.
  private scrolling = false;
  private slidingSize = false;
  private moved = false;
  private lastY = 0;

  // Hit rects (recomputed each render).
  private back: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private searchBox: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private sortBtn: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private forgedBtn: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private spinBtn: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private resetBtn: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private sizeTrack: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private smaller: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private bigger: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private chips: Rect[] = [];
  private galleryRect: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private sheetClose: Rect = { x: 0, y: 0, w: 0, h: 0 };

  public constructor(private readonly content: LoadedContent, private readonly nav: ScreenNav) {
    this.quality = rendererBackendName() === "WebGL2" ? "med" : "low";
    this.catalog = buildCatalog(content);
  }

  public enter(): void {
    this.rebuildFigures();
    this.requery();
  }

  public exit(): void {
    resetEngineScene();
    this.figures.clear();
  }

  /**
   * Spawn EVERY catalog figure once into the one shared scene. This is the whole
   * point of the gallery — all of them are live at the same time — and it is the
   * only expensive operation here, so it runs on enter and on the forged toggle,
   * never on a filter/sort/search change (those only re-place existing figures).
   */
  private rebuildFigures(): void {
    resetEngineScene();
    buildGalleryStage();
    this.figures.clear();
    for (const entry of this.catalog) {
      // `true`: give each gallery miniature a grounding contact-shadow blob (the
      // gallery stage has no floor, so figures would otherwise read as floating).
      this.figures.set(entry.card.id, new FigureInstance(this.content, entry.card.id, this.forged, this.quality, true));
    }
  }

  private requery(): void {
    this.entries = queryCatalog(this.catalog, { group: this.group, search: this.search, sort: this.sort });
    this.scroll = 0;
  }

  private selectedEntry(): CatalogEntry | undefined {
    return this.entries.find((e) => e.card.id === this.selected);
  }

  public update(): void {
    this.tick += 1;
  }

  // ── the 3D gallery ───────────────────────────────────────────────────────────

  /** World units per screen pixel — the exact inverse of the camera's projection
   * at the z = 0 gallery plane, so a cell rect maps 1:1 onto its miniature. */
  private worldPerPixel(): number {
    return 1 / PX_PER_UNIT;
  }

  /** Screen point (CSS px) → world point on the gallery plane. */
  private toWorld(sx: number, sy: number): Vec3 {
    const k = this.worldPerPixel();
    return vec3((sx - this.viewW / 2) * k, (this.viewH / 2 - sy) * k, 0);
  }

  public renderScene3D(): void {
    // A near-orthographic long lens: every miniature sits on z = 0, so a single
    // fixed camera gives every cell the same scale with negligible perspective
    // skew across the grid.
    const k = this.worldPerPixel();
    const dist = (this.viewH * k) / (2 * Math.tan(FOV_Y / 2));
    setCamera3D({ position: vec3(0, 0, dist), target: vec3(0, 0, 0), fovY: FOV_Y, near: 0.1, far: dist * 4 });

    for (const fig of this.figures.values()) {
      fig.park();
    }
    const spin = quatFromAxisAngle(Y_AXIS, this.spinning ? this.tick * SPIN_PER_TICK : 0);
    const area = this.galleryRect;
    for (const cell of this.layout.cells) {
      const top = area.y + cell.y - this.scroll;
      if (top + cell.size < area.y - cell.size || top > area.y + area.h) {
        continue; // culled: parked above, and stays parked
      }
      const entry = this.entries[cell.index];
      const fig = entry ? this.figures.get(entry.card.id) : undefined;
      if (fig === undefined) {
        continue;
      }
      // Fit the figure's own bounding height into the cell, then turn it about the
      // vertical axis through its bounding-box CENTRE, so it spins in place and
      // stays centred in the icon at every angle.
      const s = (cell.size * k * FIT) / fig.height;
      // Ground the feet to a shared baseline near the cell floor (the reference
      // stands each miniature on the cell floor with the spare room as headroom),
      // then place the bbox centre s·height/2 above it. Y-spin preserves the
      // vertical axis, so this pivots in place without lifting the figure off the
      // baseline at any angle.
      const feet = this.toWorld(area.x + cell.x + cell.size / 2, top + cell.size * (1 - GROUND_MARGIN));
      const centre = vec3(feet.x, feet.y + (fig.height * s) / 2, feet.z);
      const root: RootFrame = { position: sub(centre, rotateVec(spin, vec3(0, fig.midY * s, 0))), rotation: spin, scale: s };
      // The shared subtle idle (breathing / weapon-ready stance) plays under the
      // turntable spin — the same animator the gameplay arena drives.
      fig.animateIdle(this.tick);
      fig.pose(root);
    }
    renderScene();
  }

  // ── 2D overlay ───────────────────────────────────────────────────────────────

  public render(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    ctx.clearRect(0, 0, w, h);
    this.viewW = w;
    this.viewH = h;
    this.portrait = w < 640;

    const barH = this.portrait ? 36 : 40;
    const navH = 26;
    const toolH = 26;
    const inspectorW = this.portrait ? 0 : Math.min(240, w * 0.28);
    const top = barH + navH + toolH;
    this.galleryRect = { x: 0, y: top, w: w - inspectorW, h: h - top };
    this.layout = layoutGallery(this.entries, this.galleryRect.w, this.icon, this.sort);
    this.clampScroll();

    // Gallery decorations FIRST (the miniatures show through the transparent
    // overlay), then the opaque chrome on top so nothing bleeds under it.
    this.drawGallery(ctx);
    this.drawTopBar(ctx, w, barH);
    this.drawGroupNav(ctx, w, barH, navH);
    this.drawToolStrip(ctx, w, barH + navH, toolH);
    if (this.portrait) {
      this.drawSheet(ctx, w, h);
    } else {
      this.drawInspector(ctx, { x: w - inspectorW, y: top, w: inspectorW, h: h - top });
    }
  }

  private clampScroll(): void {
    const max = Math.max(0, this.layout.contentHeight - this.galleryRect.h);
    this.scroll = Math.max(0, Math.min(max, this.scroll));
  }

  private drawGallery(ctx: CanvasRenderingContext2D): void {
    const area = this.galleryRect;
    ctx.save();
    ctx.beginPath();
    ctx.rect(area.x, area.y, area.w, area.h);
    ctx.clip();

    for (const header of this.layout.headers) {
      const y = area.y + header.y - this.scroll;
      if (y + HEADER_H < area.y || y > area.y + area.h) {
        continue;
      }
      ctx.fillStyle = "rgba(18,15,12,0.9)";
      ctx.fillRect(area.x, y, area.w, HEADER_H);
      ctx.fillStyle = PALETTE.brass;
      ctx.fillRect(area.x + 8, y + HEADER_H - 2, area.w - 16, 1);
      text(ctx, header.label.toUpperCase(), area.x + 10, y + HEADER_H / 2, { size: 10, weight: 800, color: PALETTE.brassLight });
      text(ctx, `${header.count}`, area.x + area.w - 10, y + HEADER_H / 2, { size: 10, weight: 700, align: "right", color: PALETTE.inkDim });
    }

    const small = this.icon < 68;
    for (const cell of this.layout.cells) {
      const y = area.y + cell.y - this.scroll;
      if (y + cell.size + CAPTION_H < area.y || y > area.y + area.h) {
        continue;
      }
      const entry = this.entries[cell.index];
      if (entry === undefined) {
        continue;
      }
      const x = area.x + cell.x;
      const active = entry.card.id === this.selected;
      const accent = entry.group === "neutral" ? hexToRgba("#b8934a") : hexToRgba(this.content.group(entry.group).accent);
      const rgb = `${Math.round(accent[0] * 255)},${Math.round(accent[1] * 255)},${Math.round(accent[2] * 255)}`;

      // A translucent well so a dark miniature reads, plus a group-accent frame —
      // never a filled panel, or it would hide the 3D behind it.
      ctx.fillStyle = active ? "rgba(58,44,24,0.45)" : "rgba(12,11,10,0.35)";
      ctx.fillRect(x, y, cell.size, cell.size);
      ctx.strokeStyle = active ? PALETTE.brassLight : `rgba(${rgb},0.55)`;
      ctx.lineWidth = active ? 2 : 1;
      ctx.strokeRect(x + 0.5, y + 0.5, cell.size - 1, cell.size - 1);
      ctx.fillStyle = `rgba(${rgb},1)`;
      ctx.fillRect(x, y, cell.size, 2);

      if (!small) {
        text(ctx, entry.card.name, x + cell.size / 2, y + cell.size + 9, { size: 9, weight: 700, align: "center", max: cell.size, color: active ? PALETTE.brassLight : PALETTE.ink });
        text(ctx, `T${entry.card.tier} · ${entry.card.baseAttack}/${entry.card.baseHealth}${entry.token ? " · token" : ""}`, x + cell.size / 2, y + cell.size + 20, { size: 8, weight: 600, align: "center", max: cell.size, color: PALETTE.inkDim });
      }
    }
    ctx.restore();

    if (this.entries.length === 0) {
      text(ctx, `no figures match “${this.search}”`, area.x + area.w / 2, area.y + 40, { size: 12, weight: 700, align: "center", color: PALETTE.inkDim });
    }
  }

  private drawTopBar(ctx: CanvasRenderingContext2D, w: number, barH: number): void {
    panel(ctx, { x: 0, y: 0, w, h: barH }, PALETTE.bg1, PALETTE.panelEdge, 0);
    this.back = { x: 6, y: 5, w: this.portrait ? 42 : 56, h: barH - 10 };
    button(ctx, this.back, "<", { fill: "#241d16", edge: PALETTE.brass, text: PALETTE.brassLight });

    const btnW = this.portrait ? 58 : 70;
    this.forgedBtn = { x: w - btnW - 6, y: 5, w: btnW, h: barH - 10 };
    button(ctx, this.forgedBtn, this.forged ? "FORGED" : "NORMAL", { fill: this.forged ? "#2a1d12" : "#1a1f26", edge: this.forged ? PALETTE.ember : PALETTE.steel, text: this.forged ? PALETTE.brassLight : PALETTE.ink });
    this.sortBtn = { x: w - btnW * 2 - 12, y: 5, w: btnW, h: barH - 10 };
    button(ctx, this.sortBtn, SORT_LABEL[this.sort], { fill: "#1a1f26", edge: PALETTE.steel, text: PALETTE.ink, ...(this.portrait ? {} : { sub: "SORT" }) });

    // The search field: a real focusable text input drawn on the canvas.
    const sx = this.back.x + this.back.w + 8;
    this.searchBox = { x: sx, y: 5, w: Math.max(80, this.sortBtn.x - sx - 10), h: barH - 10 };
    panel(ctx, this.searchBox, "#100e0c", this.searchFocused ? PALETTE.brassLight : PALETTE.panelEdge, 4);
    const shown = this.search.length > 0 ? this.search : this.searchFocused ? "" : "search name · tribe · keyword";
    const caret = this.searchFocused && Math.floor(this.tick / 20) % 2 === 0 ? "|" : "";
    text(ctx, "SEARCH", this.searchBox.x + 8, this.searchBox.y + this.searchBox.h / 2, { size: 8, weight: 800, color: PALETTE.steel });
    text(ctx, `${shown}${caret}`, this.searchBox.x + 52, this.searchBox.y + this.searchBox.h / 2, {
      size: 11,
      weight: 700,
      max: this.searchBox.w - 60,
      color: this.search.length > 0 ? PALETTE.ink : PALETTE.inkDim,
    });
  }

  private drawGroupNav(ctx: CanvasRenderingContext2D, w: number, y: number, navH: number): void {
    const cw = w / LAB_GROUPS.length;
    this.chips = LAB_GROUPS.map((g, i) => {
      const r: Rect = { x: i * cw, y, w: cw - 2, h: navH };
      const active = g === this.group;
      panel(ctx, r, active ? "#2a2118" : "#161310", active ? PALETTE.brassLight : PALETTE.panelEdge, 4);
      const name = this.portrait ? GROUP_LABEL[g].slice(0, 5).toUpperCase() : GROUP_LABEL[g].toUpperCase();
      text(ctx, name, r.x + r.w / 2, r.y + r.h / 2, { size: this.portrait ? 8 : 9, weight: 800, align: "center", color: active ? PALETTE.brassLight : PALETTE.inkDim });
      return r;
    });
  }

  /** The strip under the group chips: result count, spin toggle, reset, and the
   * icon-SIZE slider (the gallery's zoom). */
  private drawToolStrip(ctx: CanvasRenderingContext2D, w: number, y: number, toolH: number): void {
    panel(ctx, { x: 0, y, w, h: toolH }, "#131110", PALETTE.panelEdge, 0);
    const cy = y + toolH / 2;
    text(ctx, `${this.entries.length} FIGURES`, 8, cy, { size: 9, weight: 800, color: PALETTE.brassLight });

    const bw = this.portrait ? 42 : 54;
    this.spinBtn = { x: this.portrait ? 84 : 110, y: y + 3, w: bw, h: toolH - 6 };
    button(ctx, this.spinBtn, this.spinning ? "SPIN" : "HOLD", { fill: "#1a1f26", edge: this.spinning ? PALETTE.brass : PALETTE.steel, text: PALETTE.ink });
    this.resetBtn = { x: this.spinBtn.x + bw + 6, y: y + 3, w: bw, h: toolH - 6 };
    button(ctx, this.resetBtn, "RESET", { fill: "#241d16", edge: PALETTE.panelEdge, text: PALETTE.ink });

    // Size control: [−] [track] [+]. Dragging the track resizes the icons live.
    const sq = toolH - 6;
    this.bigger = { x: w - sq - 6, y: y + 3, w: sq, h: sq };
    const trackW = Math.min(140, Math.max(56, w * 0.22));
    this.sizeTrack = { x: this.bigger.x - trackW - 4, y: y + 8, w: trackW, h: toolH - 16 };
    this.smaller = { x: this.sizeTrack.x - sq - 4, y: y + 3, w: sq, h: sq };
    button(ctx, this.smaller, "-", { fill: "#1a1f26", edge: PALETTE.steel, text: PALETTE.ink });
    button(ctx, this.bigger, "+", { fill: "#1a1f26", edge: PALETTE.steel, text: PALETTE.ink });
    panel(ctx, this.sizeTrack, "#0d0c0b", PALETTE.panelEdge, 3);
    const t = this.sizeFraction();
    ctx.fillStyle = PALETTE.brass;
    ctx.fillRect(this.sizeTrack.x + 2, this.sizeTrack.y + 2, Math.max(2, (this.sizeTrack.w - 4) * t), this.sizeTrack.h - 4);
    text(ctx, `${Math.round(this.icon)}px`, this.smaller.x - 8, cy, { size: 9, weight: 700, align: "right", color: PALETTE.inkDim });
  }

  private drawInspector(ctx: CanvasRenderingContext2D, area: Rect): void {
    panel(ctx, area, "#0d0b0a", PALETTE.panelEdge, 0);
    const entry = this.selectedEntry();
    if (entry === undefined) {
      text(ctx, "TAP A FIGURE", area.x + 10, area.y + 20, { size: 10, weight: 800, color: PALETTE.inkDim });
      text(ctx, "drag to scroll · wheel to resize", area.x + 10, area.y + 38, { size: 9, weight: 600, color: PALETTE.inkDim });
      return;
    }
    this.drawDetails(ctx, entry, area);
  }

  /** Portrait: the inspector is a bottom sheet over the full-width gallery. */
  private drawSheet(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    const entry = this.selectedEntry();
    if (!this.sheet || entry === undefined) {
      this.sheetClose = { x: 0, y: 0, w: 0, h: 0 };
      return;
    }
    const area: Rect = { x: 0, y: h * 0.42, w, h: h * 0.58 };
    panel(ctx, area, "#0d0b0a", PALETTE.brass, 0);
    this.sheetClose = { x: w - 34, y: area.y + 6, w: 28, h: 24 };
    this.drawDetails(ctx, entry, area);
    button(ctx, this.sheetClose, "X", { fill: "#241d16", edge: PALETTE.panelEdge, text: PALETTE.ink });
  }

  private drawDetails(ctx: CanvasRenderingContext2D, entry: CatalogEntry, area: Rect): void {
    const def = figureForCard(this.content, entry.card.id);
    const lang = languageFor(def.language);
    const seed = figureSeed(entry.card.id, def.seedSalt);
    const fig = this.figures.get(entry.card.id);
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
    line("part count", `${fig?.partCount ?? 0}`, (fig?.partCount ?? 0) <= 28 ? PALETTE.good : PALETTE.bad);
    line("forged", this.forged ? "yes" : "no");
    y += 6;
    text(ctx, "GROUP VISUAL LANGUAGE", area.x + 8, y, { size: 9, weight: 800, color: PALETTE.ember });
    y += 15;
    line("joint style", lang.jointStyle);
    line("primitives", lang.preferredPrimitives.slice(0, 3).join(","));
    line("default anim", lang.defaultAnimation);
    line("accent", lang.groupColorHex || "—");
  }

  // ── controls ─────────────────────────────────────────────────────────────────

  private sizeFraction(): number {
    return (this.icon - 44) / (260 - 44);
  }

  private setIcon(px: number): void {
    this.icon = clampIcon(px);
  }

  private cycleSort(): void {
    const i = SORT_MODES.indexOf(this.sort);
    this.sort = SORT_MODES[(i + 1) % SORT_MODES.length] as SortMode;
    this.requery();
  }

  private selectGroup(group: LabGroup): void {
    if (group !== this.group) {
      this.group = group;
      this.requery();
    }
  }

  /** Wheel resizes the icons (the gallery's zoom); it never moves a camera. */
  public onWheel(delta: number): void {
    this.setIcon(this.icon * (delta > 0 ? 0.9 : 1.11));
  }

  public onPinch(factor: number): void {
    if (factor > 0) {
      this.setIcon(this.icon * factor);
    }
  }

  /** Typed text goes to the search field while it is focused. */
  public onKey(key: string): void {
    if (key === "/" && !this.searchFocused) {
      this.searchFocused = true;
      return;
    }
    if (!this.searchFocused) {
      return;
    }
    if (key === "Escape") {
      this.searchFocused = false;
      if (this.search.length > 0) {
        this.search = "";
        this.requery();
      }
      return;
    }
    if (key === "Enter") {
      this.searchFocused = false;
      return;
    }
    if (key === "Backspace") {
      this.search = this.search.slice(0, -1);
      this.requery();
      return;
    }
    if (key.length === 1) {
      this.search += key;
      this.requery();
    }
  }

  public onPointerDown(x: number, y: number): void {
    if (inRect(this.back, x, y)) {
      this.nav.goto("main_menu");
      return;
    }
    if (inRect(this.searchBox, x, y)) {
      this.searchFocused = true;
      return;
    }
    this.searchFocused = false;
    if (inRect(this.sheetClose, x, y)) {
      this.sheet = false;
      return;
    }
    if (inRect(this.sortBtn, x, y)) {
      this.cycleSort();
      return;
    }
    if (inRect(this.forgedBtn, x, y)) {
      this.forged = !this.forged;
      this.rebuildFigures();
      return;
    }
    if (inRect(this.spinBtn, x, y)) {
      this.spinning = !this.spinning;
      return;
    }
    if (inRect(this.resetBtn, x, y)) {
      this.icon = 108;
      this.scroll = 0;
      this.search = "";
      this.sort = "tribe";
      this.group = "all";
      this.spinning = true;
      this.requery();
      return;
    }
    if (inRect(this.smaller, x, y)) {
      this.setIcon(this.icon - 16);
      return;
    }
    if (inRect(this.bigger, x, y)) {
      this.setIcon(this.icon + 16);
      return;
    }
    if (inRect(this.sizeTrack, x, y)) {
      this.slidingSize = true;
      this.dragSize(x);
      return;
    }
    const chip = this.chips.findIndex((r) => inRect(r, x, y));
    if (chip >= 0) {
      this.selectGroup(LAB_GROUPS[chip] as LabGroup);
      return;
    }
    if (inRect(this.galleryRect, x, y)) {
      this.scrolling = true;
      this.moved = false;
      this.lastY = y;
    }
  }

  private dragSize(x: number): void {
    const t = Math.max(0, Math.min(1, (x - this.sizeTrack.x) / Math.max(1, this.sizeTrack.w)));
    this.setIcon(44 + t * (260 - 44));
  }

  public onPointerMove(x: number, y: number): void {
    if (this.slidingSize) {
      this.dragSize(x);
      return;
    }
    if (this.scrolling) {
      const dy = y - this.lastY;
      if (Math.abs(dy) > 4) {
        this.moved = true;
      }
      this.scroll -= dy;
      this.clampScroll();
      this.lastY = y;
    }
  }

  public onPointerUp(x: number, y: number): void {
    this.slidingSize = false;
    if (this.scrolling) {
      this.scrolling = false;
      if (!this.moved) {
        this.selectAt(x, y);
      }
    }
  }

  /** Hit-test the grid. The target is the icon PLUS its caption band — at small
   * icon sizes the icon alone is under the 44px touch minimum. */
  private selectAt(x: number, y: number): void {
    const area = this.galleryRect;
    const cell = this.layout.cells.find((c) => {
      const cy = area.y + c.y - this.scroll;
      const cx = area.x + c.x;
      return x >= cx && x <= cx + c.size && y >= cy && y <= cy + c.size + CAPTION_H;
    });
    const entry = cell ? this.entries[cell.index] : undefined;
    if (entry === undefined) {
      return;
    }
    this.selected = this.selected === entry.card.id ? null : entry.card.id;
    this.sheet = this.selected !== null;
  }

  // ── dev/test hooks (for the app shell + capture harness) ─────────────────────
  public debugSelect(group: LabGroup, cardId: string): void {
    this.selectGroup(group);
    this.selected = this.entries.some((e) => e.card.id === cardId) ? cardId : this.selected;
    this.sheet = this.selected !== null;
    this.scrollToSelected();
  }

  /** Bring the selection into view (a programmatic select can land off-screen). */
  private scrollToSelected(): void {
    const index = this.entries.findIndex((e) => e.card.id === this.selected);
    const cell = this.layout.cells.find((c) => c.index === index);
    if (cell !== undefined) {
      this.scroll = cell.y - this.galleryRect.h / 2 + cell.size / 2;
      this.clampScroll();
    }
  }

  public debugSetForged(forged: boolean): void {
    if (forged !== this.forged) {
      this.forged = forged;
      this.rebuildFigures();
    }
  }

  public debugZoom(factor: number): void {
    this.setIcon(this.icon * factor);
  }

  public debugSearch(term: string): void {
    this.search = term;
    this.requery();
  }

  public debugSort(sort: SortMode): void {
    this.sort = sort;
    this.requery();
  }

  /** Set the icon size directly in px (clamped). Deterministic capture control —
   * more reliable than accumulating `debugZoom` factors to a target. */
  public debugSetIcon(px: number): void {
    this.setIcon(px);
    this.scrollToSelected();
  }

  /** Toggle the turntable spin. Paused holds every figure at yaw 0 (front-facing),
   * which is what a deterministic screenshot wants. */
  public debugSpin(on: boolean): void {
    this.spinning = on;
  }

  /** The ordered card ids currently in the gallery (post filter/search/sort) — so
   * a capture agent can pick a real card without hard-coding the content. */
  public debugCardIds(): string[] {
    return this.entries.map((e) => e.card.id);
  }

  /** The gallery grid's screen rect (figures + captions region) — lets a capture
   * tool clip to just the grid for a chrome-free frame. */
  public debugGalleryRect(): Rect {
    return this.galleryRect;
  }

  public debugInfo(): { group: string; card: string; forged: boolean; parts: number; count: number; zoom: number; sort: string; search: string; live: number; columns: number } {
    const entry = this.selectedEntry();
    return {
      group: this.group,
      card: entry?.card.id ?? "",
      forged: this.forged,
      parts: entry ? (this.figures.get(entry.card.id)?.partCount ?? 0) : 0,
      count: this.entries.length,
      zoom: Math.round(this.icon),
      sort: this.sort,
      search: this.search,
      live: this.figures.size,
      columns: this.layout.columns,
    };
  }
}
