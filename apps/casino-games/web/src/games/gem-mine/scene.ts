/*
 * scene.ts — Gem Mine presentation: a small cluster of procedural rocks on a
 * mine floor (each an overlapping box/sphere lump with a fixed per-index tilt
 * and subtle tint), minecart rails, a lantern on a post, and support beams.
 * The strike ceremony swings a pickaxe in, cracks the chosen rock across three
 * staged beats, then breaks it into bounded ballistic fragments and lifts out
 * a gem scaled by rarity (or an honest empty core). Pure view:
 * `mineScene(runtime, state)` returns a Scene value.
 */

import type { MaterialSpec, Rgba, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { EngineVec3, GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { RARITY_COLORS, REWARD_MATERIALS, rewardBeam, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01, easeOutBack } from "../../presentation/stage/easing.ts";
import { contactShadow, SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { addV3, quatAxisAngle, QUAT_IDENTITY, quatPitch, quatYaw, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { MineSpec, MineState } from "./game.ts";
import {
  crackStagesAt,
  fragmentCount,
  fragmentPose,
  mineCamera,
  mineChoiceCount,
  mineTimeline,
  pickSwing,
  rockPosition,
  rockTintIndex,
  rockWobble,
  rockYaw,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const ROCK_TINTS: readonly Rgba[] = [
  [0.5, 0.47, 0.44, 1],
  [0.46, 0.44, 0.42, 1],
  [0.54, 0.5, 0.45, 1],
];

const rockMaterials = (): Readonly<Record<string, MaterialSpec>> =>
  Object.fromEntries(ROCK_TINTS.map((color, i): readonly [string, MaterialSpec] => [`Rock${i}`, { baseColor: color }]));

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  ...rockMaterials(),
  Beam: { baseColor: [0.5, 0.36, 0.22, 1] },
  Crack: { baseColor: [0.08, 0.07, 0.06, 1] },
  DustMote: { baseColor: [0.68, 0.58, 0.44, 1], emissive: [0.12, 0.1, 0.07, 1], opacity: 0.7 },
  EmptyCore: { baseColor: [0.2, 0.19, 0.18, 1], emissive: [0.03, 0.03, 0.04, 1] },
  Lantern: { baseColor: [1, 0.85, 0.5, 1], emissive: [1, 0.78, 0.4, 1] },
  LanternPost: { baseColor: [0.3, 0.28, 0.26, 1] },
  MineFloor: { baseColor: [0.4, 0.35, 0.3, 1] },
  PickHead: { baseColor: [0.62, 0.65, 0.7, 1] },
  PickHandle: { baseColor: [0.52, 0.36, 0.2, 1] },
  Rail: { baseColor: [0.55, 0.57, 0.6, 1] },
};

export const MINE_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── one rock ──────────────────────────────────────────────────────────────────

/** A procedural rock: 3 overlapping lumps at a fixed per-index tilt, plus the
 * crack lines that appear as stages land, and its ground shadow. Broken=true
 * hides the whole solid (fragments take over). */
const rockInstances = (
  index: number,
  origin: EngineVec3,
  wobble: number,
  cracks: number,
  broken: boolean,
  ring: "none" | "focus" | "hover",
): readonly SceneInstance[] => {
  const material = `Rock${rockTintIndex(index)}`;
  const q = quatYaw(rockYaw(index) + wobble);
  const shadow = contactShadow(`rock${index}:shadow`, origin, 0.55);
  const rings: SceneInstance[] =
    ring === "none"
      ? []
      : [
          {
            key: `rock${index}:ring`,
            material: ring === "hover" ? "StageGold" : "StageFloorAccent",
            mesh: "cylinder",
            transform: { position: v3(origin.x, 0.02, origin.z), rotation: QUAT_IDENTITY, scale: v3(1.1, 0.02, 1.1) },
          },
        ];
  if (broken) {
    return [shadow, ...rings];
  }
  const lump = (suffix: string, local: EngineVec3, scale: EngineVec3, mesh: "box" | "sphere"): SceneInstance => ({
    key: `rock${index}:${suffix}`,
    material,
    mesh,
    transform: { position: addV3(v3(origin.x, 0.3, origin.z), local), rotation: q, scale },
  });
  const crackLines: SceneInstance[] = Array.from({ length: cracks }, (_, c) => ({
    key: `rock${index}:crack${c}`,
    material: "Crack",
    mesh: "box",
    transform: {
      position: addV3(v3(origin.x, 0.36 + c * 0.03, origin.z), v3(0, 0, 0.34)),
      rotation: quatAxisAngle(v3(0, 0, 1), (c - 1) * 0.7),
      scale: v3(0.05, 0.5, 0.04),
    },
  }));
  return [
    lump("a", v3(0, 0, 0), v3(0.62, 0.58, 0.6), "box"),
    lump("b", v3(0.18, 0.1, 0.06), v3(0.44, 0.44, 0.44), "sphere"),
    lump("c", v3(-0.16, 0.06, -0.05), v3(0.4, 0.4, 0.42), "box"),
    ...crackLines,
    shadow,
    ...rings,
  ];
};

/** The pickaxe swinging in from the right of the chosen rock during strikes. */
const pickaxe = (target: EngineVec3, swing: number, approachT: number): readonly SceneInstance[] => {
  const rest = addV3(target, v3(1.5, 1.1, 0.2));
  const contact = addV3(target, v3(0.55, 0.55, 0.1));
  const at = v3(
    rest.x + (contact.x - rest.x) * approachT,
    rest.y + (contact.y - rest.y) * approachT - swing * 0.5,
    rest.z,
  );
  const tilt = quatAxisAngle(v3(0, 0, 1), 0.9 - swing * 1.3);
  return [
    {
      key: "pick:handle",
      material: "PickHandle",
      mesh: "box",
      transform: { position: at, rotation: tilt, scale: v3(0.06, 0.7, 0.06) },
    },
    {
      key: "pick:head",
      material: "PickHead",
      mesh: "box",
      transform: { position: v3(at.x, at.y + 0.34, at.z), rotation: tilt, scale: v3(0.5, 0.08, 0.09) },
    },
  ];
};

// ── the fixed mine props (pure tables) ─────────────────────────────────────────

const LANTERN_AT: EngineVec3 = v3(-3.2, 0, 1.4);

const mineProps = (span: number, tick: number): readonly SceneInstance[] => {
  const flicker = 1 + Math.sin(tick * 0.12) * 0.06;
  const rails: SceneInstance[] = [-0.35, 0.35].map((x, i) => ({
    key: `rail:${i}`,
    material: "Rail",
    mesh: "box",
    transform: { position: v3(x, 0.02, span * 0.32), rotation: QUAT_IDENTITY, scale: v3(0.07, 0.04, span * 0.9) },
  }));
  const ties: SceneInstance[] = Array.from({ length: 5 }, (_, i) => ({
    key: `tie:${i}`,
    material: "Beam",
    mesh: "box",
    transform: { position: v3(0, 0.015, span * 0.32 - i * 0.7 + 1.4), rotation: QUAT_IDENTITY, scale: v3(0.95, 0.03, 0.12) },
  }));
  return [
    {
      key: "mine:floor",
      material: "MineFloor",
      mesh: "cylinder",
      transform: { position: v3(0, -0.03, 0), rotation: QUAT_IDENTITY, scale: v3(span, 0.06, span) },
    },
    ...rails,
    ...ties,
    {
      key: "lantern:post",
      material: "LanternPost",
      mesh: "cylinder",
      transform: { position: v3(LANTERN_AT.x, 0.7, LANTERN_AT.z), rotation: QUAT_IDENTITY, scale: v3(0.08, 1.4, 0.08) },
    },
    {
      key: "lantern:bulb",
      material: "Lantern",
      mesh: "sphere",
      transform: { position: v3(LANTERN_AT.x, 1.5, LANTERN_AT.z), rotation: QUAT_IDENTITY, scale: v3(0.22 * flicker, 0.22 * flicker, 0.22 * flicker) },
    },
    {
      key: "beam:left",
      material: "Beam",
      mesh: "box",
      transform: { position: v3(-span * 0.42, 1.0, -span * 0.4), rotation: QUAT_IDENTITY, scale: v3(0.16, 2.0, 0.16) },
    },
    {
      key: "beam:right",
      material: "Beam",
      mesh: "box",
      transform: { position: v3(span * 0.42, 1.0, -span * 0.4), rotation: QUAT_IDENTITY, scale: v3(0.16, 2.0, 0.16) },
    },
    {
      key: "beam:top",
      material: "Beam",
      mesh: "box",
      transform: { position: v3(0, 2.0, -span * 0.4), rotation: QUAT_IDENTITY, scale: v3(span * 0.9, 0.16, 0.16) },
    },
  ];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const mineScene = (runtime: GameRuntime<MineSpec>, state: MineState): Scene => {
  const session = state.session;
  const count = mineChoiceCount(session);
  const seed = session.seed;
  const tick = session.tick;
  const liveliness = runtime.config.gameSpecific.wobbleLiveliness;
  const selected = state.extra.choice.selected;
  const plan = session.committed;
  const span = 8;
  const timeline = mineTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing"
      ? phaseAge(session)
      : session.phase === "celebrating" || session.phase === "complete"
        ? timeline.total
        : -1;
  const idle = session.phase === "ready" || session.phase === "intro";
  const cracksOnSelected = revealAge >= 0 ? crackStagesAt(timeline, revealAge) : 0;
  const broken = revealAge >= timeline.breakAt;

  const rocks = Array.from({ length: count }, (_, index) => {
    const origin = rockPosition(index, count);
    const isSelected = selected === index;
    const wobble = idle ? rockWobble(index, tick, seed, liveliness) : 0;
    const ring: "none" | "focus" | "hover" =
      session.phase === "ready"
        ? state.extra.choice.hovered === index
          ? "hover"
          : state.extra.choice.focused === index
            ? "focus"
            : "none"
        : "none";
    return rockInstances(index, origin, wobble, isSelected ? cracksOnSelected : 0, isSelected && broken, ring);
  }).flat();

  // The strike ceremony at the chosen rock.
  const ceremony: SceneInstance[] = [];
  if (selected !== null && revealAge >= 0 && plan !== null) {
    const origin = rockPosition(selected, count);
    const approachT = clamp01(revealAge / timeline.approachEnd);
    ceremony.push(...pickaxe(origin, pickSwing(timeline, revealAge), approachT));

    // Impact sparkle on each strike.
    timeline.strikes.forEach((strike, k) => {
      ceremony.push(...sparkleRing(`impact${k}`, v3(origin.x, 0.4, origin.z), 5, plan.presentationSeed, revealAge - strike, 22));
    });

    // The break: bounded fragments on analytic arcs.
    if (broken) {
      const n = fragmentCount(plan.presentationSeed);
      const fragAge = revealAge - timeline.breakAt;
      for (let i = 0; i < n; i += 1) {
        const frag = fragmentPose(origin, plan.presentationSeed, i, fragAge);
        ceremony.push({
          key: `frag${i}`,
          material: `Rock${rockTintIndex(selected)}`,
          mesh: i % 2 === 0 ? "box" : "sphere",
          transform: {
            position: frag.position,
            rotation: quatAxisAngle(v3(0.4, 0.8, 0.2), frag.spin),
            scale: v3(frag.size, frag.size, frag.size),
          },
        });
      }

      // The reveal: a rarity gem, or an honest empty core.
      const riseT = clamp01((revealAge - timeline.breakAt) / (timeline.revealEnd - timeline.breakAt));
      const rarity = outcomeRarity(session);
      if (plan.win && rarity !== "loss") {
        const gemScale = rarity === "jackpot" ? 1.5 : rarity === "rare" ? 1.25 : rarity === "uncommon" ? 1.05 : 0.85;
        ceremony.push(...rewardProp("gem", rarity, v3(origin.x, 0.55, origin.z), riseT, tick, gemScale));
        if (rarity !== "common") {
          ceremony.push(rewardBeam("gem:beam", v3(origin.x, 0.4, origin.z), riseT, 2.2));
        }
      } else {
        ceremony.push({
          key: "core:empty",
          material: "EmptyCore",
          mesh: "sphere",
          transform: {
            position: v3(origin.x, 0.4 + easeOutBack(riseT) * 0.12, origin.z),
            rotation: quatYaw(tick * 0.02),
            scale: v3(0.34, 0.34, 0.34),
          },
        });
        ceremony.push(...sparkleRing("core:calm", v3(origin.x, 0.5, origin.z), 5, plan.presentationSeed, revealAge - timeline.breakAt, 55));
      }
    }
  }

  // Celebration at the rock.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(rockPosition(selected, count), v3(0, 0.9, 0));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Idle dust motes (AMBIENT/PARTICLES streams, bounded ≤ 8 total).
  const motes: SceneInstance[] = idle
    ? sparkleRing("motes", v3(LANTERN_AT.x, 0.9, LANTERN_AT.z), 8, seed, tick % 90, 120).map((mote) => ({
        ...mote,
        material: "DustMote",
      }))
    : [];

  // Camera: showcase framing, easing toward the chosen rock during the strikes.
  const base = mineCamera(count);
  const focusT = selected !== null && revealAge >= 0 ? clamp01(revealAge / timeline.approachEnd) : 0;
  const camera =
    selected !== null && focusT > 0
      ? revealFocusCamera(base, addV3(rockPosition(selected, count), v3(0, 0.4, 0)), focusT, runtime.settings.reducedMotion ? 0.22 : 0.55)
      : base;

  // Lantern point light, plus a rarity-tinted glow once the gem is unveiled.
  const lights: SceneLight[] = [
    ...stageLights(selected !== null ? rockPosition(selected, count) : v3(0, 0.4, 0), 0.5),
    { key: "light:lantern", light: { color: [1, 0.82, 0.5, 1], intensity: 0.7, kind: "point", position: v3(LANTERN_AT.x, 1.5, LANTERN_AT.z) } },
  ];
  if (selected !== null && plan !== null && plan.win && broken) {
    const rarity = outcomeRarity(session);
    lights.push({
      key: "light:gem",
      light: {
        color: rarity === "loss" ? [1, 1, 1, 1] : RARITY_COLORS[rarity],
        intensity: 1.4,
        kind: "point",
        position: addV3(rockPosition(selected, count), v3(0, 1.0, 0.3)),
      },
    });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(span * 2), ...mineProps(span, tick), ...rocks, ...ceremony, ...celebration, ...motes],
    lights,
  };
};
