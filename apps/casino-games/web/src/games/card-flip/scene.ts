/*
 * scene.ts — Card Flip presentation: a grid of thick, gold-cornered cards
 * propped upright on the table, an original diamond-motif back shared by every
 * card, the vertical-axis flip reveal (lift → edge-on spin → face-up →
 * contact-bounce settle), the honesty flip of every remaining card at round
 * completion, and the tier-scaled celebration. Pure view:
 * `cardFlipScene(runtime, state)` returns a Scene value.
 */

import type { EngineVec3, GameResources, MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { Rarity } from "../../chance-engine/configuration/schema.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { REWARD_MATERIALS, rewardMaterialOf, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, smoothstep } from "../../presentation/stage/easing.ts";
import { contactShadow, SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import {
  addV3,
  QUAT_IDENTITY,
  quatMul,
  quatPitch,
  quatRoll,
  quatYaw,
  rotateByQuat,
  v3,
} from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { CardFlipSpec, CardFlipState } from "./game.ts";
import { cardBreath, cardCamera, cardFlipPose, cardPosition, cardTimeline, revealAgeOf, tierRarityOf, winnersOf } from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  CardBack: { baseColor: [0.16, 0.42, 0.52, 1] },
  CardBackDim: { baseColor: [0.14, 0.28, 0.34, 1] },
  CardBackMotif: { baseColor: [1, 0.83, 0.4, 1], emissive: [0.4, 0.3, 0.08, 1] },
  CardBorder: { baseColor: [0.97, 0.94, 0.86, 1] },
  CardBorderBright: { baseColor: [1, 0.98, 0.9, 1], emissive: [0.25, 0.22, 0.12, 1] },
  CardBorderDim: { baseColor: [0.72, 0.7, 0.66, 1] },
  CardFace: { baseColor: [0.99, 0.97, 0.9, 1] },
  CardRing: { baseColor: [1, 0.8, 0.32, 1], emissive: [0.22, 0.15, 0.03, 1] },
  CardRingBright: { baseColor: [1, 0.88, 0.42, 1], emissive: [0.55, 0.4, 0.1, 1] },
};

export const CARD_FLIP_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── card proportions ────────────────────────────────────────────────────────────

const CARD_W = 0.92;
const CARD_H = 1.3;
const LEAN = 0.34;
const BASE_Y = 0.62;

interface CardPose {
  readonly origin: EngineVec3;
  readonly bob: number;
  readonly tilt: number;
  readonly lift: number;
  readonly angle: number;
  readonly squash: number;
  readonly dim: boolean;
  readonly focusRing: boolean;
  readonly hoverRing: boolean;
  /** Front face content once flipping: a tier rarity, or null for an honest
   * empty face. `undefined` keeps the face bare (no symbol spawned). */
  readonly face: Rarity | null | undefined;
}

/** All instances of one posed card (border body, back plate + motif, front
 * plate, optional face symbol, shadow, selection ring). */
const cardInstances = (key: string, pose: CardPose): readonly SceneInstance[] => {
  const q = quatMul(quatPitch(-LEAN), quatYaw(pose.angle + pose.tilt));
  const squashY = 1 - pose.squash;
  const center = v3(pose.origin.x, pose.origin.y + BASE_Y * squashY + pose.bob + pose.lift, pose.origin.z);
  const border = pose.dim ? "CardBorderDim" : pose.hoverRing ? "CardBorderBright" : "CardBorder";
  const back = pose.dim ? "CardBackDim" : "CardBack";

  const part = (suffix: string, mesh: string, local: EngineVec3, scale: EngineVec3, material: string, extraQ = QUAT_IDENTITY): SceneInstance => ({
    key: `${key}:${suffix}`,
    material,
    mesh,
    transform: {
      position: addV3(center, rotateByQuat(v3(local.x, local.y * squashY, local.z), q)),
      rotation: quatMul(q, extraQ),
      scale: v3(scale.x, scale.y * squashY, scale.z),
    },
  });

  const diamond = quatRoll(Math.PI / 4);
  const symbol: SceneInstance[] =
    pose.face === undefined
      ? []
      : pose.face === null
        ? [part("empty", "sphere", v3(0, 0, -0.075), v3(0.26, 0.26, 0.06), "TryAgain")]
        : [
            part("prize", "sphere", v3(0, 0.14, -0.075), v3(0.36, 0.36, 0.08), rewardMaterialOf(pose.face)),
            part("prizegem", "box", v3(0, -0.36, -0.075), v3(0.18, 0.18, 0.03), rewardMaterialOf(pose.face), diamond),
          ];

  const rings: SceneInstance[] = [];
  if (pose.focusRing || pose.hoverRing) {
    rings.push({
      key: `${key}:ring`,
      material: pose.hoverRing ? "CardRingBright" : "CardRing",
      mesh: "cylinder",
      transform: {
        position: v3(pose.origin.x, 0.02, pose.origin.z),
        rotation: QUAT_IDENTITY,
        scale: v3(CARD_W * 1.5, 0.02, CARD_W * 1.5),
      },
    });
  }

  return [
    part("body", "box", v3(0, 0, 0), v3(CARD_W + 0.1, CARD_H + 0.1, 0.05), border),
    part("backplate", "box", v3(0, 0, 0.045), v3(CARD_W, CARD_H, 0.035), back),
    part("gem", "box", v3(0, 0, 0.07), v3(0.3, 0.3, 0.03), "CardBackMotif", diamond),
    part("gemtop", "box", v3(0, CARD_H * 0.33, 0.07), v3(0.13, 0.13, 0.03), "CardBackMotif", diamond),
    part("gembot", "box", v3(0, -CARD_H * 0.33, 0.07), v3(0.13, 0.13, 0.03), "CardBackMotif", diamond),
    part("front", "box", v3(0, 0, -0.045), v3(CARD_W, CARD_H, 0.035), "CardFace"),
    ...symbol,
    contactShadow(`${key}:shadow`, pose.origin, CARD_W * 0.56),
    ...rings,
  ];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const cardFlipScene = (runtime: GameRuntime<CardFlipSpec>, state: CardFlipState): Scene => {
  const session = state.session;
  const count = session.config.choiceCount ?? 8;
  const columns = runtime.config.gameSpecific.columns;
  const seed = session.seed;
  const tick = session.tick;
  const selected = state.extra.choice.selected;
  const plan = session.committed;
  const winners = winnersOf(session);
  const timeline = cardTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge = revealAgeOf(session, timeline.total);
  const breathing = session.phase === "ready" || session.phase === "intro";
  const completing = session.phase === "complete";
  const completeAge = completing ? phaseAge(session) : -1;

  const cards = Array.from({ length: count }, (_, index) => {
    const origin = cardPosition(index, count, columns);
    const breath = breathing ? cardBreath(index, count, tick, seed) : { bob: 0, tilt: 0 };
    const isSelected = selected === index;

    // The honesty flip: at completion every remaining card turns face-up.
    const honestyT = completing && !isSelected ? clamp01((completeAge - index * 3) / 18) : 0;
    const flip = isSelected ? cardFlipPose(revealAge, timeline) : { angle: Math.PI * smoothstep(honestyT), lift: 0, squash: 0 };

    const showFace = isSelected ? revealAge >= timeline.liftEnd : flip.angle > 0;
    const face: Rarity | null | undefined = !showFace
      ? undefined
      : isSelected
        ? plan !== null && plan.win
          ? (tierRarityOf(session, plan.tierId) ?? null)
          : null
        : (tierRarityOf(session, winners?.[index] ?? null) ?? null);

    return cardInstances(`card${index}`, {
      angle: flip.angle,
      bob: breath.bob,
      dim: revealAge >= 0 && !isSelected && !completing,
      face,
      focusRing: session.phase === "ready" && state.extra.choice.focused === index,
      hoverRing: session.phase === "ready" && state.extra.choice.hovered === index,
      lift: flip.lift,
      origin,
      squash: flip.squash,
      tilt: breath.tilt,
    });
  }).flat();

  // Reward / empty reveal rising above the settled, face-up card.
  const rewardInstances: SceneInstance[] = [];
  if (selected !== null && plan !== null && revealAge >= timeline.settleEnd) {
    const at = addV3(cardPosition(selected, count, columns), v3(0, 1.35, 0));
    const riseT = clamp01((revealAge - timeline.settleEnd) / (timeline.riseEnd - timeline.settleEnd));
    const rarity = outcomeRarity(session);
    if (plan.win && rarity !== "loss") {
      rewardInstances.push(...rewardProp("reward", rarity, at, riseT, tick));
    } else {
      rewardInstances.push(...sparkleRing("dust", at, 6, plan.presentationSeed, revealAge - timeline.settleEnd, 50));
    }
  }

  // Celebration.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(cardPosition(selected, count, columns), v3(0, 1.7, 0));
    if (plan.win) {
      celebration.push(...confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session)));
    } else {
      celebration.push(...sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session)));
    }
  }

  // Camera: table framing, easing toward the selected card during the reveal.
  const base = cardCamera(count, columns);
  const focusT = revealAge >= 0 ? clamp01(revealAge / timeline.liftEnd) : 0;
  const camera =
    selected !== null && focusT > 0
      ? revealFocusCamera(base, addV3(cardPosition(selected, count, columns), v3(0, 0.7, 0)), focusT, runtime.settings.reducedMotion ? 0.2 : 0.45)
      : base;

  // Warm face light once the flip lands.
  const lights: SceneLight[] = [...stageLights(selected !== null ? cardPosition(selected, count, columns) : v3(0, 0, 0), 0.5)];
  if (selected !== null && revealAge >= timeline.flipEnd) {
    lights.push({
      key: "light:card",
      light: {
        color: [1, 0.9, 0.62, 1],
        intensity: 1.2,
        kind: "point",
        position: addV3(cardPosition(selected, count, columns), v3(0, 1.6, 0.7)),
      },
    });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(16), ...cards, ...rewardInstances, ...celebration],
    lights,
  };
};
