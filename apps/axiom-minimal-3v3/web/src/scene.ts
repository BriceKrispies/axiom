/*
 * scene.ts — the ONE gameplay file that touches the engine. It builds every visible
 * thing procedurally (no external assets) through the SDK's 3D scene surface — the
 * half court with painted markings, the backboard + torus rim + net, six box-and-
 * sphere humanoids, the ball, and the controlled-player marker — and each frame
 * moves the dynamic nodes to match the SDK-free `SceneView` the session hands it.
 *
 * Mesh conventions (as in the sibling heat-check / swipe-basketball demos): the
 * `box` mesh is a UNIT CUBE (scale = full extents); the `sphere` mesh is UNIT
 * DIAMETER (scale = 2·radius). The rim + net ring have no primitive, so they're
 * generated as real meshes (`meshgen.ts`). A node's material is fixed at spawn, so
 * every material is created here in `buildScene` on the first fixed tick (before
 * the 3D surface binds — see `frameLocked` in harness.ts).
 *
 * The third-person follow camera also lives here: it is presentation smoothing
 * (lerped position/target behind the controlled player, aimed toward the hoop),
 * not game state, so the deterministic session never sees it.
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
import { type Quat, type Vec3, IDENTITY_QUAT, add, distXZ, lerp, mix, quatFromEulerXyz, rotY, scale, vec3 } from "./vec.ts";
import { torusY } from "./meshgen.ts";
import type { FigureView, SceneView } from "./types.ts";
import * as C from "./constants.ts";

// ── SDK transform adapters ────────────────────────────────────────────────────

const MIN_EXTENT = 0.02;
const sdk = (v: Vec3): { x: number; y: number; z: number } => ({ x: v.x, y: v.y, z: v.z });
const boxScale = (s: Vec3): Vec3 =>
  vec3(Math.max(s.x, MIN_EXTENT), Math.max(s.y, MIN_EXTENT), Math.max(s.z, MIN_EXTENT));
const sphereScale = (r: number): Vec3 => vec3(r * 2, r * 2, r * 2);
const xform = (position: Vec3, scale_: Vec3, rotation: Quat = IDENTITY_QUAT): Transform => ({
  position: sdk(position),
  rotation,
  scale: sdk(scale_),
});

// ── palette ───────────────────────────────────────────────────────────────────

const PALETTE = {
  Backboard: [0.94, 0.95, 1, 1],
  BallOrange: [0.98, 0.5, 0.16, 1],
  BlueAccent: [0.86, 0.92, 0.98, 1],
  BlueMain: [0.2, 0.5, 0.88, 1],
  Court: [0.72, 0.55, 0.38, 1],
  Dark: [0.08, 0.09, 0.13, 1],
  Lane: [0.62, 0.44, 0.3, 1],
  Line: [0.94, 0.95, 0.98, 1],
  Net: [0.9, 0.92, 0.96, 1],
  RedAccent: [0.95, 0.6, 0.55, 1],
  RedMain: [0.78, 0.2, 0.2, 1],
  Rim: [1, 0.44, 0.14, 1],
} as const;

type MaterialName = keyof typeof PALETTE;

interface Materials {
  readonly base: Map<MaterialName, number>;
  readonly marker: number;
}

const buildMaterials = (): Materials => {
  const base = new Map<MaterialName, number>();
  for (const name of Object.keys(PALETTE) as MaterialName[]) {
    base.set(name, createMaterial({ baseColor: PALETTE[name] as Rgba }));
  }
  return {
    base,
    marker: createMaterial({ baseColor: [1, 0.95, 0.4, 1], emissive: [0.9, 0.8, 0.25, 1], opacity: 0.8 }),
  };
};

// ── the symbolic figure (blue player / red defender) ──────────────────────────

interface Figure {
  readonly head: Entity;
  readonly torso: Entity;
  readonly hips: Entity;
  readonly armL: Entity;
  readonly armR: Entity;
  readonly legL: Entity;
  readonly legR: Entity;
}

const spawnAtOrigin = (mesh: number, material: number): Entity =>
  spawnRenderable(mesh, material, xform(vec3(0, -100, 0), vec3(1e-4, 1e-4, 1e-4)));

const makeFigure = (box: number, sphere: number, main: number, accent: number): Figure => ({
  armL: spawnAtOrigin(box, accent),
  armR: spawnAtOrigin(box, accent),
  head: spawnAtOrigin(sphere, accent),
  hips: spawnAtOrigin(box, main),
  legL: spawnAtOrigin(box, main),
  legR: spawnAtOrigin(box, main),
  torso: spawnAtOrigin(box, main),
});

/**
 * Pose a figure from its `FigureView`: yawed to its facing, leaning with movement,
 * crouching in the gather, rising on a jump, shooting arm raising toward release.
 * Local offsets are yaw-rotated around the figure origin (feet center).
 */
const poseFigure = (fig: Figure, v: FigureView): void => {
  const rot = quatFromEulerXyz(v.leanF * C.LEAN_MAX, v.yaw, -v.leanS * C.LEAN_MAX);
  const bodyY = -0.2 * v.crouch + v.jumpY + v.bobY;
  const at = (local: Vec3): Vec3 => add(add(v.pos, rotY(local, v.yaw)), vec3(0, bodyY, 0));

  setNodeTransform(fig.head, xform(at(vec3(0, 1.62, 0)), sphereScale(0.16), rot));
  setNodeTransform(fig.torso, xform(at(vec3(0, 1.15, 0)), boxScale(vec3(0.42, 0.6, 0.26)), rot));
  setNodeTransform(fig.hips, xform(at(vec3(0, 0.78, 0)), boxScale(vec3(0.4, 0.3, 0.26)), rot));

  const legH = 0.74 - 0.12 * v.crouch;
  const legY = 0.4 - 0.12 * v.crouch;
  setNodeTransform(fig.legL, xform(at(vec3(-0.12, legY, -0.02)), boxScale(vec3(0.16, legH, 0.18)), rot));
  setNodeTransform(fig.legR, xform(at(vec3(0.12, legY, 0.02)), boxScale(vec3(0.16, legH, 0.18)), rot));

  // Shooting arm (right) rises overhead toward release; the guide arm follows less.
  const ayR = mix(1.15, 1.74, v.armRaise);
  setNodeTransform(
    fig.armR,
    xform(at(vec3(mix(0.34, 0.12, v.armRaise), ayR, mix(0, 0.2, v.armRaise))), boxScale(vec3(0.14, mix(0.55, 0.5, v.armRaise), 0.14)), rot),
  );
  const ayL = mix(1.15, 1.52, v.armRaise * 0.6);
  setNodeTransform(
    fig.armL,
    xform(at(vec3(-mix(0.34, 0.18, v.armRaise * 0.6), ayL, mix(0, 0.1, v.armRaise))), boxScale(vec3(0.14, 0.5, 0.14)), rot),
  );
};

// ── static build ──────────────────────────────────────────────────────────────

const buildCourt = (box: number, mats: Materials): void => {
  const m = mats.base;
  // Floor + painted key.
  spawnRenderable(box, m.get("Court")!, xform(vec3(0, -0.05, 6.5), boxScale(vec3(16, 0.1, 24))));
  spawnRenderable(box, m.get("Lane")!, xform(vec3(0, 0.005, C.HOOP_Z - C.KEY_LENGTH / 2), boxScale(vec3(C.KEY_HALF_W * 2, 0.02, C.KEY_LENGTH))));
  // Baseline (behind the hoop), half-court line, sidelines.
  spawnRenderable(box, m.get("Line")!, xform(vec3(0, 0.012, C.HOOP_Z + 0.9), boxScale(vec3(14, 0.02, 0.12))));
  spawnRenderable(box, m.get("Line")!, xform(vec3(0, 0.012, C.BOUND_Z_MIN - 0.6), boxScale(vec3(14, 0.02, 0.12))));
  for (const sx of [-7, 7]) {
    spawnRenderable(box, m.get("Line")!, xform(vec3(sx, 0.012, 6.5), boxScale(vec3(0.12, 0.02, 12.6))));
  }
  // Key sides + free-throw line.
  for (const sx of [-C.KEY_HALF_W, C.KEY_HALF_W]) {
    spawnRenderable(box, m.get("Line")!, xform(vec3(sx, 0.012, C.HOOP_Z - C.KEY_LENGTH / 2), boxScale(vec3(0.1, 0.02, C.KEY_LENGTH))));
  }
  spawnRenderable(box, m.get("Line")!, xform(vec3(0, 0.012, C.HOOP_Z - C.KEY_LENGTH), boxScale(vec3(C.KEY_HALF_W * 2, 0.02, 0.12))));
  // Simplified 3-point arc: small boxes on a semicircle centered under the hoop.
  const segs = 20;
  for (let k = 0; k <= segs; k += 1) {
    const a = mix(-1.25, 1.25, k / segs);
    spawnRenderable(
      box,
      m.get("Line")!,
      xform(vec3(Math.sin(a) * C.THREE_PT_RADIUS, 0.012, C.HOOP_Z - Math.cos(a) * C.THREE_PT_RADIUS), boxScale(vec3(0.18, 0.02, 0.18))),
    );
  }
};

const buildHoop = (box: number, mats: Materials): void => {
  const m = mats.base;
  spawnRenderable(
    box,
    m.get("Backboard")!,
    xform(vec3(0, C.BACKBOARD_Y, C.HOOP_Z + 0.45), boxScale(vec3(C.BACKBOARD_HALF_W * 2, C.BACKBOARD_HALF_H * 2, C.BACKBOARD_HALF_D * 2))),
  );
  // Backboard inner square.
  spawnRenderable(box, m.get("Dark")!, xform(vec3(0, C.HOOP_Y + 0.22, C.HOOP_Z + 0.4), boxScale(vec3(0.5, 0.35, 0.02))));
  // Post.
  spawnRenderable(box, m.get("Dark")!, xform(vec3(0, 1.9, C.HOOP_Z + 0.9), boxScale(vec3(0.14, 3.8, 0.14))));
  // Rim torus.
  const rim = createMeshData(torusY(C.RIM_RADIUS, C.RIM_TUBE, C.RIM_SEGMENTS * 2, 8));
  spawnRenderable(rim, m.get("Rim")!, xform(vec3(0, C.HOOP_Y, C.HOOP_Z), vec3(1, 1, 1)));
  // Net strands + gather ring.
  for (let i = 0; i < C.RIM_SEGMENTS; i += 1) {
    const a = (2 * Math.PI * i) / C.RIM_SEGMENTS;
    const p = vec3(Math.cos(a) * C.RIM_RADIUS, C.HOOP_Y - 0.16, C.HOOP_Z + Math.sin(a) * C.RIM_RADIUS);
    spawnRenderable(box, m.get("Net")!, xform(p, boxScale(vec3(0.012, 0.3, 0.012))));
  }
  const ring = createMeshData(torusY(C.RIM_RADIUS * 0.68, 0.012, C.RIM_SEGMENTS, 5));
  spawnRenderable(ring, m.get("Net")!, xform(vec3(0, C.HOOP_Y - 0.3, C.HOOP_Z), vec3(1, 1, 1)));
};

// ── handles + camera state ────────────────────────────────────────────────────

export interface SceneHandles {
  readonly blues: readonly [Figure, Figure, Figure];
  readonly defenders: readonly [Figure, Figure, Figure];
  readonly ball: Entity;
  readonly marker: Entity;
  /** Mutable follow-camera smoothing state (presentation only, not game state). */
  cam: { pos: Vec3; target: Vec3; initialized: boolean };
}

/** Build the whole scene, set the lights, and return the dynamic handles. */
export const buildScene = (): SceneHandles => {
  clearScene();
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const mats = buildMaterials();

  buildCourt(box, mats);
  buildHoop(box, mats);

  const blueMain = mats.base.get("BlueMain")!;
  const blueAccent = mats.base.get("BlueAccent")!;
  const redMain = mats.base.get("RedMain")!;
  const redAccent = mats.base.get("RedAccent")!;
  const blues: [Figure, Figure, Figure] = [
    makeFigure(box, sphere, blueMain, blueAccent),
    makeFigure(box, sphere, blueMain, blueAccent),
    makeFigure(box, sphere, blueMain, blueAccent),
  ];
  const defenders: [Figure, Figure, Figure] = [
    makeFigure(box, sphere, redMain, redAccent),
    makeFigure(box, sphere, redMain, redAccent),
    makeFigure(box, sphere, redMain, redAccent),
  ];

  const ball = spawnAtOrigin(sphere, mats.base.get("BallOrange")!);
  const marker = spawnAtOrigin(box, mats.marker);

  addLight({ color: [1, 0.96, 0.9, 1], direction: sdk(vec3(-0.3, -0.72, 0.6)), intensity: 2.1, kind: "directional" });
  addLight({ color: [0.6, 0.72, 0.95, 1], direction: sdk(vec3(0.5, -0.4, -0.5)), intensity: 0.9, kind: "directional" });
  addLight({ color: [0.7, 0.72, 0.85, 1], direction: sdk(vec3(0, 1, -0.2)), intensity: 0.75, kind: "directional" });

  return {
    ball,
    blues,
    cam: { initialized: false, pos: vec3(0, C.CAM_HEIGHT, -2), target: C.HOOP_POS },
    defenders,
    marker,
  };
};

// ── per-frame dynamic update ──────────────────────────────────────────────────

/**
 * The third-person follow camera: behind the controlled player along the
 * player→hoop line, above, aimed ahead toward the hoop. Lerped for smoothness;
 * snapped on the first frame, on a possession reset, and on any jump larger than
 * `CAM_SNAP_DIST` so play never becomes confusing.
 */
const applyCamera = (h: SceneHandles, view: SceneView): void => {
  const anchor = view.cameraAnchor;
  const dirToHoop =
    distXZ(anchor, C.HOOP_POS) < 1e-6
      ? vec3(0, 0, 1)
      : scale(vec3(C.HOOP_POS.x - anchor.x, 0, C.HOOP_POS.z - anchor.z), 1 / distXZ(anchor, C.HOOP_POS));
  const desiredPos = add(add(anchor, scale(dirToHoop, -C.CAM_BACK)), vec3(0, C.CAM_HEIGHT, 0));
  const desiredTarget = add(add(anchor, scale(dirToHoop, C.CAM_AIM_AHEAD)), vec3(0, C.CAM_AIM_Y, 0));
  const snap =
    !h.cam.initialized || view.justReset || distXZ(h.cam.pos, desiredPos) > C.CAM_SNAP_DIST;
  h.cam.pos = snap ? desiredPos : lerp(h.cam.pos, desiredPos, C.CAM_LERP);
  h.cam.target = snap ? desiredTarget : lerp(h.cam.target, desiredTarget, C.CAM_LERP);
  h.cam.initialized = true;
  setCamera3D({
    far: C.CAM_FAR,
    fovY: C.CAM_FOV_Y,
    near: C.CAM_NEAR,
    position: sdk(h.cam.pos),
    target: sdk(h.cam.target),
  });
};

/** Move every dynamic node to match the session view. Called once per fixed tick. */
export const applyFrame = (h: SceneHandles, view: SceneView): void => {
  applyCamera(h, view);
  for (let i = 0; i < 3; i += 1) {
    poseFigure(h.blues[i]!, view.blues[i]!);
    poseFigure(h.defenders[i]!, view.defenders[i]!);
  }
  setNodeTransform(h.ball, xform(view.ball, sphereScale(C.BALL_RADIUS)));
  // The controlled-player marker: a thin pulsing disc at the handler's feet.
  const cp = view.blues[view.controlledIndex]!.pos;
  const pulse = 1 + 0.08 * Math.sin(view.tick * 0.12);
  setNodeTransform(h.marker, xform(vec3(cp.x, 0.03, cp.z), boxScale(vec3(0.9 * pulse, 0.02, 0.9 * pulse))));
};
