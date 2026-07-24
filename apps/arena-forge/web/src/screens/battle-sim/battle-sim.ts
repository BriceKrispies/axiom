/*
 * battle-sim.ts — the Battle Simulator screen. Pick one of a handful of data-driven
 * team presets (see presets.ts); the sim auto-matches an enemy warband of roughly
 * equal power (power.ts), runs the REAL deterministic combat (battle.ts), and plays
 * it back as a watchable fight in the shared 3D arena. It owns a tiny internal state
 * machine — `select` (choose a team) and `battle` (VS intro → fight → result) — and
 * reuses the shared `FigureDirector`, combat playback, and audio cues, so nothing
 * here re-implements a system gameplay already has. It never decides a combat beat:
 * every attack/death comes from the event stream the engine produced.
 */

import { renderScene } from "@axiom/web-engine";
import type { LoadedContent } from "../../sim/content/load.ts";
import { deriveSeed } from "../../sim/rng.ts";
import { FIXED_HZ } from "../../sim/tuning.ts";
import { FigureDirector } from "../../figures/runtime/figure-director.ts";
import { buildArena } from "../../figures/scene/arena-scene.ts";
import { reconstructFrame } from "../../presentation/combat-playback.ts";
import type { CombatFrame, PlayUnit } from "../../presentation/combat-playback.ts";
import { AudioCues } from "../../audio/cues.ts";
import { type Rect, button, inRect, panel, shade, text } from "../../ui/draw.ts";
import { PALETTE } from "../../ui/theme.ts";
import type { Screen, ScreenNav } from "../screen.ts";
import { TEAM_PRESETS } from "./presets.ts";
import type { PresetUnit, TeamPreset } from "./presets.ts";
import { buildEnemyTeam, hashString, teamPower } from "./power.ts";
import { type BattleData, forgedById, runBattle } from "./battle.ts";

const STAGE = "tempered" as const;
const INTRO_TICKS = 42;
const PLAYBACK_SECONDS = 9;
const OFFSCREEN: Rect = { x: -100, y: -100, w: 0, h: 0 };

type Stage = "select" | "battle";
type BattlePhase = "intro" | "fight" | "done";

export class BattleSimScreen implements Screen {
  private readonly content: LoadedContent;
  private readonly nav: ScreenNav;
  private readonly figures: FigureDirector;
  private readonly audio = new AudioCues();

  private stage: Stage = "select";
  private tick = 0;

  // ── select ──
  private presetRects: Rect[] = [];
  private backRect: Rect = OFFSCREEN;

  // ── battle ──
  private presetIndex = 0;
  private enemyUnits: readonly PresetUnit[] = [];
  private data: BattleData | null = null;
  private forgedMap = new Map<number, boolean>();
  private cursor = 0;
  private audioCursor = 0;
  private phase: BattlePhase = "intro";
  private introTick = 0;
  private speed = 1;
  private enemyGen = 0;
  private rematchGen = 0;
  private frame: CombatFrame | null = null;
  private menuBtn: Rect = OFFSCREEN;
  private teamBtn: Rect = OFFSCREEN;
  private rematchBtn: Rect = OFFSCREEN;
  private newEnemyBtn: Rect = OFFSCREEN;
  private speedBtn: Rect = OFFSCREEN;
  private skipBtn: Rect = OFFSCREEN;

  public constructor(content: LoadedContent, nav: ScreenNav) {
    this.content = content;
    this.nav = nav;
    this.figures = new FigureDirector(content);
  }

  public enter(): void {
    this.enterSelect();
  }

  public exit(): void {
    this.figures.dispose();
  }

  // ── stage transitions ─────────────────────────────────────────────────────────
  private enterSelect(): void {
    this.stage = "select";
    this.data = null;
    this.frame = null;
    this.figures.dispose();
    buildArena(STAGE);
  }

  private startBattle(index: number): void {
    this.presetIndex = index;
    this.enemyGen = 0;
    this.regenEnemy();
    this.stage = "battle";
  }

  /** Draw a fresh enemy team of ~equal power, then run the fight from scratch. */
  private regenEnemy(): void {
    const preset = this.preset();
    const target = teamPower(this.content, preset.units);
    const enemySeed = deriveSeed(hashString(preset.id), this.enemyGen);
    this.enemyUnits = buildEnemyTeam(this.content, target, enemySeed);
    this.rematchGen = 0;
    this.runFight();
  }

  /** Re-run combat for the current teams (a new seed = new swings, same lineups). */
  private runFight(): void {
    const preset = this.preset();
    const seed = deriveSeed(hashString(preset.id), this.enemyGen, this.rematchGen, 0x5b);
    this.data = runBattle(this.content, preset.units, this.enemyUnits, seed);
    this.forgedMap = forgedById(this.data);
    this.cursor = 0;
    this.audioCursor = 0;
    this.phase = "intro";
    this.introTick = 0;
  }

  private preset(): TeamPreset {
    return TEAM_PRESETS[this.presetIndex] as TeamPreset;
  }

  // ── update ────────────────────────────────────────────────────────────────────
  public update(): void {
    if (this.stage !== "battle" || this.data === null) {
      return;
    }
    if (this.phase === "intro") {
      this.introTick += 1;
      if (this.introTick >= INTRO_TICKS) {
        this.phase = "fight";
      }
      return;
    }
    if (this.phase === "fight") {
      const len = this.data.stream.length;
      const perTick = Math.max(1, len / (PLAYBACK_SECONDS * FIXED_HZ)) * this.speed;
      const prev = this.cursor;
      this.cursor = Math.min(len, this.cursor + perTick);
      const crossed = this.data.stream.slice(Math.floor(prev), Math.floor(this.cursor));
      this.audio.play(crossed, 0);
      this.audioCursor = Math.floor(this.cursor);
      if (this.cursor >= len) {
        this.phase = "done";
      }
    }
  }

  // ── 3D ────────────────────────────────────────────────────────────────────────
  public renderScene3D(): void {
    this.tick += 1;
    if (this.stage === "select" || this.data === null) {
      renderScene();
      return;
    }
    const frame = reconstructFrame(this.data.snapA, this.data.snapB, this.data.stream, Math.floor(this.cursor));
    this.frame = frame;
    this.figures.syncScene(
      frame.units.map((unitFrame) => ({
        instanceId: unitFrame.instanceId,
        cardId: unitFrame.cardId,
        forged: this.forgedMap.get(unitFrame.instanceId) ?? false,
        slot: unitFrame.slot,
        enemy: unitFrame.side !== "a",
      })),
      STAGE,
    );
    this.figures.render(this.tick, this.phase === "intro" ? null : frame);
    renderScene();
  }

  // ── 2D overlay ──────────────────────────────────────────────────────────────
  public render(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    ctx.clearRect(0, 0, w, h);
    if (this.stage === "select") {
      this.renderSelect(ctx, w, h);
      return;
    }
    this.renderBattle(ctx, w, h);
  }

  private renderSelect(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    // A scrim so the preset list reads over the dim arena.
    ctx.save();
    ctx.globalAlpha = 0.72;
    ctx.fillStyle = PALETTE.bg0;
    ctx.fillRect(0, 0, w, h);
    ctx.restore();

    text(ctx, "BATTLE SIMULATOR", w / 2, Math.max(26, h * 0.07), { size: Math.min(30, w / 14), weight: 800, align: "center", color: PALETTE.brassLight });
    text(ctx, "PICK A TEAM · FACE A MATCHED FOE · WATCH THE FIGHT", w / 2, Math.max(46, h * 0.07) + 20, { size: Math.min(12, w / 34), weight: 700, align: "center", color: PALETTE.ember });

    const n = TEAM_PRESETS.length;
    const cardW = Math.min(w * 0.9, 460);
    const x0 = (w - cardW) / 2;
    const top = Math.max(70, h * 0.16);
    const backH = 44;
    const availH = h - top - backH - 24;
    const gap = 8;
    const cardH = Math.max(46, Math.min(76, (availH - gap * (n - 1)) / n));
    this.presetRects = [];
    TEAM_PRESETS.forEach((preset, i) => {
      const r: Rect = { x: x0, y: top + i * (cardH + gap), w: cardW, h: cardH };
      this.presetRects.push(r);
      this.drawPresetCard(ctx, preset, r);
    });

    this.backRect = { x: x0, y: h - backH - 12, w: cardW, h: backH };
    button(ctx, this.backRect, "◀ MAIN MENU", { fill: "#241d16", edge: PALETTE.brass, text: PALETTE.brassLight });
  }

  private drawPresetCard(ctx: CanvasRenderingContext2D, preset: TeamPreset, r: Rect): void {
    panel(ctx, r, "#1e1712", preset.accent, 7);
    // Accent spine.
    ctx.fillStyle = preset.accent;
    ctx.fillRect(r.x + 3, r.y + 3, 4, r.h - 6);
    const power = teamPower(this.content, preset.units);
    text(ctx, preset.name, r.x + 16, r.y + r.h * 0.34, { size: Math.min(16, r.h * 0.28), weight: 800, color: PALETTE.ink, max: r.w - 120 });
    text(ctx, preset.subtitle, r.x + 16, r.y + r.h * 0.68, { size: Math.min(11, r.h * 0.2), weight: 600, color: PALETTE.inkDim, max: r.w - 120 });
    text(ctx, `${preset.units.length} units`, r.x + r.w - 14, r.y + r.h * 0.34, { size: 11, weight: 700, align: "right", color: PALETTE.inkDim });
    text(ctx, `PWR ${power}`, r.x + r.w - 14, r.y + r.h * 0.68, { size: 13, weight: 800, align: "right", color: PALETTE.brassLight });
  }

  private renderBattle(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    const frame = this.frame;
    const preset = this.preset();
    const enemyPower = teamPower(this.content, this.enemyUnits);
    const myPower = teamPower(this.content, preset.units);

    const stripH = Math.max(40, h * 0.11);
    const pad = 8;
    // Enemy strip (top).
    const enemyArea: Rect = { x: pad, y: pad + 20, w: w - pad * 2, h: stripH };
    text(ctx, `CHALLENGER · PWR ${enemyPower}`, w - pad, pad + 12, { size: 11, weight: 800, align: "right", color: PALETTE.emberHot });
    this.drawRoster(ctx, this.sideUnits(frame, "b"), enemyArea, false);
    // Player strip (bottom).
    const myArea: Rect = { x: pad, y: h - stripH - pad, w: w - pad * 2, h: stripH };
    text(ctx, `${preset.name.toUpperCase()} · PWR ${myPower}`, pad, h - stripH - pad - 8, { size: 11, weight: 800, align: "left", color: PALETTE.brassLight });
    this.drawRoster(ctx, this.sideUnits(frame, "a"), myArea, true);

    // Persistent MENU control (top-left, clear of the enemy power label).
    this.menuBtn = { x: pad, y: pad, w: 52, h: 26 };
    button(ctx, this.menuBtn, "MENU", { fill: "#241d16", edge: PALETTE.brass, text: PALETTE.brassLight });

    this.resetBattleButtons();
    if (this.phase === "intro") {
      this.renderIntro(ctx, w, h, preset, enemyPower);
    } else if (this.phase === "fight") {
      this.renderFightHud(ctx, w, h);
    } else {
      this.renderResult(ctx, w, h);
    }
  }

  private renderIntro(ctx: CanvasRenderingContext2D, w: number, h: number, preset: TeamPreset, enemyPower: number): void {
    text(ctx, preset.name, w / 2, h * 0.4, { size: Math.min(24, w / 16), weight: 800, align: "center", color: PALETTE.brassLight });
    text(ctx, "VS", w / 2, h * 0.5, { size: Math.min(40, w / 9), weight: 800, align: "center", color: PALETTE.ember });
    text(ctx, `MATCHED CHALLENGER · PWR ${enemyPower}`, w / 2, h * 0.6, { size: Math.min(15, w / 24), weight: 700, align: "center", color: PALETTE.emberHot });
    // Pulse the prompt so it reads as interactive.
    const pulse = 0.5 + 0.5 * Math.sin(this.tick * 0.12);
    text(ctx, "TAP TO BEGIN", w / 2, h * 0.72, { size: 13, weight: 800, align: "center", color: `rgba(240,196,106,${0.4 + pulse * 0.6})` });
  }

  private renderFightHud(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    const len = this.data === null ? 1 : Math.max(1, this.data.stream.length);
    const frac = Math.min(1, this.cursor / len);
    // Progress bar across the middle-top.
    const bar: Rect = { x: w * 0.2, y: h * 0.18, w: w * 0.6, h: 6 };
    panel(ctx, bar, "#181310", PALETTE.panelEdge, 3);
    ctx.fillStyle = PALETTE.ember;
    ctx.fillRect(bar.x + 1, bar.y + 1, (bar.w - 2) * frac, bar.h - 2);
    // Speed + skip controls, top-right (below the menu row).
    this.speedBtn = { x: w - 60 - 8, y: h * 0.18 + 16, w: 60, h: 30 };
    button(ctx, this.speedBtn, `${this.speed}×`, { fill: "#1a1f26", edge: PALETTE.steel, text: PALETTE.ink });
    this.skipBtn = { x: w - 60 - 8 - 66, y: h * 0.18 + 16, w: 60, h: 30 };
    button(ctx, this.skipBtn, "SKIP", { fill: "#241d16", edge: PALETTE.brass, text: PALETTE.brassLight });
  }

  private renderResult(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    const result = this.data?.result;
    const won = result?.winnerSide === "a";
    const lost = result?.winnerSide === "b";
    const title = won ? "YOUR TEAM WINS" : lost ? "CHALLENGER WINS" : "DRAW";
    const color = won ? PALETTE.good : lost ? PALETTE.bad : PALETTE.brassLight;

    const bw = Math.min(w * 0.86, 460);
    const bx = (w - bw) / 2;
    const banner: Rect = { x: bx, y: h * 0.34, w: bw, h: 64 };
    shade(ctx, { x: 0, y: h * 0.32, w, h: 120 }, PALETTE.bg0, 0.55);
    panel(ctx, banner, "#1a1510", color, 8);
    text(ctx, title, w / 2, banner.y + 26, { size: Math.min(24, w / 16), weight: 800, align: "center", color });
    const survivors = result?.survivors ?? 0;
    const sub = result?.winnerSide === null ? "no survivors on either side" : `${survivors} survivor${survivors === 1 ? "" : "s"} standing`;
    text(ctx, sub, w / 2, banner.y + 48, { size: 12, weight: 700, align: "center", color: PALETTE.inkDim });

    // Action row.
    const btnH = 44;
    const gap = 8;
    const cols = 3;
    const cw = (bw - gap * (cols - 1)) / cols;
    const by = banner.y + banner.h + 12;
    this.rematchBtn = { x: bx, y: by, w: cw, h: btnH };
    this.newEnemyBtn = { x: bx + (cw + gap), y: by, w: cw, h: btnH };
    this.teamBtn = { x: bx + 2 * (cw + gap), y: by, w: cw, h: btnH };
    button(ctx, this.rematchBtn, "REMATCH", { fill: "#241d16", edge: PALETTE.brass, text: PALETTE.brassLight, sub: "new swings" });
    button(ctx, this.newEnemyBtn, "NEW FOE", { fill: "#2a1a10", edge: PALETTE.ember, text: PALETTE.brassLight, sub: "fresh enemy" });
    button(ctx, this.teamBtn, "TEAM", { fill: "#1a1f26", edge: PALETTE.steel, text: PALETTE.ink, sub: "change" });
  }

  private resetBattleButtons(): void {
    this.rematchBtn = OFFSCREEN;
    this.newEnemyBtn = OFFSCREEN;
    this.teamBtn = OFFSCREEN;
    this.speedBtn = OFFSCREEN;
    this.skipBtn = OFFSCREEN;
  }

  private sideUnits(frame: CombatFrame | null, side: "a" | "b"): PlayUnit[] {
    if (frame === null) {
      return [];
    }
    return frame.units.filter((unit) => unit.side === side).sort((x, y) => x.slot - y.slot);
  }

  private drawRoster(ctx: CanvasRenderingContext2D, units: readonly PlayUnit[], area: Rect, mine: boolean): void {
    const n = Math.max(1, units.length);
    const gap = 4;
    const cw = (area.w - gap * (n - 1)) / n;
    units.forEach((unit, i) => {
      const r: Rect = { x: area.x + i * (cw + gap), y: area.y, w: cw, h: area.h };
      const dead = !unit.alive;
      const edge = dead ? "#3a2f26" : mine ? PALETTE.brass : PALETTE.emberHot;
      panel(ctx, r, dead ? "#141010" : "#221a14", edge, 5);
      const flashing = !dead && this.audioCursor - unit.hitFlash >= 0 && this.audioCursor - unit.hitFlash < 4;
      if (flashing) {
        shade(ctx, r, PALETTE.emberHot, 0.4);
      }
      const name = this.content.card(unit.cardId).name;
      text(ctx, name, r.x + r.w / 2, r.y + r.h * 0.34, { size: Math.min(10, r.w * 0.16), weight: 700, align: "center", color: dead ? "#6b6157" : PALETTE.ink, max: r.w - 4 });
      text(ctx, `${unit.attack}/${unit.health}`, r.x + r.w / 2, r.y + r.h * 0.74, { size: Math.min(13, r.w * 0.24), weight: 800, align: "center", color: dead ? "#6b6157" : mine ? PALETTE.good : PALETTE.bad });
    });
  }

  // ── input ─────────────────────────────────────────────────────────────────────
  public onPointerDown(x: number, y: number): void {
    if (this.stage === "select") {
      if (inRect(this.backRect, x, y)) {
        this.nav.goto("main_menu");
        return;
      }
      const hit = this.presetRects.findIndex((r) => inRect(r, x, y));
      if (hit >= 0) {
        this.startBattle(hit);
      }
      return;
    }
    if (inRect(this.menuBtn, x, y)) {
      this.nav.goto("main_menu");
      return;
    }
    if (this.phase === "intro") {
      this.phase = "fight";
      this.introTick = INTRO_TICKS;
      return;
    }
    if (this.phase === "fight") {
      if (inRect(this.speedBtn, x, y)) {
        this.cycleSpeed();
      } else if (inRect(this.skipBtn, x, y)) {
        this.skipToEnd();
      }
      return;
    }
    // done
    if (inRect(this.rematchBtn, x, y)) {
      this.rematch();
    } else if (inRect(this.newEnemyBtn, x, y)) {
      this.newEnemy();
    } else if (inRect(this.teamBtn, x, y)) {
      this.enterSelect();
    }
  }

  public onPointerMove(): void {
    // No drag interactions.
  }

  public onPointerUp(): void {
    // Selection happens on press.
  }

  public onKey(key: string): void {
    if (key === "Escape") {
      if (this.stage === "battle") {
        this.enterSelect();
      } else {
        this.nav.goto("main_menu");
      }
      return;
    }
    if (key === "Enter" && this.stage === "battle" && this.phase === "intro") {
      this.phase = "fight";
      this.introTick = INTRO_TICKS;
    }
  }

  private cycleSpeed(): void {
    this.speed = this.speed === 1 ? 2 : this.speed === 2 ? 4 : 1;
  }

  private skipToEnd(): void {
    if (this.data !== null) {
      this.cursor = this.data.stream.length;
      this.phase = "done";
    }
  }

  private rematch(): void {
    this.rematchGen += 1;
    this.runFight();
  }

  private newEnemy(): void {
    this.enemyGen += 1;
    this.regenEnemy();
  }

  // ── dev/test hooks ─────────────────────────────────────────────────────────────
  public debugStage(): { stage: Stage; phase: BattlePhase; presets: number } {
    return { stage: this.stage, phase: this.phase, presets: TEAM_PRESETS.length };
  }

  public debugStartBattle(index: number): void {
    this.startBattle(index);
  }

  public debugFinish(): void {
    this.skipToEnd();
  }

  public debugResult(): { winner: string | null; survivors: number; myPower: number; enemyPower: number } | null {
    if (this.data === null) {
      return null;
    }
    return {
      winner: this.data.result.winnerSide,
      survivors: this.data.result.survivors,
      myPower: teamPower(this.content, this.preset().units),
      enemyPower: teamPower(this.content, this.enemyUnits),
    };
  }
}
