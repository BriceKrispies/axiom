/*
 * scene.ts — Scratch Reveal presentation: a bright prize ticket seen face-on.
 * A rounded card with a decorative gold border, a foil area drawn as a grid of
 * small flat tiles (the app's scratch mask — each tile vanishes as it is
 * scratched), and, hidden beneath, the committed symbol: a tier-colored gem on
 * a win or a friendly cloud on a loss, built from primitive shapes. The camera
 * is a fixed face-on rig whose projection maps the logical foil rectangle
 * exactly onto the world tiles, so the cursor and the visible foil agree. Pure
 * view.
 */

import type { EngineVec3, MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { Camera3D, GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH } from "../../presentation/cameras/picking.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { RARITY_COLORS, rewardMaterialOf, REWARD_MATERIALS } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutCubic, pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights } from "../../presentation/stage/props.ts";
import { QUAT_IDENTITY, quatRoll, quatYaw, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { ScratchSpec, ScratchState } from "./game.ts";
import {
  dissolveTimeline,
  foilLayout,
  symbolAreaTiles,
  tileCenter,
  tileCount,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  CardFace: { baseColor: [0.99, 0.97, 0.92, 1] },
  CardShine: { baseColor: [1, 1, 1, 1], emissive: [0.3, 0.3, 0.34, 1] },
  CloudBody: { baseColor: [0.86, 0.92, 1, 1], emissive: [0.28, 0.32, 0.4, 1] },
  FoilA: { baseColor: [0.68, 0.72, 0.82, 1], emissive: [0.18, 0.2, 0.26, 1] },
  FoilB: { baseColor: [0.6, 0.65, 0.77, 1], emissive: [0.14, 0.16, 0.22, 1] },
  GemGlow: { baseColor: [1, 0.95, 0.7, 1], emissive: [1, 0.9, 0.55, 1], opacity: 0.7 },
};

export const SCRATCH_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── the fixed face-on camera + its logical→world inverse ────────────────────────

const CARD_Y = 1.5;
const CARD_DIST = 6.2;
const FOVY = 0.85;
const HALF_TAN = Math.tan(FOVY / 2);
const ASPECT = CANVAS_WIDTH / CANVAS_HEIGHT;

const scratchCamera = (): Camera3D => ({
  far: 300,
  fovY: FOVY,
  near: 0.1,
  position: v3(0, CARD_Y, CARD_DIST),
  target: v3(0, CARD_Y, 0),
});

/** Map a logical canvas point (sx, sy) to the world point on the card plane
 * (z = 0) that projects there under `scratchCamera`. Exact inverse of the
 * straight-on projection, so tiles land under the cursor. */
const logicalToWorld = (sx: number, sy: number, z = 0): EngineVec3 => {
  const ndcX = (sx / CANVAS_WIDTH - 0.5) * 2;
  const ndcY = (0.5 - sy / CANVAS_HEIGHT) * 2;
  return v3(ndcX * CARD_DIST * HALF_TAN * ASPECT, CARD_Y + ndcY * CARD_DIST * HALF_TAN, z);
};

/** World size of a logical span (dx, dy) on the card plane. */
const worldSpan = (dx: number, dy: number): { readonly w: number; readonly h: number } => ({
  h: (dy / CANVAS_HEIGHT) * 2 * CARD_DIST * HALF_TAN,
  w: (dx / CANVAS_WIDTH) * 2 * CARD_DIST * HALF_TAN * ASPECT,
});

// ── the hidden symbol ───────────────────────────────────────────────────────────

/** The winning gem: a spinning tier-colored core with facets, plus a glow. */
const gemSymbol = (rarity: keyof typeof RARITY_COLORS, at: EngineVec3, tick: number, pulseT: number): readonly SceneInstance[] => {
  const material = rewardMaterialOf(rarity);
  const size = 0.7 * (1 + pulseT * 0.12);
  const spin = quatYaw(tick * 0.03);
  return [
    { key: "sym:glow", material: "GemGlow", mesh: "sphere", transform: { position: at, rotation: QUAT_IDENTITY, scale: v3(size * 2.2, size * 2.2, 0.1) } },
    { key: "sym:core", material, mesh: "sphere", transform: { position: at, rotation: spin, scale: v3(size, size * 1.15, size) } },
    { key: "sym:facet", material, mesh: "box", transform: { position: at, rotation: quatRoll(tick * 0.03 + 0.6), scale: v3(size * 0.8, size * 0.8, size * 0.6) } },
    ...Array.from({ length: 5 }, (_, i): SceneInstance => {
      const a = (i / 5) * Math.PI * 2;
      return {
        key: `sym:ray${i}`,
        material,
        mesh: "box",
        transform: { position: v3(at.x + Math.cos(a) * size * 1.1, at.y + Math.sin(a) * size * 1.1, at.z - 0.02), rotation: quatRoll(a), scale: v3(size * 0.5, size * 0.14, 0.05) },
      };
    }),
  ];
};

/** The friendly loss cloud: a cluster of soft overlapping spheres. */
const cloudSymbol = (at: EngineVec3): readonly SceneInstance[] => {
  const puffs: readonly (readonly [number, number, number])[] = [
    [0, 0, 0.62],
    [-0.55, -0.08, 0.5],
    [0.55, -0.08, 0.5],
    [-0.28, 0.22, 0.44],
    [0.3, 0.2, 0.46],
  ];
  return puffs.map(([dx, dy, r], i) => ({
    key: `sym:puff${i}`,
    material: "CloudBody",
    mesh: "sphere",
    transform: { position: v3(at.x + dx, at.y + dy, at.z), rotation: QUAT_IDENTITY, scale: v3(r, r, r) },
  }));
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const scratchScene = (runtime: GameRuntime<ScratchSpec>, state: ScratchState): Scene => {
  const session = state.session;
  const spec = runtime.config.gameSpecific;
  const layout = foilLayout(spec);
  const tick = session.tick;
  const plan = session.committed;
  const timeline = dissolveTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing"
      ? phaseAge(session)
      : session.phase === "celebrating" || session.phase === "complete"
        ? timeline.total
        : -1;

  // Ticket slide-in during intro/ready.
  const slide =
    session.phase === "intro" ? clamp01(phaseAge(session) / 24) : session.phase === "ready" ? 1 : 1;
  const slideX = (1 - easeOutCubic(slide)) * -6;

  const centerWorld = logicalToWorld(CANVAS_WIDTH / 2, CANVAS_HEIGHT / 2, 0);
  const cardCenter = v3(centerWorld.x + slideX, centerWorld.y, centerWorld.z - 0.12);
  const cardSpan = worldSpan(layout.width + 150, layout.height + 150);

  // Card + border behind the foil.
  const card: SceneInstance[] = [
    { key: "card:border", material: "StageGold", mesh: "box", transform: { position: v3(cardCenter.x, cardCenter.y, cardCenter.z - 0.02), rotation: QUAT_IDENTITY, scale: v3(cardSpan.w + 0.24, cardSpan.h + 0.24, 0.14) } },
    { key: "card:face", material: "CardFace", mesh: "box", transform: { position: cardCenter, rotation: QUAT_IDENTITY, scale: v3(cardSpan.w, cardSpan.h, 0.12) } },
  ];

  // The hidden symbol at the card center, slightly behind the foil plane.
  const symbolAt = v3(centerWorld.x + slideX, centerWorld.y, -0.06);
  const rarity = outcomeRarity(session);
  const revealed = revealAge >= 0;
  const symbolPulse = revealed ? pulse(clamp01((revealAge - timeline.dissolveEnd) / (timeline.total - timeline.dissolveEnd))) : 0;
  const symbol: SceneInstance[] =
    plan === null
      ? []
      : plan.win && rarity !== "loss"
        ? [...gemSymbol(rarity, symbolAt, tick, symbolPulse)]
        : [...cloudSymbol(symbolAt)];

  // The foil tiles: present unless scratched; during the reveal they dissolve
  // (shrink to nothing) in index order.
  const scratched = state.extra.scratched;
  const total = tileCount(layout);
  const dissolveWindow = Math.max(1, timeline.dissolveEnd);
  const tileWorld = worldSpan(layout.width / layout.columns, layout.height / layout.rows);
  const showFoil = session.phase !== "celebrating" && session.phase !== "complete";
  const foil: SceneInstance[] = [];
  if (showFoil) {
    for (let index = 0; index < total; index += 1) {
      if (scratched.has(index)) {
        continue;
      }
      const col = index % layout.columns;
      const row = Math.floor(index / layout.columns);
      const logical = tileCenter(layout, col, row);
      const at = logicalToWorld(logical.x, logical.y, 0.02);
      let shrink = 1;
      if (revealAge >= 0) {
        const start = (index / total) * (timeline.dissolveEnd * 0.5);
        const dissolveT = clamp01((revealAge - start) / (dissolveWindow * 0.5));
        shrink = 1 - dissolveT;
      }
      if (shrink <= 0.02) {
        continue;
      }
      foil.push({
        key: `foil:${index}`,
        material: (col + row) % 2 === 0 ? "FoilA" : "FoilB",
        mesh: "box",
        transform: {
          position: v3(at.x + slideX, at.y, at.z),
          rotation: QUAT_IDENTITY,
          scale: v3(tileWorld.w * 0.98 * shrink, tileWorld.h * 0.98 * shrink, 0.06),
        },
      });
    }
  }

  // Scratch debris sparkles at the freshest scratch (bounded, decorative).
  const debris: SceneInstance[] = [];
  const last = state.extra.lastScratch;
  if (session.phase === "interacting" && last !== null) {
    const at = logicalToWorld(last.x, last.y, 0.1);
    debris.push(...sparkleRing("grit", v3(at.x + slideX, at.y, at.z), 5, session.seed, tick % 18, 18));
  }

  // Celebration once the symbol is fully revealed.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = v3(symbolAt.x, symbolAt.y + 0.3, 0.2);
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  const lights: SceneLight[] = [...stageLights(v3(0, CARD_Y, 1), 0.7)];

  return {
    camera: scratchCamera(),
    clearColor: SKY_CLEAR,
    instances: [...card, ...symbol, ...foil, ...debris, ...celebration],
    lights,
  };
};
