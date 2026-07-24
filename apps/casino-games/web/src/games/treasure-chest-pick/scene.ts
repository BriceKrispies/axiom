/*
 * scene.ts — Treasure Chest Pick presentation: nine carved-wood, gold-gilded
 * chests staged as a small arcade ritual. Idle chests breathe out of unison;
 * a chosen chest lifts, tilts toward the camera,
 * and pools warm light beneath it while the other eight dim and go still; the
 * reveal is a readable sequence — anticipation shake, latch drop with a recoil
 * snap, warm light through the seam, a weighty overshooting lid, a compact light
 * burst, and a prize that rises fully clear of the chest to own the frame (or a
 * playful dust puff on an empty chest). Pure view: `chestScene(runtime, state)`
 * returns a Scene value; every animated quantity is a pure function of the tick.
 *
 * Nothing here reads the population or the winning slot for cosmetics: the idle
 * dance draws only from the ambient stream, and the breathe is pure in
 * (index, tick) — so no wobble can hint at which chest holds a prize.
 */

import type { Camera3D, MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { EngineQuat, EngineVec3, GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { REWARD_MATERIALS, rewardMaterialOf } from "../../presentation/rewards/tiers.ts";
import { celebrationFor, outcomeRarity, speedTicks } from "../round-state.ts";
import { clamp01, easeOutBack, easeOutCubic, lerp, pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import {
  addV3,
  hingedTransform,
  QUAT_IDENTITY,
  quatMul,
  quatPitch,
  quatRoll,
  quatYaw,
  rotateByQuat,
  scaleV3,
  v3,
} from "../../presentation/stage/vectors.ts";
import type { ChestSpec, ChestState, DecorDrag, HeroFraming } from "./game.ts";
import {
  CHEST_BODY as BODY,
  CHEST_BODY_TOP as BODY_TOP,
  CHEST_HEIGHT,
  CHEST_LATCH as LATCH,
  CHEST_LID as LID,
  CHEST_LID_ARCH,
  CHEST_TIMING,
  chestCamera,
  chestPosition,
  crabIdle,
  dancePose,
  flightProgress,
  heroFraming,
  idlePhase,
  palmSway,
  revealTimeline,
  spiralFlight,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

/**
 * The background veil, as a graded ladder of fixed-opacity materials.
 * A material's opacity is registered once at bind time and cannot be animated
 * per-instance, so the ramp is quantized into `dimSteps` rungs and the veil
 * instance simply picks the rung matching its current darkness. With enough
 * rungs the ramp is smooth at the speed it plays (a step lands every few
 * ticks), which is why the count is generous rather than minimal.
 *
 * Dimming the LIGHTS instead — or as well — would be the obvious alternative
 * and is wrong here: the hero chest is lit by the same rig as the stage it is
 * leaving, so darkening the rig darkens the very thing the shot exists to show.
 * The veil dims strictly what sits behind the chest, and leaves it untouched.
 */
const VEIL_MATERIALS: Readonly<Record<string, MaterialSpec>> = Object.fromEntries(
  Array.from({ length: CHEST_TIMING.dimSteps }, (_, i): readonly [string, MaterialSpec] => [
    `Veil${i}`,
    { baseColor: [0.02, 0.03, 0.06, 1], opacity: ((i + 1) / CHEST_TIMING.dimSteps) * CHEST_TIMING.dimVeil },
  ]),
);

/** The veil rung for a darkness level in [0, 1], or null when fully clear. */
const veilMaterialOf = (level: number): string | null => {
  const rung = Math.ceil(clamp01(level) * CHEST_TIMING.dimSteps);
  return rung <= 0 ? null : `Veil${Math.min(CHEST_TIMING.dimSteps, rung) - 1}`;
};

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  // The beach margin around the inset lagoon. The shared StageFloor is a pale,
  // near-white cream ([0.94, 0.9, 0.82]) that under the bright warm key lifts to
  // milky bone — the reference sand is a rich, saturated golden tan. Override it
  // for THIS game only (the shared material stays neutral for the other casino
  // stages): pull the blue channel well down and widen the red→blue spread so the
  // warm rig lands the beach at golden sand rather than bleached cream. This is a
  // pure palette warm/saturation move — no grade/tonemap stage exists here.
  StageFloor: { baseColor: [0.9, 0.75, 0.47, 1] },
  // Wood, value-stepped so the chest reads solid without a texture: the lid
  // catches the key light (lightest), the front boards sit mid, side boards go
  // darker, and the gaps between planks are the darkest brown. The ladder is
  // deliberately WIDE and pulled toward warm tan (rather than a uniform
  // saturated orange), because with no albedo texture the only thing carving
  // the chest into stacked planks is this value spread: a distinctly lighter
  // lid catching the key light, and near-black seams reading as the gaps
  // between boards — the carved-wood look of the reference lives in the step,
  // not the hue.
  // The champion ladder rendered as bleached pale pine under the bright warm
  // rig; the reference chests are saturated saddle-brown oak. The whole ramp is
  // pulled DARKER and WARMER (higher red-to-blue ratio) so that after the key
  // light lifts it, the lit faces land at rich caramel rather than milky tan —
  // the value spread that carves the planks is preserved, just seated on a
  // deeper, more saturated wood.
  WoodLid: { baseColor: [0.64, 0.44, 0.25, 1] },
  WoodBrown: { baseColor: [0.52, 0.34, 0.18, 1] },
  WoodSide: { baseColor: [0.38, 0.25, 0.13, 1] },
  WoodGap: { baseColor: [0.18, 0.1, 0.045, 1] },
  WoodDim: { baseColor: [0.28, 0.19, 0.11, 1] },
  WoodDimSide: { baseColor: [0.22, 0.15, 0.09, 1] },
  ChestInterior: { baseColor: [0.11, 0.07, 0.035, 1] },
  // Gold, likewise stepped: a pale highlight on upward edges/latch, the main
  // yellow on front trim, a darker ochre on side-facing straps — not uniformly
  // emissive, so it reads as metal catching light rather than glowing.
  GildTop: { baseColor: [1, 0.9, 0.5, 1], emissive: [0.28, 0.22, 0.06, 1] },
  GildFront: { baseColor: [0.98, 0.76, 0.28, 1], emissive: [0.12, 0.09, 0.02, 1] },
  GildSide: { baseColor: [0.7, 0.5, 0.18, 1] },
  GildDim: { baseColor: [0.5, 0.38, 0.16, 1] },
  GildBright: { baseColor: [1, 0.9, 0.46, 1], emissive: [0.5, 0.4, 0.12, 1] },
  // Warm reveal light: a layered pool under the chosen chest, seam leak, inner
  // glow, and the burst — all additive-emissive translucent discs/slabs.
  PoolCore: { baseColor: [1, 0.86, 0.5, 1], emissive: [1, 0.78, 0.4, 1], opacity: 0.5 },
  PoolMid: { baseColor: [1, 0.84, 0.48, 1], emissive: [0.9, 0.66, 0.3, 1], opacity: 0.28 },
  PoolOuter: { baseColor: [1, 0.82, 0.46, 1], emissive: [0.8, 0.58, 0.24, 1], opacity: 0.14 },
  SeamGlow: { baseColor: [1, 0.9, 0.55, 1], emissive: [1, 0.82, 0.4, 1], opacity: 0.7 },
  InnerGlow: { baseColor: [1, 0.85, 0.5, 1], emissive: [0.72, 0.54, 0.26, 1], opacity: 0.7 },
  BurstGlow: { baseColor: [1, 0.92, 0.62, 1], emissive: [1, 0.85, 0.5, 1], opacity: 0.42 },
  BurstRay: { baseColor: [1, 0.9, 0.58, 1], emissive: [1, 0.82, 0.44, 1], opacity: 0.22 },
  Mote: { baseColor: [1, 0.95, 0.72, 1], emissive: [1, 0.9, 0.6, 1] },
  // The arcade stage: a turquoise platform with a rim, a warm central glow, and
  // a darker edge falloff — an intentional board, not a flat marker.
  // The lagoon reads as vivid turquoise in reference, not a grey-green wash: the
  // main pool sits at a deeper, more saturated teal (low red, wide green/blue) so
  // the warm key light lifts it toward turquoise instead of desaturating it to
  // grey, and the outer band is a richer deep teal so the ring falloff still
  // reads as water rather than a muddy edge.
  PlatformSide: { baseColor: [0.11, 0.55, 0.57, 1] },
  CenterGlow: { baseColor: [1, 0.88, 0.6, 1], emissive: [0.14, 0.1, 0.04, 1], opacity: 0.1 },
  EdgeVignette: { baseColor: [0.03, 0.2, 0.26, 1], opacity: 0.34 },
  BoardRivet: { baseColor: [1, 0.82, 0.34, 1], emissive: [0.3, 0.22, 0.05, 1] },
  // Like every other translucent overlay here, the puff carries a little
  // emissive: a purely Lambert translucent grey reads as a dark blob against
  // the warm, brightly-lit chest mouth it coughs out of, which is the opposite
  // of the light, playful "nothing here this time" it is meant to be.
  DustPuff: { baseColor: [0.8, 0.75, 0.68, 1], emissive: [0.34, 0.31, 0.27, 1], opacity: 0.5 },
  // Beach set-dressing (palm, sandcastle, crab, shells) — value-stepped so each
  // prop reads as a chunky faceted assembly under the raking key, matching the
  // reference's toy-diorama shore. No emissive on the solid props (they are lit
  // by the same rig as the chests); the sand tones are pulled a touch lighter and
  // warmer than the floor slab so the castle and shore reads as dry sculpted sand.
  PalmBark: { baseColor: [0.46, 0.32, 0.19, 1] },
  PalmBarkDark: { baseColor: [0.33, 0.22, 0.12, 1] },
  PalmLeaf: { baseColor: [0.29, 0.53, 0.22, 1] },
  PalmLeafDark: { baseColor: [0.19, 0.4, 0.16, 1] },
  Coconut: { baseColor: [0.36, 0.26, 0.15, 1] },
  CastleSand: { baseColor: [0.93, 0.85, 0.63, 1] },
  CastleSandDark: { baseColor: [0.82, 0.72, 0.5, 1] },
  CastleDoor: { baseColor: [0.4, 0.32, 0.2, 1] },
  CastleFlag: { baseColor: [0.85, 0.22, 0.16, 1] },
  CastlePole: { baseColor: [0.32, 0.22, 0.12, 1] },
  CrabShell: { baseColor: [0.83, 0.26, 0.19, 1] },
  CrabShellDark: { baseColor: [0.62, 0.17, 0.12, 1] },
  CrabEye: { baseColor: [0.06, 0.05, 0.05, 1] },
  Shell: { baseColor: [0.96, 0.86, 0.8, 1] },
  Starfish: { baseColor: [0.92, 0.5, 0.29, 1] },
  ...VEIL_MATERIALS,
};

export const CHEST_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── small builders ──────────────────────────────────────────────────────────────

/** A flat disc (thin cylinder) — pools, glows, platform layers. */
const disc = (key: string, material: string, at: EngineVec3, radius: number, height = 0.02): SceneInstance => ({
  key,
  material,
  mesh: "cylinder",
  transform: { position: at, rotation: QUAT_IDENTITY, scale: v3(radius * 2, height, radius * 2) },
});

/**
 * The barrel top is faceted into this many box slats. The engine's mesh
 * vocabulary is box / sphere / cylinder — there is no half-cylinder, and a full
 * cylinder sunk to show only its top half would expose its round underside the
 * moment the lid swings open. So the dome is an honest arc of flat slats, which
 * also sits right with the chunky faceted look of everything else here: each
 * slat catches the key light at its own angle, so the curve reads from shading
 * as well as silhouette. Eight slats span the 180° arc (~22.5° a facet) so the
 * crown reads as a smooth rounded barrel top rather than a peaked five-slat
 * ridge — the reference chests carry a full, round hump, not a tent.
 */
const LID_ARC_SLATS = 8;
const LID_ARC_THICKNESS = 0.11;
/** How far the gold bands stand proud of the wood arc they wrap. */
const LID_BAND_SWELL = 0.025;

/**
 * One arc of slats sweeping the lid's full depth, from its back edge up over
 * the crown and down to its front edge.
 *
 * Every slat is placed by its OWN chord: the two arc points bounding it give
 * the slat's center, its length, and its tilt directly, so the facets meet edge
 * to edge at any slat count and any `swell` without a seam to tune.
 *
 * The offset is rotated by the LID's quaternion while the slat carries the lid
 * rotation composed with its own tilt — so the whole arc is welded to the lid
 * board beneath it. It swings on the hinge when the lid opens, and it rides the
 * chest's yaw/pitch/spiral when the chest moves, exactly like every other part.
 */
const lidArc = (
  keyPrefix: string,
  hinge: EngineVec3,
  lidQ: EngineQuat,
  grow: number,
  material: string,
  width: number,
  swell: number,
  atX = 0,
): readonly SceneInstance[] => {
  const depthRadius = LID.z / 2 + swell;
  const riseRadius = CHEST_LID_ARCH - LID_ARC_THICKNESS / 2 + swell;
  // The arc's mid-thickness surface, swept as a half-ellipse over the lid board.
  const arcAt = (t: number): { readonly y: number; readonly z: number } => {
    const angle = -Math.PI / 2 + t * Math.PI;
    return { y: LID.y + riseRadius * Math.cos(angle), z: LID.z / 2 + depthRadius * Math.sin(angle) };
  };
  return Array.from({ length: LID_ARC_SLATS }, (_, i): SceneInstance => {
    const from = arcAt(i / LID_ARC_SLATS);
    const to = arcAt((i + 1) / LID_ARC_SLATS);
    const dy = to.y - from.y;
    const dz = to.z - from.z;
    // A slat's local +Z runs along its chord. quatPitch(a) sends +Z to
    // (0, −sin a, cos a), so the tilt that aligns it with (dz, dy) is this.
    const tilt = Math.atan2(-dy, dz);
    return {
      key: `${keyPrefix}${i}`,
      material,
      mesh: "box",
      transform: {
        position: addV3(hinge, rotateByQuat(scaleV3(v3(atX, (from.y + to.y) / 2, (from.z + to.z) / 2), grow), lidQ)),
        rotation: quatMul(lidQ, quatPitch(tilt)),
        scale: scaleV3(v3(width, LID_ARC_THICKNESS, Math.hypot(dy, dz)), grow),
      },
    };
  });
};

interface ChestPose {
  /** The chest's GRID slot on the board — where its ground decor (warm pool,
   * focus ring) stays. Fixed for the whole round. */
  readonly origin: EngineVec3;
  /** Where the chest BODY actually is. Equal to `origin` (plus lift) while the
   * chest sits on the board, and somewhere along the spiral once it flies. */
  readonly at: EngineVec3;
  /** Flight progress in [0, 1]. Ground decor belongs to a chest that is ON the
   * board, so it fades out as this rises — an airborne chest pools no light on
   * a board it has left. */
  readonly flight: number;
  readonly yaw: number;
  readonly pitch: number;
  readonly squash: number;
  readonly scale: number;
  readonly lidAngle: number;
  readonly latchAngle: number;
  readonly dim: boolean;
  readonly selected: boolean;
  readonly focusRing: boolean;
  readonly hoverRing: boolean;
  readonly seam: number;
  readonly glow: number;
}

/** All instances of one posed chest (body, planks, gilding, latch, lid,
 * selection pool, seam light). Materials are chosen by facing (front/side/top)
 * and by pose state (dim / selected) rather than by texture. */
const chestInstances = (key: string, pose: ChestPose): readonly SceneInstance[] => {
  // Tilt toward the camera (a small back-pitch) when chosen; yaw carries idle sway.
  const q = quatMul(quatYaw(pose.yaw), quatPitch(-pose.pitch));
  const squashY = 1 - pose.squash;
  const squashXZ = 1 + pose.squash * 0.55;
  const grow = pose.scale;
  const origin = pose.at;
  // How much of the chest is still "on the board" — gates every ground-anchored
  // decoration below.
  const grounded = 1 - clamp01(pose.flight);

  const wood = pose.dim ? "WoodDim" : "WoodBrown";
  const woodSide = pose.dim ? "WoodDimSide" : "WoodSide";
  const woodLid = pose.dim ? "WoodDim" : "WoodLid";
  // Front trim brightens on hover/selection.
  const trimFront = pose.dim ? "GildDim" : pose.selected || pose.hoverRing ? "GildBright" : "GildFront";
  const trimSide = pose.dim ? "GildDim" : "GildSide";
  const trimTop = pose.dim ? "GildDim" : "GildTop";

  const part = (suffix: string, local: EngineVec3, scale: EngineVec3, material: string, extraQ = QUAT_IDENTITY): SceneInstance => ({
    key: `${key}:${suffix}`,
    material,
    mesh: "box",
    transform: {
      position: addV3(origin, rotateByQuat(v3(local.x * squashXZ * grow, local.y * squashY * grow, local.z * squashXZ * grow), q)),
      rotation: quatMul(q, extraQ),
      scale: v3(scale.x * squashXZ * grow, scale.y * squashY * grow, scale.z * squashXZ * grow),
    },
  });

  // Lid on its back hinge; latch hangs from the lid's front lip.
  const lidQ = quatMul(q, quatPitch(pose.lidAngle));
  const lidHingeLocal = v3(0, BODY.y, -BODY.z / 2);
  const lidHinge = addV3(origin, rotateByQuat(v3(lidHingeLocal.x * grow, lidHingeLocal.y * squashY * grow, lidHingeLocal.z * squashXZ * grow), q));
  const lid: SceneInstance = {
    key: `${key}:lid`,
    material: woodLid,
    mesh: "box",
    transform: hingedTransform(lidHinge, scaleV3(v3(0, LID.y / 2, LID.z / 2), grow), lidQ, scaleV3(LID, grow)),
  };
  const lidRim: SceneInstance = {
    key: `${key}:lidrim`,
    material: trimTop,
    mesh: "box",
    transform: hingedTransform(lidHinge, scaleV3(v3(0, LID.y / 2, LID.z - 0.02), grow), lidQ, scaleV3(v3(LID.x + 0.02, LID.y + 0.03, 0.05), grow)),
  };
  // The barrel top, and the two gold bands that wrap over it — continuations of
  // the body straps below, so the gilding runs unbroken from board to crown.
  const dome = lidArc(`${key}:dome`, lidHinge, lidQ, grow, woodLid, LID.x, 0);
  const bands = [-1, 1]
    .map((side) =>
      lidArc(`${key}:band${side < 0 ? "L" : "R"}`, lidHinge, lidQ, grow, trimSide, 0.075, LID_BAND_SWELL, side * BODY.x * 0.28),
    )
    .flat();

  const latchQ = quatMul(lidQ, quatPitch(pose.latchAngle));
  const latchHinge = addV3(lidHinge, rotateByQuat(scaleV3(v3(0, 0.02, LID.z - 0.01), grow), lidQ));
  const latch: SceneInstance = {
    key: `${key}:latch`,
    material: trimTop,
    mesh: "box",
    transform: hingedTransform(latchHinge, scaleV3(v3(0, -LATCH.y / 2, LATCH.z / 2), grow), latchQ, scaleV3(LATCH, grow)),
  };

  const interior: SceneInstance = part("interior", v3(0, BODY.y - 0.03, 0), v3(BODY.x - 0.1, 0.05, BODY.z - 0.1), "ChestInterior");
  const glow: SceneInstance[] =
    pose.glow > 0
      ? [part("glow", v3(0, BODY.y - 0.02, 0), scaleV3(v3(BODY.x - 0.2, 0.22, BODY.z - 0.2), pose.glow), "InnerGlow")]
      : [];

  // Warm light pool under a chosen chest: three layered translucent discs read
  // as a soft radial gradient rather than a flat marker.
  const pool: SceneInstance[] =
    (pose.glow > 0 || pose.selected) && grounded > 0.02
      ? [
          disc(`${key}:pool2`, "PoolOuter", v3(pose.origin.x, 0.02, pose.origin.z), BODY.x * (1.05 + pose.glow * 0.3) * grounded, 0.012),
          disc(`${key}:pool1`, "PoolMid", v3(pose.origin.x, 0.028, pose.origin.z), BODY.x * (0.78 + pose.glow * 0.22) * grounded, 0.012),
          disc(`${key}:pool0`, "PoolCore", v3(pose.origin.x, 0.036, pose.origin.z), BODY.x * (0.5 + pose.glow * 0.18) * grounded, 0.012),
        ]
      : [];

  // Warm seam light leaking from the lid/body join before it fully opens.
  const seam: SceneInstance[] =
    pose.seam > 0
      ? [
          {
            key: `${key}:seam`,
            material: "SeamGlow",
            mesh: "box",
            transform: {
              position: addV3(origin, rotateByQuat(v3(0, BODY.y * grow, (BODY.z / 2 - 0.02) * grow), q)),
              rotation: q,
              scale: v3((LID.x - 0.06) * grow, (0.02 + pose.seam * 0.14) * grow, 0.05 * grow),
            },
          },
        ]
      : [];

  // Hover/focus feedback is a soft warm pool, not a flat gold marker: an active
  // pointer/tap hover reads a touch brighter than a resting keyboard cursor.
  const ringBase = v3(pose.origin.x, 0.024, pose.origin.z);
  const rings: SceneInstance[] = pose.hoverRing
    ? [disc(`${key}:ring1`, "PoolMid", ringBase, BODY.x * 1.05, 0.012), disc(`${key}:ring0`, "PoolCore", ringBase, BODY.x * 0.62, 0.012)]
    : pose.focusRing
      ? [disc(`${key}:ring`, "PoolOuter", ringBase, BODY.x * 1.0, 0.012)]
      : [];

  return [
    ...pool,
    part("body", v3(0, BODY.y / 2, 0), BODY, wood),
    // Board gap lines (darkest) read as separate planks without a texture. The
    // reference chest bodies are carved into a stack of four distinct boards, so
    // three evenly-spaced grooves divide the face rather than two — and each
    // groove stands a touch prouder and thicker than a hairline so the near-black
    // seam actually reads as the gap between planks under the bright rig, which is
    // the only thing carving the untextured wood into stacked boards.
    part("gap1", v3(0, BODY.y * 0.26, 0), v3(BODY.x + 0.02, 0.034, BODY.z + 0.02), "WoodGap"),
    part("gap2", v3(0, BODY.y * 0.5, 0), v3(BODY.x + 0.02, 0.034, BODY.z + 0.02), "WoodGap"),
    part("gap3", v3(0, BODY.y * 0.74, 0), v3(BODY.x + 0.02, 0.034, BODY.z + 0.02), "WoodGap"),
    // Side-facing wood on the end caps for a value step.
    part("endL", v3(-BODY.x / 2 + 0.02, BODY.y / 2, 0), v3(0.04, BODY.y - 0.04, BODY.z - 0.04), woodSide),
    part("endR", v3(BODY.x / 2 - 0.02, BODY.y / 2, 0), v3(0.04, BODY.y - 0.04, BODY.z - 0.04), woodSide),
    // Gilding: side straps (darker ochre), corner edges (side), front lock plate (bright).
    part("strapL", v3(-BODY.x * 0.28, BODY.y / 2, 0), v3(0.07, BODY.y + 0.02, BODY.z + 0.03), trimSide),
    part("strapR", v3(BODY.x * 0.28, BODY.y / 2, 0), v3(0.07, BODY.y + 0.02, BODY.z + 0.03), trimSide),
    part("edgeL", v3(-BODY.x / 2, BODY.y / 2, BODY.z / 2), v3(0.05, BODY.y + 0.02, 0.05), trimSide),
    part("edgeR", v3(BODY.x / 2, BODY.y / 2, BODY.z / 2), v3(0.05, BODY.y + 0.02, 0.05), trimSide),
    part("plate", v3(0, BODY.y * 0.5, BODY.z / 2 + 0.005), v3(0.26, 0.2, 0.04), trimFront),
    interior,
    ...glow,
    ...seam,
    lid,
    ...dome,
    ...bands,
    lidRim,
    latch,
    ...rings,
  ];
};

// ── the light burst (bounded: soft glow + a few rays + a few motes) ─────────────

const lightBurst = (at: EngineVec3, tick: number, t: number, s: number): readonly SceneInstance[] => {
  // t is burst progress 0→1 over the burst window; intensity peaks early, fades.
  // `s` scales the whole figure with the hero chest it erupts from.
  const strength = pulse(t);
  if (strength <= 0.001) {
    return [];
  }
  const rise = (0.2 + t * 0.9) * s;
  const glow: SceneInstance = disc("burst:glow", "BurstGlow", v3(at.x, at.y + 0.05 * s, at.z), (0.35 + strength * 0.9) * s, 0.02);
  const rays = [0, 1, 2, 3, 4].map((i) => {
    const a = (i / 5) * Math.PI * 2 + tick * 0.02;
    const spread = 0.28 * strength * s;
    return {
      key: `burst:ray${i}`,
      material: "BurstRay",
      mesh: "box",
      transform: {
        position: v3(at.x + Math.cos(a) * spread, at.y + (0.3 * s + rise * 0.5), at.z + Math.sin(a) * spread),
        rotation: quatYaw(a),
        scale: scaleV3(v3(0.05 + strength * 0.05, 0.5 + strength * 1.3, 0.05 + strength * 0.05), s),
      },
    } satisfies SceneInstance;
  });
  const motes = Array.from({ length: CHEST_TIMING.burstParticles }, (_, i) => {
    const a = (i / CHEST_TIMING.burstParticles) * Math.PI * 2 + i * 1.3;
    const r = (0.15 + (i % 3) * 0.12) * (0.4 + t) * s;
    const climb = rise * (0.6 + (i % 4) * 0.18);
    const size = (0.05 + (i % 2) * 0.02) * strength * s;
    return {
      key: `burst:mote${i}`,
      material: "Mote",
      mesh: "sphere",
      transform: { position: v3(at.x + Math.cos(a) * r, at.y + 0.25 * s + climb, at.z + Math.sin(a) * r), rotation: QUAT_IDENTITY, scale: v3(size, size, size) },
    } satisfies SceneInstance;
  });
  return [glow, ...rays, ...motes];
};

// ── the hero prize (rises fully clear of the chest, large, spinning, pulsing) ──

/**
 * The prize the winning chest yields — a big spinning rarity gem that climbs
 * fully out to hover as the frame's focal point, with a settle bob, a size
 * pulse, and a pulsing halo behind it. `at` is the chest's open mouth; `riseT`
 * the climb progress; `settle` ramps in the idle bob/pulse once it has arrived.
 */
const heroPrize = (rarity: Parameters<typeof rewardMaterialOf>[0], at: EngineVec3, riseT: number, tick: number, settle: number, s: number): readonly SceneInstance[] => {
  const material = rewardMaterialOf(rarity);
  // The climb and the gem both scale with the hero chest, but DAMPED: at full
  // hero scale an undamped rise would carry the prize straight out of frame.
  const rise = s * CHEST_TIMING.riseDamp;
  const gem = s * CHEST_TIMING.prizeDamp;
  const climb = CHEST_TIMING.riseHeight * easeOutBack(riseT) * rise;
  const bob = Math.sin(tick * 0.12) * 0.035 * settle * gem;
  const center = v3(at.x, at.y + climb + bob, at.z);
  const rarityBonus = rarity === "jackpot" ? 0.18 : rarity === "rare" ? 0.1 : 0;
  const size = (0.54 + rarityBonus) * (0.5 + 0.5 * riseT) * (1 + Math.sin(tick * 0.16) * 0.04 * settle) * gem;
  const halo = 0.82 * (0.5 + 0.5 * riseT) * (0.9 + Math.sin(tick * 0.14) * 0.12 * settle) * gem;
  const spin = quatYaw(tick * 0.04);
  return [
    disc("reward:halo", "BurstGlow", v3(center.x, center.y, center.z + 0.001), halo, 0.02),
    { key: "reward:core", material, mesh: "sphere", transform: { position: center, rotation: spin, scale: v3(size, size, size) } },
    { key: "reward:facet", material, mesh: "box", transform: { position: center, rotation: quatYaw(tick * 0.04 + 0.7), scale: v3(size * 0.72, size * 0.72, size * 0.72) } },
  ];
};

// ── the arcade platform (rim, central glow, edge falloff, corner rivets) ────────

/**
 * Radius of the turquoise lagoon the chests sit on. In the reference the water
 * is a small rounded pool with a wide golden-sand beach all around it, not a
 * full-frame flood — so the disc is sized only a little larger than the nine
 * chests it holds (whose grid reaches ~3.6 world-units from center), leaving a
 * broad ring of the sandy `stageRoom` floor showing around the pool. The other
 * discs (edge vignette, center glow) and the rim rivets keep their original
 * proportions relative to this radius.
 */
export const WATER_RADIUS = 5.0;

const platform = (): readonly SceneInstance[] => [
  disc("plat:vignette", "EdgeVignette", v3(0, -0.048, 0), WATER_RADIUS * (9 / 8.4), 0.006),
  disc("plat:side", "PlatformSide", v3(0, -0.062, 0), WATER_RADIUS, 0.06),
  disc("plat:glow", "CenterGlow", v3(0, -0.03, 0), WATER_RADIUS * (4.4 / 8.4), 0.006),
  ...[
    [-1, -1],
    [1, -1],
    [-1, 1],
    [1, 1],
  ].map(([sx, sz], i) => ({
    key: `plat:rivet${i}`,
    material: "BoardRivet",
    mesh: "cylinder" as const,
    transform: { position: v3((sx ?? 0) * WATER_RADIUS * (6.7 / 8.4), -0.02, (sz ?? 0) * WATER_RADIUS * (6.7 / 8.4)), rotation: QUAT_IDENTITY, scale: v3(0.34, 0.05, 0.34) },
  })),
];

// ── the beach set-dressing (palm, sandcastle, crab, shells) ─────────────────────

/*
 * The reference stages the lagoon inside a lived-in cartoon beach: a leaning palm
 * at the far left, a turreted sandcastle flying a red flag at the far right, a
 * little red crab on the near sand, and shells/starfish dotted around the shore.
 * The champion left that sand bare, so the frame read as a lone pool of chests.
 *
 * None of it needs a new primitive — every prop is an assembly of the same box /
 * cylinder / sphere vocabulary the chests are built from, placed ONCE on the sand
 * ring OUTSIDE the water disc (radius > the vignette so nothing floats on the
 * lagoon) so the decor frames the pool the way the reference does. It is purely
 * static cosmetic dressing: it reads neither the outcome nor the tick, and sits
 * behind the veil so a hero reveal still dims it away with the rest of the stage.
 */

/** A single decor box/cylinder/sphere at a world position. */
const decorPart = (
  key: string,
  material: string,
  mesh: "box" | "cylinder" | "sphere",
  position: EngineVec3,
  scale: EngineVec3,
  rotation: EngineQuat = QUAT_IDENTITY,
): SceneInstance => ({ key, material, mesh, transform: { position, rotation, scale } });

/** A leaning palm swaying in the wind: a curved stack of tapering bark cylinders,
 * a coconut cluster, and a fan of drooping frond boards radiating from the crown.
 * `tick` drives a gentle whole-crown sway (bend grows with height, so the trunk
 * arcs and the crown leads) plus a faster per-frond flutter — a pure function of
 * the tick via `palmSway`, so it can never correlate with the outcome. */
const PALM_CROWN_Y = 2.66;
const palmTree = (origin: EngineVec3, tick: number): readonly SceneInstance[] => {
  const sway = palmSway(tick);
  const segs = [
    { y: 0.4, x: 0.0, r: 0.34, tilt: 0.04, mat: "PalmBarkDark" },
    { y: 1.08, x: 0.12, r: 0.3, tilt: 0.12, mat: "PalmBark" },
    { y: 1.74, x: 0.3, r: 0.26, tilt: 0.22, mat: "PalmBarkDark" },
    { y: 2.34, x: 0.56, r: 0.22, tilt: 0.34, mat: "PalmBark" },
  ];
  // Bend scales with height so the base stays planted and the crown travels most.
  const bendAt = (y: number): number => sway.bend * (y / PALM_CROWN_Y) ** 1.6;
  const trunk = segs.map((s, i) =>
    decorPart(
      `palm:trunk${i}`,
      s.mat,
      "cylinder",
      addV3(origin, v3(s.x + bendAt(s.y) * 1.4, s.y, 0)),
      v3(s.r * 2, 0.72, s.r * 2),
      quatRoll(-s.tilt - bendAt(s.y)),
    ),
  );
  const crown = addV3(origin, v3(0.74 + sway.bend * 1.4, PALM_CROWN_Y, 0));
  // The whole crown rolls with the wind, carrying the coconuts and frond bases.
  const crownRoll = quatRoll(-sway.bend);
  const coconuts = [v3(-0.14, -0.04, 0.12), v3(0.12, -0.02, -0.14), v3(-0.02, -0.16, -0.02)].map((d, i) =>
    decorPart(`palm:coco${i}`, "Coconut", "sphere", addV3(crown, rotateByQuat(d, crownRoll)), v3(0.2, 0.2, 0.2)),
  );
  const fronds = Array.from({ length: 7 }, (_, i): SceneInstance => {
    const a = (i / 7) * Math.PI * 2;
    const droop = 0.55 + (i % 2) * 0.12 + sway.flutter(i);
    const q = quatMul(crownRoll, quatMul(quatYaw(a), quatPitch(droop)));
    const len = 1.5 + (i % 3) * 0.14;
    return decorPart(
      `palm:frond${i}`,
      i % 2 === 0 ? "PalmLeaf" : "PalmLeafDark",
      "box",
      addV3(crown, rotateByQuat(v3(0, 0.05, len / 2), q)),
      v3(0.34, 0.09, len),
      q,
    );
  });
  return [...trunk, ...coconuts, ...fronds];
};

/** Yaw of the whole sandcastle so its square base runs parallel to the diagonal
 * shore line it sits behind (rather than square to the world axes). Clockwise
 * from the top-down view. */
const CASTLE_YAW = -1.05;

/** A turreted sandcastle: a broad base, a central keep with two flanking turrets,
 * crenellations, an arched door, and a red pennant on a pole. The whole assembly
 * is yawed by `CASTLE_YAW` about its origin so it lines up with the shore. */
const sandcastle = (origin: EngineVec3): readonly SceneInstance[] => {
  const q = quatYaw(CASTLE_YAW);
  // Place a part given in castle-local space: rotate its offset into the yawed
  // frame and compose the yaw into its own rotation, so the castle turns as one.
  const place = (key: string, material: string, mesh: "box" | "cylinder", local: EngineVec3, scale: EngineVec3): SceneInstance =>
    decorPart(key, material, mesh, addV3(origin, rotateByQuat(local, q)), scale, q);
  const base = place("castle:base", "CastleSandDark", "box", v3(0, 0.28, 0), v3(2.4, 0.56, 2.0));
  const towers = [
    { key: "keep", x: 0, r: 0.52, h: 1.7, mat: "CastleSand" },
    { key: "turnL", x: -0.92, r: 0.34, h: 1.15, mat: "CastleSandDark" },
    { key: "turnR", x: 0.92, r: 0.34, h: 1.15, mat: "CastleSandDark" },
  ];
  const towerParts = towers
    .map((t): readonly SceneInstance[] => {
      const top = 0.56 + t.h;
      const shaft = place(`castle:${t.key}`, t.mat, "cylinder", v3(t.x, 0.56 + t.h / 2, 0), v3(t.r * 2, t.h, t.r * 2));
      const crenels = Array.from({ length: 6 }, (_, i): SceneInstance => {
        const a = (i / 6) * Math.PI * 2;
        return place(
          `castle:${t.key}cren${i}`,
          "CastleSand",
          "box",
          v3(t.x + Math.cos(a) * t.r * 0.82, top + 0.11, Math.sin(a) * t.r * 0.82),
          v3(0.16, 0.22, 0.16),
        );
      });
      return [shaft, ...crenels];
    })
    .flat();
  const door = place("castle:door", "CastleDoor", "box", v3(0, 0.5, 1.0), v3(0.42, 0.62, 0.08));
  const poleTop = 0.56 + 1.7;
  const pole = place("castle:pole", "CastlePole", "cylinder", v3(0, poleTop + 0.42, 0), v3(0.05, 0.84, 0.05));
  const flag = place("castle:flag", "CastleFlag", "box", v3(0.24, poleTop + 0.66, 0), v3(0.44, 0.28, 0.03));
  return [base, ...towerParts, door, pole, flag];
};

/** A stubby cartoon crab with a small set of idle animations: a domed shell, two
 * eyestalks, two front claws, and a row of little legs down each side. `crabIdle`
 * elects one bit of business (scuttle / claw wave / bob / turn) or a rest on a
 * random interval from the ambient stream; here every part is placed through the
 * resulting body frame so the crab scoots, bobs, turns, waves, and breathes as
 * one creature. Pure in (tick, seed) — outcome-independent. */
const crab = (origin: EngineVec3, tick: number, seed: number): readonly SceneInstance[] => {
  const pose = crabIdle(tick, seed);
  const bodyQ = quatYaw(pose.yaw);
  const bodyShift = v3(pose.scootX, pose.bob, 0);
  // Place a part given in body-local space: rotate its offset into the (turned)
  // body frame, add the whole-body scoot/bob, and compose the body yaw into its
  // own rotation, so one pose moves the crab as a single creature.
  const place = (key: string, material: string, mesh: "box" | "sphere", local: EngineVec3, scale: EngineVec3, localRot: EngineQuat = QUAT_IDENTITY): SceneInstance =>
    decorPart(key, material, mesh, addV3(origin, addV3(bodyShift, rotateByQuat(local, bodyQ))), scale, quatMul(bodyQ, localRot));
  const body = place("crab:body", "CrabShell", "sphere", v3(0, 0.2, 0), v3(0.62, 0.4 * (1 + pose.breath), 0.5));
  const eyes = [-1, 1]
    .map((s): readonly SceneInstance[] => [
      place(`crab:stalk${s}`, "CrabShell", "box", v3(s * 0.14, 0.44, 0.16), v3(0.06, 0.18, 0.06), quatRoll(-s * pose.eye)),
      place(`crab:eye${s}`, "CrabEye", "sphere", v3(s * 0.14 + s * pose.eye * 0.12, 0.55, 0.16), v3(0.1, 0.1, 0.1)),
    ])
    .flat();
  const claws = [-1, 1]
    .map((s): readonly SceneInstance[] => {
      // Each claw lifts and snaps on its own phase, so a wave alternates sides.
      const lift = pose.clawLift * (0.7 + 0.3 * Math.sin(tick * 0.5 + (s > 0 ? 0 : Math.PI)));
      return [
        place(`crab:arm${s}`, "CrabShellDark", "box", v3(s * 0.42, 0.18 + lift * 0.12, 0.24), v3(0.1, 0.09, 0.28), quatRoll(s * lift)),
        place(`crab:claw${s}`, "CrabShell", "sphere", v3(s * 0.5, 0.18 + lift * 0.3, 0.42), v3(0.22, 0.18, 0.2), quatRoll(s * lift)),
      ];
    })
    .flat();
  const legs = [-1, 1]
    .map((s): readonly SceneInstance[] =>
      [-0.16, 0.02, 0.2].map((z, i) => {
        const wiggle = pose.legWiggle * Math.sin(tick * 0.7 + i * 1.2);
        return place(`crab:leg${s}_${i}`, "CrabShellDark", "box", v3(s * 0.38, 0.08, z), v3(0.24, 0.06, 0.07), quatYaw(s * 0.5 + s * wiggle));
      }),
    )
    .flat();
  return [body, ...eyes, ...claws, ...legs];
};

/** Shells and a couple of starfish scattered on the shore. Positions are on the
 * sand ring (radius clear of the water vignette). */
const beachLitter = (): readonly SceneInstance[] => {
  const shells = [v3(5.2, 0, 1.9), v3(-3.7, 0, -4.7), v3(2.6, 0, 5.2), v3(6.0, 0, -0.9), v3(-6.0, 0, -1.7)].map((at, i) =>
    decorPart(`shell${i}`, "Shell", "sphere", v3(at.x, 0.09, at.z), v3(0.28, 0.16, 0.24)),
  );
  const starfish = [v3(4.5, 0, 3.9), v3(-3.0, 0, 5.2)]
    .map((at, i): readonly SceneInstance[] => {
      const arms = Array.from({ length: 5 }, (_, k): SceneInstance =>
        decorPart(`star${i}:arm${k}`, "Starfish", "box", v3(at.x, 0.05, at.z), v3(0.12, 0.05, 0.44), quatYaw((k / 5) * Math.PI * 2)),
      );
      return arms;
    })
    .flat();
  return [...shells, ...starfish];
};

/** The whole shore of set-dressing. The palm/castle/crab are placed at the
 * player-controlled positions in `decor` (they can be picked up and moved); the
 * one currently held is lifted so it reads as "in hand". The palm and crab are
 * alive (wind sway / idle animations); `tick`/`seed` drive only those poses, via
 * pure ambient-keyed values — nothing here reads the outcome. The litter is
 * fixed. */
const HELD_LIFT = v3(0, 0.5, 0);
const beachDecor = (tick: number, seed: number, decor: DecorDrag): readonly SceneInstance[] => {
  const at = (key: keyof DecorDrag["props"]): EngineVec3 => addV3(decor.props[key], decor.held === key ? HELD_LIFT : v3(0, 0, 0));
  return [...palmTree(at("palm"), tick), ...sandcastle(at("castle")), ...crab(at("crab"), tick, seed), ...beachLitter()];
};

// ── the background veil ─────────────────────────────────────────────────────────

/**
 * A dark sheet hung across the frustum BETWEEN the hero chest and everything
 * else — the board, the eight other chests, the platform, the backdrop. As the
 * chosen chest spirals forward the veil rises behind it, so the stage falls
 * away into near-darkness and the chest is left owning a quiet frame.
 *
 * A veil is the honest tool here rather than dimming the lights: the pavilion
 * backdrop is EMISSIVE, so it ignores lighting entirely and would stay bright
 * while everything around it fell dark. Occluding it works on every material.
 * The renderer draws translucent geometry after opaque, depth-tested but
 * without depth writes, so the hero chest — nearer than the veil — punches
 * through it for free, with no sorting work here.
 */
const backgroundVeil = (camera: Camera3D, framing: HeroFraming, level: number): readonly SceneInstance[] => {
  const material = veilMaterialOf(level);
  if (material === null) {
    return [];
  }
  const depth = framing.distance + CHEST_TIMING.veilGap;
  const half = depth * Math.tan(camera.fovY / 2);
  // Turn the sheet's +Z face back down the view axis so it squarely faces the
  // camera, and oversize it well past the frustum so no aspect ratio can
  // uncover an edge.
  return [
    {
      key: "veil",
      material,
      mesh: "box",
      transform: {
        position: addV3(camera.position, scaleV3(framing.forward, depth)),
        rotation: quatPitch(Math.atan2(framing.forward.y, -framing.forward.z)),
        scale: v3(half * 9, half * 5, 0.02),
      },
    },
  ];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const chestScene = (runtime: GameRuntime<ChestSpec>, state: ChestState): Scene => {
  const session = state.session;
  const count = session.config.choiceCount ?? 9;
  const seed = session.seed;
  const tick = session.tick;
  const speed = session.config.presentationSpeed;
  const spec = runtime.config.gameSpecific;
  const choice = state.extra.choice;
  const selected = choice.selected;
  const plan = session.committed;
  const timeline = revealTimeline(speed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing" ? phaseAge(session) : session.phase === "celebrating" || session.phase === "complete" ? timeline.total : -1;
  const idleActive = session.phase === "ready" || session.phase === "intro";
  const liveliness = idleActive ? spec.danceLiveliness : 0;
  // A winning reveal earns the warm treasure glow + light burst; an empty chest
  // opens through the same ritual but stays dim inside (honest, not broken).
  const winReveal = plan !== null && plan.win && outcomeRarity(session) !== "loss";

  // Master selection ramp: 0 before a pick, eases in over the commit pause, then
  // holds at 1 through the whole reveal and result — the whole scene reorganizes.
  const selectT =
    selected === null
      ? 0
      : session.phase === "committing"
        ? clamp01(phaseAge(session) / speedTicks(CHEST_TIMING.liftInTicks, speed))
        : session.phase === "ready"
          ? 0
          : 1;
  const selectEase = easeOutCubic(selectT);

  // Burst progress (0→1 over the burst window, right after the lid opens).
  const burstT = revealAge >= timeline.burstAt ? clamp01((revealAge - timeline.burstAt) / Math.max(1, timeline.lidEnd - timeline.lidStart)) : 0;

  // ── the hero flight ───────────────────────────────────────────────────────────
  // The camera does NOT move in this game. Instead the chosen chest leaves the
  // board and spirals into a close hero framing derived from that fixed camera
  // — which is why the other eight stay exactly where the player left them.
  //
  // The flown transform is computed ONCE here and is the single anchor every
  // downstream element hangs off (prize, burst, reveal lights, celebration).
  // Before this, six call sites each independently re-derived "where the chosen
  // chest is" from its grid slot; now the chest moves and they all follow it.
  const camera = chestCamera(count);
  const framing = heroFraming(camera);
  const flight = selected === null ? 0 : flightProgress(session, speed);
  const liftAmount = CHEST_TIMING.lift * selectEase;
  // `framing.anchor` frames the chest's CENTER; a chest is posed from its base.
  const heroBase = addV3(framing.anchor, v3(0, (-CHEST_HEIGHT / 2) * framing.scale, 0));
  const flown = spiralFlight(
    addV3(selected === null ? v3(0, 0, 0) : chestPosition(selected, count), v3(0, liftAmount, 0)),
    heroBase,
    flight,
    framing,
  );
  const heroScale = lerp(lerp(1, CHEST_TIMING.selectScale, selectEase), framing.scale, flown.grow);
  /** The chosen chest's open mouth, wherever the flight has carried it. */
  const heroTop = addV3(flown.position, v3(0, BODY_TOP * heroScale, 0));

  const chests = Array.from({ length: count }, (_, index) => {
    const origin = chestPosition(index, count);
    const dance = dancePose(index, count, tick, seed, liveliness);
    const isSelected = selected === index;

    // Continuous, per-chest-desynced idle breathe (stilled once a pick is made).
    const idleGate = liveliness * (1 - selectT);
    const ph = idlePhase(index);
    const clock = (tick / CHEST_TIMING.idleBobPeriod) * 2 * Math.PI;
    const idleBob = Math.sin(clock + ph) * CHEST_TIMING.idleBobAmp * idleGate;
    const idleTwist = Math.sin(clock * 0.5 + ph) * CHEST_TIMING.idleTwistAmp * idleGate;

    // Anticipation brace: a tiny shiver before the latch moves (selected only).
    const bracing = isSelected && revealAge >= 0 && revealAge < timeline.braceEnd;
    const braceT = bracing ? revealAge / timeline.braceEnd : 0;
    const shiver = bracing ? Math.sin(revealAge * 1.5) * CHEST_TIMING.shakeMag * pulse(braceT) : 0;

    // Latch: swings open over [latchStart, latchEnd] with a recoil snap at the end.
    const latchT = isSelected ? clamp01((revealAge - timeline.latchStart) / Math.max(1, timeline.latchEnd - timeline.latchStart)) : 0;
    const latchRecoil = isSelected && revealAge >= timeline.latchEnd && revealAge < timeline.latchEnd + 4 ? Math.sin((revealAge - timeline.latchEnd) * 1.3) * CHEST_TIMING.latchRecoil * (1 - (revealAge - timeline.latchEnd) / 4) : 0;
    // Lid: opens with an overshoot-and-settle after the pause.
    const lidT = isSelected ? clamp01((revealAge - timeline.lidStart) / Math.max(1, timeline.lidEnd - timeline.lidStart)) : 0;
    // Seam light grows from latch-land through the lid opening.
    const seam = isSelected ? clamp01((revealAge - timeline.seamStart) / Math.max(1, timeline.lidEnd - timeline.seamStart)) * (1 - lidT * 0.6) : 0;

    const dimmed = selected !== null && !isSelected;

    // A chosen chest rides the spiral; every other chest stays in its slot,
    // breathing on the idle bob.
    const lift = isSelected ? liftAmount : idleBob;
    const at = isSelected ? flown.position : v3(origin.x, origin.y + lift, origin.z);

    return chestInstances(`chest${index}`, {
      at,
      dim: dimmed,
      flight: isSelected ? flight : 0,
      focusRing: session.phase === "ready" && choice.focused === index && choice.hovered !== index && choice.armed !== index,
      glow: isSelected ? easeOutCubic(lidT) * (winReveal ? 1 : 0.32) : 0,
      hoverRing: session.phase === "ready" && (choice.hovered === index || choice.armed === index),
      latchAngle: easeOutCubic(latchT) * CHEST_TIMING.latchDrop + latchRecoil,
      lidAngle: -easeOutBack(lidT) * CHEST_TIMING.lidOpen,
      origin,
      pitch: isSelected ? CHEST_TIMING.tilt * selectEase + flown.tumble : 0,
      scale: isSelected ? heroScale : 1,
      seam,
      selected: isSelected,
      squash: dance.squash + (bracing ? pulse(braceT) * 0.05 : 0),
      yaw: dance.twist + idleTwist + shiver + (isSelected ? flown.spin : 0),
    });
  }).flat();

  // Reward / empty reveal rising fully clear of the selected, open chest.
  const rewardInstances: SceneInstance[] = [];
  const burst: SceneInstance[] = [];
  if (selected !== null && plan !== null && revealAge >= timeline.lidEnd) {
    const chestTop = heroTop;
    const riseT = clamp01((revealAge - timeline.lidEnd) / Math.max(1, timeline.riseEnd - timeline.lidEnd));
    const settle = clamp01((revealAge - timeline.riseEnd) / 20);
    const rarity = outcomeRarity(session);

    if (rarity !== "loss") {
      // A win: the warm light burst fires and the prize climbs fully clear of
      // the chest to hover as the frame's focal point.
      burst.push(...lightBurst(chestTop, tick, burstT, heroScale));
      rewardInstances.push(...heroPrize(rarity, chestTop, riseT, tick, settle, heroScale));
    } else {
      // An empty chest: a playful grey dust puff coughs up and out (no burst,
      // no prize) — a clear, warm "nothing here this time".
      const puffs = Array.from({ length: 6 }, (_, i) => {
        const local = revealAge - timeline.lidEnd - i * 2.5;
        const life = 46;
        if (local < 0 || local > life) {
          return null;
        }
        const pt = local / life;
        const a = (i / 6) * Math.PI * 2 + i;
        // The puff belongs to the chest, so it grows and climbs with it — damped
        // on the climb for the same reason the prize is: to stay in frame.
        const puff = heroScale * CHEST_TIMING.prizeDamp;
        const spread = (0.16 + pt * 0.5) * puff;
        const size = (0.16 + pt * 0.3) * (1 - pt * 0.35) * puff;
        return {
          key: `dust:${i}`,
          material: "DustPuff",
          mesh: "sphere",
          transform: {
            position: v3(
              chestTop.x + Math.cos(a) * spread,
              chestTop.y + (0.05 + pt * 0.75) * heroScale * CHEST_TIMING.riseDamp,
              chestTop.z + Math.sin(a) * spread,
            ),
            rotation: QUAT_IDENTITY,
            scale: v3(size, size * 0.82, size),
          },
        } satisfies SceneInstance;
      }).filter((d): d is SceneInstance => d !== null);
      rewardInstances.push(...puffs);
    }
  }

  // Celebration.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(heroTop, v3(0, 0.4 * heroScale, 0));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Lights: standard rig, a warm escape light once the lid parts, and a brief
  // burst light that flashes the chest faces at the pop. All three follow the
  // FLOWN chest, so the reveal stays lit as it travels off the board.
  const focus = selected === null ? v3(0, 0, 0) : flown.position;
  // Beach sun, not casino sky. The shared rig pairs the warm key with a cool
  // sky fill tuned at 0.35 — right indoors, but here it lifts and cools every
  // shadow face, milkifying the chests and washing the warm seams to grey. This
  // scene's reference is a single warm raked sun: lit lids blazing, side/front
  // boards falling to deep warm brown. So we knock the cool fill down hard for
  // THIS scene only, letting the shadow faces settle onto the warm key plus the
  // neutral ambient floor — widening the light-driven lit-vs-shadow spread that
  // sculpts each faceted chest and keeping the darks warm rather than blue.
  const lights: SceneLight[] = stageLights(focus, 0.5 + 0.4 * selectEase).map((entry) =>
    entry.key === "light:fill" ? { key: entry.key, light: { ...entry.light, intensity: 0.12 } } : entry,
  );
  if (selected !== null && revealAge >= timeline.pauseEnd) {
    const warm = clamp01((revealAge - timeline.pauseEnd) / 12);
    lights.push({
      key: "light:chest",
      light: { color: [1, 0.82, 0.45, 1], intensity: 1.3 * warm * (winReveal ? 1 : 0.4), kind: "point", position: addV3(flown.position, scaleV3(v3(0, 1.1, 0.3), heroScale)) },
    });
  }
  if (winReveal && selected !== null && burstT > 0 && burstT < 1) {
    lights.push({
      key: "light:burst",
      light: { color: [1, 0.9, 0.6, 1], intensity: 1.8 * pulse(burstT), kind: "point", position: addV3(flown.position, scaleV3(v3(0, 1.5, 0.2), heroScale)) },
    });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    // The veil sits between the board and the hero chest: everything before it
    // in this list is what gets dimmed, everything after it stays clear.
    instances: [
      // The floor-ring is pulled in to the water radius so the sandy floor slab
      // reads as a wide beach around the inset lagoon rather than one more
      // turquoise disc flooding the frame out to the old ring radius.
      //
      // The slab itself is sized MUCH larger than the water so the sandy beach
      // fills the frame all the way to the top edge. At the tabletop pitch the
      // top-of-frame frustum ray strikes the ground well past the old radius-8
      // slab, so its far edge fell short and the emissive pastel backdrop/sky
      // leaked in as a light-blue horizon band across the top — a horizon the
      // reference does not have (there the sandy beach, with palm and sandcastle,
      // runs unbroken to the top edge with no sky showing). Extending the slab
      // past the furthest in-frame ground point drops that whole band onto beach,
      // cropping the horizon out and matching the reference's full-bleed sand.
      // The turquoise ring (accentRadius = WATER_RADIUS) is unchanged, so the
      // inset lagoon and its beach margin keep exactly the held framing.
      ...stageRoom(48, WATER_RADIUS),
      ...platform(),
      ...beachDecor(tick, seed, state.extra.decor),
      ...chests,
      ...backgroundVeil(camera, framing, flight),
      ...burst,
      ...rewardInstances,
      ...celebration,
    ],
    lights,
  };
};
