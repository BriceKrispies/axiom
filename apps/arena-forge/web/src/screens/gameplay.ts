/*
 * gameplay.ts — the gameplay screen. This is the existing Arena Forge match
 * orchestration (authoritative local host + mobile pointer controller + audio +
 * 3D figure director + combat playback) lifted verbatim behind the `Screen`
 * lifecycle. `enter()` starts a fresh local match (the existing startup — no
 * setup wizard); `exit()` disposes the figure scene. A small riveted MENU control
 * in the top-left of the arena (clear of the warband/shop interaction area)
 * returns to the main menu. Nothing here changed about the match rules — only the
 * navigation seam was added.
 */

import { renderScene } from "@axiom/web-engine";
import type { LoadedContent } from "../sim/content/load.ts";
import type { Command } from "../sim/commands.ts";
import { FigureDirector } from "../figures/runtime/figure-director.ts";
import type { FigurePlacement } from "../figures/runtime/figure-director.ts";
import type { MatchState, PlayerState, WarbandSnapshot } from "../sim/model.ts";
import { DEFAULT_RULES, FIXED_HZ } from "../sim/tuning.ts";
import { LocalMatchHost } from "../api/local-host.ts";
import { AudioCues } from "../audio/cues.ts";
import { reconstructFrame } from "../presentation/combat-playback.ts";
import type { CombatFrame } from "../presentation/combat-playback.ts";
import { computeLayout } from "../ui/layout.ts";
import type { Layout } from "../ui/layout.ts";
import { Interaction } from "../ui/interaction.ts";
import { renderFrame } from "../ui/render.ts";
import type { SimEvent } from "../sim/events.ts";
import { type Rect, button, inRect } from "../ui/draw.ts";
import { PALETTE } from "../ui/theme.ts";
import { type Screen, type ScreenNav, performanceNow } from "./screen.ts";

const HUMAN = 0;

interface CombatData {
  readonly snapA: WarbandSnapshot;
  readonly snapB: WarbandSnapshot;
  readonly stream: readonly SimEvent[];
}

export class GameplayScreen implements Screen {
  private readonly content: LoadedContent;
  private host: LocalMatchHost;
  private readonly audio = new AudioCues();
  private readonly interaction: Interaction;
  private readonly figures: FigureDirector;
  private clientSeq = 0;
  private cursor = 0;
  private combatCursor = 0;
  private combat: CombatData | null = null;
  private lastPhase = "";
  private seed: number;
  private lastLayout: Layout | null = null;
  private frameTick = 0;
  private frameCombat: CombatFrame | null = null;
  private menuButton: Rect = { x: 6, y: 52, w: 46, h: 30 };

  public constructor(content: LoadedContent, private readonly nav: ScreenNav, seed: number) {
    this.content = content;
    this.seed = seed;
    this.host = new LocalMatchHost({ seed, content });
    this.interaction = new Interaction(HUMAN, (cmd: Command) => this.submit(cmd));
    this.figures = new FigureDirector(this.content);
  }

  public enter(): void {
    this.host.start();
  }

  public exit(): void {
    this.figures.dispose();
  }

  public renderScene3D(): void {
    const view = this.view();
    this.frameCombat = view.phase.startsWith("combat") ? this.combatFrame() : null;
    const stage = (view.players[HUMAN] as PlayerState).presentationStage;
    this.figures.syncScene(this.placements(view, this.frameCombat), stage);
    this.frameTick += 1;
    this.figures.render(this.frameTick, this.frameCombat);
    renderScene();
  }

  /** Translate the current match state (or combat frame) into neutral figure
   * placements for the shared director. In combat, figures follow the event-derived
   * unit set (including summons); out of combat, the human's own warband. */
  private placements(view: MatchState, combat: CombatFrame | null): FigurePlacement[] {
    if (combat !== null) {
      const mySide = this.humanSide(view, combat);
      return combat.units.map((u) => ({ instanceId: u.instanceId, cardId: u.cardId, forged: false, slot: u.slot, enemy: u.side !== mySide }));
    }
    const p = view.players[HUMAN] as PlayerState;
    return p.warband.flatMap((u, slot) => (u === null ? [] : [{ instanceId: u.instanceId, cardId: u.cardId, forged: u.forged, slot, enemy: false }]));
  }

  private humanSide(view: MatchState, combat: CombatFrame): "a" | "b" {
    const human = view.players[HUMAN] as PlayerState;
    const aIsHuman = combat.units.some((u) => u.side === "a" && human.warband.some((w) => w?.instanceId === u.instanceId));
    return aIsHuman ? "a" : "b";
  }

  private submit(command: Command): void {
    this.clientSeq += 1;
    this.host.submit({ clientSeq: this.clientSeq, playerId: HUMAN, command });
  }

  private view(): MatchState {
    return this.host.view();
  }

  private buildCombat(): CombatData | null {
    const rr = this.host.getMatch().getRoundResults().find((r) => r.pairing.a === HUMAN || r.pairing.b === HUMAN);
    if (rr === undefined) {
      return null;
    }
    const stream = this.host.getMatch().getEvents().filter((e) => "combatId" in e && e.combatId === rr.result.combatId);
    return { snapA: rr.snapA, snapB: rr.snapB, stream };
  }

  public update(): void {
    this.host.tick();
    const view = this.view();
    const batch = this.host.eventsSince(this.cursor);
    this.audio.play(batch.events, HUMAN);
    this.cursor = batch.cursor;

    if (view.phase !== this.lastPhase) {
      if (view.phase === "combat") {
        this.combat = this.buildCombat();
        this.combatCursor = 0;
      }
      this.lastPhase = view.phase;
    }
    if (view.phase === "combat" && this.combat !== null) {
      const perTick = Math.max(1, this.combat.stream.length / (DEFAULT_RULES.combatPlaybackSeconds * FIXED_HZ));
      this.combatCursor = Math.min(this.combat.stream.length, this.combatCursor + perTick);
    }
  }

  private combatFrame(): CombatFrame | null {
    if (this.combat === null) {
      return null;
    }
    return reconstructFrame(this.combat.snapA, this.combat.snapB, this.combat.stream, Math.floor(this.combatCursor));
  }

  private opponentLabel(view: MatchState): string {
    const pairing = view.pairings.find((p) => p.a === HUMAN || p.b === HUMAN);
    if (pairing === undefined) {
      return "—";
    }
    if (pairing.a === HUMAN && pairing.b === null) {
      return pairing.ghostOf === null ? "BYE" : "GHOST";
    }
    const oppId = pairing.a === HUMAN ? pairing.b : pairing.a;
    return oppId === null ? "GHOST" : (view.players[oppId] as PlayerState).name;
  }

  private timerFrac(view: MatchState): number {
    if (view.phase === "shop") {
      const span = DEFAULT_RULES.shopTimerSeconds * FIXED_HZ;
      return (view.phaseDeadlineTick - view.tick) / span;
    }
    if (view.phase === "combat" && this.combat !== null) {
      return 1 - this.combatCursor / Math.max(1, this.combat.stream.length);
    }
    return 1;
  }

  public render(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    const view = this.view();
    const p = view.players[HUMAN] as PlayerState;
    const combat = view.phase.startsWith("combat");
    const layout = computeLayout(w, h, combat, p.shop.length, p.hand.length);
    this.lastLayout = layout;
    this.interaction.setContext(layout, view);
    this.interaction.tick(performanceNow());
    renderFrame({
      ctx,
      layout,
      view,
      content: this.content,
      rules: DEFAULT_RULES,
      ui: this.interaction.ui,
      humanId: HUMAN,
      combat: combat ? this.frameCombat : null,
      timerFrac: this.timerFrac(view),
      opponentLabel: this.opponentLabel(view),
      figures3d: true,
    });
    // A small MENU control clear of the warband/shop interaction area.
    this.menuButton = { x: 6, y: layout.hud.h + 6, w: 48, h: 30 };
    button(ctx, this.menuButton, "MENU", { fill: "#241d16", edge: PALETTE.brass, text: PALETTE.brassLight });
  }

  public onPointerDown(x: number, y: number): void {
    if (inRect(this.menuButton, x, y)) {
      this.nav.goto("main_menu");
      return;
    }
    if (this.view().phase === "match_complete") {
      return;
    }
    this.interaction.onDown(x, y, performanceNow());
  }

  public onPointerMove(x: number, y: number): void {
    this.interaction.onMove(x, y, performanceNow());
  }

  public onPointerUp(x: number, y: number): void {
    if (this.view().phase === "match_complete") {
      this.restart();
      return;
    }
    this.interaction.onUp(x, y, performanceNow());
  }

  public restart(): void {
    this.seed += 1;
    this.host = new LocalMatchHost({ seed: this.seed, content: this.content });
    this.host.start();
    this.cursor = 0;
    this.combatCursor = 0;
    this.combat = null;
    this.lastPhase = "";
    this.frameCombat = null;
    this.figures.dispose();
  }

  // ── dev/test controls (delegated by the app shell) ───────────────────────────
  public debugAdvancePhase(): void {
    this.host.advancePhase();
  }

  public debugLayout(): Layout | null {
    return this.lastLayout;
  }

  public debugShowcaseForge(): void {
    const p = this.view().players[HUMAN] as PlayerState;
    const showcase: readonly [number, string, boolean][] = [
      [1, "iron_recruit", false],
      [2, "ember_stoker", false],
      [3, "bloom_pod_tender", true],
      [4, "echo_flicker_sprite", false],
      [5, "iron_colossus", true],
    ];
    let id = 990001;
    for (const [slot, cardId, forged] of showcase) {
      const card = this.content.cards.find((c) => c.id === cardId);
      if (card === undefined) {
        continue;
      }
      id += 1;
      p.warband[slot] = {
        instanceId: id,
        cardId,
        forged,
        attack: card.baseAttack + (forged ? card.forgedStats.attack : 0),
        health: card.baseHealth + (forged ? card.forgedStats.health : 0),
        grantedKeywords: [],
        visualStage: forged ? 1 : 0,
      };
    }
    p.gold = 8;
  }

  public debugSummary(): { phase: string; round: number; gold: number; health: number; warband: number; hand: number; shop: number } {
    const view = this.view();
    const p = view.players[HUMAN] as PlayerState;
    return {
      phase: view.phase,
      round: view.round,
      gold: p.gold,
      health: p.health,
      warband: p.warband.filter((u) => u !== null).length,
      hand: p.hand.length,
      shop: p.shop.length,
    };
  }

  public debugFigures(): { quality: string; live: number } {
    return { quality: this.figures.qualityTier(), live: this.figures.liveFigureCount() };
  }
}
