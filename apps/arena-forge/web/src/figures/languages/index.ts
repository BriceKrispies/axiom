/*
 * languages/index.ts — the five GroupVisualLanguages (Ironbound, Emberkin,
 * Bloomtide, Echowisp, Neutral). A language is the shared palette + primitive
 * dialect + animation defaults every figure of that group inherits, so group-mates
 * are immediately coherent while each card varies by seed. `groupColorHex` must
 * equal the sim's `groups.ts` accent (a validation check). This is presentation
 * data only; the simulation never sees a color.
 */

import type { GroupId } from "../../sim/ids.ts";
import type { GroupVisualLanguage, Rgba } from "../grammar.ts";

/** Parse `#rrggbb` to linear-ish 0..1 Rgba. */
export const hexToRgba = (hex: string, alpha = 1): Rgba => {
  const n = Number.parseInt(hex.replace("#", ""), 16);
  return [((n >> 16) & 0xff) / 255, ((n >> 8) & 0xff) / 255, (n & 0xff) / 255, alpha];
};

const IRONBOUND: GroupVisualLanguage = {
  id: "ironbound",
  palette: {
    primary: hexToRgba("#828b98"),
    secondary: hexToRgba("#3a4149"),
    accent: hexToRgba("#bf7d2a"),
    metal: hexToRgba("#a7b1c2"),
    emissive_core: hexToRgba("#7a4a1a"),
    glow: hexToRgba("#f0a850"),
    eye: hexToRgba("#ffcf7a"),
    shadow_base: hexToRgba("#1a1c20"),
  },
  emissiveRoles: { emissive_core: hexToRgba("#c8681a"), glow: hexToRgba("#ff9a3d"), eye: hexToRgba("#ffcf7a") },
  // Forged-metal knights: bare metal + plate roles are glossy; the brass accent trim,
  // cape/banner, and painted secondary read matte.
  roughnessRoles: { metal: 0.16, primary: 0.28, secondary: 0.6, accent: 0.68 },
  preferredPrimitives: ["box", "rounded_box", "cylinder", "wedge", "plate"],
  jointStyle: "rigid",
  defaultAnimation: "ironbound_default",
  modifierTint: hexToRgba("#f0c46a"),
  groupColorHex: "#7C8A99",
};

const EMBERKIN: GroupVisualLanguage = {
  id: "emberkin",
  palette: {
    primary: hexToRgba("#2a2320"),
    secondary: hexToRgba("#4a3a30"),
    accent: hexToRgba("#e2572b"),
    metal: hexToRgba("#5a4a42"),
    emissive_core: hexToRgba("#ff7a2d"),
    glow: hexToRgba("#ff5a1a"),
    eye: hexToRgba("#ffd23d"),
    shadow_base: hexToRgba("#140d0a"),
  },
  emissiveRoles: { emissive_core: hexToRgba("#ff6a1a"), glow: hexToRgba("#ff4d1a"), accent: hexToRgba("#e2572b"), eye: hexToRgba("#ffd23d") },
  // Charred, sooty bodies read matte; the metal remnants keep a dull semi-gloss.
  roughnessRoles: { metal: 0.42, primary: 0.72, secondary: 0.64, accent: 0.5 },
  preferredPrimitives: ["cone", "wedge", "capsule", "sphere", "box"],
  jointStyle: "beveled",
  defaultAnimation: "emberkin_default",
  modifierTint: hexToRgba("#ff8a3d"),
  groupColorHex: "#E2572B",
};

const BLOOMTIDE: GroupVisualLanguage = {
  id: "bloomtide",
  palette: {
    primary: hexToRgba("#3fae64"),
    secondary: hexToRgba("#2f8f7a"),
    accent: hexToRgba("#e8dfae"),
    metal: hexToRgba("#6a7a3a"),
    emissive_core: hexToRgba("#5adf9a"),
    glow: hexToRgba("#7affc0"),
    eye: hexToRgba("#ff9ac0"),
    shadow_base: hexToRgba("#122016"),
  },
  emissiveRoles: { emissive_core: hexToRgba("#4adf9a"), glow: hexToRgba("#6affb0"), eye: hexToRgba("#ff9ac0") },
  opacityRoles: { glow: 0.5 },
  // Living plant matter is fully matte; only the mineral base keeps a little sheen.
  roughnessRoles: { primary: 0.85, secondary: 0.8, accent: 0.72, metal: 0.5 },
  preferredPrimitives: ["capsule", "cone", "sphere", "segmented", "plate"],
  jointStyle: "organic",
  defaultAnimation: "bloomtide_default",
  modifierTint: hexToRgba("#8fffc0"),
  groupColorHex: "#3FAE64",
};

const ECHOWISP: GroupVisualLanguage = {
  id: "echowisp",
  palette: {
    primary: hexToRgba("#9b6bd1"),
    secondary: hexToRgba("#5a4a8a"),
    accent: hexToRgba("#a8d8e0"),
    metal: hexToRgba("#b0b0c0"),
    emissive_core: hexToRgba("#e8e0ff"),
    glow: hexToRgba("#b48fff"),
    eye: hexToRgba("#a8f0ff"),
    shadow_base: hexToRgba("#161022"),
  },
  emissiveRoles: { emissive_core: hexToRgba("#d8c8ff"), glow: hexToRgba("#a87aff"), eye: hexToRgba("#a8f0ff") },
  opacityRoles: { primary: 0.9, glow: 0.45, accent: 0.8 },
  // Polished arcane crystal — glossy across the board.
  roughnessRoles: { primary: 0.3, secondary: 0.4, accent: 0.34, metal: 0.28 },
  preferredPrimitives: ["sphere", "ring", "billboard", "cone", "capsule"],
  jointStyle: "organic",
  defaultAnimation: "echowisp_default",
  modifierTint: hexToRgba("#c8a8ff"),
  groupColorHex: "#9B6BD1",
};

const NEUTRAL: GroupVisualLanguage = {
  id: "neutral",
  palette: {
    primary: hexToRgba("#8a8578"),
    secondary: hexToRgba("#6a5a44"),
    accent: hexToRgba("#b8934a"),
    metal: hexToRgba("#9a9488"),
    emissive_core: hexToRgba("#5a4a2a"),
    glow: hexToRgba("#d8b060"),
    eye: hexToRgba("#c8b890"),
    shadow_base: hexToRgba("#1a1712"),
  },
  emissiveRoles: { glow: hexToRgba("#d8b060") },
  // Utility constructs: worked metal with a semi-gloss, muted painted bodies.
  roughnessRoles: { metal: 0.36, primary: 0.6, secondary: 0.55, accent: 0.5 },
  preferredPrimitives: ["box", "cylinder", "plate", "rounded_box"],
  jointStyle: "rigid",
  defaultAnimation: "neutral_default",
  modifierTint: hexToRgba("#d8b060"),
  groupColorHex: "",
};

const LANGUAGES: Readonly<Record<string, GroupVisualLanguage>> = {
  ironbound: IRONBOUND,
  emberkin: EMBERKIN,
  bloomtide: BLOOMTIDE,
  echowisp: ECHOWISP,
  neutral: NEUTRAL,
};

export const languageFor = (id: GroupId | "neutral"): GroupVisualLanguage => LANGUAGES[id] ?? NEUTRAL;

export const allLanguages = (): readonly GroupVisualLanguage[] => Object.values(LANGUAGES);
