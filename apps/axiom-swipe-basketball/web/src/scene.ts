/*
 * scene.ts — the ONE gameplay file that touches the engine. It builds every visible
 * thing procedurally (no external assets) via the SDK's 3D scene surface, sets the
 * fixed front-facing camera + light rig once, and each frame moves the dynamic
 * nodes (balls, their seam rings, the rim-hit flash, the seven-segment scoreboard)
 * to match the SDK-free `SwipeBasketballSession`.
 *
 * Mesh conventions (SDK / soccer precedent): the `box` mesh is a UNIT CUBE, so its
 * scale is the full extents (thin panels clamp their near-zero dimension); the
 * `sphere` mesh is UNIT DIAMETER, so its scale is 2·radius. The hoop rim and the
 * ball seams have no primitive, so they're generated as real meshes (`meshgen.ts`).
 */

import {
  type Entity,
  type Rgba,
  type Transform,
  addLight,
  clearScene,
  createMaterial,
  createMesh,
  createMeshData,
  setCamera3D,
  setNodeTransform,
  spawnRenderable,
} from "@axiom/game";
import { type Quat, type Vec3, IDENTITY_QUAT, quatFromEulerXyz, vec3 } from "./vec.ts";
import { type Geometry, mergeGeometry, rotateGeometry, torusY } from "./meshgen.ts";
import { type BallView, type SwipeBasketballSession, rackPositions } from "./session.ts";
import {
  BACKBOARD_HALF_D,
  BACKBOARD_HALF_H,
  BACKBOARD_HALF_W,
  BACKBOARD_Y,
  BACKBOARD_Z,
  BALL_RADIUS,
  CABINET_FAR_Z,
  CABINET_HALF_WIDTH,
  CABINET_NEAR_Z,
  CAMERA_FAR,
  CAMERA_FOV_Y,
  CAMERA_NEAR,
  CAMERA_POS,
  CAMERA_TARGET,
  FRONT_LIP_Y,
  HOOP_X,
  HOOP_Y,
  HOOP_Z,
  RACK_Z,
  RAMP_FAR_Y,
  RAMP_FAR_Z,
  RAMP_NEAR_Y,
  RAMP_NEAR_Z,
  RIM_RADIUS,
  RIM_SEGMENTS,
  RIM_TUBE,
} from "./constants.ts";

// ── SDK transform adapters ────────────────────────────────────────────────────

const MIN_EXTENT = 0.012;
const sdkVec = (v: Vec3): { x: number; y: number; z: number } => ({ x: v.x, y: v.y, z: v.z });
const boxScale = (size: Vec3): Vec3 =>
  vec3(Math.max(size.x, MIN_EXTENT), Math.max(size.y, MIN_EXTENT), Math.max(size.z, MIN_EXTENT));
const sphereScale = (radius: number): Vec3 => vec3(radius * 2, radius * 2, radius * 2);
const xform = (position: Vec3, scale: Vec3, rotation: Quat = IDENTITY_QUAT): Transform => ({
  position: sdkVec(position),
  rotation,
  scale: sdkVec(scale),
});
const TINY: Vec3 = vec3(0.0001, 0.0001, 0.0001);
const PARKED: Vec3 = vec3(0, -100, 0);

// ── palette ───────────────────────────────────────────────────────────────────

const PALETTE = {
  BackWall: [0.13, 0.15, 0.22, 1],
  Backboard: [0.95, 0.96, 1, 1],
  BackboardTrim: [0.88, 0.22, 0.24, 1],
  BallOrange: [0.98, 0.5, 0.16, 1],
  BallSeam: [0.1, 0.06, 0.04, 1],
  CabinetBlue: [0.22, 0.3, 0.58, 1],
  CabinetTrim: [0.98, 0.58, 0.18, 1],
  LaneMark: [0.98, 0.86, 0.3, 1],
  Net: [0.9, 0.92, 0.95, 1],
  RampGray: [0.4, 0.44, 0.52, 1],
  Rim: [1, 0.4, 0.13, 1],
  ScoreOff: [0.18, 0.06, 0.06, 1],
} as const;

type MaterialName = keyof typeof PALETTE;

// Emissive materials (built separately so they glow).
interface Materials {
  readonly base: Map<MaterialName, number>;
  readonly scoreOn: number;
  readonly flash: number;
}

const buildMaterials = (): Materials => {
  const base = new Map<MaterialName, number>();
  for (const name of Object.keys(PALETTE) as MaterialName[]) {
    base.set(name, createMaterial({ baseColor: PALETTE[name] as Rgba }));
  }
  return {
    base,
    flash: createMaterial({ baseColor: [1, 0.85, 0.4, 1], emissive: [1, 0.7, 0.25, 1] }),
    scoreOn: createMaterial({ baseColor: [1, 0.4, 0.15, 1], emissive: [1, 0.45, 0.12, 1] }),
  };
};

// ── seven-segment scoreboard geometry ─────────────────────────────────────────

/** A single segment's local placement within a digit cell (centred at origin). */
interface Segment {
  readonly pos: Vec3;
  readonly size: Vec3;
}

const SEG_T = 0.028; // segment thickness
const SEG_L = 0.11; // segment length
const DIGIT_D = 0.03; // depth (toward the viewer)

// Segments a,b,c,d,e,f,g in the classic seven-segment layout.
const SEGMENTS: readonly Segment[] = [
  { pos: vec3(0, 0.135, 0), size: vec3(SEG_L, SEG_T, DIGIT_D) }, // a  top
  { pos: vec3(0.07, 0.068, 0), size: vec3(SEG_T, SEG_L, DIGIT_D) }, // b  top-right
  { pos: vec3(0.07, -0.068, 0), size: vec3(SEG_T, SEG_L, DIGIT_D) }, // c  bottom-right
  { pos: vec3(0, -0.135, 0), size: vec3(SEG_L, SEG_T, DIGIT_D) }, // d  bottom
  { pos: vec3(-0.07, -0.068, 0), size: vec3(SEG_T, SEG_L, DIGIT_D) }, // e  bottom-left
  { pos: vec3(-0.07, 0.068, 0), size: vec3(SEG_T, SEG_L, DIGIT_D) }, // f  top-left
  { pos: vec3(0, 0, 0), size: vec3(SEG_L, SEG_T, DIGIT_D) }, // g  middle
];

/** Which segments (a…g) are lit for each digit 0–9. */
const DIGIT_TABLE: readonly (readonly boolean[])[] = [
  [true, true, true, true, true, true, false], // 0
  [false, true, true, false, false, false, false], // 1
  [true, true, false, true, true, false, true], // 2
  [true, true, true, true, false, false, true], // 3
  [false, true, true, false, false, true, true], // 4
  [true, false, true, true, false, true, true], // 5
  [true, false, true, true, true, true, true], // 6
  [true, true, true, false, false, false, false], // 7
  [true, true, true, true, true, true, true], // 8
  [true, true, true, true, false, true, true], // 9
];

const DIGIT_OFFSETS: readonly number[] = [-0.135, 0.135]; // tens, ones (x within the board)
const DIGIT_BOARD_Y = BACKBOARD_Y;
const DIGIT_BOARD_Z = BACKBOARD_Z + BACKBOARD_HALF_D + 0.02;

interface SegHandle {
  readonly entity: Entity;
  readonly digit: number; // 0 = tens, 1 = ones
  readonly seg: number; // 0…6
  readonly worldPos: Vec3;
  readonly size: Vec3;
}

// ── the dynamic handles the game drives each frame ────────────────────────────

export interface SceneHandles {
  readonly ballNodes: readonly Entity[];
  readonly seamNodes: readonly Entity[];
  readonly rimFlash: Entity;
  readonly segments: readonly SegHandle[];
}

// ── procedural meshes ─────────────────────────────────────────────────────────

/** The hoop rim as a real torus lying in the XZ plane, centred at the origin. */
const rimMesh = (): Geometry => torusY(RIM_RADIUS, RIM_TUBE, RIM_SEGMENTS * 2, 8);

/** The basketball's three orthogonal dark seam rings on a unit-diameter sphere. */
const seamMesh = (): Geometry => {
  const ring = torusY(0.5, 0.02, 28, 6);
  return mergeGeometry([
    ring,
    rotateGeometry(ring, quatFromEulerXyz(Math.PI / 2, 0, 0)),
    rotateGeometry(ring, quatFromEulerXyz(0, 0, Math.PI / 2)),
  ]);
};

// ── static build ──────────────────────────────────────────────────────────────

const buildCabinet = (box: number, mats: Materials): void => {
  const m = mats.base;
  const midZ = (CABINET_NEAR_Z + CABINET_FAR_Z) / 2;
  const depth = CABINET_NEAR_Z - CABINET_FAR_Z;

  // Arcade back wall (behind the machine).
  spawnRenderable(box, m.get("BackWall")!, xform(vec3(0, 1.6, CABINET_FAR_Z - 0.5), boxScale(vec3(7, 4.4, 0.2))));
  // Two dim background accent panels for a little arcade colour.
  spawnRenderable(box, m.get("CabinetBlue")!, xform(vec3(-2.4, 1.7, CABINET_FAR_Z - 0.45), boxScale(vec3(0.9, 3.4, 0.1))));
  spawnRenderable(box, m.get("CabinetTrim")!, xform(vec3(2.4, 1.7, CABINET_FAR_Z - 0.45), boxScale(vec3(0.9, 3.4, 0.1))));

  // Side panels (the cabinet walls) + their orange trim caps.
  const sideX = CABINET_HALF_WIDTH + 0.06;
  for (const sx of [-sideX, sideX]) {
    spawnRenderable(box, m.get("CabinetBlue")!, xform(vec3(sx, 0.9, midZ), boxScale(vec3(0.12, 1.8, depth))));
    spawnRenderable(box, m.get("CabinetTrim")!, xform(vec3(sx, 1.82, midZ), boxScale(vec3(0.16, 0.1, depth))));
  }

  // Front lip / return tray shelf.
  spawnRenderable(box, m.get("CabinetTrim")!, xform(vec3(0, FRONT_LIP_Y / 2, CABINET_NEAR_Z), boxScale(vec3(2 * CABINET_HALF_WIDTH, FRONT_LIP_Y, 0.1))));
  spawnRenderable(box, m.get("RampGray")!, xform(vec3(0, RAMP_NEAR_Y - 0.03, RACK_Z), boxScale(vec3(2 * CABINET_HALF_WIDTH * 0.9, 0.05, 0.34))));
};

const buildRamp = (box: number, mats: Materials): void => {
  const m = mats.base;
  const rise = RAMP_FAR_Y - RAMP_NEAR_Y;
  const run = RAMP_NEAR_Z - RAMP_FAR_Z;
  const angle = Math.atan2(rise, run); // +rx lifts the far (−Z) end
  const rot = quatFromEulerXyz(angle, 0, 0);
  const midY = (RAMP_NEAR_Y + RAMP_FAR_Y) / 2;
  const midZ = (RAMP_NEAR_Z + RAMP_FAR_Z) / 2;
  const rampLen = Math.hypot(run, rise);
  spawnRenderable(box, m.get("RampGray")!, xform(vec3(0, midY, midZ), boxScale(vec3(2 * CABINET_HALF_WIDTH, 0.06, rampLen)), rot));
  // Three yellow lane markings running up the ramp.
  for (const lx of [-0.45, 0, 0.45]) {
    spawnRenderable(box, m.get("LaneMark")!, xform(vec3(lx, midY + 0.04, midZ), boxScale(vec3(0.03, 0.02, rampLen * 0.86)), rot));
  }
};

const buildBackboardAndHoop = (box: number, sphere: number, mats: Materials): SceneHandles["rimFlash"] => {
  const m = mats.base;
  // Backboard panel + red target trim.
  spawnRenderable(box, m.get("Backboard")!, xform(vec3(0, BACKBOARD_Y, BACKBOARD_Z), boxScale(vec3(BACKBOARD_HALF_W * 2, BACKBOARD_HALF_H * 2, BACKBOARD_HALF_D * 2))));
  const trimZ = BACKBOARD_Z + BACKBOARD_HALF_D + 0.01;
  spawnRenderable(box, m.get("BackboardTrim")!, xform(vec3(0, HOOP_Y + 0.16, trimZ), boxScale(vec3(0.34, 0.24, 0.02))));
  // Two support posts behind the backboard.
  for (const sx of [-BACKBOARD_HALF_W * 0.7, BACKBOARD_HALF_W * 0.7]) {
    spawnRenderable(box, m.get("CabinetBlue")!, xform(vec3(sx, BACKBOARD_Y - 0.5, BACKBOARD_Z - 0.12), boxScale(vec3(0.06, 1.6, 0.06))));
  }

  // Hoop rim — a real torus.
  const rim = createMeshData(rimMesh());
  spawnRenderable(rim, m.get("Rim")!, xform(vec3(HOOP_X, HOOP_Y, HOOP_Z), vec3(1, 1, 1)));

  // Net: straight strands hanging from the rim + a lower gather ring.
  const strandRadius = RIM_RADIUS;
  for (let i = 0; i < RIM_SEGMENTS; i += 1) {
    const a = (2 * Math.PI * i) / RIM_SEGMENTS;
    spawnRenderable(box, m.get("Net")!, xform(vec3(HOOP_X + Math.cos(a) * strandRadius, HOOP_Y - 0.13, HOOP_Z + Math.sin(a) * strandRadius), boxScale(vec3(0.012, 0.26, 0.012))));
  }
  const netRing = createMeshData(torusY(RIM_RADIUS * 0.7, 0.01, RIM_SEGMENTS, 5));
  spawnRenderable(netRing, m.get("Net")!, xform(vec3(HOOP_X, HOOP_Y - 0.24, HOOP_Z), vec3(1, 1, 1)));

  // The rim-hit flash (hidden until a contact), an emissive sphere.
  return spawnRenderable(sphere, mats.flash, xform(PARKED, sphereScale(0.001)));
};

const buildScoreboard = (box: number, mats: Materials): SegHandle[] => {
  const segments: SegHandle[] = [];
  for (let d = 0; d < DIGIT_OFFSETS.length; d += 1) {
    const dx = DIGIT_OFFSETS[d]!;
    for (let s = 0; s < SEGMENTS.length; s += 1) {
      const seg = SEGMENTS[s]!;
      const worldPos = vec3(dx + seg.pos.x, DIGIT_BOARD_Y + seg.pos.y, DIGIT_BOARD_Z);
      const entity = spawnRenderable(box, mats.scoreOn, xform(worldPos, TINY));
      segments.push({ digit: d, entity, seg: s, size: seg.size, worldPos });
    }
  }
  return segments;
};

const buildRack = (sphere: number, seam: number, mats: Materials): { ballNodes: Entity[]; seamNodes: Entity[] } => {
  const ballNodes: Entity[] = [];
  const seamNodes: Entity[] = [];
  for (const home of rackPositions()) {
    ballNodes.push(spawnRenderable(sphere, mats.base.get("BallOrange")!, xform(home, sphereScale(BALL_RADIUS))));
    seamNodes.push(spawnRenderable(seam, mats.base.get("BallSeam")!, xform(home, sphereScale(BALL_RADIUS * 1.02))));
  }
  return { ballNodes, seamNodes };
};

/** Build the whole scene, set the camera + lights, and return the dynamic handles. */
export const buildScene = (): SceneHandles => {
  clearScene();
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const seam = createMeshData(seamMesh());
  const mats = buildMaterials();

  buildCabinet(box, mats);
  buildRamp(box, mats);
  const rimFlash = buildBackboardAndHoop(box, sphere, mats);
  const segments = buildScoreboard(box, mats);
  const rack = buildRack(sphere, seam, mats);

  // Warm key light from the front-right, cool fill, soft top ambient — added once.
  addLight({ color: [1, 0.96, 0.88, 1], direction: sdkVec(vec3(-0.35, -0.62, -0.7)), intensity: 1.75, kind: "directional" });
  addLight({ color: [0.7, 0.78, 0.95, 1], direction: sdkVec(vec3(0.55, -0.35, 0.5)), intensity: 0.75, kind: "directional" });
  addLight({ color: [0.68, 0.72, 0.8, 1], direction: sdkVec(vec3(0, 1, 0.15)), intensity: 0.7, kind: "directional" });

  setCamera3D({
    far: CAMERA_FAR,
    fovY: CAMERA_FOV_Y,
    near: CAMERA_NEAR,
    position: sdkVec(vec3(CAMERA_POS.x, CAMERA_POS.y, CAMERA_POS.z)),
    target: sdkVec(vec3(CAMERA_TARGET.x, CAMERA_TARGET.y, CAMERA_TARGET.z)),
  });

  return { ballNodes: rack.ballNodes, rimFlash, seamNodes: rack.seamNodes, segments };
};

// ── per-frame dynamic update ──────────────────────────────────────────────────

const applyBalls = (handles: SceneHandles, balls: readonly BallView[]): void => {
  for (let i = 0; i < balls.length; i += 1) {
    const pos = balls[i]!.pos;
    setNodeTransform(handles.ballNodes[i]!, xform(pos, sphereScale(BALL_RADIUS)));
    setNodeTransform(handles.seamNodes[i]!, xform(pos, sphereScale(BALL_RADIUS * 1.02)));
  }
};

const applyScoreboard = (handles: SceneHandles, score: number): void => {
  const shown = Math.min(Math.max(score, 0), 99);
  const digits = [Math.floor(shown / 10), shown % 10];
  for (const seg of handles.segments) {
    const lit = DIGIT_TABLE[digits[seg.digit]!]![seg.seg]!;
    setNodeTransform(seg.entity, lit ? xform(seg.worldPos, boxScale(seg.size)) : xform(seg.worldPos, TINY));
  }
};

const applyFlash = (handles: SceneHandles, session: SwipeBasketballSession): void => {
  const contact = session.lastContact;
  const show = contact !== null && contact.impactSpeed > 1.2 && (contact.material === "rim" || contact.material === "backboard");
  setNodeTransform(
    handles.rimFlash,
    show ? xform(contact.point, sphereScale(0.05 + Math.min(contact.impactSpeed, 6) * 0.012)) : xform(PARKED, sphereScale(0.001)),
  );
};

/** Move every dynamic node to match the session. Called once per rendered frame. */
export const applyFrame = (handles: SceneHandles, session: SwipeBasketballSession): void => {
  applyBalls(handles, session.ballViews());
  applyScoreboard(handles, session.score);
  applyFlash(handles, session);
};
