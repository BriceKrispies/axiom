/*
 * scene.ts — Prize Drop presentation: a shallow-3D pachinko board facing the
 * camera. A token launcher rides along the top, a staggered field of pegs (small
 * camera-facing cylinders) fills the middle, and the reward slots at the foot are
 * drawn at widths proportional to their compiled probability (the picture is the
 * odds). While aiming, a ghost token and an aim line mark the release column; the
 * reveal drops the token through the pegs — squashing on each strike, trailing a
 * few fading echoes, sparkling at the peg it hits — to settle in the committed
 * slot, which pulses on impact. Pure view: returns a Scene value.
 */

import type { GameResources, MaterialSpec, Rgba, Scene, SceneInstance } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { showcaseCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { RARITY_COLORS, REWARD_MATERIALS } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutElastic, lerp, pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { QUAT_IDENTITY, quatPitch, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor } from "../round-state.ts";
import type { DropSpec, DropState } from "./game.ts";
import {
  BOARD_HALF,
  committedSlotIndex,
  dropTimeline,
  dropWorldX,
  fallProgress,
  PEG_COLS,
  PEG_ROWS,
  SLOT_Y,
  slotRangesOf,
  tokenPathX,
  tokenPathY,
  tokenSquash,
  TOP_Y,
} from "./game.ts";

const LOSS_COLORS: readonly Rgba[] = [
  [0.78, 0.86, 0.98, 1],
  [0.95, 0.87, 0.98, 1],
];

const rarityColorOf = (runtime: GameRuntime<DropSpec>, tierId: string | null): Rgba => {
  const tier = runtime.config.rewardTiers.find((t) => t.id === tierId);
  return tier === undefined ? (LOSS_COLORS[0] as Rgba) : RARITY_COLORS[tier.rarity];
};

/** Declared once per mount: primitives + a per-slot floor material colored by
 * the slot's tier (compiled widths make the odds legible, colors make the tier). */
export const dropResources = (runtime: GameRuntime<DropSpec>): GameResources => {
  const spec = runtime.config.gameSpecific;
  const materials: Record<string, MaterialSpec> = {
    ...STAGE_MATERIALS,
    ...REWARD_MATERIALS,
    ...CONFETTI_MATERIALS,
    AimLine: { baseColor: [1, 0.9, 0.6, 1], emissive: [0.7, 0.55, 0.2, 1], opacity: 0.5 },
    Board: { baseColor: [0.24, 0.3, 0.42, 1], emissive: [0.05, 0.07, 0.1, 1] },
    Launcher: { baseColor: [0.98, 0.55, 0.45, 1], emissive: [0.2, 0.09, 0.07, 1] },
    Peg: { baseColor: [0.9, 0.93, 1, 1], emissive: [0.28, 0.32, 0.4, 1] },
    Token: { baseColor: [1, 0.82, 0.3, 1], emissive: [0.6, 0.42, 0.12, 1] },
    TokenGhost: { baseColor: [1, 0.9, 0.55, 1], emissive: [0.4, 0.3, 0.1, 1], opacity: 0.32 },
    TokenTrail: { baseColor: [1, 0.85, 0.4, 1], emissive: [0.5, 0.36, 0.12, 1], opacity: 0.4 },
  };
  spec.slots.forEach((slot, i) => {
    const color = slot.tierId === null ? (LOSS_COLORS[i % 2] as Rgba) : rarityColorOf(runtime, slot.tierId);
    materials[`slot${i}`] = { baseColor: color, emissive: [color[0] * 0.22, color[1] * 0.22, color[2] * 0.22, 1] };
  });
  return {
    materials,
    meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
  };
};

const pegRowY = (row: number): number => lerp(TOP_Y - 0.55, SLOT_Y + 0.8, (row - 0.5) / PEG_ROWS);

/** The current token fall progress for the active phase (0 at rest on the launcher). */
const tokenProgress = (state: DropState): number => {
  const session = state.session;
  const timeline = dropTimeline(session.config.presentationSpeed, false);
  if (session.phase === "revealing") {
    return fallProgress(phaseAge(session), timeline);
  }
  return session.phase === "celebrating" || session.phase === "complete" ? 1 : 0;
};

export const dropScene = (runtime: GameRuntime<DropSpec>, state: DropState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const ranges = slotRangesOf(runtime);
  const seed = session.committed?.presentationSeed ?? session.seed;
  const round = session.round;
  const dropX = dropWorldX(state.extra.dropPosition);
  const slotIndex = committedSlotIndex(session);
  const slotCenter = ranges[slotIndex]?.center ?? 0;
  const p = tokenProgress(state);

  // The board backing panel.
  const chrome: SceneInstance[] = [
    { key: "board", material: "Board", mesh: "box", transform: { position: v3(0, (TOP_Y + SLOT_Y) / 2, -0.35), rotation: QUAT_IDENTITY, scale: v3(BOARD_HALF * 2 + 0.5, TOP_Y - SLOT_Y + 1.2, 0.3) } },
  ];

  // Staggered peg field.
  const pegs: SceneInstance[] = [];
  for (let row = 1; row <= PEG_ROWS; row += 1) {
    const y = pegRowY(row);
    const offset = row % 2 === 0 ? (BOARD_HALF * 2) / PEG_COLS / 2 : 0;
    for (let col = 0; col < PEG_COLS; col += 1) {
      const x = -BOARD_HALF + 0.35 + offset + (col * (BOARD_HALF * 2 - 0.7)) / (PEG_COLS - 1);
      pegs.push({
        key: `peg${row}:${col}`,
        material: "Peg",
        mesh: "cylinder",
        transform: { position: v3(x, y, 0), rotation: quatPitch(Math.PI / 2), scale: v3(0.13, 0.26, 0.13) },
      });
    }
  }

  // Reward slots along the foot: floor + dividers, drawn at compiled widths.
  const slots: SceneInstance[] = [];
  const celebrating = session.phase === "celebrating";
  ranges.forEach((range, i) => {
    const width = Math.max(0.06, range.end - range.start);
    const landed = (celebrating || session.phase === "complete") && i === slotIndex;
    const glow = landed ? 0.4 + pulse((session.tick % 30) / 30) * 0.4 : 0;
    slots.push({
      key: `slot${i}`,
      material: `slot${i}`,
      mesh: "box",
      transform: { position: v3(range.center, SLOT_Y - 0.32, 0), rotation: QUAT_IDENTITY, scale: v3(width - 0.05, 0.5 + glow * 0.2, 0.5) },
    });
    slots.push({
      key: `divider${i}`,
      material: "StageGold",
      mesh: "box",
      transform: { position: v3(range.end, SLOT_Y - 0.05, 0.05), rotation: QUAT_IDENTITY, scale: v3(0.06, 0.7, 0.4) } });
  });
  slots.push({ key: "divider-left", material: "StageGold", mesh: "box", transform: { position: v3(-BOARD_HALF, SLOT_Y - 0.05, 0.05), rotation: QUAT_IDENTITY, scale: v3(0.06, 0.7, 0.4) } });

  // Launcher rail + aiming affordances.
  const aim: SceneInstance[] = [
    { key: "launcher", material: "Launcher", mesh: "box", transform: { position: v3(0, TOP_Y + 0.5, 0), rotation: QUAT_IDENTITY, scale: v3(BOARD_HALF * 2 + 0.4, 0.28, 0.4) } },
  ];
  if (session.phase === "ready") {
    aim.push(
      { key: "ghost", material: "TokenGhost", mesh: "sphere", transform: { position: v3(dropX, TOP_Y, 0.12), rotation: QUAT_IDENTITY, scale: v3(0.34, 0.34, 0.34) } },
      { key: "aimline", material: "AimLine", mesh: "box", transform: { position: v3(dropX, (TOP_Y + SLOT_Y) / 2, 0.1), rotation: QUAT_IDENTITY, scale: v3(0.04, TOP_Y - SLOT_Y, 0.04) } },
    );
  }

  // The token itself (idle on the launcher, falling, or resting in its slot).
  const settleBounce = session.phase === "celebrating" ? (1 - easeOutElastic(clamp01(phaseAge(session) / 20))) * 0.25 : 0;
  const tokenX = tokenPathX(dropX, slotCenter, p, seed, round);
  const tokenY = tokenPathY(p) + settleBounce;
  const squash = session.phase === "revealing" ? tokenSquash(p) : 0;
  const token: SceneInstance = {
    key: "token",
    material: "Token",
    mesh: "sphere",
    transform: { position: v3(tokenX, tokenY, 0.14), rotation: QUAT_IDENTITY, scale: v3(0.34 * (1 + squash * 0.6), 0.34 * (1 - squash), 0.34) },
  };

  // A short trail of fading echoes behind a falling token.
  const trail: SceneInstance[] = [];
  if (session.phase === "revealing") {
    for (let k = 1; k <= 4; k += 1) {
      const tp = Math.max(0, p - k * 0.03);
      trail.push({
        key: `trail${k}`,
        material: "TokenTrail",
        mesh: "sphere",
        transform: { position: v3(tokenPathX(dropX, slotCenter, tp, seed, round), tokenPathY(tp), 0.13), rotation: QUAT_IDENTITY, scale: v3(0.3 - k * 0.04, 0.3 - k * 0.04, 0.3 - k * 0.04) },
      });
    }
  }

  // Peg sparkle at the freshly-struck peg.
  const sparks: SceneInstance[] = [];
  if (session.phase === "revealing" && p < 1) {
    const row = Math.min(PEG_ROWS, Math.floor(p * PEG_ROWS) + 1);
    const frac = (p * PEG_ROWS) % 1;
    if (frac < 0.3) {
      sparks.push(...sparkleRing("peg", v3(tokenX, pegRowY(row), 0.2), 5, seed, Math.round(frac * 20), 14));
    }
  }

  // Celebration at the resting token.
  const celebration: SceneInstance[] = [];
  const plan = session.committed;
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = v3(slotCenter, SLOT_Y + 0.4, 0.2);
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  return {
    camera: showcaseCamera(v3(0, (TOP_Y + SLOT_Y) / 2, 0), 8.2, 0.4, 0.82),
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(16), ...chrome, ...pegs, ...slots, ...aim, token, ...trail, ...sparks, ...celebration],
    lights: stageLights(v3(0, 2.2, 1), 0.55),
  };
};
