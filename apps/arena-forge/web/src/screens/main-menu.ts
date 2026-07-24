/*
 * main-menu.ts — the Arena Forge title screen: the entrance to an arcane
 * industrial tournament. It builds a REAL forge scene (the shared arena platform
 * + lights) with one genuine procedural representative from each launch group
 * idling on the platform, plus drifting forge embers, then overlays a large
 * stamped-metal ARENA FORGE title plaque and the PLAY / FIGURE LAB forge
 * controls. The figures use their real `figureForCard` definitions, materials,
 * and quality behavior — there are no menu-only models. No gameplay simulation
 * runs behind the menu.
 */

import { rendererBackendName, renderScene, setNodeTransform, spawnRenderable } from "@axiom/web-engine";
import type { Entity } from "@axiom/web-engine";
import type { LoadedContent } from "../sim/content/load.ts";
import type { QualityTier } from "../figures/parts.ts";
import type { RootFrame } from "../figures/compose.ts";
import { type Vec3, quatFromEulerXyz, vec3 } from "../figures/vec3.ts";
import { FigureInstance } from "../figures/scene/figure-instance.ts";
import { rawMaterial } from "../figures/scene/materials.ts";
import { meshFor } from "../figures/primitives.ts";
import { buildArena } from "../figures/scene/arena-scene.ts";
import { type Rect, button, inRect, panel, rivet, text } from "../ui/draw.ts";
import { PALETTE } from "../ui/theme.ts";
import { type Screen, type ScreenNav, resetEngineScene } from "./screen.ts";

const REPRESENTATIVES: readonly string[] = ["iron_recruit", "ember_stoker", "bloom_pod_tender", "echo_flicker_sprite"];
const EMBER_COUNT = 14;

interface Ember {
  readonly entity: Entity;
  readonly x: number;
  readonly z: number;
  readonly speed: number;
  readonly phase: number;
  readonly size: number;
}

export class MainMenuScreen implements Screen {
  private readonly quality: QualityTier;
  private reps: FigureInstance[] = [];
  private embers: Ember[] = [];
  private tick = 0;
  private play: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private sim: Rect = { x: 0, y: 0, w: 0, h: 0 };
  private lab: Rect = { x: 0, y: 0, w: 0, h: 0 };

  public constructor(private readonly content: LoadedContent, private readonly nav: ScreenNav) {
    this.quality = rendererBackendName() === "WebGL2" ? "med" : "low";
  }

  public enter(): void {
    resetEngineScene();
    buildArena("kindled");
    this.reps = REPRESENTATIVES.filter((id) => this.content.cards.some((c) => c.id === id)).map(
      (id) => new FigureInstance(this.content, id, false, this.quality),
    );
    const emberMat = rawMaterial({ baseColor: [1, 0.62, 0.28, 1], emissive: [1, 0.5, 0.16, 1], opacity: 0.85 });
    const billboard = meshFor("billboard", this.quality);
    this.embers = Array.from({ length: EMBER_COUNT }, (_, i) => {
      const entity = spawnRenderable(billboard, emberMat, { position: vec3(0, -100, 0), rotation: [0, 0, 0, 1], scale: vec3(0.05, 0.05, 0.05) });
      // Deterministic spread (no randomness).
      const x = ((i * 137) % 90) / 90 * 8 - 4;
      const z = ((i * 71) % 50) / 50 * 3 - 1.5;
      return { entity, x, z, speed: 0.006 + (i % 5) * 0.002, phase: (i * 0.7) % 3, size: 0.04 + (i % 3) * 0.02 };
    });
    this.tick = 0;
  }

  public exit(): void {
    resetEngineScene();
    this.reps = [];
    this.embers = [];
  }

  public update(): void {
    this.tick += 1;
  }

  public renderScene3D(): void {
    const spread = this.reps.length > 1 ? 1.7 : 0;
    this.reps.forEach((rep, i) => {
      const x = (i - (this.reps.length - 1) / 2) * spread;
      const bob = Math.sin(this.tick * 0.045 + i * 1.3) * 0.025;
      const turn = Math.sin(this.tick * 0.012 + i) * 0.25;
      const root: RootFrame = { position: vec3(x, bob, 0.4), rotation: quatFromEulerXyz(0, turn, 0), scale: 1.05 };
      rep.pose(root);
    });
    for (const ember of this.embers) {
      const t = (this.tick * ember.speed + ember.phase) % 3;
      const pos: Vec3 = vec3(ember.x + Math.sin(this.tick * 0.02 + ember.phase) * 0.3, 0.2 + t, ember.z + 1.2);
      const flicker = ember.size * (0.7 + 0.3 * Math.sin(this.tick * 0.2 + ember.phase));
      setNodeTransform(ember.entity, { position: pos, rotation: [0, 0, 0, 1], scale: vec3(flicker, flicker, flicker) });
    }
    renderScene();
  }

  public render(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    // Transparent overlay so the 3D forge shows through.
    ctx.clearRect(0, 0, w, h);

    // ── Title plaque (stamped metal, heavy border, rivets) ──
    const pw = Math.min(w * 0.8, 620);
    const ph = Math.min(h * 0.26, 150);
    const plaque: Rect = { x: (w - pw) / 2, y: Math.max(10, h * 0.06), w: pw, h: ph };
    panel(ctx, plaque, "#1c1712", PALETTE.brass, 10);
    panel(ctx, { x: plaque.x + 5, y: plaque.y + 5, w: pw - 10, h: ph - 10 }, "#231b13", PALETTE.brassLight, 8);
    for (const [rx, ry] of [[0.04, 0.16], [0.96, 0.16], [0.04, 0.84], [0.96, 0.84]] as const) {
      rivet(ctx, plaque.x + pw * rx, plaque.y + ph * ry, PALETTE.brassLight);
    }
    text(ctx, "ARENA FORGE", w / 2, plaque.y + ph * 0.42, { size: Math.min(46, pw / 9), weight: 800, align: "center", color: PALETTE.brassLight });
    text(ctx, "ARCANE INDUSTRIAL TOURNAMENT", w / 2, plaque.y + ph * 0.74, { size: Math.min(13, pw / 34), weight: 700, align: "center", color: PALETTE.ember });

    // ── Forge-control buttons (large, ≥44px). Three across on wide screens,
    //    stacked on narrow ones. ──
    const bh = Math.max(50, h * 0.12);
    const gap = 12;
    const cx = w / 2;
    if (w >= 700) {
      const bw = Math.min(w * 0.27, 240);
      const total = bw * 3 + gap * 2;
      const x0 = cx - total / 2;
      const by = h - bh - Math.max(18, h * 0.07);
      this.play = { x: x0, y: by, w: bw, h: bh };
      this.sim = { x: x0 + (bw + gap), y: by, w: bw, h: bh };
      this.lab = { x: x0 + 2 * (bw + gap), y: by, w: bw, h: bh };
    } else {
      const bw = Math.min(w * 0.82, 320);
      const x0 = cx - bw / 2;
      const by = h - Math.max(14, h * 0.05) - bh;
      this.lab = { x: x0, y: by, w: bw, h: bh };
      this.sim = { x: x0, y: by - bh - 10, w: bw, h: bh };
      this.play = { x: x0, y: by - 2 * (bh + 10), w: bw, h: bh };
    }
    button(ctx, this.play, "PLAY", { fill: "#2a1a10", edge: PALETTE.ember, text: PALETTE.brassLight, sub: "ENTER THE ARENA" });
    button(ctx, this.sim, "BATTLE SIM", { fill: "#26170f", edge: PALETTE.emberHot, text: PALETTE.brassLight, sub: "PICK · WATCH · FIGHT" });
    button(ctx, this.lab, "FIGURE LAB", { fill: "#1a1f26", edge: PALETTE.steel, text: PALETTE.ink, sub: "INSPECT · ANIMATE" });

    text(ctx, "build v0.2 · 38 figures · canvas2d/webgl2", 10, h - 12, { size: 10, weight: 700, color: PALETTE.inkDim });
  }

  public onPointerDown(x: number, y: number): void {
    if (inRect(this.play, x, y)) {
      this.nav.goto("gameplay");
      return;
    }
    if (inRect(this.sim, x, y)) {
      this.nav.goto("battle_sim");
      return;
    }
    if (inRect(this.lab, x, y)) {
      this.nav.goto("figure_lab");
    }
  }

  public onPointerMove(): void {
    // No drag interactions on the menu.
  }

  public onPointerUp(): void {
    // Selection happens on press.
  }
}
