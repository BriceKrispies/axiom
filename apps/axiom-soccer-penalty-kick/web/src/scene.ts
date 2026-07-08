/*
 * The 3D scene — a faithful port of the soccer diorama (`penalty_scene.rs` +
 * `penalty_render_meshed.rs` + the live `web.rs` shell). It builds the ~628 static
 * renderables once (pitch, markings, blob shadows, net, stadium wall, crowd, ad
 * boards), spawns the dynamic actors (ball + shadow, 16-part goalie, 13-box
 * kicker, the movable goal frame, a save flash), and drives the fixed broadcast
 * camera + two-light rig. Each frame the game hands `applyFrame` a snapshot and
 * the ~37 dynamic nodes are moved with `setNodeTransform`.
 *
 * Catalog conventions (matching the SDK / retro-fps precedent): the `box` mesh is
 * a unit cube — scale = FULL extents. The `sphere` mesh is unit-diameter — scale =
 * 2·radius. Quads/lines are drawn as thin boxes (near-zero extents clamped).
 */

import {
  type Entity,
  type Rgba,
  addLight,
  clearScene,
  createMaterial,
  createMesh,
  setCamera3D,
  setNodeTransform,
  spawnRenderable,
} from "@axiom/game";
import { type MaterialName, PALETTE } from "./palette.ts";
import { type Transform, type Vec3, DEG_TO_RAD, add, boxScale, sdkVec, sphereScale, vec3, xform } from "./engine.ts";
import type { PenaltyBallPose } from "./ball.ts";
import { GOALIE_Z, GROUND_Y } from "./scene-constants.ts";
import { GOALIE_HAIR, PART_COUNT, PART_MATERIAL, PART_SIZE } from "./goalie.ts";
import { type KickerBox, kickerBoxesAt, kickerHairAt } from "./kicker.ts";

// ── material registry ────────────────────────────────────────────────────────

type MaterialHandles = Map<MaterialName, number>;

const registerMaterials = (): MaterialHandles => {
  const handles: MaterialHandles = new Map();
  for (const name of Object.keys(PALETTE) as MaterialName[]) {
    handles.set(name, createMaterial({ baseColor: PALETTE[name] as Rgba }));
  }
  return handles;
};

// ── the scene handles the game drives each frame ─────────────────────────────

export interface SceneHandles {
  readonly box: number;
  readonly sphere: number;
  readonly materials: MaterialHandles;
  readonly ball: Entity;
  readonly ballShadow: Entity;
  readonly goalieParts: readonly Entity[];
  readonly goalieHair: Entity;
  readonly kickerBoxes: readonly Entity[];
  readonly kickerHair: Entity;
  readonly goalFrame: readonly [Entity, Entity, Entity]; // left post, right post, crossbar
  readonly saveFlash: Entity;
}

// ── static build ─────────────────────────────────────────────────────────────

const buildField = (box: number, mats: MaterialHandles): void => {
  spawnRenderable(box, mats.get("DarkerGrassBand")!, xform(vec3(0, 0.0, 7), boxScale(vec3(68, 0, 26))));
  for (let i = 0; i < 8; i += 1) {
    const z = -6 + 3.25 * (i + 0.5);
    const mat = i % 2 === 0 ? "FieldGrass" : "DarkerGrassBand";
    spawnRenderable(box, mats.get(mat)!, xform(vec3(0, 0.005, z), boxScale(vec3(68, 0, 3.055))));
  }
};

const buildMarkings = (box: number, mats: MaterialHandles): void => {
  const line = mats.get("WhiteFieldLines")!;
  const lines: [Vec3, Vec3][] = [
    [vec3(0, 0.02, 0), vec3(40.3, 0, 0.17)],
    [vec3(0, 0.02, 16.5), vec3(40.3, 0, 0.17)],
    [vec3(-20.15, 0.02, 8.25), vec3(0.17, 0, 16.5)],
    [vec3(20.15, 0.02, 8.25), vec3(0.17, 0, 16.5)],
    [vec3(0, 0.02, 5.5), vec3(18.32, 0, 0.17)],
    [vec3(-9.16, 0.02, 2.75), vec3(0.17, 0, 5.5)],
    [vec3(9.16, 0.02, 2.75), vec3(0.17, 0, 5.5)],
  ];
  for (const [pos, size] of lines) spawnRenderable(box, line, xform(pos, boxScale(size)));
  spawnRenderable(box, line, xform(vec3(0, 0.021, 11), boxScale(vec3(0.34, 0, 0.34))));
};

const buildStaticShadows = (box: number, mats: MaterialHandles): void => {
  const blob = mats.get("BlobShadow")!;
  spawnRenderable(box, blob, xform(vec3(-0.7, 0.03, 12.6), boxScale(vec3(0.84, 0, 1.72))));
  spawnRenderable(box, blob, xform(vec3(0, 0.03, GOALIE_Z), boxScale(vec3(1.24, 0, 0.88))));
};

const buildNet = (box: number, mats: MaterialHandles): void => {
  const net = mats.get("NetOffWhite")!;
  const t = 0.038;
  for (let i = 0; i < 30; i += 1) spawnRenderable(box, net, xform(vec3(-3.66 + (7.32 * i) / 29, 1.22, -0.95), boxScale(vec3(t, 2.44, t))));
  for (let j = 0; j < 15; j += 1) spawnRenderable(box, net, xform(vec3(0, (2.44 * j) / 14, -0.95), boxScale(vec3(7.32, t, t))));
  for (let i = 0; i < 12; i += 1) spawnRenderable(box, net, xform(vec3(-3.66 + (7.32 * i) / 11, 2.44, -0.475), boxScale(vec3(t, t, 0.95))));
  for (let k = 0; k < 7; k += 1) spawnRenderable(box, net, xform(vec3(-3.66, (2.44 * k) / 6, -0.475), boxScale(vec3(t, t, 0.95))));
  for (let k = 0; k < 7; k += 1) spawnRenderable(box, net, xform(vec3(3.66, (2.44 * k) / 6, -0.475), boxScale(vec3(t, t, 0.95))));
};

const buildBackdrop = (box: number, mats: MaterialHandles): void => {
  spawnRenderable(box, mats.get("StadiumWallDarkGray")!, xform(vec3(0, 0.6, -4.6), boxScale(vec3(68, 1.2, 0.4))));
  // Crowd: 4 rows × 44 columns × 3 vertical cells = 528 cards.
  const crowdMats: MaterialName[] = ["CrowdMutedColors", "CrowdMutedColorsAltA", "CrowdMutedColorsAltB"];
  const rows: [number, number][] = [
    [2.0, 0.0],
    [3.7, 0.5],
    [5.4, 0.0],
    [7.1, 0.5],
  ];
  const cardW = 1.4682;
  rows.forEach(([y, phase], row) => {
    for (let i = 0; i < 44; i += 1) {
      const x = -32.3 + cardW * (i + 0.5 + phase);
      for (let s = 0; s < 3; s += 1) {
        const cy = y - 1.2 + 0.8 * (s + 0.5);
        const mat = crowdMats[(i + row * 2 + s) % 3]!;
        spawnRenderable(box, mats.get(mat)!, xform(vec3(x, cy, -4.9), boxScale(vec3(1.3214, 0.688, 0.2))));
      }
    }
  });
  // Ad boards: 9 red panels (AXIOM / SPORTS text is baked in the Rust texture; flat red here).
  const ad = mats.get("AdBoardRed")!;
  for (let i = 0; i < 9; i += 1) {
    const x = -6.222 + 12.444 * (i / 8);
    spawnRenderable(box, ad, xform(vec3(x, 0.62, -2.6), boxScale(vec3(1.2996, 1.25, 0.12))));
  }
};

/** Build the whole scene, set the camera + lights, and return the dynamic-node handles. */
export const buildScene = (): SceneHandles => {
  clearScene();
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const materials = registerMaterials();

  buildBackdrop(box, materials);
  buildField(box, materials);
  buildMarkings(box, materials);
  buildStaticShadows(box, materials);
  buildNet(box, materials);

  // Movable goal frame.
  const goalFrame: [Entity, Entity, Entity] = [
    spawnRenderable(box, materials.get("GoalFrameWhite")!, xform(vec3(-3.66, 1.22, 0), boxScale(vec3(0.12, 2.44, 0.12)))),
    spawnRenderable(box, materials.get("GoalFrameWhite")!, xform(vec3(3.66, 1.22, 0), boxScale(vec3(0.12, 2.44, 0.12)))),
    spawnRenderable(box, materials.get("GoalFrameWhite")!, xform(vec3(0, 2.44, 0), boxScale(vec3(7.44, 0.12, 0.12)))),
  ];

  // Dynamic ball + its shadow.
  const ballShadow = spawnRenderable(box, materials.get("BlobShadow")!, xform(vec3(0, 0.03, 11), boxScale(vec3(0.704, 0, 0.64))));
  const ball = spawnRenderable(sphere, materials.get("BallWhite")!, xform(vec3(0, 0.32, 11), sphereScale(0.32)));

  // Dynamic goalie (16 parts, root invisible) + hair.
  const goalieParts: Entity[] = [];
  for (let i = 0; i < PART_COUNT; i += 1) {
    goalieParts.push(spawnRenderable(box, materials.get(PART_MATERIAL[i]!)!, xform(vec3(0, -100, 0), boxScale(PART_SIZE[i]!))));
  }
  const goalieHair = spawnRenderable(box, materials.get(GOALIE_HAIR.material)!, xform(GOALIE_HAIR.center, boxScale(GOALIE_HAIR.size)));

  // Dynamic kicker (13 boxes + hair), spawned at the display frame.
  const initial = kickerBoxesAt(0);
  const kickerBoxes = initial.map((b) => spawnRenderable(box, materials.get(b.material)!, xform(b.position, boxScale(b.scale), b.rotation)));
  const hair0 = kickerHairAt(0);
  const kickerHair = spawnRenderable(box, materials.get(hair0.material)!, xform(hair0.position, boxScale(hair0.scale), hair0.rotation));

  // Save impact flash (hidden until a save), an emissive sphere.
  const saveFlash = spawnRenderable(sphere, materials.get("ImpactFlash")!, xform(vec3(0, -100, 0), sphereScale(0.001)));

  // The two-light rig + a hemisphere-ambient approximation — added ONCE (the
  // per-frame camera juice goes through `setCamera`, which never re-adds lights).
  addLight({ kind: "directional", direction: sdkVec(vec3(-0.5, -0.66, -0.56)), color: [1.0, 0.95, 0.83, 1], intensity: 1.25 });
  addLight({ kind: "directional", direction: sdkVec(vec3(0.46, -0.52, 0.42)), color: [0.6, 0.7, 0.86, 1], intensity: 0.55 });
  addLight({ kind: "directional", direction: sdkVec(vec3(0.0, 1.0, 0.0)), color: [0.55, 0.6, 0.68, 1], intensity: 0.35 });

  setCamera(vec3(0, 0, 0));
  return { box, sphere, materials, ball, ballShadow, goalieParts, goalieHair, kickerBoxes, kickerHair, goalFrame, saveFlash };
};

/** Set the fixed broadcast camera, plus additive juice `offset` (called every frame). */
export const setCamera = (offset: Vec3): void => {
  setCamera3D({
    position: sdkVec(add(vec3(1.1, 2.1, 27.6), offset)),
    target: sdkVec(add(vec3(0.1, 0.75, 4.5), offset)),
    fovY: 12.5 * DEG_TO_RAD,
    near: 0.1,
    far: 120,
  });
};

// ── per-frame dynamic update ─────────────────────────────────────────────────

/** Everything the scene needs to render one frame. */
export interface FrameSnapshot {
  readonly ball: PenaltyBallPose;
  readonly goalieWorld: readonly Transform[];
  readonly kickerTick: number;
  readonly frameShake: { readonly target: "LeftPost" | "RightPost" | "Crossbar"; readonly offset: Vec3 } | null;
  readonly saveFlash: { readonly position: Vec3; readonly size: number; readonly alpha: number } | null;
}

const GOAL_FRAME_BASE: readonly Vec3[] = [vec3(-3.66, 1.22, 0), vec3(3.66, 1.22, 0), vec3(0, 2.44, 0)];
const GOAL_FRAME_SIZE: readonly Vec3[] = [vec3(0.12, 2.44, 0.12), vec3(0.12, 2.44, 0.12), vec3(7.44, 0.12, 0.12)];

const applyKicker = (handles: SceneHandles, tick: number): void => {
  const boxes: KickerBox[] = kickerBoxesAt(tick);
  boxes.forEach((b, i) => setNodeTransform(handles.kickerBoxes[i]!, xform(b.position, boxScale(b.scale), b.rotation)));
  const hair = kickerHairAt(tick);
  setNodeTransform(handles.kickerHair, xform(hair.position, boxScale(hair.scale), hair.rotation));
};

const applyGoalie = (handles: SceneHandles, world: readonly Transform[]): void => {
  for (let i = 0; i < PART_COUNT; i += 1) {
    // Root (i=0) is invisible; park it far below.
    const size = i === 0 ? vec3(0.001, 0.001, 0.001) : PART_SIZE[i]!;
    setNodeTransform(handles.goalieParts[i]!, xform(world[i]!.translation, boxScale(size), world[i]!.rotation));
  }
  const head = world[3]!.translation;
  setNodeTransform(handles.goalieHair, xform(vec3(head.x, head.y + 0.2, head.z), boxScale(GOALIE_HAIR.size)));
};

const applyGoalFrame = (handles: SceneHandles, shake: FrameSnapshot["frameShake"]): void => {
  const targets = ["LeftPost", "RightPost", "Crossbar"] as const;
  for (let i = 0; i < 3; i += 1) {
    const offset = shake && shake.target === targets[i] ? shake.offset : vec3(0, 0, 0);
    setNodeTransform(handles.goalFrame[i]!, xform(add(GOAL_FRAME_BASE[i]!, offset), boxScale(GOAL_FRAME_SIZE[i]!)));
  }
};

/** Move all dynamic nodes to `snapshot`. Called once per rendered frame. */
export const applyFrame = (handles: SceneHandles, snapshot: FrameSnapshot): void => {
  const ball = snapshot.ball;
  setNodeTransform(handles.ball, xform(ball.position, sphereScale(ball.radius)));
  setNodeTransform(
    handles.ballShadow,
    xform(ball.shadowCenter, boxScale(vec3(ball.shadowRadiusX * 2, 0, ball.shadowRadiusZ * 2))),
  );
  applyGoalie(handles, snapshot.goalieWorld);
  applyKicker(handles, snapshot.kickerTick);
  applyGoalFrame(handles, snapshot.frameShake);
  const flash = snapshot.saveFlash;
  setNodeTransform(
    handles.saveFlash,
    flash && flash.alpha > 0 ? xform(flash.position, sphereScale(flash.size)) : xform(vec3(0, -100, 0), sphereScale(0.001)),
  );
};

export { GROUND_Y };
