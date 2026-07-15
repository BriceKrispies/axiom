/*
 * scene.ts — Present Pop presentation: a grid of wrapped presents that vary by
 * slot index (box color, ribbon direction, bow shape, wrap motif — all fixed
 * by index, none of it hinting at value), the shake → squash → spring → BURST
 * reveal (lid + wall panels and ribbon shards flung outward on analytic
 * trajectory arcs), the reward rising from the center (or a soft empty puff),
 * and the tier-scaled celebration. Pure view.
 */

import type { EngineVec3, GameResources, MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { REWARD_MATERIALS, rewardBeam, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01 } from "../../presentation/stage/easing.ts";
import { contactShadow, SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { addV3, quatAxisAngle, QUAT_IDENTITY, quatMul, quatYaw, rotateByQuat, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { PresentPopSpec, PresentPopState } from "./game.ts";
import {
  boxPose,
  BURST_PANELS,
  BURST_RIBBONS,
  burstPiece,
  hopPose,
  popTimeline,
  presentCamera,
  presentPosition,
  revealAgeOf,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  BoxBlue: { baseColor: [0.42, 0.66, 0.98, 1] },
  BoxCoral: { baseColor: [0.98, 0.52, 0.46, 1] },
  BoxGreen: { baseColor: [0.5, 0.82, 0.52, 1] },
  BoxLilac: { baseColor: [0.75, 0.62, 0.98, 1] },
  Ribbon: { baseColor: [1, 0.95, 0.72, 1], emissive: [0.3, 0.26, 0.12, 1] },
  RibbonGold: { baseColor: [1, 0.82, 0.34, 1], emissive: [0.34, 0.24, 0.06, 1] },
  WrapMotif: { baseColor: [1, 1, 0.94, 1], emissive: [0.22, 0.2, 0.14, 1] },
};

export const PRESENT_POP_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── present proportions & per-slot variation tables (fixed by index) ────────────

const BOX = v3(0.9, 0.82, 0.9);
const BOX_COLORS = ["BoxCoral", "BoxBlue", "BoxGreen", "BoxLilac"];

interface PresentPose {
  readonly origin: EngineVec3;
  readonly index: number;
  readonly hop: number;
  readonly wiggle: number;
  readonly shakeX: number;
  readonly squashY: number;
  readonly lift: number;
  readonly dim: boolean;
  readonly focusRing: boolean;
  readonly hoverRing: boolean;
}

/** All instances of one wrapped, un-burst present (box, wrap motif, ribbon in
 * its fixed direction, bow in its fixed style, contact shadow, ring). */
const presentInstances = (key: string, pose: PresentPose): readonly SceneInstance[] => {
  const q = quatYaw(pose.wiggle);
  const boxColor = BOX_COLORS[pose.index % BOX_COLORS.length] as string;
  const material = pose.dim ? "StageHousingDark" : boxColor;
  const ribbonAlongX = pose.index % 2 === 0; // fixed ribbon direction by slot
  const crossedBow = pose.index % 3 !== 2; // two-bar bow vs a sphere knot
  const originY = pose.origin.y + (BOX.y / 2) * pose.squashY + pose.hop + pose.lift;
  const center = v3(pose.origin.x + pose.shakeX, originY, pose.origin.z);
  const boxScale = v3(BOX.x, BOX.y * pose.squashY, BOX.z);

  const part = (suffix: string, mesh: string, local: EngineVec3, scale: EngineVec3, mat: string, extraQ = QUAT_IDENTITY): SceneInstance => ({
    key: `${key}:${suffix}`,
    material: mat,
    mesh,
    transform: { position: addV3(center, rotateByQuat(v3(local.x, local.y * pose.squashY, local.z), q)), rotation: quatMul(q, extraQ), scale },
  });

  const topY = (BOX.y / 2) * pose.squashY;
  const ribbon: SceneInstance[] = ribbonAlongX
    ? [
        part("ribX", "box", v3(0, 0, 0), v3(BOX.x + 0.04, BOX.y * pose.squashY + 0.04, 0.18), "RibbonGold"),
        part("ribXtop", "box", v3(0, topY, 0), v3(BOX.x + 0.06, 0.06, 0.2), "RibbonGold"),
      ]
    : [
        part("ribZ", "box", v3(0, 0, 0), v3(0.18, BOX.y * pose.squashY + 0.04, BOX.z + 0.04), "RibbonGold"),
        part("ribZtop", "box", v3(0, topY, 0), v3(0.2, 0.06, BOX.z + 0.06), "RibbonGold"),
      ];

  const bow: SceneInstance[] = crossedBow
    ? [
        part("bowA", "box", v3(0, topY + 0.12, 0), v3(0.34, 0.12, 0.1), "Ribbon", quatYaw(0.5)),
        part("bowB", "box", v3(0, topY + 0.12, 0), v3(0.34, 0.12, 0.1), "Ribbon", quatYaw(-0.5)),
      ]
    : [part("knot", "sphere", v3(0, topY + 0.14, 0), v3(0.24, 0.24, 0.24), "Ribbon")];

  // Wrap motif: a few small dots, whose count/placement is fixed by slot index.
  const motif: SceneInstance[] = [-0.22, 0.22].map((x, i) =>
    part(`m${i}`, "box", v3(x, 0.1, BOX.z / 2 + 0.01), v3(0.12, 0.12, 0.02), "WrapMotif"),
  );

  const rings: SceneInstance[] = [];
  if (pose.focusRing || pose.hoverRing) {
    rings.push({
      key: `${key}:ring`,
      material: pose.hoverRing ? "RibbonGold" : "StageGold",
      mesh: "cylinder",
      transform: { position: v3(pose.origin.x, 0.02, pose.origin.z), rotation: QUAT_IDENTITY, scale: v3(BOX.x * 1.5, 0.02, BOX.x * 1.5) },
    });
  }

  return [
    part("box", "box", v3(0, 0, 0), boxScale, material),
    ...ribbon,
    ...bow,
    ...motif,
    contactShadow(`${key}:shadow`, pose.origin, BOX.x * 0.62),
    ...rings,
  ];
};

/** The burst debris for the selected present: BURST_PANELS lid/wall panels plus
 * BURST_RIBBONS ribbon shards on analytic trajectory arcs. */
const burstInstances = (key: string, origin: EngineVec3, presentationSeed: number, ageTicks: number, lifeTicks: number): readonly SceneInstance[] => {
  const panels = Array.from({ length: BURST_PANELS }, (_, i) => {
    const piece = burstPiece(origin, presentationSeed, i, ageTicks, lifeTicks);
    return piece === null
      ? null
      : {
          key: `${key}:panel${i}`,
          material: BOX_COLORS[i % BOX_COLORS.length] as string,
          mesh: "box",
          transform: { position: piece.position, rotation: quatAxisAngle(piece.axis, piece.spin), scale: v3(0.34 * piece.fade + 0.05, 0.34 * piece.fade + 0.05, 0.05) },
        };
  });
  const ribbons = Array.from({ length: BURST_RIBBONS }, (_, r) => {
    const piece = burstPiece(origin, presentationSeed, BURST_PANELS + r, ageTicks, lifeTicks);
    return piece === null
      ? null
      : {
          key: `${key}:ribbon${r}`,
          material: "RibbonGold",
          mesh: "box",
          transform: { position: piece.position, rotation: quatAxisAngle(piece.axis, piece.spin), scale: v3(0.08, 0.28 * piece.fade + 0.03, 0.04) },
        };
  });
  return [...panels, ...ribbons].filter((instance): instance is SceneInstance => instance !== null);
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const presentPopScene = (runtime: GameRuntime<PresentPopSpec>, state: PresentPopState): Scene => {
  const session = state.session;
  const count = session.config.choiceCount ?? 6;
  const seed = session.seed;
  const tick = session.tick;
  const selected = state.extra.choice.selected;
  const plan = session.committed;
  const presentationSeed = plan?.presentationSeed ?? seed;
  const timeline = popTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge = revealAgeOf(session, timeline.total);
  const liveliness = session.phase === "ready" || session.phase === "intro" ? runtime.config.gameSpecific.hopLiveliness : 0;

  const presents = Array.from({ length: count }, (_, index) => {
    const origin = presentPosition(index, count);
    const isSelected = selected === index;
    const box = isSelected ? boxPose(revealAge, timeline, presentationSeed) : { burst: false, lift: 0, shakeX: 0, squashY: 1 };
    // Once the selected present has burst, its solid form disappears (the burst
    // debris + reward take over).
    if (isSelected && box.burst) {
      return [];
    }
    const hop = isSelected ? { hop: 0, wiggle: 0 } : hopPose(index, count, tick, seed, liveliness);
    return presentInstances(`gift${index}`, {
      dim: revealAge >= 0 && !isSelected,
      focusRing: session.phase === "ready" && state.extra.choice.focused === index,
      hop: hop.hop,
      hoverRing: session.phase === "ready" && state.extra.choice.hovered === index,
      index,
      lift: box.lift,
      origin,
      shakeX: box.shakeX,
      squashY: box.squashY,
      wiggle: hop.wiggle,
    });
  }).flat();

  // Burst debris + reward rising from the burst center.
  const burstAndReward: SceneInstance[] = [];
  if (selected !== null && plan !== null && revealAge >= timeline.burstStart) {
    const at = addV3(presentPosition(selected, count), v3(0, 0.5, 0));
    const burstAge = revealAge - timeline.burstStart;
    const lifeTicks = timeline.riseEnd - timeline.burstStart;
    burstAndReward.push(...burstInstances("burst", at, presentationSeed, burstAge, lifeTicks));
    const riseT = clamp01(burstAge / (timeline.riseEnd - timeline.burstStart));
    const rarity = outcomeRarity(session);
    if (plan.win && rarity !== "loss") {
      burstAndReward.push(...rewardProp("reward", rarity, addV3(at, v3(0, 0.2, 0)), riseT, tick));
      if (celebrationFor(runtime.settings, session).beam) {
        burstAndReward.push(rewardBeam("beam", addV3(at, v3(0, 0.2, 0)), riseT, 2.2));
      }
    } else {
      burstAndReward.push(...sparkleRing("puff", addV3(at, v3(0, 0.3, 0)), 6, presentationSeed, burstAge, 50));
    }
  }

  // Celebration.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(presentPosition(selected, count), v3(0, 1.2, 0));
    if (plan.win) {
      celebration.push(...confettiBurst("confetti", at, profile.particles, presentationSeed, phaseAge(session)));
    } else {
      celebration.push(...sparkleRing("cheer", at, profile.particles, presentationSeed, phaseAge(session)));
    }
  }

  // Camera: showcase framing, easing toward the selected present during the reveal.
  const base = presentCamera(count);
  const focusT = revealAge >= 0 ? clamp01(revealAge / timeline.shakeEnd) : 0;
  const camera =
    selected !== null && focusT > 0
      ? revealFocusCamera(base, addV3(presentPosition(selected, count), v3(0, 0.6, 0)), focusT, runtime.settings.reducedMotion ? 0.2 : 0.5)
      : base;

  // Warm light at the burst once it opens.
  const lights: SceneLight[] = [...stageLights(selected !== null ? presentPosition(selected, count) : v3(0, 0, 0), 0.5)];
  if (selected !== null && revealAge >= timeline.burstStart) {
    lights.push({
      key: "light:pop",
      light: {
        color: [1, 0.9, 0.6, 1],
        intensity: 1.5,
        kind: "point",
        position: addV3(presentPosition(selected, count), v3(0, 1, 0.4)),
      },
    });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(18), ...presents, ...burstAndReward, ...celebration],
    lights,
  };
};
