/*
 * scene.ts — Mystery Portal presentation: floating elliptical rings of beads
 * (each slot with its own color, bead density, and edge shape), softly
 * emissive interior discs, the approach-and-white-out reveal, and the small
 * reward vignette that swaps in behind the chosen portal — a pedestal with a
 * rarity prop and tier-colored light for wins, a calm pastel chamber with a
 * gentle sparkle for a loss. Pure view: `portalScene(runtime, state)` returns
 * a Scene value.
 */

import type { MaterialSpec, Rgba, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { EngineVec3, GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { RARITY_COLORS, REWARD_MATERIALS, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeInCubic, easeOutCubic } from "../../presentation/stage/easing.ts";
import { contactShadow, pedestal, SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { addV3, QUAT_IDENTITY, quatPitch, quatRoll, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { PortalSpec, PortalState } from "./game.ts";
import {
  PORTAL_BEADS,
  PORTAL_EDGES,
  portalCamera,
  portalChoiceCount,
  portalFocusT,
  portalIdlePose,
  portalPosition,
  portalTimeline,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

/** One distinct hue per slot index (identity, never a hint at contents). */
const PORTAL_COLORS: readonly Rgba[] = [
  [0.45, 0.9, 1, 1],
  [1, 0.55, 0.62, 1],
  [1, 0.85, 0.4, 1],
  [0.5, 0.95, 0.65, 1],
  [0.75, 0.6, 1, 1],
  [1, 0.7, 0.42, 1],
];

const portalMaterials = (): Readonly<Record<string, MaterialSpec>> =>
  Object.fromEntries(
    PORTAL_COLORS.flatMap((color, i): readonly (readonly [string, MaterialSpec])[] => [
      [`Portal${i}`, { baseColor: color, emissive: [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, 1] }],
      [
        `PortalDisc${i}`,
        {
          baseColor: [0.55 + color[0] * 0.35, 0.55 + color[1] * 0.35, 0.55 + color[2] * 0.35, 1],
          emissive: [color[0] * 0.35, color[1] * 0.35, color[2] * 0.35, 1],
          opacity: 0.85,
        },
      ],
    ]),
  );

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  ...portalMaterials(),
  ChamberPastel: { baseColor: [0.9, 0.88, 0.97, 1], emissive: [0.2, 0.19, 0.24, 1] },
  Whiteout: { baseColor: [1, 1, 1, 1], emissive: [1, 1, 1, 1] },
};

export const PORTAL_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── one portal ──────────────────────────────────────────────────────────────────

const RING_X = 0.78;
const RING_Y = 1.0;

interface PortalPose {
  readonly center: EngineVec3;
  /** Overall ring/disc scale multiplier (expansion / recession). */
  readonly size: number;
  /** Shimmer strength in [0, 1] (settles to 0 during the approach). */
  readonly shimmer: number;
  readonly breathe: number;
  readonly focusRing: boolean;
  readonly hoverRing: boolean;
  /** The shimmer phase offset the beads twinkle around (from the idle pose). */
  readonly shimmerPhaseAtRender: number;
}

/** All instances of one posed portal: the bead (or diamond) ring, its interior
 * disc, and its ground contact shadow. Materials are fixed per key. */
const portalInstances = (index: number, pose: PortalPose, tick: number): readonly SceneInstance[] => {
  const beadCount = PORTAL_BEADS[index % PORTAL_BEADS.length] as number;
  const edge = PORTAL_EDGES[index % PORTAL_EDGES.length] as "bead" | "diamond";
  const rx = RING_X * pose.breathe * pose.size;
  const ry = RING_Y * pose.breathe * pose.size;
  const spin = tick * 0.008 + index * 0.5;

  const beads: SceneInstance[] = Array.from({ length: beadCount }, (_, j) => {
    const a = (j / beadCount) * Math.PI * 2 + spin;
    const twinkle = 0.5 + 0.5 * Math.sin(tick * 0.28 + j * 1.1 + pose.shimmerPhaseAtRender);
    const size = 0.075 * pose.size * (0.85 + 0.35 * twinkle * pose.shimmer);
    return {
      key: `portal${index}:bead${j}`,
      material: `Portal${index}`,
      mesh: edge === "diamond" ? "box" : "sphere",
      transform: {
        position: v3(pose.center.x + Math.cos(a) * rx, pose.center.y + Math.sin(a) * ry, pose.center.z),
        rotation: edge === "diamond" ? quatRoll(a + Math.PI / 4) : QUAT_IDENTITY,
        scale: v3(size, size, size),
      },
    };
  });

  const disc: SceneInstance = {
    key: `portal${index}:disc`,
    material: `PortalDisc${index}`,
    mesh: "cylinder",
    transform: {
      position: v3(pose.center.x, pose.center.y, pose.center.z - 0.02),
      rotation: quatPitch(Math.PI / 2),
      scale: v3(rx * 1.85, 0.05, ry * 1.9),
    },
  };

  const rings: SceneInstance[] =
    pose.focusRing || pose.hoverRing
      ? [
          {
            key: `portal${index}:ring`,
            material: pose.hoverRing ? "StageGold" : "StageFloorAccent",
            mesh: "cylinder",
            transform: {
              position: v3(pose.center.x, 0.02, pose.center.z),
              rotation: QUAT_IDENTITY,
              scale: v3(1.5, 0.02, 1.5),
            },
          },
        ]
      : [];

  return [...beads, disc, contactShadow(`portal${index}:shadow`, v3(pose.center.x, 0, pose.center.z), 0.7), ...rings];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const portalScene = (runtime: GameRuntime<PortalSpec>, state: PortalState): Scene => {
  const session = state.session;
  const count = portalChoiceCount(session);
  const seed = session.seed;
  const tick = session.tick;
  const liveliness = runtime.config.gameSpecific.shimmerLiveliness;
  const selected = state.extra.choice.selected;
  const plan = session.committed;
  const timeline = portalTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing"
      ? phaseAge(session)
      : session.phase === "celebrating" || session.phase === "complete"
        ? timeline.total
        : -1;
  const approachT = revealAge >= 0 ? clamp01(revealAge / timeline.approachEnd) : 0;

  const portals = Array.from({ length: count }, (_, index) => {
    const center = portalPosition(index, count);
    const idle = portalIdlePose(index, tick, seed, liveliness);
    const isSelected = selected === index;
    // The chosen portal expands and stabilizes; the others recede and still.
    const size = isSelected ? 1 + 0.4 * easeOutCubic(approachT) : 1 - 0.25 * approachT;
    const settle = 1 - approachT;
    const pose: PortalPose = {
      breathe: 1 + (idle.breathe - 1) * settle,
      center: v3(center.x, center.y + idle.bob * settle, center.z),
      focusRing: session.phase === "ready" && state.extra.choice.focused === index,
      hoverRing: session.phase === "ready" && state.extra.choice.hovered === index,
      shimmer: liveliness * settle,
      shimmerPhaseAtRender: idle.shimmerPhase,
      size,
    };
    return portalInstances(index, pose, tick);
  }).flat();

  // The white-out disc: grows to cover the chosen portal, then recedes.
  const whiteout: SceneInstance[] = [];
  if (selected !== null && revealAge > timeline.approachEnd && revealAge < timeline.whiteoutEnd) {
    const center = portalPosition(selected, count);
    const growT = clamp01((revealAge - timeline.approachEnd) / (timeline.whiteoutPeak - timeline.approachEnd));
    const fadeT = clamp01((revealAge - timeline.whiteoutPeak) / (timeline.whiteoutEnd - timeline.whiteoutPeak));
    const cover = revealAge <= timeline.whiteoutPeak ? easeOutCubic(growT) : 1 - easeInCubic(fadeT);
    const d = 0.2 + cover * 5.4;
    whiteout.push({
      key: "whiteout",
      material: "Whiteout",
      mesh: "cylinder",
      transform: {
        position: v3(center.x, center.y, center.z + 0.15),
        rotation: quatPitch(Math.PI / 2),
        scale: v3(d, 0.04, d),
      },
    });
  }

  // The small reward vignette behind the chosen portal (swapped in under the
  // white-out — a few instances, never a duplicated scene).
  const vignette: SceneInstance[] = [];
  const lights: SceneLight[] = [
    ...stageLights(selected !== null ? portalPosition(selected, count) : v3(0, 1.2, 0), 0.55),
  ];
  if (selected !== null && plan !== null && revealAge >= timeline.whiteoutPeak) {
    const center = portalPosition(selected, count);
    const behind = v3(center.x, 0, center.z - 1.4);
    const vignT = clamp01((revealAge - timeline.whiteoutPeak) / (timeline.vignetteEnd - timeline.whiteoutPeak));
    const rarity = outcomeRarity(session);
    vignette.push(...pedestal("vign", behind, 0.8, 0.5));
    if (plan.win && rarity !== "loss") {
      vignette.push(...rewardProp("vign:reward", rarity, v3(behind.x, 1.05, behind.z), vignT, tick));
      const color = RARITY_COLORS[rarity];
      lights.push({
        key: "light:vignette",
        light: {
          color,
          intensity: 0.9 + 0.6 * vignT,
          kind: "point",
          position: v3(behind.x, 1.9, behind.z + 0.5),
        },
      });
    } else {
      vignette.push({
        key: "vign:chamber",
        material: "ChamberPastel",
        mesh: "box",
        transform: { position: v3(behind.x, 1.25, behind.z - 0.8), rotation: QUAT_IDENTITY, scale: v3(2.8, 2.5, 0.18) },
      });
      vignette.push(...sparkleRing("vign:calm", v3(behind.x, 1.0, behind.z), 6, plan.presentationSeed, revealAge - timeline.whiteoutPeak, 60));
    }
  }

  // Celebration at the chosen portal.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(portalPosition(selected, count), v3(0, 1.0, 0.3));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Camera: showcase framing; the reveal draws the eye through the portal
  // (strong pull, softened under reduced motion). Zero until a selection exists.
  const base = portalCamera(count);
  const focusT = portalFocusT(session, selected, runtime.settings.reducedMotion);
  const camera =
    selected !== null && focusT > 0
      ? revealFocusCamera(
          base,
          addV3(portalPosition(selected, count), v3(0, 0.15, -0.4)),
          focusT,
          runtime.settings.reducedMotion ? 0.25 : 0.7,
        )
      : base;

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(18), ...portals, ...vignette, ...whiteout, ...celebration],
    lights,
  };
};
