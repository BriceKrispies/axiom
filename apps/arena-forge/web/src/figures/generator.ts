/*
 * generator.ts — expands a declarative `FigureDefinition` into a concrete, flat
 * list of parts ready for composition and spawning. It resolves tier / forged /
 * LOD gating, mirror twins, and bounded repetition, and re-parents each part to
 * its index in the final array (parent-before-child preserved). Deterministic:
 * repeat counts and any jitter come from the figure's seed (`variation.ts`), never
 * randomness. Pure and SDK-free — it emits data; the scene layer turns each part
 * into an engine node with a cached mesh + a role-resolved material.
 */

import { type Vec3, ZERO, add, scale, vec3 } from "./vec3.ts";
import { QUALITY_ORDER } from "./parts.ts";
import type { AttachPoint, MaterialRole, PrimitiveType, QualityTier } from "./parts.ts";
import type { FigureDefinition, FigurePartDefinition, RestTransform } from "./grammar.ts";
import type { ComposePart } from "./compose.ts";
import { figureSeed, pickInt } from "./variation.ts";

export interface ExpandedPart {
  readonly id: string;
  readonly tag: FigurePartDefinition["tag"];
  readonly primitive: PrimitiveType;
  readonly material: MaterialRole;
  readonly attach?: AttachPoint;
  readonly parentIndex: number;
  readonly compose: ComposePart;
  readonly bounds: boolean;
}

export interface ExpandedFigure {
  readonly cardId: string;
  readonly parts: readonly ExpandedPart[];
  readonly attachIndex: ReadonlyMap<AttachPoint, number>;
  readonly groundY: number;
  readonly footprint: number;
}

const addRest = (base: RestTransform, step: RestTransform, k: number): RestTransform => ({
  position: add(base.position, scale(step.position, k)),
  rotationEuler: add(base.rotationEuler, scale(step.rotationEuler, k)),
  scale: base.scale * Math.pow(step.scale, k),
});

const mirrorRest = (r: RestTransform, axis: "x" | "z"): RestTransform => ({
  position: axis === "x" ? vec3(-r.position.x, r.position.y, r.position.z) : vec3(r.position.x, r.position.y, -r.position.z),
  // Flip the two Euler components that mirror across the chosen plane.
  rotationEuler: axis === "x" ? vec3(r.rotationEuler.x, -r.rotationEuler.y, -r.rotationEuler.z) : vec3(-r.rotationEuler.x, -r.rotationEuler.y, r.rotationEuler.z),
  scale: r.scale,
});

const composeOf = (part: FigurePartDefinition, rest: RestTransform, parentIndex: number): ComposePart => ({
  parentIndex,
  rest,
  extents: part.extents,
  offset: part.offset ?? ZERO,
});

/** Expand a figure for a given quality + forged state into flat parts. */
export const expandFigure = (def: FigureDefinition, quality: QualityTier, forged: boolean): ExpandedFigure => {
  const seed = figureSeed(def.cardId, def.seedSalt);
  const q = QUALITY_ORDER[quality];
  const authored: FigurePartDefinition[] = [...def.parts, ...(forged ? def.forgedAugment ?? [] : [])];

  // Pass 1: which authored parts survive tier / forged / LOD gating + parent chain.
  const kept = new Set<string>();
  for (const part of authored) {
    const tierOk = part.tierMin === undefined || def.tier >= part.tierMin;
    const forgedOk = !part.forgedOnly || forged;
    const lodOk = part.lodMin === undefined || q >= QUALITY_ORDER[part.lodMin];
    const parentOk = part.parent === null || kept.has(part.parent);
    if (tierOk && forgedOk && lodOk && parentOk) {
      kept.add(part.id);
    }
  }

  // Pass 2: emit base + mirror/repeat expansions; record the base index per id.
  const parts: ExpandedPart[] = [];
  const baseIndex = new Map<string, number>();
  const attachIndex = new Map<AttachPoint, number>();
  const push = (id: string, src: FigurePartDefinition, rest: RestTransform, parentIndex: number): number => {
    const index = parts.length;
    parts.push({
      id,
      tag: src.tag,
      primitive: src.primitive,
      material: src.material,
      ...(src.attach ? { attach: src.attach } : {}),
      parentIndex,
      compose: composeOf(src, rest, parentIndex),
      bounds: src.bounds ?? true,
    });
    if (src.attach !== undefined && !attachIndex.has(src.attach)) {
      attachIndex.set(src.attach, index);
    }
    return index;
  };

  for (const part of authored) {
    if (!kept.has(part.id)) {
      continue;
    }
    const parentIndex = part.parent === null ? -1 : baseIndex.get(part.parent) ?? -1;
    const base = push(part.id, part, part.rest, parentIndex);
    baseIndex.set(part.id, base);

    if (part.repeat !== undefined) {
      const rep = part.repeat;
      const cap = rep.countVariationKey !== undefined ? pickInt(seed, rep.countVariationKey, rep.min ?? 1, rep.count) : rep.count;
      for (let k = 1; k < cap; k += 1) {
        push(`${part.id}#${k}`, part, addRest(part.rest, rep.step, k), parentIndex);
      }
    } else if (part.mirror !== undefined) {
      push(`${part.id}${part.mirror.idSuffix}`, part, mirrorRest(part.rest, part.mirror.axis), parentIndex);
    }
  }

  return { cardId: def.cardId, parts, attachIndex, groundY: def.groundY, footprint: def.footprint };
};

/** Total primitive count of an expanded figure — the budget-check quantity. */
export const partCount = (fig: ExpandedFigure): number => fig.parts.length;

/** A crude grounding/selection AABB half-extent from bounds-contributing parts. */
export const figureBounds = (fig: ExpandedFigure): { min: Vec3; max: Vec3 } => {
  let min = vec3(Infinity, Infinity, Infinity);
  let max = vec3(-Infinity, -Infinity, -Infinity);
  for (const p of fig.parts) {
    if (!p.bounds) {
      continue;
    }
    const c = p.compose.rest.position;
    const e = p.compose.extents;
    min = vec3(Math.min(min.x, c.x - e.x), Math.min(min.y, c.y - e.y), Math.min(min.z, c.z - e.z));
    max = vec3(Math.max(max.x, c.x + e.x), Math.max(max.y, c.y + e.y), Math.max(max.z, c.z + e.z));
  }
  return { min, max };
};
