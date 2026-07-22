/*
 * bodyplans.ts — the procedural figure grammar. Each group has a body-plan builder
 * that assembles a coherent `FigurePartDefinition[]` from the group's primitive
 * dialect, scaled by tier and varied by the card's stable seed. Group-mates share
 * palette + primitive vocabulary + proportions dialect (coherence), while every
 * card differs on ≥3 seed-driven attributes — proportions, weapon/focal choice,
 * crest/orbiter counts, accent placement (individual silhouette). This is the
 * data-driven grammar the miniatures are generated from; bespoke per-card overrides
 * can layer on top later. Pure/SDK-free.
 */

import { type Vec3, vec3 } from "./vec3.ts";
import type { Tier } from "../sim/content/schema.ts";
import type { FigureDefinition, FigurePartDefinition, RestTransform, Silhouette } from "./grammar.ts";
import type { AttachPoint, MaterialRole, PrimitiveType, SemanticPart } from "./parts.ts";
import { figureSeed, pickFloat, pickInt } from "./variation.ts";

type Group = "ironbound" | "emberkin" | "bloomtide" | "echowisp" | "neutral";

interface PartOpts {
  readonly rot?: Vec3;
  readonly offset?: Vec3;
  readonly attach?: AttachPoint;
  readonly mirror?: FigurePartDefinition["mirror"];
  readonly repeat?: FigurePartDefinition["repeat"];
  readonly tierMin?: Tier;
  readonly forgedOnly?: boolean;
  readonly lodMin?: FigurePartDefinition["lodMin"];
}

const rest = (pos: Vec3, rot: Vec3 = vec3(0, 0, 0), s = 1): RestTransform => ({ position: pos, rotationEuler: rot, scale: s });

const P = (
  id: string,
  parent: string | null,
  tag: SemanticPart,
  primitive: PrimitiveType,
  material: MaterialRole,
  pos: Vec3,
  ext: Vec3,
  opts: PartOpts = {},
): FigurePartDefinition => ({
  id,
  parent,
  tag,
  primitive,
  material,
  rest: rest(pos, opts.rot ?? vec3(0, 0, 0)),
  extents: ext,
  ...(opts.offset ? { offset: opts.offset } : {}),
  ...(opts.attach ? { attach: opts.attach } : {}),
  ...(opts.mirror ? { mirror: opts.mirror } : {}),
  ...(opts.repeat ? { repeat: opts.repeat } : {}),
  ...(opts.tierMin ? { tierMin: opts.tierMin } : {}),
  ...(opts.forgedOnly ? { forgedOnly: opts.forgedOnly } : {}),
  ...(opts.lodMin ? { lodMin: opts.lodMin } : {}),
});

const tierScale = (tier: Tier): number => 0.82 + tier * 0.055;

// ── Ironbound: rigid forged-metal constructs ──────────────────────────────────
const ironbound = (seed: number, tier: Tier): FigurePartDefinition[] => {
  const w = pickFloat(seed, "torsoW", 0.44, 0.62);
  const bulk = pickFloat(seed, "bulk", 0.9, 1.15);
  const heavyWeapon = pickInt(seed, "weapon", 0, 2); // 0 hammer, 1 blade, 2 fist
  const parts: FigurePartDefinition[] = [
    P("base", null, "base", "plate", "metal", vec3(0, 0.08, 0), vec3(w + 0.3, 0.14, 0.5)),
    P("torso", "base", "torso", "rounded_box", "primary", vec3(0, 0.72 * bulk, 0), vec3(w, 0.7 * bulk, 0.4)),
    P("chest", "torso", "shell", "rounded_box", "metal", vec3(0, 0.1, 0.16), vec3(w * 0.8, 0.4, 0.14), { attach: "chest" }),
    // A layered breastplate: a beveled gorget collar over the chest and a hanging
    // fauld skirt at the waist, so the front reads as stacked plate, not one slab.
    P("gorget", "torso", "shell", "rounded_box", "metal", vec3(0, 0.34 * bulk, 0.14), vec3(w * 0.66, 0.12, 0.26)),
    P("fauld", "torso", "back", "plate", "secondary", vec3(0, -0.3, 0.1), vec3(w * 1.02, 0.24, 0.24), { rot: vec3(0.16, 0, 0) }),
    P("head", "torso", "head", "box", "metal", vec3(0, 0.55 * bulk, 0.02), vec3(0.3, 0.3, 0.32), { attach: "crown" }),
    P("eye", "head", "eye", "box", "eye", vec3(0, 0.02, 0.17), vec3(0.16, 0.05, 0.04)),
    // Overlapping pauldrons: a big beveled cap dome with a lower lame layered under it.
    P("shoulder", "torso", "shoulder", "plate", "accent", vec3(w * 0.62, 0.28, 0), vec3(0.26, 0.2, 0.34), { mirror: { axis: "x", idSuffix: "_r" } }),
    P("pauldron_cap", "torso", "shoulder", "rounded_box", "metal", vec3(w * 0.66, 0.36, 0), vec3(0.36, 0.22, 0.42), { rot: vec3(0, 0, -0.16), mirror: { axis: "x", idSuffix: "_r" } }),
    // Articulated arm on BOTH sides: an upper arm, a beveled forearm couter, and a
    // chunky gauntlet fist at the wrist — so each limb terminates in a fist (a
    // defining knight silhouette) instead of a stub. All mirrored across x.
    P("arm", "torso", "upper_arm", "box", "primary", vec3(w * 0.66, 0.1, 0), vec3(0.17, 0.3, 0.18), { mirror: { axis: "x", idSuffix: "_r" } }),
    P("forearm", "torso", "fore_arm", "rounded_box", "metal", vec3(w * 0.66, -0.3, 0.02), vec3(0.155, 0.24, 0.165), { mirror: { axis: "x", idSuffix: "_r" } }),
    P("fist", "torso", "hand", "rounded_box", "metal", vec3(w * 0.66, -0.58, 0.05), vec3(0.18, 0.16, 0.2), { mirror: { axis: "x", idSuffix: "_r" } }),
    // Braced heavy-warrior stance: the legs plant SHOULDER-WIDE so the two thighs
    // read as a distinct, grounded pair (at w*0.36 their inner edges crossed the
    // centreline and fused into one limp central pillar — a mannequin, not a bruiser).
    P("leg", "base", "thigh", "box", "primary", vec3(w * 0.58, 0.34, 0), vec3(0.19, 0.5, 0.2), { mirror: { axis: "x", idSuffix: "_r" } }),
  ];
  // Weapon in the right hand (a fixed hero hand, not mirrored).
  parts.push(P("hand", "torso", "hand", "box", "metal", vec3(w * 0.72, -0.32, 0.08), vec3(0.16, 0.16, 0.16), { attach: "right_hand" }));
  if (heavyWeapon === 0) {
    parts.push(P("haft", "hand", "weapon", "cylinder", "metal", vec3(0, 0.28, 0), vec3(0.08, 0.6, 0.08)));
    parts.push(P("head_w", "haft", "weapon", "box", "accent", vec3(0, 0.34, 0), vec3(0.34, 0.24, 0.24), { attach: "weapon_tip" }));
  } else if (heavyWeapon === 1) {
    parts.push(P("blade", "hand", "weapon", "wedge", "accent", vec3(0, 0.5, 0), vec3(0.16, 0.9, 0.06), { attach: "weapon_tip" }));
  } else {
    parts.push(P("gauntlet", "hand", "weapon", "rounded_box", "accent", vec3(0, -0.02, 0.06), vec3(0.28, 0.26, 0.3), { attach: "weapon_tip" }));
  }
  // Off hand shield for durable silhouettes.
  parts.push(P("shield", "torso", "shield", "plate", "accent", vec3(-w * 0.78, -0.05, 0.12), vec3(0.36, 0.56, 0.1), { rot: vec3(0, 0.2, 0) }));
  // Segmented tassets guarding the hips (a second waist layer under the fauld).
  parts.push(P("tasset", "base", "thigh", "plate", "metal", vec3(w * 0.52, 0.5, 0.12), vec3(0.24, 0.32, 0.14), { tierMin: 2, rot: vec3(0.12, 0, 0), mirror: { axis: "x", idSuffix: "_r" } }));
  // A lower pauldron lame that overlaps the cap, added as the figure gets fancier.
  parts.push(P("pauldron_lame", "torso", "shoulder", "plate", "metal", vec3(w * 0.66, 0.15, 0.02), vec3(0.32, 0.13, 0.38), { tierMin: 3, lodMin: "med", rot: vec3(0.22, 0, -0.1), mirror: { axis: "x", idSuffix: "_r" } }));
  // Tier embellishments.
  parts.push(P("crest", "head", "crest", "wedge", "accent", vec3(0, 0.2, -0.02), vec3(0.2, 0.24, 0.14), { tierMin: 3, rot: vec3(-0.3, 0, 0) }));
  parts.push(P("furnace", "chest", "ember_seam", "box", "emissive_core", vec3(0, 0, 0.06), vec3(0.14, 0.18, 0.05), { tierMin: 4 }));
  parts.push(P("banner", "torso", "banner", "plate", "accent", vec3(-w * 0.3, 0.9, -0.2), vec3(0.24, 0.7, 0.03), { tierMin: 5, rot: vec3(0.1, 0, 0) }));
  return parts;
};

// ── Emberkin: lean aggressive figures around a bright ember core ───────────────
const emberkin = (seed: number, tier: Tier): FigurePartDefinition[] => {
  const lean = pickFloat(seed, "lean", 0.32, 0.46);
  const horns = pickInt(seed, "horns", 1, 3);
  const parts: FigurePartDefinition[] = [
    // The base must NOT be rotated — it is the root, so any rotation would flip the
    // whole body. The downward molten spike is a separate LEAF (its flip affects
    // only itself).
    P("base", null, "base", "cylinder", "shadow_base", vec3(0, 0.1, 0), vec3(0.52, 0.2, 0.52)),
    P("emberfoot", "base", "ember_seam", "cone", "glow", vec3(0, -0.05, 0), vec3(0.36, 0.3, 0.36), { rot: vec3(Math.PI, 0, 0), lodMin: "med" }),
    P("torso", "base", "torso", "capsule", "primary", vec3(0, 0.78, 0), vec3(lean, 0.9, lean)),
    P("core", "torso", "core", "sphere", "emissive_core", vec3(0, 0.05, 0.02), vec3(0.34, 0.34, 0.34), { attach: "chest" }),
    P("head", "torso", "head", "wedge", "primary", vec3(0, 0.56, 0.02), vec3(0.28, 0.3, 0.3)),
    P("eye", "head", "eye", "box", "eye", vec3(0, 0, 0.15), vec3(0.14, 0.04, 0.03)),
    P("horn", "head", "crest", "cone", "accent", vec3(0.1, 0.18, -0.02), vec3(0.12, 0.34, 0.12), { repeat: { count: horns, mode: "fan", step: rest(vec3(-0.1, 0, 0), vec3(0, 0, 0.3)) } }),
    P("arm", "torso", "upper_arm", "capsule", "secondary", vec3(lean + 0.14, 0.06, 0), vec3(0.13, 0.62, 0.13), { rot: vec3(0, 0, -0.3), mirror: { axis: "x", idSuffix: "_r" } }),
    P("leg", "base", "thigh", "capsule", "secondary", vec3(lean * 0.6, 0.3, 0), vec3(0.13, 0.5, 0.13), { mirror: { axis: "x", idSuffix: "_r" } }),
    P("hand", "torso", "hand", "sphere", "secondary", vec3(lean + 0.34, -0.28, 0.06), vec3(0.14, 0.14, 0.14), { attach: "right_hand" }),
    P("blade", "hand", "weapon", "cone", "accent", vec3(0, 0.34, 0.1), vec3(0.12, 0.7, 0.12), { rot: vec3(0.6, 0, 0), attach: "weapon_tip" }),
    P("seam", "torso", "ember_seam", "box", "glow", vec3(0.0, 0.28, 0.14), vec3(0.06, 0.34, 0.04), { tierMin: 3, mirror: { axis: "x", idSuffix: "_r" } }),
    P("flame", "head", "flame_tongue", "cone", "glow", vec3(0, 0.36, -0.04), vec3(0.16, 0.4, 0.16), { tierMin: 4, lodMin: "med" }),
    P("mantle", "torso", "back", "wedge", "accent", vec3(0, 0.2, -0.22), vec3(0.5, 0.7, 0.08), { tierMin: 5, rot: vec3(0.2, 0, 0) }),
  ];
  return parts;
};

// ── Bloomtide: plants, fungi, seeds — no humanoid requirement ───────────────────
const bloomtide = (seed: number, tier: Tier): FigurePartDefinition[] => {
  const petals = pickInt(seed, "petals", 4, 7);
  const stemBend = pickFloat(seed, "bend", -0.12, 0.12);
  const capKind: PrimitiveType = pickInt(seed, "cap", 0, 1) === 0 ? "sphere" : "cone";
  const parts: FigurePartDefinition[] = [
    P("base", null, "base", "plate", "metal", vec3(0, 0.06, 0), vec3(0.6, 0.1, 0.6)),
    P("root", "base", "vine", "segmented", "metal", vec3(0.18, 0.16, 0.1), vec3(0.3, 0.4, 0.3), { rot: vec3(0.4, 0, 0.3), repeat: { count: 3, mode: "fan", step: rest(vec3(0, 0, 0), vec3(0, 2.1, 0)) } }),
    P("stem", "base", "stem", "capsule", "primary", vec3(0, 0.74, 0), vec3(0.22, 0.95, 0.22), { rot: vec3(stemBend, 0, stemBend) }),
    P("cap", "stem", "cap", capKind, "secondary", vec3(0, 0.62, 0), vec3(0.5, 0.5, 0.5), { attach: "crown" }),
    P("petal", "cap", "petal", "wedge", "accent", vec3(0.28, 0, 0), vec3(0.34, 0.1, 0.5), { rot: vec3(0, 0, -0.5), repeat: { count: petals, mode: "ring", step: rest(vec3(0, 0, 0), vec3(0, (Math.PI * 2) / petals, 0)) } }),
    P("core", "cap", "core", "sphere", "emissive_core", vec3(0, 0, 0.04), vec3(0.22, 0.22, 0.22), { attach: "chest" }),
    P("eye", "cap", "eye", "sphere", "eye", vec3(0.08, 0.05, 0.2), vec3(0.07, 0.07, 0.07)),
    P("leaf", "stem", "leaf", "wedge", "primary", vec3(0.24, -0.1, 0), vec3(0.4, 0.06, 0.28), { rot: vec3(0, 0, -0.7), mirror: { axis: "x", idSuffix: "_r" } }),
    P("frond", "stem", "leaf", "wedge", "primary", vec3(0.2, 0.3, 0.1), vec3(0.34, 0.05, 0.24), { tierMin: 3, rot: vec3(0.3, 0.6, -0.5), mirror: { axis: "x", idSuffix: "_r" } }),
    P("pod", "cap", "spore_node", "sphere", "glow", vec3(0.22, 0.24, 0.1), vec3(0.14, 0.14, 0.14), { tierMin: 4, lodMin: "med", repeat: { count: 3, mode: "ring", step: rest(vec3(0, 0, 0), vec3(0, 2.1, 0)) } }),
    P("bloom", "cap", "petal", "wedge", "glow", vec3(0.16, 0.3, 0), vec3(0.22, 0.08, 0.3), { tierMin: 5, rot: vec3(-0.5, 0, -0.4), repeat: { count: 5, mode: "ring", step: rest(vec3(0, 0, 0), vec3(0, 1.256, 0)) } }),
  ];
  return parts;
};

// ── Echowisp: floating, asymmetric magical constructs — a core + satellites ─────
const echowisp = (seed: number, tier: Tier): FigurePartDefinition[] => {
  const orbiters = pickInt(seed, "orbiters", 2, 4);
  const hover = pickFloat(seed, "hover", 0.85, 1.05);
  const parts: FigurePartDefinition[] = [
    P("base", null, "base", "ring", "shadow_base", vec3(0, 0.03, 0), vec3(0.7, 0.05, 0.7)),
    P("core", "base", "core", "sphere", "primary", vec3(0, hover, 0), vec3(0.5, 0.56, 0.5), { attach: "core" }),
    P("inner", "core", "core", "sphere", "emissive_core", vec3(0, 0, 0), vec3(0.28, 0.28, 0.28), { attach: "chest" }),
    P("mask", "core", "face", "plate", "accent", vec3(0, 0.04, 0.24), vec3(0.36, 0.44, 0.08)),
    P("eye", "mask", "eye", "sphere", "eye", vec3(0, 0, 0.06), vec3(0.1, 0.1, 0.06)),
    P("halo", "core", "ring_accent", "ring", "accent", vec3(0, 0.34, 0), vec3(0.7, 0.1, 0.7), { rot: vec3(0.5, 0, 0), attach: "crown" }),
    P("orbiter", "core", "orbiter", "sphere", "glow", vec3(0.52, 0, 0), vec3(0.16, 0.16, 0.16), { repeat: { count: orbiters, mode: "ring", step: rest(vec3(0, 0, 0), vec3(0, (Math.PI * 2) / orbiters, 0)) }, countVariationKey: "orbiters" } as PartOpts),
    P("hand", "core", "hand", "wedge", "secondary", vec3(0.42, -0.34, 0.05), vec3(0.2, 0.26, 0.1), { attach: "right_hand", mirror: { axis: "x", idSuffix: "_r" } }),
    P("ring2", "core", "ring_accent", "ring", "secondary", vec3(0, 0, 0), vec3(0.9, 0.14, 0.9), { rot: vec3(1.2, 0.4, 0), tierMin: 3 }),
    P("shard", "core", "satellite", "cone", "glow", vec3(0, 0.6, 0), vec3(0.14, 0.4, 0.14), { tierMin: 4, lodMin: "med", repeat: { count: 3, mode: "ring", step: rest(vec3(0.3, 0, 0), vec3(0, 2.1, 0)) } }),
    P("afterimage", "core", "afterimage", "billboard", "glow", vec3(-0.35, 0, -0.1), vec3(0.5, 0.6, 1), { tierMin: 5, lodMin: "med" }),
  ];
  return parts;
};

// ── Neutral: muted utility constructs, not of any group ─────────────────────────
const neutral = (seed: number, tier: Tier): FigurePartDefinition[] => {
  const wide = pickFloat(seed, "wide", 0.4, 0.56);
  const parts: FigurePartDefinition[] = [
    P("base", null, "base", "cylinder", "metal", vec3(0, 0.1, 0), vec3(0.5, 0.2, 0.5)),
    P("torso", "base", "torso", "box", "primary", vec3(0, 0.68, 0), vec3(wide, 0.66, 0.4)),
    // A cheap beveled chest plate + shin greave keep the utility construct reading as
    // worked metal, coherent with the Ironbound plate dialect without new machinery.
    P("chestplate", "torso", "shell", "rounded_box", "metal", vec3(0, 0.12, 0.18), vec3(wide * 0.82, 0.34, 0.12), { attach: "chest" }),
    P("pack", "torso", "back", "box", "secondary", vec3(0, 0.05, -0.26), vec3(wide * 0.7, 0.5, 0.16)),
    P("head", "torso", "head", "cylinder", "metal", vec3(0, 0.5, 0.02), vec3(0.26, 0.28, 0.26), { attach: "crown" }),
    P("eye", "head", "eye", "box", "eye", vec3(0, 0.02, 0.14), vec3(0.14, 0.05, 0.03)),
    P("arm", "torso", "upper_arm", "cylinder", "secondary", vec3(wide * 0.7, 0, 0), vec3(0.15, 0.5, 0.15), { mirror: { axis: "x", idSuffix: "_r" } }),
    P("leg", "base", "thigh", "cylinder", "secondary", vec3(wide * 0.4, 0.3, 0), vec3(0.15, 0.42, 0.15), { mirror: { axis: "x", idSuffix: "_r" } }),
    P("greave", "base", "shin", "rounded_box", "metal", vec3(wide * 0.4, 0.24, 0.08), vec3(0.17, 0.24, 0.16), { tierMin: 2, mirror: { axis: "x", idSuffix: "_r" } }),
    P("tool", "torso", "weapon", "plate", "accent", vec3(wide * 0.9, -0.2, 0.12), vec3(0.14, 0.5, 0.1), { attach: "right_hand", rot: vec3(0.2, 0, 0) }),
    P("lamp", "head", "crest", "sphere", "glow", vec3(0, 0.2, 0.05), vec3(0.14, 0.14, 0.14), { tierMin: 4, lodMin: "med" }),
  ];
  return parts;
};

const BUILDERS: Readonly<Record<Group, (seed: number, tier: Tier) => FigurePartDefinition[]>> = {
  ironbound, emberkin, bloomtide, echowisp, neutral,
};

const silhouetteFor = (group: Group, tier: Tier): Silhouette =>
  group === "echowisp" ? "caster" : tier >= 5 ? "colossus" : tier <= 2 ? "grunt" : "bruiser";

/** Build the full procedural figure definition for a card. */
export const buildFigureDefinition = (cardId: string, group: Group, tier: Tier, seedSalt: number): FigureDefinition => {
  const seed = figureSeed(cardId, seedSalt);
  const parts = BUILDERS[group](seed, tier);
  const scale = tierScale(tier);
  // A forged augmentation: an emissive halo + brighter core accent, group-tinted.
  const forgedAugment: FigurePartDefinition[] = [
    P("forge_halo", "base", "ring_accent", "ring", "glow", vec3(0, 0.02, 0), vec3(0.9 * scale, 0.06, 0.9 * scale), { forgedOnly: true, lodMin: "med" }),
    P("forge_crown", parts.some((p) => p.id === "head") ? "head" : "core", "crown", "cone", "accent", vec3(0, 0.24, 0), vec3(0.18, 0.3, 0.18), { forgedOnly: true }),
  ];
  return {
    cardId,
    language: group,
    silhouette: silhouetteFor(group, tier),
    tier,
    parts: parts.map((p) => (p.parent === null ? { ...p, rest: { ...p.rest, scale: p.rest.scale * scale } } : p)),
    forgedAugment,
    animation: `${group}_default`,
    seedSalt,
    groundY: 0,
    footprint: 0.4 * scale,
  };
};
