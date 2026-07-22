/*
 * grammar.ts — the typed, declarative figure-generation grammar. A `FigureDefinition`
 * is pure DATA: a flat, parent-before-child list of `FigurePartDefinition`s (modeled
 * on Rust `axiom-figure`'s `{parent, rest, boxSize, boxOffset, tag}`), a group visual
 * language it inherits palette + shape dialect from, and refs to animation + modifier
 * profiles. There is NO behavior here and no per-card renderer code — the generator
 * interprets these records. Adding a card only adds data; adding a new body plan or
 * primitive is the one thing that touches the generator, reviewed as a grammar change.
 *
 * Geometric contract (the engine transform is a single TRS with NO shear): a part's
 * `rest` propagates only RIGID + UNIFORM scale to its children; a part's non-uniform
 * `extents` are applied ONLY at its own leaf node. `compose.ts` enforces this.
 */

import type { Tier } from "../sim/content/schema.ts";
import type { CardId, GroupId } from "../sim/ids.ts";
import type { Vec3 } from "./vec3.ts";
import type { AttachPoint, MaterialRole, PrimitiveType, QualityTier, SemanticPart } from "./parts.ts";

/** An RGBA color in linear-ish 0..1 (structurally the engine's `Rgba`). */
export type Rgba = readonly [number, number, number, number];

/** A rigid + uniform-scale local transform relative to the parent part. */
export interface RestTransform {
  readonly position: Vec3;
  /** Intrinsic XYZ Euler angles in radians. */
  readonly rotationEuler: Vec3;
  /** UNIFORM scale only (non-uniform shape comes from `extents`). */
  readonly scale: number;
}

export const REST_IDENTITY: RestTransform = { position: { x: 0, y: 0, z: 0 }, rotationEuler: { x: 0, y: 0, z: 0 }, scale: 1 };

export interface MirrorSpec {
  readonly axis: "x" | "z";
  /** Appended to the part id to name the generated twin (e.g. "_r"). */
  readonly idSuffix: string;
}

export interface RepeatSpec {
  /** Copies to emit (hard-capped by validation). */
  readonly count: number;
  readonly mode: "ring" | "fan" | "stack" | "row";
  /** Per-copy delta applied cumulatively. */
  readonly step: RestTransform;
  /** If set, the visual seed picks a count in `[min, count]`. */
  readonly countVariationKey?: string;
  readonly min?: number;
}

/** One part of a figure. `parent` must name an EARLIER part (or null for the root). */
export interface FigurePartDefinition {
  readonly id: string;
  readonly parent: string | null;
  readonly tag: SemanticPart;
  readonly primitive: PrimitiveType;
  readonly rest: RestTransform;
  /** Non-uniform primitive extents (applied only at this leaf). */
  readonly extents: Vec3;
  /** Pre-rotation local offset of the primitive from the joint pivot. */
  readonly offset?: Vec3;
  readonly material: MaterialRole;
  readonly attach?: AttachPoint;
  readonly mirror?: MirrorSpec;
  readonly repeat?: RepeatSpec;
  /** Present only at/above this figure tier (complexity progression). */
  readonly tierMin?: Tier;
  /** Present only on the forged augmentation. */
  readonly forgedOnly?: boolean;
  /** Couples this part's deterministic jitter to a named seed channel. */
  readonly variationKey?: string;
  /** Contributes to grounding + selection bounds (default true). */
  readonly bounds?: boolean;
  /** Dropped below this quality tier. */
  readonly lodMin?: QualityTier;
}

/** The strategic silhouette class (drives default proportions/animation weight). */
export type Silhouette = "grunt" | "bruiser" | "caster" | "colossus" | "swarmling" | "token";

/** The card-specific miniature. */
export interface FigureDefinition {
  readonly cardId: CardId;
  readonly language: GroupId | "neutral";
  readonly silhouette: Silhouette;
  readonly tier: Tier;
  readonly parts: readonly FigurePartDefinition[];
  /** Extra parts merged in when the unit is forged. */
  readonly forgedAugment?: readonly FigurePartDefinition[];
  /** Named animation profile (see anim/profiles.ts). */
  readonly animation: string;
  /** Stable base for the visual seed (mixed with the card id). */
  readonly seedSalt: number;
  /** Feet plane in figure space (for grounding). */
  readonly groundY: number;
  /** Slot hit-target radius. */
  readonly footprint: number;
}

/** The shared palette + shape dialect a group's figures inherit. */
export interface GroupVisualLanguage {
  readonly id: GroupId | "neutral";
  readonly palette: Readonly<Record<MaterialRole, Rgba>>;
  readonly emissiveRoles?: Partial<Readonly<Record<MaterialRole, Rgba>>>;
  readonly opacityRoles?: Partial<Readonly<Record<MaterialRole, number>>>;
  /** Per-role surface roughness in 0..1 (0 = glossy/metal, 1 = matte/cloth). Inert
   * in the diffuse-only engine today; authored so metal/plate roles read glossy and
   * cloth/accent/organic roles read matte the moment an engine specular term lands.
   * Unset ⇒ the material leaves `roughness` undefined (engine treats it as matte). */
  readonly roughnessRoles?: Partial<Readonly<Record<MaterialRole, number>>>;
  readonly preferredPrimitives: readonly PrimitiveType[];
  readonly jointStyle: "rigid" | "beveled" | "organic";
  readonly defaultAnimation: string;
  /** Accent used for this group's modifier decorations. */
  readonly modifierTint: Rgba;
  /** Must equal the `groups.ts` accent hex (validated). */
  readonly groupColorHex: string;
}

/** How a modifier is classified for visual selection + priority. */
export type ModifierClass = "keyword" | "temp_attack" | "temp_protect" | "permanent_attack" | "permanent_health" | "forged" | "aura" | "debuff";

/** A data-driven modifier visual (no status-specific renderer code). */
export interface FigureModifierVisual {
  readonly id: string;
  readonly triggerClass: ModifierClass;
  /** Optional exact keyword match (e.g. "ward", "guard", "armored"). */
  readonly keyword?: string;
  readonly shape: "shell" | "ring" | "plates" | "aura" | "halo" | "seam" | "orbiter";
  readonly primitive: PrimitiveType;
  readonly attach: AttachPoint;
  readonly material: { readonly baseColor: Rgba; readonly emissive?: Rgba; readonly opacity?: number };
  /** Layer order — higher is drawn/scaled further out. */
  readonly priority: number;
  readonly pulseHz?: number;
  /** Played once when the modifier ends (e.g. ward break). */
  readonly consumeAnim?: "pop" | "shatter" | "fade";
  readonly lodMin?: QualityTier;
}
