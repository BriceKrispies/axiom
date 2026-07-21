/*
 * figure-director.ts — the single presentation orchestrator the game owns. It
 * builds the 3D scene (floor + lights + one figure per live unit) and re-poses it
 * every tick from immutable simulation state + the combat playback frame. Because
 * the engine exposes no per-node despawn (only `clearScene`), it rebuilds the whole
 * scene only when the SET of figures changes (a buy/sell/forge/summon — discrete,
 * rare events), and otherwise just re-poses existing nodes each frame (cheap, no
 * spawning). It never decides anything: attack/damage/death come from the event-
 * derived combat frame; figures only visualize them.
 */

import { clearScene, rendererBackendName } from "@axiom/web-engine";
import type { LoadedContent } from "../../sim/content/load.ts";
import type { MatchState, PlayerState } from "../../sim/model.ts";
import type { CombatFrame } from "../../presentation/combat-playback.ts";
import { type Vec3, add, quatFromEulerXyz, vec3 } from "../vec3.ts";
import type { QualityTier } from "../parts.ts";
import type { RootFrame } from "../compose.ts";
import { FigureInstance } from "../scene/figure-instance.ts";
import { buildArena, enemySlotPos, warbandSlotPos } from "../scene/arena-scene.ts";
import { resetMeshCache } from "../primitives.ts";
import { resetMaterialCache } from "../scene/materials.ts";

const HUMAN = 0;
const FACE_CAMERA = quatFromEulerXyz(0, 0, 0);
const FACE_AWAY = quatFromEulerXyz(0, Math.PI, 0);

interface Placed {
  readonly inst: FigureInstance;
  readonly slot: number;
  readonly enemy: boolean;
}

export class FigureDirector {
  private readonly quality: QualityTier;
  private placed: Placed[] = [];
  private signature = "";
  private stage: ArenaStageLite = "workshop";

  public constructor(private readonly content: LoadedContent) {
    this.quality = rendererBackendName() === "WebGL2" ? "med" : "low";
  }

  public qualityTier(): QualityTier {
    return this.quality;
  }

  public liveFigureCount(): number {
    return this.placed.length;
  }

  /** Rebuild the scene iff the figure set changed; called each tick before pose. */
  public sync(view: MatchState, combat: CombatFrame | null): void {
    const inCombat = view.phase.startsWith("combat") && combat !== null;
    const sig = inCombat ? this.combatSignature(view, combat) : this.warbandSignature(view);
    const stage = (view.players[HUMAN] as PlayerState).presentationStage as ArenaStageLite;
    if (sig === this.signature && stage === this.stage) {
      return;
    }
    this.signature = sig;
    this.stage = stage;
    this.rebuild(view, combat, inCombat);
  }

  private warbandSignature(view: MatchState): string {
    const p = view.players[HUMAN] as PlayerState;
    return `w|${view.phase}|${p.warband.map((u) => (u === null ? "-" : `${u.cardId}:${u.forged ? 1 : 0}`)).join(",")}`;
  }

  private combatSignature(view: MatchState, combat: CombatFrame): string {
    return `c|${combat.units.map((u) => `${u.side}:${u.instanceId}:${u.cardId}`).sort().join(",")}`;
  }

  private rebuild(view: MatchState, combat: CombatFrame | null, inCombat: boolean): void {
    clearScene();
    resetMeshCache();
    resetMaterialCache();
    buildArena(this.stage);
    this.placed = [];
    if (inCombat && combat !== null) {
      const mySide = this.humanSide(view, combat);
      for (const u of combat.units) {
        const enemy = u.side !== mySide;
        this.placed.push({ inst: new FigureInstance(this.content, u.cardId, false, this.quality), slot: u.slot, enemy });
      }
      return;
    }
    const p = view.players[HUMAN] as PlayerState;
    p.warband.forEach((u, slot) => {
      if (u !== null) {
        this.placed.push({ inst: new FigureInstance(this.content, u.cardId, u.forged, this.quality), slot, enemy: false });
      }
    });
  }

  private humanSide(view: MatchState, combat: CombatFrame): "a" | "b" {
    const human = view.players[HUMAN] as PlayerState;
    const aIsHuman = combat.units.some((u) => u.side === "a" && human.warband.some((w) => w?.instanceId === u.instanceId));
    return aIsHuman ? "a" : "b";
  }

  /** Re-pose every figure for this tick (idle sway; combat attacker/defender/dead). */
  public render(tick: number, combat: CombatFrame | null): void {
    for (let i = 0; i < this.placed.length; i += 1) {
      const it = this.placed[i] as Placed;
      const unit = combat?.units.find((u) => u.instanceId === this.combatUnitId(it, combat));
      if (combat !== null && unit !== undefined && !unit.alive) {
        it.inst.park();
        continue;
      }
      it.inst.pose(this.frameFor(it, tick, combat));
    }
  }

  private combatUnitId(it: Placed, combat: CombatFrame): number {
    // Match by slot+side during combat (placed order mirrors combat.units order).
    const idx = this.placed.indexOf(it);
    return combat.units[idx]?.instanceId ?? -1;
  }

  private frameFor(it: Placed, tick: number, combat: CombatFrame | null): RootFrame {
    const bob = Math.sin(tick * 0.06 + it.slot * 1.3) * 0.02;
    if (combat === null) {
      return { position: add(warbandSlotPos(it.slot), vec3(0, bob, 0)), rotation: FACE_CAMERA, scale: 1 };
    }
    const base: Vec3 = it.enemy ? enemySlotPos(it.slot) : warbandSlotPos(it.slot);
    const unit = combat.units[this.placed.indexOf(it)];
    const attacking = unit !== undefined && combat.attacker === unit.instanceId;
    const defending = unit !== undefined && combat.defender === unit.instanceId;
    const forward = it.enemy ? -1 : 1;
    const lunge = attacking ? 0.22 : defending ? -0.08 : 0;
    return {
      position: add(base, vec3(0, bob, forward * lunge)),
      rotation: it.enemy ? FACE_AWAY : FACE_CAMERA,
      scale: 1,
    };
  }

  /** Drop the whole scene (restart). */
  public dispose(): void {
    clearScene();
    resetMeshCache();
    resetMaterialCache();
    this.placed = [];
    this.signature = "";
  }
}

type ArenaStageLite = "workshop" | "kindled" | "tempered" | "masterwork";
