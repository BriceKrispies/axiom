/*
 * theme.ts — the "arcane industrial tournament" palette and per-stage arena
 * treatment. Original identity: dark forge slate, brass and ember, with each
 * group carrying its own accent (read from content). No glassmorphism, no thin
 * outline icons, no pastel gradients — heavy readable silhouettes and warm metal.
 * This is presentation data only; the simulation never sees a color.
 */

import type { ArenaStage } from "../sim/model.ts";

export const PALETTE = {
  bg0: "#0c0a09",
  bg1: "#151210",
  panel: "#1e1a16",
  panelEdge: "#3a2f26",
  brass: "#c8933f",
  brassLight: "#f0c46a",
  ember: "#ff7a2d",
  emberHot: "#ff4d2d",
  steel: "#8a94a6",
  ink: "#e8e0d4",
  inkDim: "#9a8f80",
  good: "#7fd88a",
  bad: "#ff5a5a",
  gold: "#ffd23d",
  health: "#ff6a6a",
} as const;

/** Per-stage arena intensity: how much machinery/lighting/particle richness the
 * presentation shows. Derived from the sim's `ArenaStage` (never the reverse). */
export interface StageTreatment {
  readonly floor: string;
  readonly glow: string;
  readonly glowStrength: number;
  readonly machinery: number;
  readonly particleScale: number;
  readonly label: string;
}

export const STAGE_TREATMENT: Readonly<Record<ArenaStage, StageTreatment>> = {
  workshop: { floor: "#17130f", glow: "#5a3a1a", glowStrength: 0.12, machinery: 1, particleScale: 0.5, label: "WORKSHOP" },
  kindled: { floor: "#1c1610", glow: "#7a441c", glowStrength: 0.22, machinery: 2, particleScale: 0.8, label: "KINDLED" },
  tempered: { floor: "#221812", glow: "#a5561f", glowStrength: 0.34, machinery: 3, particleScale: 1.1, label: "TEMPERED" },
  masterwork: { floor: "#2a1c12", glow: "#ff7a2d", glowStrength: 0.5, machinery: 4, particleScale: 1.5, label: "MASTERWORK" },
};

/** A readable text color for a group accent background. */
export const onAccent = (): string => "#0c0a09";
