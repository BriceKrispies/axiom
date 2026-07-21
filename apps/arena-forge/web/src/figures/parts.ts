/*
 * parts.ts — the semantic vocabulary of the figure grammar: the named part slots,
 * material roles, effect attach points, primitive kinds, and quality tiers. These
 * are the nouns every `FigureDefinition` and animation/modifier definition targets
 * BY NAME (never by scene-node path), so content stays renderer-agnostic and the
 * validator can prove every reference resolves. Modeled on the Rust
 * `axiom-figure` part `tag` + `axiom-animation` channel vocabulary (design ref).
 */

/** A named semantic part. Figures need not be humanoid — plant, wisp, construct,
 * and floating forms all draw from this shared set. */
export type SemanticPart =
  | "root" | "base" | "pedestal" | "body" | "core" | "torso" | "shell"
  | "head" | "face" | "eye" | "crest" | "crown" | "banner"
  | "shoulder" | "upper_arm" | "fore_arm" | "hand" | "weapon" | "shield" | "back"
  | "hip" | "thigh" | "shin" | "foot"
  | "tail" | "wing" | "petal" | "leaf" | "stem" | "vine" | "cap"
  | "spore_node" | "flame_tongue" | "ember_seam"
  | "afterimage" | "orbiter" | "satellite" | "ring_accent" | "accent";

/** A material role resolved to a concrete material once, from the group palette.
 * A part's material is FIXED at spawn — glow/flash come from dedicated nodes. */
export type MaterialRole =
  | "primary" | "secondary" | "accent" | "metal"
  | "emissive_core" | "glow" | "eye" | "shadow_base";

/** An anchor tag on a part where effects / modifier decorations attach. */
export type AttachPoint =
  | "core" | "chest" | "crown" | "feet" | "aura_center" | "overhead"
  | "weapon_tip" | "left_hand" | "right_hand";

/** The primitive kinds the generator can instantiate. `box`/`sphere`/`cylinder`
 * use the engine built-ins; the rest are generated via `meshgen.ts`. */
export type PrimitiveType =
  | "box" | "rounded_box" | "sphere" | "capsule" | "cylinder"
  | "cone" | "wedge" | "plate" | "ring" | "segmented" | "billboard";

export const PRIMITIVE_TYPES: readonly PrimitiveType[] = [
  "box", "rounded_box", "sphere", "capsule", "cylinder",
  "cone", "wedge", "plate", "ring", "segmented", "billboard",
];

export const MATERIAL_ROLES: readonly MaterialRole[] = [
  "primary", "secondary", "accent", "metal", "emissive_core", "glow", "eye", "shadow_base",
];

/** Mobile quality tiers; drives LOD gating and mesh segment counts. */
export type QualityTier = "low" | "med" | "high";

export const QUALITY_ORDER: Readonly<Record<QualityTier, number>> = { low: 0, med: 1, high: 2 };
