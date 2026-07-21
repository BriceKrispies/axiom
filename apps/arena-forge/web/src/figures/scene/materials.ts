/*
 * materials.ts — resolves a (group language, material role) pair to a cached engine
 * material handle from the group palette. Materials are created ONCE and reused
 * across every figure of a group — a part's material is fixed at spawn (glow/flash
 * come from dedicated emissive parts, never from recoloring), matching the sibling
 * scene discipline. `resetMaterialCache()` mirrors the engine's `clearScene`.
 */

import { createMaterial } from "@axiom/web-engine";
import type { Handle, MaterialSpec } from "@axiom/web-engine";
import type { GroupId } from "../../sim/ids.ts";
import type { MaterialRole } from "../parts.ts";
import { languageFor } from "../languages/index.ts";

const cache = new Map<string, Handle>();

export const materialFor = (languageId: GroupId | "neutral", role: MaterialRole): Handle => {
  const key = `${languageId}:${role}`;
  const hit = cache.get(key);
  if (hit !== undefined) {
    return hit;
  }
  const lang = languageFor(languageId);
  const emissive = lang.emissiveRoles?.[role];
  const opacity = lang.opacityRoles?.[role];
  const spec: MaterialSpec = {
    baseColor: lang.palette[role],
    ...(emissive ? { emissive } : {}),
    ...(opacity !== undefined ? { opacity } : {}),
  };
  const handle = createMaterial(spec);
  cache.set(key, handle);
  return handle;
};

/** A raw material for effect/modifier decorations (not from a group palette). */
export const rawMaterial = (spec: MaterialSpec): Handle => createMaterial(spec);

export const resetMaterialCache = (): void => cache.clear();
