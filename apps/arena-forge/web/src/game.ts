/*
 * game.ts — the Arena Forge application shell. It is no longer the gameplay
 * orchestrator (that moved to `screens/gameplay.ts`); it is the centralized screen
 * router. It boots on the MAIN MENU, holds one active `Screen` via `ScreenRouter`,
 * and forwards the harness's per-frame `update`/`renderScene3D`/`render`/pointer
 * callbacks to it. `window.__arena` still points here, so the browser tests +
 * capture harness drive navigation and per-screen debug hooks through it. No
 * gameplay simulation runs unless the gameplay screen is active.
 */

import { loadDefaultContent } from "./sim/content/bundle.ts";
import type { LoadedContent } from "./sim/content/load.ts";
import type { Layout } from "./ui/layout.ts";
import { type Screen, type ScreenNav, type ScreenState } from "./screens/screen.ts";
import { ScreenRouter } from "./screens/router.ts";
import { MainMenuScreen } from "./screens/main-menu.ts";
import { GameplayScreen } from "./screens/gameplay.ts";
import { FigureLabScreen } from "./screens/figure-lab/figure-lab.ts";
import type { LabGroup, SortMode } from "./screens/figure-lab/catalog.ts";

export class ArenaForgeGame {
  private readonly content: LoadedContent;
  private seed: number;
  private readonly router: ScreenRouter;

  public constructor(seed: number, content?: LoadedContent) {
    this.content = content ?? loadDefaultContent();
    this.seed = seed;
    this.router = new ScreenRouter((state, nav) => this.build(state, nav), "main_menu");
  }

  private build(state: ScreenState, nav: ScreenNav): Screen {
    if (state === "gameplay") {
      this.seed += 1;
      return new GameplayScreen(this.content, nav, this.seed);
    }
    if (state === "figure_lab") {
      return new FigureLabScreen(this.content, nav);
    }
    return new MainMenuScreen(this.content, nav);
  }

  // ── harness-driven per-frame callbacks ───────────────────────────────────────
  public update(): void {
    this.router.update();
  }

  public renderScene3D(): void {
    this.router.renderScene3D();
  }

  public render(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    this.router.render(ctx, w, h);
  }

  public onPointerDown(x: number, y: number): void {
    this.router.onPointerDown(x, y);
  }

  public onPointerMove(x: number, y: number): void {
    this.router.onPointerMove(x, y);
  }

  public onPointerUp(x: number, y: number): void {
    this.router.onPointerUp(x, y);
  }

  public onWheel(deltaY: number): void {
    this.router.onWheel(deltaY);
  }

  public onPinch(factor: number): void {
    this.router.onPinch(factor);
  }

  public onKey(key: string): void {
    this.router.onKey(key);
  }

  // ── navigation + dev/test hooks (the `window.__arena` surface) ────────────────
  public debugScreen(): ScreenState {
    return this.router.state;
  }

  public debugGoto(state: ScreenState): void {
    this.router.goto(state);
  }

  private gameplay(): GameplayScreen | null {
    return this.router.screen instanceof GameplayScreen ? this.router.screen : null;
  }

  private lab(): FigureLabScreen | null {
    return this.router.screen instanceof FigureLabScreen ? this.router.screen : null;
  }

  public debugAdvancePhase(): void {
    this.gameplay()?.debugAdvancePhase();
  }

  public debugLayout(): Layout | null {
    return this.gameplay()?.debugLayout() ?? null;
  }

  public debugShowcaseForge(): void {
    this.gameplay()?.debugShowcaseForge();
  }

  public debugSummary(): { phase: string; round: number; gold: number; health: number; warband: number; hand: number; shop: number } {
    return this.gameplay()?.debugSummary() ?? { phase: this.router.state, round: 0, gold: 0, health: 0, warband: 0, hand: 0, shop: 0 };
  }

  public debugFigures(): { quality: string; live: number } {
    return this.gameplay()?.debugFigures() ?? { quality: "-", live: 0 };
  }

  public debugLabSelect(group: LabGroup, cardId: string): void {
    this.lab()?.debugSelect(group, cardId);
  }

  public debugLabForged(forged: boolean): void {
    this.lab()?.debugSetForged(forged);
  }

  public debugLabInfo(): ReturnType<FigureLabScreen["debugInfo"]> | null {
    return this.lab()?.debugInfo() ?? null;
  }

  public debugLabZoom(factor: number): void {
    this.lab()?.debugZoom(factor);
  }

  public debugLabSearch(term: string): void {
    this.lab()?.debugSearch(term);
  }

  public debugLabSort(sort: SortMode): void {
    this.lab()?.debugSort(sort);
  }

  public debugLabSetIcon(px: number): void {
    this.lab()?.debugSetIcon(px);
  }

  public debugLabSpin(on: boolean): void {
    this.lab()?.debugSpin(on);
  }

  public debugLabCardIds(): string[] {
    return this.lab()?.debugCardIds() ?? [];
  }

  public debugLabGalleryRect(): { x: number; y: number; w: number; h: number } | null {
    return this.lab()?.debugGalleryRect() ?? null;
  }
}
