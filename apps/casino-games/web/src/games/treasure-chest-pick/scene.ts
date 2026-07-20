/*
 * scene.ts — Treasure Chest Pick presentation: nine carved-wood, gold-gilded
 * chests staged as a small arcade ritual. Idle chests breathe out of unison and
 * flash an occasional gold gleam; a chosen chest lifts, tilts toward the camera,
 * and pools warm light beneath it while the other eight dim and go still; the
 * reveal is a readable sequence — anticipation shake, latch drop with a recoil
 * snap, warm light through the seam, a weighty overshooting lid, a compact light
 * burst, and a prize that rises fully clear of the chest to own the frame (or a
 * playful dust puff on an empty chest). Pure view: `chestScene(runtime, state)`
 * returns a Scene value; every animated quantity is a pure function of the tick.
 *
 * Nothing here reads the population or the winning slot for cosmetics: the idle
 * dance draws only from the ambient stream, and the gleam/breathe are pure in
 * (index, tick) — so no wobble can hint at which chest holds a prize.
 */

import type { MaterialSpec, Scene, SceneInstance, SceneLight } from "@axiom/web-engine";
import type { EngineVec3, GameResources } from "@axiom/web-engine";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { revealFocusCamera } from "../../presentation/cameras/presets.ts";
import { REWARD_MATERIALS, rewardMaterialOf } from "../../presentation/rewards/tiers.ts";
import { celebrationFor, outcomeRarity, speedTicks } from "../round-state.ts";
import { clamp01, easeOutBack, easeOutCubic, lerp, pulse, smoothstep } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import {
  addV3,
  hingedTransform,
  QUAT_IDENTITY,
  quatMul,
  quatPitch,
  quatYaw,
  rotateByQuat,
  scaleV3,
  v3,
} from "../../presentation/stage/vectors.ts";
import type { ChestSpec, ChestState } from "./game.ts";
import { CHEST_TIMING, chestCamera, chestPosition, dancePose, goldGleam, idlePhase, revealTimeline } from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  // Wood, value-stepped so the chest reads solid without a texture: the lid
  // catches the key light (lightest), the front boards sit mid, side boards go
  // darker, and the gaps between planks are the darkest brown.
  WoodLid: { baseColor: [0.64, 0.42, 0.24, 1] },
  WoodBrown: { baseColor: [0.56, 0.36, 0.2, 1] },
  WoodSide: { baseColor: [0.44, 0.28, 0.15, 1] },
  WoodGap: { baseColor: [0.3, 0.19, 0.1, 1] },
  WoodDim: { baseColor: [0.34, 0.24, 0.16, 1] },
  WoodDimSide: { baseColor: [0.27, 0.19, 0.12, 1] },
  ChestInterior: { baseColor: [0.14, 0.09, 0.05, 1] },
  // Gold, likewise stepped: a pale highlight on upward edges/latch, the main
  // yellow on front trim, a darker ochre on side-facing straps — not uniformly
  // emissive, so it reads as metal catching light rather than glowing.
  GildTop: { baseColor: [1, 0.9, 0.5, 1], emissive: [0.28, 0.22, 0.06, 1] },
  GildFront: { baseColor: [0.98, 0.76, 0.28, 1], emissive: [0.12, 0.09, 0.02, 1] },
  GildSide: { baseColor: [0.7, 0.5, 0.18, 1] },
  GildDim: { baseColor: [0.5, 0.38, 0.16, 1] },
  GildGleam: { baseColor: [1, 0.97, 0.72, 1], emissive: [0.7, 0.62, 0.32, 1] },
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
  PlatformSide: { baseColor: [0.2, 0.56, 0.56, 1] },
  CenterGlow: { baseColor: [1, 0.88, 0.6, 1], emissive: [0.14, 0.1, 0.04, 1], opacity: 0.1 },
  EdgeVignette: { baseColor: [0.05, 0.16, 0.2, 1], opacity: 0.3 },
  BoardRivet: { baseColor: [1, 0.82, 0.34, 1], emissive: [0.3, 0.22, 0.05, 1] },
  // Selection lift shadow (compressed + darker) and the empty-chest dust puff.
  ShadowSoft: { baseColor: [0.1, 0.14, 0.18, 1], opacity: 0.26 },
  ShadowLift: { baseColor: [0.06, 0.09, 0.12, 1], opacity: 0.44 },
  DustPuff: { baseColor: [0.66, 0.6, 0.52, 1], opacity: 0.55 },
};

export const CHEST_RESOURCES: GameResources = {
  materials: MATERIALS,
  meshes: { box: { kind: "box" }, cylinder: { kind: "cylinder" }, sphere: { kind: "sphere" } },
};

// ── chest proportions ───────────────────────────────────────────────────────────

const BODY = v3(1.3, 0.6, 0.92);
const LID = v3(1.34, 0.26, 0.96);
const LATCH = v3(0.2, 0.24, 0.05);
const BODY_TOP = 0.62;

// ── small builders ──────────────────────────────────────────────────────────────

/** A flat disc (thin cylinder) — pools, glows, platform layers. */
const disc = (key: string, material: string, at: EngineVec3, radius: number, height = 0.02): SceneInstance => ({
  key,
  material,
  mesh: "cylinder",
  transform: { position: at, rotation: QUAT_IDENTITY, scale: v3(radius * 2, height, radius * 2) },
});

interface ChestPose {
  readonly origin: EngineVec3;
  readonly yaw: number;
  readonly pitch: number;
  readonly squash: number;
  readonly lift: number;
  readonly scale: number;
  readonly lidAngle: number;
  readonly latchAngle: number;
  readonly dim: boolean;
  readonly selected: boolean;
  readonly focusRing: boolean;
  readonly hoverRing: boolean;
  readonly gleam: number;
  readonly seam: number;
  readonly glow: number;
}

/** All instances of one posed chest (body, planks, gilding, latch, lid, shadow,
 * selection pool, seam light). Materials are chosen by facing (front/side/top)
 * and by pose state (dim / selected / gleaming) rather than by texture. */
const chestInstances = (key: string, pose: ChestPose): readonly SceneInstance[] => {
  // Tilt toward the camera (a small back-pitch) when chosen; yaw carries idle sway.
  const q = quatMul(quatYaw(pose.yaw), quatPitch(-pose.pitch));
  const squashY = 1 - pose.squash;
  const squashXZ = 1 + pose.squash * 0.55;
  const grow = pose.scale;
  const origin = v3(pose.origin.x, pose.origin.y + pose.lift, pose.origin.z);

  const wood = pose.dim ? "WoodDim" : "WoodBrown";
  const woodSide = pose.dim ? "WoodDimSide" : "WoodSide";
  const woodLid = pose.dim ? "WoodDim" : "WoodLid";
  // Front trim brightens on hover/selection; the sweep gleam overrides the top trim.
  const trimFront = pose.dim ? "GildDim" : pose.selected || pose.hoverRing ? "GildBright" : "GildFront";
  const trimSide = pose.dim ? "GildDim" : "GildSide";
  const trimTop = pose.gleam > 0.35 ? "GildGleam" : pose.dim ? "GildDim" : "GildTop";

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
    pose.glow > 0 || pose.selected
      ? [
          disc(`${key}:pool2`, "PoolOuter", v3(pose.origin.x, 0.02, pose.origin.z), BODY.x * (1.05 + pose.glow * 0.3), 0.012),
          disc(`${key}:pool1`, "PoolMid", v3(pose.origin.x, 0.028, pose.origin.z), BODY.x * (0.78 + pose.glow * 0.22), 0.012),
          disc(`${key}:pool0`, "PoolCore", v3(pose.origin.x, 0.036, pose.origin.z), BODY.x * (0.5 + pose.glow * 0.18), 0.012),
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

  // Shadow: compressed + darker when lifted, softer when grounded.
  const lifted = pose.lift > 0.001;
  const shadowRadius = BODY.x * 0.72 * grow * (lifted ? 0.82 : 1);
  const shadow: SceneInstance = disc(`${key}:shadow`, lifted ? "ShadowLift" : "ShadowSoft", v3(pose.origin.x, 0.014, pose.origin.z), shadowRadius, 0.01);

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
    shadow,
    part("body", v3(0, BODY.y / 2, 0), BODY, wood),
    // Board gap lines (darkest) read as separate planks without a texture.
    part("gap1", v3(0, BODY.y * 0.36, 0), v3(BODY.x + 0.014, 0.03, BODY.z + 0.014), "WoodGap"),
    part("gap2", v3(0, BODY.y * 0.68, 0), v3(BODY.x + 0.014, 0.03, BODY.z + 0.014), "WoodGap"),
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
    lidRim,
    latch,
    ...rings,
  ];
};

// ── the light burst (bounded: soft glow + a few rays + a few motes) ─────────────

const lightBurst = (at: EngineVec3, tick: number, t: number): readonly SceneInstance[] => {
  // t is burst progress 0→1 over the burst window; intensity peaks early, fades.
  const strength = pulse(t);
  if (strength <= 0.001) {
    return [];
  }
  const rise = 0.2 + t * 0.9;
  const glow: SceneInstance = disc("burst:glow", "BurstGlow", v3(at.x, at.y + 0.05, at.z), 0.35 + strength * 0.9, 0.02);
  const rays = [0, 1, 2, 3, 4].map((i) => {
    const a = (i / 5) * Math.PI * 2 + tick * 0.02;
    const spread = 0.28 * strength;
    return {
      key: `burst:ray${i}`,
      material: "BurstRay",
      mesh: "box",
      transform: {
        position: v3(at.x + Math.cos(a) * spread, at.y + 0.3 + rise * 0.5, at.z + Math.sin(a) * spread),
        rotation: quatYaw(a),
        scale: v3(0.05 + strength * 0.05, 0.5 + strength * 1.3, 0.05 + strength * 0.05),
      },
    } satisfies SceneInstance;
  });
  const motes = Array.from({ length: CHEST_TIMING.burstParticles }, (_, i) => {
    const a = (i / CHEST_TIMING.burstParticles) * Math.PI * 2 + i * 1.3;
    const r = (0.15 + (i % 3) * 0.12) * (0.4 + t);
    const climb = rise * (0.6 + (i % 4) * 0.18);
    const size = (0.05 + (i % 2) * 0.02) * strength;
    return {
      key: `burst:mote${i}`,
      material: "Mote",
      mesh: "sphere",
      transform: { position: v3(at.x + Math.cos(a) * r, at.y + 0.25 + climb, at.z + Math.sin(a) * r), rotation: QUAT_IDENTITY, scale: v3(size, size, size) },
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
const heroPrize = (rarity: Parameters<typeof rewardMaterialOf>[0], at: EngineVec3, riseT: number, tick: number, settle: number): readonly SceneInstance[] => {
  const material = rewardMaterialOf(rarity);
  const climb = CHEST_TIMING.riseHeight * easeOutBack(riseT);
  const bob = Math.sin(tick * 0.12) * 0.035 * settle;
  const center = v3(at.x, at.y + climb + bob, at.z);
  const rarityBonus = rarity === "jackpot" ? 0.18 : rarity === "rare" ? 0.1 : 0;
  const size = (0.54 + rarityBonus) * (0.5 + 0.5 * riseT) * (1 + Math.sin(tick * 0.16) * 0.04 * settle);
  const halo = 0.82 * (0.5 + 0.5 * riseT) * (0.9 + Math.sin(tick * 0.14) * 0.12 * settle);
  const spin = quatYaw(tick * 0.04);
  return [
    disc("reward:halo", "BurstGlow", v3(center.x, center.y, center.z + 0.001), halo, 0.02),
    { key: "reward:core", material, mesh: "sphere", transform: { position: center, rotation: spin, scale: v3(size, size, size) } },
    { key: "reward:facet", material, mesh: "box", transform: { position: center, rotation: quatYaw(tick * 0.04 + 0.7), scale: v3(size * 0.72, size * 0.72, size * 0.72) } },
  ];
};

// ── the arcade platform (rim, central glow, edge falloff, corner rivets) ────────

const platform = (): readonly SceneInstance[] => [
  disc("plat:vignette", "EdgeVignette", v3(0, -0.048, 0), 9, 0.006),
  disc("plat:side", "PlatformSide", v3(0, -0.062, 0), 8.4, 0.06),
  disc("plat:glow", "CenterGlow", v3(0, -0.03, 0), 4.4, 0.006),
  ...[
    [-1, -1],
    [1, -1],
    [-1, 1],
    [1, 1],
  ].map(([sx, sz], i) => ({
    key: `plat:rivet${i}`,
    material: "BoardRivet",
    mesh: "cylinder" as const,
    transform: { position: v3((sx ?? 0) * 6.7, -0.02, (sz ?? 0) * 6.7), rotation: QUAT_IDENTITY, scale: v3(0.34, 0.05, 0.34) },
  })),
];

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

    return chestInstances(`chest${index}`, {
      dim: dimmed,
      focusRing: session.phase === "ready" && choice.focused === index && choice.hovered !== index && choice.armed !== index,
      glow: isSelected ? easeOutCubic(lidT) * (winReveal ? 1 : 0.32) : 0,
      gleam: idleActive && !dimmed ? goldGleam(index, tick) : 0,
      hoverRing: session.phase === "ready" && (choice.hovered === index || choice.armed === index),
      latchAngle: easeOutCubic(latchT) * CHEST_TIMING.latchDrop + latchRecoil,
      lidAngle: -easeOutBack(lidT) * CHEST_TIMING.lidOpen,
      lift: isSelected ? CHEST_TIMING.lift * selectEase : 0,
      origin,
      pitch: isSelected ? CHEST_TIMING.tilt * selectEase : 0,
      scale: isSelected ? lerp(1, CHEST_TIMING.selectScale, selectEase) : 1,
      seam,
      selected: isSelected,
      squash: dance.squash + (bracing ? pulse(braceT) * 0.05 : 0),
      yaw: dance.twist + idleTwist + shiver,
    });
  }).flat();

  // Reward / empty reveal rising fully clear of the selected, open chest.
  const rewardInstances: SceneInstance[] = [];
  const burst: SceneInstance[] = [];
  if (selected !== null && plan !== null && revealAge >= timeline.lidEnd) {
    const lift = CHEST_TIMING.lift * selectEase;
    const chestTop = addV3(chestPosition(selected, count), v3(0, BODY_TOP + lift, 0));
    const riseT = clamp01((revealAge - timeline.lidEnd) / Math.max(1, timeline.riseEnd - timeline.lidEnd));
    const settle = clamp01((revealAge - timeline.riseEnd) / 20);
    const rarity = outcomeRarity(session);

    if (rarity !== "loss") {
      // A win: the warm light burst fires and the prize climbs fully clear of
      // the chest to hover as the frame's focal point.
      burst.push(...lightBurst(chestTop, tick, burstT));
      rewardInstances.push(...heroPrize(rarity, chestTop, riseT, tick, settle));
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
        const spread = 0.16 + pt * 0.5;
        const size = (0.16 + pt * 0.3) * (1 - pt * 0.35);
        return {
          key: `dust:${i}`,
          material: "DustPuff",
          mesh: "sphere",
          transform: {
            position: v3(chestTop.x + Math.cos(a) * spread, chestTop.y + 0.05 + pt * 0.75, chestTop.z + Math.sin(a) * spread),
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
    const at = addV3(chestPosition(selected, count), v3(0, BODY_TOP + 0.4, 0));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Camera: table framing that pushes in toward the selected chest through the
  // whole selection→reveal, eased and restrained.
  const base = chestCamera(count);
  // Raise the focus above the chest so the framing centers between the open
  // chest and the prize hovering over it — the pair own the frame during reveal.
  const camera =
    selected !== null
      ? revealFocusCamera(base, addV3(chestPosition(selected, count), v3(0, 1.05, 0)), smoothstep(selectT), runtime.settings.reducedMotion ? 0.3 : CHEST_TIMING.pushIn)
      : base;

  // Lights: standard rig, a warm escape light once the lid parts, and a brief
  // burst light that flashes the stage and chest faces at the pop.
  const focus = selected !== null ? chestPosition(selected, count) : v3(0, 0, 0);
  const lights: SceneLight[] = [...stageLights(focus, 0.5 + 0.4 * selectEase)];
  if (selected !== null && revealAge >= timeline.pauseEnd) {
    const warm = clamp01((revealAge - timeline.pauseEnd) / 12);
    lights.push({
      key: "light:chest",
      light: { color: [1, 0.82, 0.45, 1], intensity: 1.3 * warm * (winReveal ? 1 : 0.4), kind: "point", position: addV3(chestPosition(selected, count), v3(0, 1.1, 0.3)) },
    });
  }
  if (winReveal && selected !== null && burstT > 0 && burstT < 1) {
    lights.push({
      key: "light:burst",
      light: { color: [1, 0.9, 0.6, 1], intensity: 1.8 * pulse(burstT), kind: "point", position: addV3(chestPosition(selected, count), v3(0, 1.5, 0.2)) },
    });
  }

  return {
    camera,
    clearColor: SKY_CLEAR,
    instances: [...stageRoom(16), ...platform(), ...chests, ...burst, ...rewardInstances, ...celebration],
    lights,
  };
};
