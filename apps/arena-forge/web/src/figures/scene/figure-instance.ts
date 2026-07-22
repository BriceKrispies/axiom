/*
 * figure-instance.ts — one live miniature in the scene. It spawns one engine node
 * per part ONCE (from cached meshes + role-resolved materials), then re-poses them
 * each tick by composing the part hierarchy to world transforms — no per-tick
 * spawning, no per-tick geometry rebuild, reused compose buffers. Parking (moving a
 * node far below with tiny scale) hides it without despawning, matching the sibling
 * pooling discipline. This is the only figure file besides the scene builder that
 * touches the engine store.
 */

import { setNodeTransform, spawnRenderable } from "@axiom/web-engine";
import type { Entity } from "@axiom/web-engine";
import type { LoadedContent } from "../../sim/content/load.ts";
import type { CardId } from "../../sim/ids.ts";
import { type Vec3, vec3 } from "../vec3.ts";
import type { QualityTier } from "../parts.ts";
import { type ExpandedPart, expandFigure } from "../generator.ts";
import { type WorldTransform, composeBuffers, composeWorld } from "../compose.ts";
import type { ComposePart, PoseDelta, RootFrame } from "../compose.ts";
import { meshFor } from "../primitives.ts";
import { figureForCard } from "../registry.ts";
import { figureSeed } from "../variation.ts";
import { idleDeltaForTag } from "../anim/idle.ts";
import { contactShadowMaterial, materialFor } from "./materials.ts";

const PARKED: WorldTransform = { position: vec3(0, -1000, 0), rotation: [0, 0, 0, 1], scale: vec3(1e-4, 1e-4, 1e-4) };
const REST_ROOT: RootFrame = { position: vec3(0, 0, 0), rotation: [0, 0, 0, 1], scale: 1 };
const TAU = Math.PI * 2;

export class FigureInstance {
  public readonly cardId: CardId;
  public readonly forged: boolean;
  public readonly partCount: number;
  /** Rest-pose bounding height in world units at root scale 1 (framing/fitting). */
  public readonly height: number;
  /** Rest-pose bounding-box centre height — the pivot to spin a figure about so it
   * tumbles in place instead of swinging around its feet. */
  public readonly midY: number;
  private readonly entities: Entity[];
  private readonly composeParts: ComposePart[];
  private readonly parts: readonly ExpandedPart[];
  private readonly buf: ReturnType<typeof composeBuffers>;
  private readonly poses: (PoseDelta | undefined)[];
  /** A stable per-figure phase so group-mates do not breathe in lockstep. */
  private readonly idlePhase: number;
  /** The grounding contact-shadow blob (gallery only), or null when the host scene
   * already has a real floor (the gameplay arena). */
  private readonly shadow: Entity | null;
  private visible = false;

  public constructor(content: LoadedContent, cardId: CardId, forged: boolean, quality: QualityTier, groundShadow = false) {
    this.cardId = cardId;
    this.forged = forged;
    const def = figureForCard(content, cardId);
    const fig = expandFigure(def, quality, forged);
    this.parts = fig.parts;
    this.composeParts = fig.parts.map((p) => p.compose);
    this.partCount = fig.parts.length;
    this.buf = composeBuffers(this.partCount);
    this.poses = fig.parts.map(() => undefined);
    this.idlePhase = ((figureSeed(cardId, 17) % 1000) / 1000) * TAU;
    this.entities = fig.parts.map((p) => spawnRenderable(meshFor(p.primitive, quality), materialFor(def.language, p.material), PARKED));
    this.shadow = groundShadow ? spawnRenderable(meshFor("sphere", quality), contactShadowMaterial(), PARKED) : null;

    // Measure the rest pose ONCE (CPU-only; the nodes stay parked). Composing at
    // the identity root gives the figure's own bounding height + centre, which is
    // what any caller needs to FIT a miniature into a slot — the gallery grid
    // sizes and spins every figure by these.
    composeWorld(this.composeParts, REST_ROOT, this.poses, this.buf.frames, this.buf.out);
    const lo = this.buf.out.reduce((m, t) => Math.min(m, t.position.y - t.scale.y / 2), Infinity);
    const hi = this.buf.out.reduce((m, t) => Math.max(m, t.position.y + t.scale.y / 2), -Infinity);
    this.height = Math.max(1e-3, hi - lo);
    this.midY = (lo + hi) / 2;
  }

  /** Set a per-part animation offset (index by part order); undefined clears it. */
  public setPose(index: number, delta: PoseDelta | undefined): void {
    if (index >= 0 && index < this.poses.length) {
      this.poses[index] = delta;
    }
  }

  public clearPoses(): void {
    for (let i = 0; i < this.poses.length; i += 1) {
      this.poses[i] = undefined;
    }
  }

  /**
   * Apply the shared deterministic idle (breathing + weapon-ready stance) for a given
   * tick. It writes a per-part `PoseDelta` from the part's semantic tag, so the SAME
   * animation plays in the gameplay arena and the gallery — the animator is shared,
   * not screen-local. `tick` is caller-supplied (never wall-clock), so it is replayable.
   */
  public animateIdle(tick: number): void {
    for (let i = 0; i < this.parts.length; i += 1) {
      this.poses[i] = idleDeltaForTag((this.parts[i] as ExpandedPart).tag, tick, this.idlePhase);
    }
  }

  /** Pose the whole figure at a world root frame and push transforms to the store. */
  public pose(root: RootFrame): void {
    composeWorld(this.composeParts, root, this.poses, this.buf.frames, this.buf.out);
    for (let i = 0; i < this.entities.length; i += 1) {
      setNodeTransform(this.entities[i] as Entity, this.buf.out[i] as WorldTransform);
    }
    if (this.shadow !== null) {
      // A flattened, translucent blob at the figure's feet so a floating gallery
      // miniature reads as grounded. Sized from the figure's own height, squashed in
      // Y, and nudged behind the figure so it never z-fights the body.
      const r = this.height * root.scale * 0.34;
      setNodeTransform(this.shadow, {
        position: vec3(root.position.x, root.position.y - r * 0.06, root.position.z - 0.06),
        rotation: [0, 0, 0, 1],
        scale: vec3(r, r * 0.14, r),
      });
    }
    this.visible = true;
  }

  public park(): void {
    if (!this.visible) {
      return;
    }
    for (const e of this.entities) {
      setNodeTransform(e, PARKED);
    }
    if (this.shadow !== null) {
      setNodeTransform(this.shadow, PARKED);
    }
    this.visible = false;
  }

  /** World position of a named attach point's part (for effect anchoring). */
  public partWorldPos(index: number): Vec3 {
    return (this.buf.out[index] as WorldTransform).position;
  }
}
