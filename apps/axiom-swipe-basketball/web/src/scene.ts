/*
 * scene.ts — the ONE gameplay file that touches the engine. It builds every visible
 * thing procedurally (no external assets) via the SDK's 3D scene surface, sets the
 * fixed front-facing camera + light rig, and each frame moves the dynamic nodes to
 * match the SDK-free `SwipeBasketballSession`: the balls (orange or a distinct
 * GOLDEN variant), the laterally-shifting hoop group, the seven-segment scoreboard
 * (which pulses in the final seconds), a rim-hit / score flash, and a small
 * camera shake on a made basket.
 *
 * Mesh conventions: the `box` mesh is a UNIT CUBE (scale = full extents); the
 * `sphere` mesh is UNIT DIAMETER (scale = 2·radius). The rim + ball seams have no
 * primitive, so they're generated as real meshes (`meshgen.ts`). A node's material
 * is fixed at spawn, so a ball is drawn as TWO nodes (orange + golden) and the
 * inactive one is parked — that's how the golden colour swaps with no re-spawn.
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
import { type Quat, type Vec3, IDENTITY_QUAT, add, quatFromEulerXyz, vec3 } from "./vec.ts";
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
  SCORE_FLASH_TICKS,
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
const parked = (): Transform => xform(PARKED, TINY);

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

interface Materials {
  readonly base: Map<MaterialName, number>;
  readonly golden: number;
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
    golden: createMaterial({ baseColor: [1, 0.82, 0.16, 1], emissive: [0.85, 0.62, 0.08, 1], roughness: 0.3 }),
    scoreOn: createMaterial({ baseColor: [1, 0.4, 0.15, 1], emissive: [1, 0.45, 0.12, 1] }),
  };
};

// ── seven-segment scoreboard geometry ─────────────────────────────────────────

interface Segment {
  readonly pos: Vec3;
  readonly size: Vec3;
}

const SEG_T = 0.028;
const SEG_L = 0.11;
const DIGIT_D = 0.03;

const SEGMENTS: readonly Segment[] = [
  { pos: vec3(0, 0.135, 0), size: vec3(SEG_L, SEG_T, DIGIT_D) },
  { pos: vec3(0.07, 0.068, 0), size: vec3(SEG_T, SEG_L, DIGIT_D) },
  { pos: vec3(0.07, -0.068, 0), size: vec3(SEG_T, SEG_L, DIGIT_D) },
  { pos: vec3(0, -0.135, 0), size: vec3(SEG_L, SEG_T, DIGIT_D) },
  { pos: vec3(-0.07, -0.068, 0), size: vec3(SEG_T, SEG_L, DIGIT_D) },
  { pos: vec3(-0.07, 0.068, 0), size: vec3(SEG_T, SEG_L, DIGIT_D) },
  { pos: vec3(0, 0, 0), size: vec3(SEG_L, SEG_T, DIGIT_D) },
];

const DIGIT_TABLE: readonly (readonly boolean[])[] = [
  [true, true, true, true, true, true, false],
  [false, true, true, false, false, false, false],
  [true, true, false, true, true, false, true],
  [true, true, true, true, false, false, true],
  [false, true, true, false, false, true, true],
  [true, false, true, true, false, true, true],
  [true, false, true, true, true, true, true],
  [true, true, true, false, false, false, false],
  [true, true, true, true, true, true, true],
  [true, true, true, true, false, true, true],
];

const DIGIT_OFFSETS: readonly number[] = [-0.135, 0.135];
const DIGIT_BOARD_Y = BACKBOARD_Y;
const DIGIT_BOARD_Z = BACKBOARD_Z + BACKBOARD_HALF_D + 0.02;

interface SegHandle {
  readonly entity: Entity;
  readonly digit: number;
  readonly seg: number;
  readonly worldPos: Vec3;
  readonly size: Vec3;
}

// ── the dynamic handles the game drives each frame ────────────────────────────

/** A ball drawn as an orange node + a golden node (+ its dark seam), one visible. */
interface BallHandle {
  readonly orange: Entity;
  readonly golden: Entity;
  readonly seam: Entity;
}

/** A hoop-group node with its centred (offset-0) base transform, shifted each frame. */
interface HoopNode {
  readonly entity: Entity;
  readonly basePos: Vec3;
  readonly scale: Vec3;
  readonly rot: Quat;
}

export interface SceneHandles {
  readonly balls: readonly BallHandle[];
  readonly hoopGroup: readonly HoopNode[];
  readonly segments: readonly SegHandle[];
  readonly rimFlash: Entity;
  readonly scoreFlash: Entity;
}

// ── procedural meshes ─────────────────────────────────────────────────────────

const rimMesh = (): Geometry => torusY(RIM_RADIUS, RIM_TUBE, RIM_SEGMENTS * 2, 8);

const seamMesh = (): Geometry => {
  const ring = torusY(0.5, 0.02, 28, 6);
  return mergeGeometry([
    ring,
    rotateGeometry(ring, quatFromEulerXyz(Math.PI / 2, 0, 0)),
    rotateGeometry(ring, quatFromEulerXyz(0, 0, Math.PI / 2)),
  ]);
};

// ── static build (fixed cabinet — never moves) ────────────────────────────────

const buildCabinet = (box: number, mats: Materials): void => {
  const m = mats.base;
  const midZ = (CABINET_NEAR_Z + CABINET_FAR_Z) / 2;
  const depth = CABINET_NEAR_Z - CABINET_FAR_Z;

  spawnRenderable(box, m.get("BackWall")!, xform(vec3(0, 1.6, CABINET_FAR_Z - 0.5), boxScale(vec3(7, 4.4, 0.2))));
  spawnRenderable(box, m.get("CabinetBlue")!, xform(vec3(-2.4, 1.7, CABINET_FAR_Z - 0.45), boxScale(vec3(0.9, 3.4, 0.1))));
  spawnRenderable(box, m.get("CabinetTrim")!, xform(vec3(2.4, 1.7, CABINET_FAR_Z - 0.45), boxScale(vec3(0.9, 3.4, 0.1))));

  const sideX = CABINET_HALF_WIDTH + 0.06;
  for (const sx of [-sideX, sideX]) {
    spawnRenderable(box, m.get("CabinetBlue")!, xform(vec3(sx, 0.9, midZ), boxScale(vec3(0.12, 1.8, depth))));
    spawnRenderable(box, m.get("CabinetTrim")!, xform(vec3(sx, 1.82, midZ), boxScale(vec3(0.16, 0.1, depth))));
  }

  spawnRenderable(box, m.get("CabinetTrim")!, xform(vec3(0, FRONT_LIP_Y / 2, CABINET_NEAR_Z), boxScale(vec3(2 * CABINET_HALF_WIDTH, FRONT_LIP_Y, 0.1))));
  spawnRenderable(box, m.get("RampGray")!, xform(vec3(0, RAMP_NEAR_Y - 0.03, RACK_Z), boxScale(vec3(2 * CABINET_HALF_WIDTH * 0.9, 0.05, 0.34))));
};

const buildRamp = (box: number, mats: Materials): void => {
  const m = mats.base;
  const rise = RAMP_FAR_Y - RAMP_NEAR_Y;
  const run = RAMP_NEAR_Z - RAMP_FAR_Z;
  const rot = quatFromEulerXyz(Math.atan2(rise, run), 0, 0);
  const midY = (RAMP_NEAR_Y + RAMP_FAR_Y) / 2;
  const midZ = (RAMP_NEAR_Z + RAMP_FAR_Z) / 2;
  const rampLen = Math.hypot(run, rise);
  spawnRenderable(box, m.get("RampGray")!, xform(vec3(0, midY, midZ), boxScale(vec3(2 * CABINET_HALF_WIDTH, 0.06, rampLen)), rot));
  for (const lx of [-0.45, 0, 0.45]) {
    spawnRenderable(box, m.get("LaneMark")!, xform(vec3(lx, midY + 0.04, midZ), boxScale(vec3(0.03, 0.02, rampLen * 0.86)), rot));
  }
};

// ── hoop group (moves laterally with the target) ──────────────────────────────

const buildHoopGroup = (box: number, mats: Materials): HoopNode[] => {
  const m = mats.base;
  const group: HoopNode[] = [];
  const add_ = (entity: Entity, basePos: Vec3, scale: Vec3, rot: Quat = IDENTITY_QUAT): void => {
    group.push({ basePos, entity, rot, scale });
  };

  // Backboard + red target trim.
  add_(spawnRenderable(box, m.get("Backboard")!, xform(vec3(0, BACKBOARD_Y, BACKBOARD_Z), boxScale(vec3(BACKBOARD_HALF_W * 2, BACKBOARD_HALF_H * 2, BACKBOARD_HALF_D * 2)))), vec3(0, BACKBOARD_Y, BACKBOARD_Z), boxScale(vec3(BACKBOARD_HALF_W * 2, BACKBOARD_HALF_H * 2, BACKBOARD_HALF_D * 2)));
  const trimZ = BACKBOARD_Z + BACKBOARD_HALF_D + 0.01;
  add_(spawnRenderable(box, m.get("BackboardTrim")!, xform(vec3(0, HOOP_Y + 0.16, trimZ), boxScale(vec3(0.34, 0.24, 0.02)))), vec3(0, HOOP_Y + 0.16, trimZ), boxScale(vec3(0.34, 0.24, 0.02)));

  // Two support posts.
  for (const sx of [-BACKBOARD_HALF_W * 0.7, BACKBOARD_HALF_W * 0.7]) {
    const p = vec3(sx, BACKBOARD_Y - 0.5, BACKBOARD_Z - 0.12);
    add_(spawnRenderable(box, m.get("CabinetBlue")!, xform(p, boxScale(vec3(0.06, 1.6, 0.06)))), p, boxScale(vec3(0.06, 1.6, 0.06)));
  }

  // Rim torus.
  const rim = createMeshData(rimMesh());
  add_(spawnRenderable(rim, m.get("Rim")!, xform(vec3(HOOP_X, HOOP_Y, HOOP_Z), vec3(1, 1, 1))), vec3(HOOP_X, HOOP_Y, HOOP_Z), vec3(1, 1, 1));

  // Net strands + gather ring.
  const strandRadius = RIM_RADIUS;
  for (let i = 0; i < RIM_SEGMENTS; i += 1) {
    const a = (2 * Math.PI * i) / RIM_SEGMENTS;
    const p = vec3(HOOP_X + Math.cos(a) * strandRadius, HOOP_Y - 0.13, HOOP_Z + Math.sin(a) * strandRadius);
    add_(spawnRenderable(box, m.get("Net")!, xform(p, boxScale(vec3(0.012, 0.26, 0.012)))), p, boxScale(vec3(0.012, 0.26, 0.012)));
  }
  const netRing = createMeshData(torusY(RIM_RADIUS * 0.7, 0.01, RIM_SEGMENTS, 5));
  add_(spawnRenderable(netRing, m.get("Net")!, xform(vec3(HOOP_X, HOOP_Y - 0.24, HOOP_Z), vec3(1, 1, 1))), vec3(HOOP_X, HOOP_Y - 0.24, HOOP_Z), vec3(1, 1, 1));

  return group;
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

const buildBalls = (sphere: number, seam: number, mats: Materials): BallHandle[] => {
  const balls: BallHandle[] = [];
  for (const home of rackPositions()) {
    const orange = spawnRenderable(sphere, mats.base.get("BallOrange")!, xform(home, sphereScale(BALL_RADIUS)));
    const golden = spawnRenderable(sphere, mats.golden, parked());
    const seamNode = spawnRenderable(seam, mats.base.get("BallSeam")!, xform(home, sphereScale(BALL_RADIUS * 1.02)));
    balls.push({ golden, orange, seam: seamNode });
  }
  return balls;
};

/** Build the whole scene, set the lights, and return the dynamic handles. */
export const buildScene = (): SceneHandles => {
  clearScene();
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const seam = createMeshData(seamMesh());
  const mats = buildMaterials();

  buildCabinet(box, mats);
  buildRamp(box, mats);
  const hoopGroup = buildHoopGroup(box, mats);
  const segments = buildScoreboard(box, mats);
  const balls = buildBalls(sphere, seam, mats);
  const rimFlash = spawnRenderable(sphere, mats.flash, parked());
  const scoreFlash = spawnRenderable(sphere, mats.flash, parked());

  addLight({ color: [1, 0.96, 0.88, 1], direction: sdkVec(vec3(-0.35, -0.62, -0.7)), intensity: 1.75, kind: "directional" });
  addLight({ color: [0.7, 0.78, 0.95, 1], direction: sdkVec(vec3(0.55, -0.35, 0.5)), intensity: 0.75, kind: "directional" });
  addLight({ color: [0.68, 0.72, 0.8, 1], direction: sdkVec(vec3(0, 1, 0.15)), intensity: 0.7, kind: "directional" });

  return { balls, hoopGroup, rimFlash, scoreFlash, segments };
};

// ── per-frame dynamic update ──────────────────────────────────────────────────

/** The fixed camera plus a small additive shake offset on a made basket. */
const applyCamera = (shake: Vec3): void => {
  setCamera3D({
    far: CAMERA_FAR,
    fovY: CAMERA_FOV_Y,
    near: CAMERA_NEAR,
    position: sdkVec(add(vec3(CAMERA_POS.x, CAMERA_POS.y, CAMERA_POS.z), shake)),
    target: sdkVec(add(vec3(CAMERA_TARGET.x, CAMERA_TARGET.y, CAMERA_TARGET.z), shake)),
  });
};

const applyBalls = (handles: SceneHandles, balls: readonly BallView[]): void => {
  for (let i = 0; i < balls.length; i += 1) {
    const ball = balls[i]!;
    const h = handles.balls[i]!;
    const t = xform(ball.pos, sphereScale(BALL_RADIUS));
    // Show the colour matching the ball's golden flag; park the other.
    setNodeTransform(h.orange, ball.golden ? parked() : t);
    setNodeTransform(h.golden, ball.golden ? t : parked());
    setNodeTransform(h.seam, xform(ball.pos, sphereScale(BALL_RADIUS * 1.02)));
  }
};

const applyHoopGroup = (handles: SceneHandles, offsetX: number): void => {
  const shift = vec3(offsetX, 0, 0);
  for (const node of handles.hoopGroup) {
    setNodeTransform(node.entity, xform(add(node.basePos, shift), node.scale, node.rot));
  }
};

const applyScoreboard = (handles: SceneHandles, score: number, offsetX: number, pulse: number): void => {
  const shown = Math.min(Math.max(score, 0), 99);
  const digits = [Math.floor(shown / 10), shown % 10];
  const shift = vec3(offsetX, 0, 0);
  for (const seg of handles.segments) {
    const lit = DIGIT_TABLE[digits[seg.digit]!]![seg.seg]!;
    const pos = add(seg.worldPos, shift);
    setNodeTransform(seg.entity, lit ? xform(pos, boxScale(vec3(seg.size.x * pulse, seg.size.y * pulse, seg.size.z))) : xform(pos, TINY));
  }
};

const applyFlashes = (handles: SceneHandles, session: SwipeBasketballSession): void => {
  // Rim/backboard contact flash at the impact point.
  const contact = session.lastContact;
  const showContact = contact !== null && contact.impactSpeed > 1.2 && (contact.material === "rim" || contact.material === "backboard");
  setNodeTransform(
    handles.rimFlash,
    showContact ? xform(contact.point, sphereScale(0.05 + Math.min(contact.impactSpeed, 6) * 0.012)) : parked(),
  );

  // Made-basket flash near the (shifted) hoop — bigger + longer for a big score.
  const since = session.ticksSinceScore;
  const showScore = since >= 0 && since < SCORE_FLASH_TICKS;
  const fade = showScore ? 1 - since / SCORE_FLASH_TICKS : 0;
  const bigMul = session.lastScoreBig ? 2.2 : 1;
  const at = vec3(HOOP_X + session.hoopOffsetX, HOOP_Y - 0.05, HOOP_Z);
  setNodeTransform(
    handles.scoreFlash,
    showScore ? xform(at, sphereScale((0.12 + 0.18 * fade) * bigMul)) : parked(),
  );
};

/** Move every dynamic node to match the session. Called once per rendered frame. */
export const applyFrame = (handles: SceneHandles, session: SwipeBasketballSession): void => {
  applyCamera(session.cameraShakeOffset());
  applyBalls(handles, session.ballViews());
  applyHoopGroup(handles, session.hoopOffsetX);
  // In the final seconds the scoreboard pulses; otherwise steady.
  const pulse = session.finalWindow ? 1 + 0.35 * (0.5 + 0.5 * Math.sin(session.tick * 0.5)) : 1;
  applyScoreboard(handles, session.score, session.hoopOffsetX, pulse);
  applyFlashes(handles, session);
};
