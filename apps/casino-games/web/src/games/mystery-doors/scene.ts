/*
 * scene.ts — Mystery Doors presentation: a row of colorful freestanding doors
 * on the showcase stage, each with a DISTINCT procedural motif (stripes / dots
 * / chevrons, keyed to its slot index and telling nothing about value), the
 * knob-turn → crack (with colored light spill) → swing-wide reveal on a side
 * hinge, the reward vignette (pedestal + rewardProp) or a friendly empty room
 * behind, and the tier-scaled celebration. Pure view.
 */

import type { EngineQuat, EngineVec3, GameResources, MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { REWARD_MATERIALS, rewardBeam, rewardProp } from "../../presentation/rewards/tiers.ts";
import { clamp01 } from "../../presentation/stage/easing.ts";
import { contactShadow, pedestal, SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import { addV3, hingedTransform, QUAT_IDENTITY, quatMul, quatRoll, quatYaw, rotateByQuat, v3 } from "../../presentation/stage/vectors.ts";
import { celebrationFor, outcomeRarity } from "../round-state.ts";
import type { DoorsSpec, DoorsState } from "./game.ts";
import { doorDance, doorOpenPose, doorPosition, doorsCamera, doorTimeline, revealAgeOf } from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  DoorFrame: { baseColor: [0.95, 0.94, 0.9, 1] },
  DoorFrameDim: { baseColor: [0.7, 0.69, 0.66, 1] },
  DoorKnob: { baseColor: [1, 0.82, 0.32, 1], emissive: [0.3, 0.2, 0.05, 1] },
  DoorPanelCoral: { baseColor: [0.98, 0.5, 0.44, 1] },
  DoorPanelLilac: { baseColor: [0.72, 0.6, 0.98, 1] },
  DoorPanelMint: { baseColor: [0.42, 0.85, 0.68, 1] },
  DoorRoom: { baseColor: [0.1, 0.12, 0.16, 1] },
  MotifCoral: { baseColor: [1, 0.86, 0.7, 1], emissive: [0.35, 0.18, 0.12, 1] },
  MotifLilac: { baseColor: [0.95, 0.9, 1, 1], emissive: [0.28, 0.2, 0.4, 1] },
  MotifMint: { baseColor: [0.9, 1, 0.94, 1], emissive: [0.14, 0.4, 0.28, 1] },
  Spill: { baseColor: [1, 0.9, 0.6, 1], emissive: [1, 0.86, 0.5, 1], opacity: 0.85 },
};

export const DOORS_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── door proportions & motif palette (motif index = slot index, fixed) ──────────

const DOOR_W = 1.4;
const DOOR_H = 2.6;
const PANEL_W = 1.16;
const PANEL_H = 2.34;
const POST = 0.16;

const PANELS = ["DoorPanelCoral", "DoorPanelMint", "DoorPanelLilac", "DoorPanelCoral", "DoorPanelMint"];
const MOTIFS = ["MotifCoral", "MotifMint", "MotifLilac", "MotifCoral", "MotifMint"];

/** The panel-face motif for door `index`: 0 stripes, 1 dots, 2 chevrons (cycled). */
const motifInstances = (key: string, index: number, base: EngineVec3, q: EngineQuat, faceZ: number): readonly SceneInstance[] => {
  const material = MOTIFS[index % MOTIFS.length] as string;
  const kind = index % 3;
  const place = (suffix: string, local: EngineVec3, scale: EngineVec3, extraQ = QUAT_IDENTITY): SceneInstance => ({
    key: `${key}:${suffix}`,
    material,
    mesh: "box",
    transform: { position: addV3(base, rotateByQuat(v3(local.x, local.y, faceZ), q)), rotation: quatMul(q, extraQ), scale },
  });
  if (kind === 0) {
    // Horizontal stripes.
    return [0, 1, 2, 3].map((r) => place(`s${r}`, v3(0, (r - 1.5) * 0.55, 0), v3(PANEL_W * 0.8, 0.08, 0.03)));
  }
  if (kind === 1) {
    // Dot grid.
    return [-0.32, 0, 0.32].flatMap((x, ci) =>
      [-0.7, 0, 0.7].map((y, ri) => place(`d${ci}_${ri}`, v3(x, y, 0), v3(0.16, 0.16, 0.03))),
    );
  }
  // Chevrons.
  return [-0.6, 0, 0.6].flatMap((y, i) => [
    place(`cL${i}`, v3(-0.22, y, 0), v3(0.5, 0.09, 0.03), quatRoll(0.5)),
    place(`cR${i}`, v3(0.22, y, 0), v3(0.5, 0.09, 0.03), quatRoll(-0.5)),
  ]);
};

interface DoorPose {
  readonly origin: EngineVec3;
  readonly index: number;
  readonly count: number;
  readonly sway: number;
  readonly rattle: number;
  readonly knob: number;
  readonly swing: number;
  readonly spill: number;
  readonly dim: boolean;
  readonly focusRing: boolean;
  readonly hoverRing: boolean;
}

/** All instances of one posed door (frame posts + lintel, hinged panel + motif,
 * knob, colored light-spill sliver, contact shadow, selection ring). */
const doorInstances = (key: string, pose: DoorPose): readonly SceneInstance[] => {
  const body = quatYaw(pose.sway); // gentle idle sway around +Y
  const origin = v3(pose.origin.x, pose.origin.y, pose.origin.z);
  const frameMat = pose.dim ? "DoorFrameDim" : "DoorFrame";

  const framePart = (suffix: string, local: EngineVec3, scale: EngineVec3): SceneInstance => ({
    key: `${key}:${suffix}`,
    material: frameMat,
    mesh: "box",
    transform: { position: addV3(origin, rotateByQuat(local, body)), rotation: body, scale },
  });

  // Hinge on the door's LEFT vertical edge; the panel swings inward (+ around Y).
  const hingeLocal = v3(-PANEL_W / 2, DOOR_H / 2, 0);
  const hinge = addV3(origin, rotateByQuat(hingeLocal, body));
  const panelQ = quatMul(body, quatMul(quatYaw(pose.swing), quatYaw(pose.rattle)));
  const panel: SceneInstance = {
    key: `${key}:panel`,
    material: PANELS[pose.index % PANELS.length] as string,
    mesh: "box",
    transform: hingedTransform(hinge, v3(PANEL_W / 2, -DOOR_H / 2, 0), panelQ, v3(PANEL_W, PANEL_H, 0.1)),
  };
  // Motif rides the panel face, built relative to the swung panel center.
  const panelCenter = addV3(hinge, rotateByQuat(v3(PANEL_W / 2, -DOOR_H / 2, 0), panelQ));
  const motifOnPanel = motifInstances(`${key}:motif`, pose.index, panelCenter, panelQ, -0.065);

  const knobBase = addV3(hinge, rotateByQuat(v3(PANEL_W - 0.18, -DOOR_H / 2, -0.09), panelQ));
  const knob: SceneInstance = {
    key: `${key}:knob`,
    material: "DoorKnob",
    mesh: "sphere",
    transform: { position: knobBase, rotation: quatMul(panelQ, quatRoll(pose.knob)), scale: v3(0.16, 0.16, 0.16) },
  };

  // Colored light-spill sliver in the door gap, appearing with the crack.
  const spill: SceneInstance[] =
    pose.spill > 0
      ? [
          {
            key: `${key}:spill`,
            material: "Spill",
            mesh: "box",
            transform: { position: addV3(origin, v3(-PANEL_W / 2 + 0.05, DOOR_H / 2 - 0.02, 0.02)), rotation: body, scale: v3(0.06, DOOR_H * 0.9 * pose.spill, 0.03) },
          },
        ]
      : [];

  const rings: SceneInstance[] = [];
  if (pose.focusRing || pose.hoverRing) {
    rings.push({
      key: `${key}:ring`,
      material: pose.hoverRing ? "DoorKnob" : "StageGold",
      mesh: "cylinder",
      transform: { position: v3(pose.origin.x, 0.02, pose.origin.z + 0.2), rotation: QUAT_IDENTITY, scale: v3(DOOR_W * 1.35, 0.02, DOOR_W * 0.9) },
    });
  }

  // The dark room revealed behind the door (its threshold + back wall).
  const room: SceneInstance[] = [
    framePart("threshold", v3(0, 0.02, -0.02), v3(PANEL_W, 0.04, 0.6)),
    { key: `${key}:room`, material: "DoorRoom", mesh: "box", transform: { position: addV3(origin, rotateByQuat(v3(0, DOOR_H / 2, -0.55), body)), rotation: body, scale: v3(PANEL_W, DOOR_H, 0.1) } },
  ];

  return [
    ...room,
    framePart("post-l", v3(-DOOR_W / 2, DOOR_H / 2, 0), v3(POST, DOOR_H + POST, 0.5)),
    framePart("post-r", v3(DOOR_W / 2, DOOR_H / 2, 0), v3(POST, DOOR_H + POST, 0.5)),
    framePart("lintel", v3(0, DOOR_H + POST / 2, 0), v3(DOOR_W + POST, POST, 0.5)),
    framePart("sill", v3(0, POST / 2, 0), v3(DOOR_W + POST, POST, 0.5)),
    panel,
    ...motifOnPanel,
    knob,
    ...spill,
    contactShadow(`${key}:shadow`, addV3(pose.origin, v3(0, 0, 0.05)), DOOR_W * 0.6),
    ...rings,
  ];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const doorsScene = (runtime: GameRuntime<DoorsSpec>, state: DoorsState): Scene => {
  const session = state.session;
  const count = session.config.choiceCount ?? 3;
  const seed = session.seed;
  const tick = session.tick;
  const selected = state.extra.choice.selected;
  const plan = session.committed;
  const timeline = doorTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
  const revealAge = revealAgeOf(session, timeline.total);
  const liveliness = session.phase === "ready" || session.phase === "intro" ? runtime.config.gameSpecific.breatheLiveliness : 0;

  const doors = Array.from({ length: count }, (_, index) => {
    const origin = doorPosition(index, count);
    const dance = doorDance(index, count, tick, seed, liveliness);
    const isSelected = selected === index;
    const open = isSelected ? doorOpenPose(revealAge, timeline) : { knob: 0, spill: 0, swing: 0 };
    return doorInstances(`door${index}`, {
      count,
      dim: revealAge >= 0 && !isSelected,
      focusRing: session.phase === "ready" && state.extra.choice.focused === index,
      hoverRing: session.phase === "ready" && state.extra.choice.hovered === index,
      index,
      knob: open.knob,
      origin,
      rattle: dance.rattle,
      spill: open.spill,
      swing: open.swing,
      sway: dance.sway,
    });
  }).flat();

  // Reward vignette / empty puff behind the swung-open door.
  const vignette: SceneInstance[] = [];
  if (selected !== null && plan !== null && revealAge >= timeline.swingEnd) {
    const at = addV3(doorPosition(selected, count), v3(0, 0.02, -0.4));
    const riseT = clamp01((revealAge - timeline.swingEnd) / (timeline.riseEnd - timeline.swingEnd));
    const rarity = outcomeRarity(session);
    if (plan.win && rarity !== "loss") {
      vignette.push(...pedestal(`podium`, at, 0.9, 0.5));
      vignette.push(...rewardProp("reward", rarity, addV3(at, v3(0, 0.9, 0)), riseT, tick));
      if (celebrationFor(runtime.settings, session).beam) {
        vignette.push(rewardBeam("beam", addV3(at, v3(0, 0.9, 0)), riseT, 2));
      }
    } else {
      vignette.push(...sparkleRing("puff", addV3(at, v3(0, 1, 0)), 6, plan.presentationSeed, revealAge - timeline.swingEnd, 50));
    }
  }

  // Celebration.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(doorPosition(selected, count), v3(0, 2, -0.2));
    if (plan.win) {
      celebration.push(...confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session)));
    } else {
      celebration.push(...sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session)));
    }
  }

  // Camera: showcase framing, easing toward the selected door during the reveal.
  const base = doorsCamera(count);
  const focusT = revealAge >= 0 ? clamp01(revealAge / timeline.crackEnd) : 0;
  const camera =
    selected !== null && focusT > 0
      ? revealFocusCamera(base, addV3(doorPosition(selected, count), v3(0, 1.1, 0)), focusT, runtime.settings.reducedMotion ? 0.2 : 0.5)
      : base;

  // Colored light spilling through the gap once cracked.
  const lights: SceneLight[] = [...stageLights(selected !== null ? doorPosition(selected, count) : v3(0, 0, 0), 0.5)];
  if (selected !== null && revealAge >= timeline.knobEnd) {
    lights.push({
      key: "light:door",
      light: {
        color: [1, 0.85, 0.5, 1],
        intensity: 1.4 * doorOpenPose(revealAge, timeline).spill,
        kind: "point",
        position: addV3(doorPosition(selected, count), v3(0, 1.3, 0.2)),
      },
    });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(18), ...doors, ...vignette, ...celebration],
    lights,
  };
};
