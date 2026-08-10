/*
 * scene.ts — the ONE gameplay file that touches the engine. It builds every visible
 * thing procedurally (no external assets) via the SDK's 3D scene surface, and each
 * frame moves the dynamic nodes to match the SDK-free `SkyDropSession`: the ball, its
 * ground shadow, the fall trail, the aim reticle, and the chase camera.
 *
 * Everything here exists to answer one question the player asks continuously and the
 * geometry does not answer on its own — WHERE AM I RELATIVE TO THE TARGET? From 240 m
 * up, the ball and the target are a handful of degrees apart and both are small, so
 * the scene leans on flat, top-down-legible cues:
 *   - the target's marker bars radiate outward, readable from directly overhead where
 *     a vertical beacon would foreshorten to a dot;
 *   - a dark shadow disc tracks the ball's horizontal position on the ground;
 *   - a trail of shrinking spheres makes the fall line visible against a flat plane;
 *   - the ground is dressed with a deterministic ring of blocks purely so the eye has
 *     something to measure 240 m against.
 *
 * Mesh conventions: the `box` mesh is a UNIT CUBE (scale = full extents); the
 * `sphere` mesh is UNIT DIAMETER (scale = 2·radius). Flat ground rings have no
 * primitive, so they are generated as real meshes (`meshgen.ts`). A node's material
 * is fixed at spawn, so anything that changes colour is drawn as several nodes with
 * the inactive ones parked far below the world.
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
  setClearColor,
  setNodeTransform,
  spawnRenderable,
} from "@axiom/game";
import { type Quat, type Vec3, IDENTITY_QUAT, add, quatFromEulerXyz, scale as scaleVec, vec3 } from "./vec.ts";
import { annulusY } from "./meshgen.ts";
import { hash01 } from "./conditions.ts";
import { BANDS } from "./target.ts";
import type { SkyDropSession } from "./session.ts";
import {
  BALLS_PER_ROUND,
  BALL_RADIUS,
  CAMERA_FAR,
  CAMERA_FOV_Y,
  CAMERA_NEAR,
  CLOUD_COUNT,
  CLOUD_INNER_RADIUS,
  CLOUD_MAX_ALTITUDE,
  CLOUD_MIN_ALTITUDE,
  CLOUD_OUTER_RADIUS,
  DROP_ALTITUDE,
  GROUND_HALF_EXTENT,
  GROUND_LAYER_BASE,
  GROUND_LAYER_STEP,
  MARKER_BAR_LENGTH,
  MARKER_BAR_WIDTH,
  RING_OUTER,
  SCENERY_COUNT,
  SCENERY_INNER_RADIUS,
  SCENERY_OUTER_RADIUS,
  SEED,
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
const PARKED: Vec3 = vec3(0, -5000, 0);
const parked = (): Transform => xform(PARKED, TINY);

// ── palette ───────────────────────────────────────────────────────────────────

const SKY: Rgba = [0.44, 0.66, 0.88, 1];

const PALETTE = {
  Ball: [1, 0.42, 0.06, 1],
  BandBull: [0.93, 0.16, 0.15, 1],
  BandInner: [0.99, 0.99, 1, 1],
  BandMid: [0.93, 0.16, 0.15, 1],
  BandOuter: [0.99, 0.99, 1, 1],
  CloudWhite: [1, 1, 1, 1],
  /** A dark collar just outside the outer ring, so the target reads against grass. */
  Collar: [0.08, 0.09, 0.07, 1],
  Ground: [0.46, 0.58, 0.36, 1],
  Marker: [1, 0.85, 0.16, 1],
  Shadow: [0.1, 0.14, 0.1, 1],
  Trail: [1, 0.72, 0.32, 1],
} as const;

/** Scenery blocks cycle these so the ground ring reads as fields and rooftops, not noise. */
const SCENERY_COLORS: readonly Rgba[] = [
  [0.38, 0.5, 0.3, 1],
  [0.56, 0.6, 0.34, 1],
  [0.63, 0.55, 0.37, 1],
  [0.44, 0.47, 0.51, 1],
  [0.34, 0.44, 0.35, 1],
];

type MaterialName = keyof typeof PALETTE;

interface Materials {
  readonly base: Map<MaterialName, number>;
  readonly scenery: readonly number[];
  readonly deadCentre: number;
  readonly grabGlow: number;
  readonly landFlash: number;
}

const buildMaterials = (): Materials => {
  const base = new Map<MaterialName, number>();
  for (const name of Object.keys(PALETTE) as MaterialName[]) {
    base.set(name, createMaterial({ baseColor: PALETTE[name] as Rgba }));
  }
  return {
    base,
    deadCentre: createMaterial({ baseColor: [1, 0.85, 0.24, 1], emissive: [0.7, 0.55, 0.1, 1] }),
    grabGlow: createMaterial({ baseColor: [1, 0.78, 0.3, 1], emissive: [0.9, 0.5, 0.12, 1] }),
    landFlash: createMaterial({ baseColor: [1, 0.94, 0.7, 1], emissive: [1, 0.82, 0.35, 1] }),
    scenery: SCENERY_COLORS.map((color) => createMaterial({ baseColor: color })),
  };
};

// ── dynamic handles ───────────────────────────────────────────────────────────

export interface SceneHandles {
  /** One node per ball in the rack — several are in the air at once. */
  readonly balls: readonly Entity[];
  /** A ground shadow per ball: the only reliable read on where a falling ball is. */
  readonly shadows: readonly Entity[];
  /** A halo around the held ball, sized by how fast it is being swung. */
  readonly grabGlow: Entity;
}

// ── static build ──────────────────────────────────────────────────────────────

/** The ground slab, wide enough to fill the view cone from the drop altitude. */
const buildGround = (box: number, mats: Materials): void => {
  spawnRenderable(
    box,
    mats.base.get("Ground")!,
    xform(vec3(0, -0.5, 0), boxScale(vec3(GROUND_HALF_EXTENT * 2, 1, GROUND_HALF_EXTENT * 2))),
  );
};

/**
 * The painted target: one flat band per scoring radius, drawn LARGEST FIRST and lifted
 * a further millimetre each time so tighter bands sit cleanly on top with no z-fight,
 * plus four marker bars radiating past the outer edge.
 *
 * The band radii come straight from `target.ts` — the same list the scorer walks — so
 * a ring the player can see is a ring they can land in, by construction.
 */
const buildTarget = (box: number, mats: Materials): void => {
  const bandMaterial: readonly number[] = [
    mats.deadCentre,
    mats.base.get("BandBull")!,
    mats.base.get("BandInner")!,
    mats.base.get("BandMid")!,
    mats.base.get("BandOuter")!,
  ];

  // Marker bars sit lowest. They overlap the collar's footprint in XZ, so they get
  // their own layer rather than sharing one.
  const marker = mats.base.get("Marker")!;
  const centre = RING_OUTER + MARKER_BAR_LENGTH / 2;
  const along = boxScale(vec3(MARKER_BAR_LENGTH, 0.06, MARKER_BAR_WIDTH));
  const across = boxScale(vec3(MARKER_BAR_WIDTH, 0.06, MARKER_BAR_LENGTH));
  spawnRenderable(box, marker, xform(vec3(centre, 0.04, 0), along));
  spawnRenderable(box, marker, xform(vec3(-centre, 0.04, 0), along));
  spawnRenderable(box, marker, xform(vec3(0, 0.04, centre), across));
  spawnRenderable(box, marker, xform(vec3(0, 0.04, -centre), across));

  // A dark collar under the bands: white-on-grass is low contrast from 240 m up, and
  // the eye finds the target by its OUTLINE long before it can resolve the bands.
  const collarMesh = createMeshData(annulusY(RING_OUTER, RING_OUTER * 1.16, 72));
  spawnRenderable(collarMesh, mats.base.get("Collar")!, xform(vec3(0, GROUND_LAYER_BASE, 0), vec3(1, 1, 1)));

  // Walk widest → tightest so each smaller band is spawned after (and above) its
  // parent. Each gets a full GROUND_LAYER_STEP of clearance — see CAMERA_NEAR for why
  // a hair's-breadth lift is not enough at this viewing distance.
  for (let i = BANDS.length - 1; i >= 0; i -= 1) {
    const outer = BANDS[i]!.radius;
    const inner = i === 0 ? 0 : BANDS[i - 1]!.radius;
    const lift = GROUND_LAYER_BASE + (BANDS.length - i) * GROUND_LAYER_STEP;
    const mesh = createMeshData(annulusY(inner, outer, 72));
    spawnRenderable(mesh, bandMaterial[i]!, xform(vec3(0, lift, 0), vec3(1, 1, 1)));
  }
};

/** The height every band clears — the ball shadow and aim reticle stack above it. */
const TARGET_STACK_TOP = GROUND_LAYER_BASE + (BANDS.length + 1) * GROUND_LAYER_STEP;

/**
 * A deterministic ring of blocks around the target. These carry no gameplay meaning
 * whatsoever — they exist so that 240 m reads as 240 m. A drop onto a bare plane has
 * no sense of scale or speed at all, because nothing passes by on the way down.
 */
const buildScenery = (box: number, mats: Materials): void => {
  for (let i = 0; i < SCENERY_COUNT; i += 1) {
    const bearing = hash01(SEED, i, 11) * Math.PI * 2;
    // Square-rooted radius so blocks spread evenly by AREA rather than bunching inward.
    const radius =
      SCENERY_INNER_RADIUS +
      (SCENERY_OUTER_RADIUS - SCENERY_INNER_RADIUS) * Math.sqrt(hash01(SEED, i, 12));
    const width = 6 + hash01(SEED, i, 13) * 26;
    const depth = 6 + hash01(SEED, i, 14) * 26;
    const height = 1.5 + hash01(SEED, i, 15) * 14;
    const spin = hash01(SEED, i, 16) * Math.PI;
    const material = mats.scenery[i % mats.scenery.length]!;
    spawnRenderable(
      box,
      material,
      xform(
        vec3(Math.cos(bearing) * radius, height / 2, Math.sin(bearing) * radius),
        boxScale(vec3(width, height, depth)),
        quatFromEulerXyz(0, spin, 0),
      ),
    );
  }
};

/**
 * A broken layer of cloud slabs between the drop altitude and the ground. Punching
 * through them is the single clearest signal that the ball has covered real distance —
 * and the annulus keeps them clear of the column directly over the target, so they
 * dress the fall without ever hiding the thing being aimed at.
 */
const buildClouds = (box: number, mats: Materials): void => {
  const cloud = mats.base.get("CloudWhite")!;
  for (let i = 0; i < CLOUD_COUNT; i += 1) {
    const bearing = hash01(SEED, i, 21) * Math.PI * 2;
    const radius = CLOUD_INNER_RADIUS + (CLOUD_OUTER_RADIUS - CLOUD_INNER_RADIUS) * hash01(SEED, i, 22);
    const altitude = CLOUD_MIN_ALTITUDE + (CLOUD_MAX_ALTITUDE - CLOUD_MIN_ALTITUDE) * hash01(SEED, i, 23);
    const width = 22 + hash01(SEED, i, 24) * 34;
    const depth = 18 + hash01(SEED, i, 25) * 30;
    spawnRenderable(
      box,
      cloud,
      xform(
        vec3(Math.cos(bearing) * radius, altitude, Math.sin(bearing) * radius),
        boxScale(vec3(width, 2.6, depth)),
        quatFromEulerXyz(0, hash01(SEED, i, 26) * Math.PI, 0),
      ),
    );
  }
};

/** Build the whole scene, set the lights, and return the dynamic handles. */
export const buildScene = (): SceneHandles => {
  clearScene();
  setClearColor(SKY);

  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const mats = buildMaterials();

  buildGround(box, mats);
  buildTarget(box, mats);
  buildScenery(box, mats);
  buildClouds(box, mats);

  const shadowMesh = createMeshData(annulusY(0, 1, 40));

  // A node per ball, all spawned up front: several are airborne at once, and landed
  // ones stay on the ground for the rest of the round so the grouping is visible.
  const balls = Array.from({ length: BALLS_PER_ROUND }, () =>
    spawnRenderable(sphere, mats.base.get("Ball")!, parked()),
  );
  const shadows = Array.from({ length: BALLS_PER_ROUND }, () =>
    spawnRenderable(shadowMesh, mats.base.get("Shadow")!, parked()),
  );
  const grabGlow = spawnRenderable(sphere, mats.grabGlow, parked());

  // A warm sun, a cool sky fill, and a weak bounce so the undersides of the clouds and
  // the ball never go fully black against the sky.
  addLight({ color: [1, 0.96, 0.88, 1], direction: sdkVec(vec3(-0.38, -0.84, -0.39)), intensity: 2.6, kind: "directional" });
  addLight({ color: [0.66, 0.78, 0.98, 1], direction: sdkVec(vec3(0.5, -0.3, 0.6)), intensity: 1.0, kind: "directional" });
  addLight({ color: [0.6, 0.68, 0.62, 1], direction: sdkVec(vec3(0, 1, 0.2)), intensity: 0.8, kind: "directional" });

  return { balls, grabGlow, shadows };
};

// ── per-frame dynamic update ──────────────────────────────────────────────────

/** The round's fixed camera. Solved once by the session; nothing here moves it. */
const applyCamera = (session: SkyDropSession): void => {
  const view = session.camera;
  setCamera3D({
    far: CAMERA_FAR,
    fovY: CAMERA_FOV_Y,
    near: CAMERA_NEAR,
    position: sdkVec(view.position),
    target: sdkVec(view.target),
  });
};

/**
 * Every ball that is out of the rack, plus a ground shadow under each.
 *
 * The shadows do the heavy lifting. With a camera that never moves, a thrown ball
 * shrinks toward a target 180 m away and quickly becomes a few pixels; its shadow, cast
 * on the ground right next to the rings, is what actually tells you where it is going.
 * The shadow widens with altitude the way a real penumbra does, so it also reads as a
 * crude altimeter — a tight dark disc means that ball is about to land.
 *
 * Balls that have landed keep their nodes on the ground for the rest of the round. That
 * is the only feedback the game gives before the scoreboard, and it is physical rather
 * than numeric: you can see your grouping walking off to one side, and correct.
 */
const applyBalls = (handles: SceneHandles, session: SkyDropSession): void => {
  const views = session.ballViews();
  for (let i = 0; i < handles.balls.length; i += 1) {
    const view = views[i];
    if (view === undefined) {
      setNodeTransform(handles.balls[i]!, parked());
      setNodeTransform(handles.shadows[i]!, parked());
      continue;
    }
    setNodeTransform(handles.balls[i]!, xform(view.pos, sphereScale(BALL_RADIUS)));

    const altitude = Math.max(0, view.pos.y - BALL_RADIUS);
    const spread = BALL_RADIUS * (1.6 + altitude * 0.02);
    setNodeTransform(
      handles.shadows[i]!,
      xform(vec3(view.pos.x, TARGET_STACK_TOP, view.pos.z), vec3(spread, 1, spread)),
    );
  }
};

/**
 * While a ball is in hand: a swelling glow around it that tracks how fast it is
 * actually being swung.
 *
 * This is the ONLY feedback offered during a round, and it is deliberately about the
 * throw rather than the outcome — no landing reticle, no aim line, no verdict, no
 * running score. It tells you how hard you are throwing, which is the thing you
 * control; it says nothing about where the ball will end up, which is the thing you are
 * being asked to judge.
 */
const applyGrabFeedback = (handles: SceneHandles, session: SkyDropSession): void => {
  const ready = session.readyBall();
  if (!session.holding || ready === null) {
    setNodeTransform(handles.grabGlow, parked());
    return;
  }
  // Saturates around a strong throw, so the glow keeps reading as "harder" across the
  // whole range a player actually uses rather than pinning immediately.
  const intensity = Math.min(session.heldSpeed / 30, 1);
  setNodeTransform(handles.grabGlow, xform(ready.pos, sphereScale(BALL_RADIUS * (1.35 + intensity * 1.5))));
};

/** Move every dynamic node to match the session. Called once per rendered frame. */
export const applyFrame = (handles: SceneHandles, session: SkyDropSession): void => {
  applyCamera(session);
  applyBalls(handles, session);
  applyGrabFeedback(handles, session);
};
