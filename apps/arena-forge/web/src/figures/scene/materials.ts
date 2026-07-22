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
  // `roughness` is authored per role (glossy metal/plate vs matte cloth/organic) and
  // is inert in today's diffuse-only engine — it becomes live the moment the engine
  // gains a specular term, at which point these materials are already correct.
  const roughness = lang.roughnessRoles?.[role];
  const spec: MaterialSpec = {
    baseColor: lang.palette[role],
    ...(emissive ? { emissive } : {}),
    ...(opacity !== undefined ? { opacity } : {}),
    ...(roughness !== undefined ? { roughness } : {}),
  };
  const handle = createMaterial(spec);
  cache.set(key, handle);
  return handle;
};

/** A shared dark, translucent material for the flattened contact-shadow blob that
 * grounds a gallery miniature. Cached like the palette materials (cleared on
 * `resetMaterialCache`, which mirrors the engine's `clearScene`). */
export const contactShadowMaterial = (): Handle => {
  const key = "__contact_shadow";
  const hit = cache.get(key);
  if (hit !== undefined) {
    return hit;
  }
  const handle = createMaterial({ baseColor: [0.02, 0.02, 0.03, 1], opacity: 0.42 });
  cache.set(key, handle);
  return handle;
};

/** A raw material for effect/modifier decorations (not from a group palette). */
export const rawMaterial = (spec: MaterialSpec): Handle => createMaterial(spec);

export const resetMaterialCache = (): void => cache.clear();
