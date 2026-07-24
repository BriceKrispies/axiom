/*
 * figure-director.ts — the single presentation orchestrator the game owns. It
 * builds the 3D scene (floor + lights + one figure per placement) and re-poses it
 * every tick from a neutral list of FIGURE PLACEMENTS plus an optional combat
 * playback frame. It is deliberately MATCH-STATE-FREE: callers (the gameplay
 * screen, the battle simulator) translate their own state into `FigurePlacement`s,
 * so one director serves every screen that shows figures. Because the engine
 * exposes no per-node despawn (only `clearScene`), it rebuilds the whole scene only
 * when the SET of placements changes (a buy/sell/forge/summon/move — discrete, rare
 * events), and otherwise just re-poses existing nodes each frame (cheap, no
 * spawning). It never decides anything: attack/damage/death come from the event-
 * derived combat frame; figures only visualize them.
 */

import { clearScene, rendererBackendName } from "@axiom/web-engine";
import type { LoadedContent } from "../../sim/content/load.ts";
import type { CombatFrame } from "../../presentation/combat-playback.ts";
import { type Vec3, add, quatFromEulerXyz, vec3 } from "../vec3.ts";
import type { QualityTier } from "../parts.ts";
import type { RootFrame } from "../compose.ts";
import { FigureInstance } from "../scene/figure-instance.ts";
import { buildArena, enemySlotPos, warbandSlotPos } from "../scene/arena-scene.ts";
import { resetMeshCache } from "../primitives.ts";
import { resetMaterialCache } from "../scene/materials.ts";

const FACE_CAMERA = quatFromEulerXyz(0, 0, 0);
const FACE_AWAY = quatFromEulerXyz(0, Math.PI, 0);

type ArenaStageLite = "workshop" | "kindled" | "tempered" | "masterwork";

/**
 * A neutral request to place one figure on the arena. The caller derives these
 * from its own state (gameplay from `MatchState`, the battle sim from the combat
 * frame + its team snapshots), so the director depends on nothing above it. `enemy`
 * selects the far row + facing; `instanceId` matches the placement to its combat
 * unit for attack/defend/death visualization.
 */
export interface FigurePlacement {
  readonly instanceId: number;
  readonly cardId: string;
  readonly forged: boolean;
  readonly slot: number;
  readonly enemy: boolean;
}

interface Placed {
  readonly inst: FigureInstance;
  readonly instanceId: number;
  readonly slot: number;
  readonly enemy: boolean;
}

export class FigureDirector {
  private readonly content: LoadedContent;
  private readonly quality: QualityTier;
  private placed: Placed[] = [];
  private signature = "";

  public constructor(content: LoadedContent) {
    this.content = content;
    this.quality = rendererBackendName() === "WebGL2" ? "med" : "low";
  }

  public qualityTier(): QualityTier {
    return this.quality;
  }

  public liveFigureCount(): number {
    return this.placed.length;
  }

  /** Rebuild the scene iff the placement set (or stage) changed; called each tick
   * before pose. Cheap when nothing structural changed (a pure signature compare). */
  public syncScene(placements: readonly FigurePlacement[], stage: ArenaStageLite): void {
    const sig = `${stage}|${placements
      .map((p) => `${p.enemy ? "e" : "a"}:${p.instanceId}:${p.cardId}:${p.forged ? 1 : 0}:${p.slot}`)
      .join(",")}`;
    if (sig === this.signature) {
      return;
    }
    this.signature = sig;
    clearScene();
    resetMeshCache();
    resetMaterialCache();
    buildArena(stage);
    this.placed = placements.map((p) => ({
      inst: new FigureInstance(this.content, p.cardId, p.forged, this.quality),
      instanceId: p.instanceId,
      slot: p.slot,
      enemy: p.enemy,
    }));
  }

  /** Re-pose every figure for this tick (idle sway; combat attacker/defender/dead).
   * `combat === null` is the static warband / VS-tableau idle. */
  public render(tick: number, combat: CombatFrame | null): void {
    for (const it of this.placed) {
      const unit = combat?.units.find((u) => u.instanceId === it.instanceId);
      if (combat !== null && (unit === undefined || !unit.alive)) {
        it.inst.park();
        continue;
      }
      // Shared deterministic idle (breathing / weapon-ready) — the same animator the
      // gallery uses — layered under the slot/lunge root frame.
      it.inst.animateIdle(tick);
      it.inst.pose(this.frameFor(it, tick, combat));
    }
  }

  private frameFor(it: Placed, tick: number, combat: CombatFrame | null): RootFrame {
    const bob = Math.sin(tick * 0.06 + it.slot * 1.3) * 0.02;
    const base: Vec3 = it.enemy ? enemySlotPos(it.slot) : warbandSlotPos(it.slot);
    const facing = it.enemy ? FACE_AWAY : FACE_CAMERA;
    if (combat === null) {
      return { position: add(base, vec3(0, bob, 0)), rotation: facing, scale: 1 };
    }
    const attacking = combat.attacker === it.instanceId;
    const defending = combat.defender === it.instanceId;
    const forward = it.enemy ? -1 : 1;
    const lunge = attacking ? 0.22 : defending ? -0.08 : 0;
    return { position: add(base, vec3(0, bob, forward * lunge)), rotation: facing, scale: 1 };
  }

  /** Drop the whole scene (screen exit / restart). */
  public dispose(): void {
    clearScene();
    resetMeshCache();
    resetMaterialCache();
    this.placed = [];
    this.signature = "";
  }
}
