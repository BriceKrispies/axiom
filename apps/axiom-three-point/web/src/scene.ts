/*
 * scene.ts — the ONE gameplay file that touches the renderer. It builds the
 * whole arena procedurally (no external assets) through the app's OWN pure-TS
 * engine (`engine/renderer.ts`) — hardwood court with painted key / free-throw
 * line / three-point arc / subtle scuffs and plank variation, glass backboard
 * with a shooter's square, the real torus rim (the SAME RIM_RADIUS/RIM_TUBE
 * constants the physics collider ring uses, so the rim you see is the rim you
 * hit), a procedural net, the pole, three ball racks, crowd silhouettes,
 * hanging banners, a 7-segment scoreboard, and the arena shell — then
 * `applyFrame` re-poses every dynamic node each frame from the SDK-free
 * `SceneView` the session hands it.
 *
 * ALL reaction values (rim vibration, board shake, net pose, ball squash,
 * trails, rack settle, crowd pulse) arrive in the view — this file never infers
 * game outcomes; it only draws. Every temporary visual lives in a fixed pool
 * spawned once at build time and parked when idle: nothing is allocated
 * per-frame, and no effect can accumulate.
 *
 * Mesh conventions: `box` is a UNIT CUBE (scale = full extents); `sphere` is
 * UNIT DIAMETER (scale = 2·radius); `cylinder` is UNIT DIAMETER × UNIT HEIGHT.
 * A node's material is fixed at spawn, so every glow/flash/highlight is a
 * dedicated pre-spawned node that gets scaled or parked, never re-colored.
 */

import type { Entity, Rgba, Transform } from "@axiom/web-engine";
import {
  addLight,
  clearScene,
  createMaterial,
  createMesh,
  createMeshData,
  rendererNodeCount,
  setCamera3D,
  setNodeTransform,
  spawnRenderable,
} from "@axiom/web-engine";
import { type Quat, type Vec3, IDENTITY_QUAT, quatFromEulerXyz, quatRotate, vec3 } from "./vec.ts";
import { torusY } from "./meshgen.ts";
import { RIM_COLLIDER_CENTERS } from "./physics.ts";
import type { BallView, SceneView } from "./types.ts";
import {
  BACKBOARD_CENTER,
  BACKBOARD_HALF,
  BALL_RADIUS,
  BALLS_PER_RACK,
  CAMERA_FAR,
  CAMERA_FOV_Y,
  CAMERA_NEAR,
  GOLDEN_BALL_INDEX,
  POLE_CENTER,
  POLE_HALF,
  POLISH_TUNING,
  PREVIEW_POINTS,
  RACK_BALL_Y,
  RACK_SLOT_SPACING,
  RIM_RADIUS,
  RIM_TUBE,
  RIM_X,
  RIM_Y,
  RIM_Z,
  STATIONS,
  rackCenter,
  rackSlotPosition,
  yawForward,
  yawRight,
} from "./constants.ts";

// ── transform adapters ────────────────────────────────────────────────────────

const MIN_EXTENT = 0.01;
const sdk = (v: Vec3): { x: number; y: number; z: number } => ({ x: v.x, y: v.y, z: v.z });
const boxScale = (s: Vec3): Vec3 => vec3(Math.max(s.x, MIN_EXTENT), Math.max(s.y, MIN_EXTENT), Math.max(s.z, MIN_EXTENT));
const sphereScale = (r: number): Vec3 => vec3(r * 2, r * 2, r * 2);
const xform = (position: Vec3, scale: Vec3, rotation: Quat = IDENTITY_QUAT): Transform => ({
  position: sdk(position),
  rotation,
  scale: sdk(scale),
});
const PARKED: Vec3 = vec3(0, -100, 0);
const TINY: Vec3 = vec3(1e-4, 1e-4, 1e-4);
const parked = (): Transform => xform(PARKED, TINY);
/** Show `entity` at `t`, or park it when `t` is null. */
const show = (entity: Entity, t: Transform | null): void => setNodeTransform(entity, t ?? parked());

/** Development counter: total retained scene nodes. */
export const sceneNodeCount = (): number => rendererNodeCount();

// ── palette ───────────────────────────────────────────────────────────────────

const PALETTE = {
  ArenaWall: [0.12, 0.14, 0.21, 1],
  Backboard: [0.92, 0.94, 0.98, 1],
  BallGoldBase: [1, 0.83, 0.28, 1],
  BallOrange: [0.92, 0.44, 0.13, 1],
  BallSeam: [0.16, 0.08, 0.04, 1],
  BannerA: [0.75, 0.28, 0.16, 1],
  BannerB: [0.2, 0.4, 0.72, 1],
  BoardFrame: [0.85, 0.3, 0.15, 1],
  Court: [0.76, 0.55, 0.32, 1],
  CourtApron: [0.5, 0.2, 0.17, 1],
  CrowdDark: [0.16, 0.17, 0.25, 1],
  CrowdLight: [0.22, 0.2, 0.28, 1],
  Key: [0.58, 0.24, 0.2, 1],
  Line: [0.92, 0.93, 0.96, 1],
  Net: [0.93, 0.94, 0.97, 1],
  Plank: [0.72, 0.51, 0.29, 1],
  Pole: [0.22, 0.24, 0.3, 1],
  RackFrame: [0.28, 0.3, 0.38, 1],
  Rim: [0.95, 0.4, 0.12, 1],
  Scoreboard: [0.09, 0.1, 0.16, 1],
  StandDark: [0.13, 0.14, 0.22, 1],
  StandLight: [0.17, 0.19, 0.28, 1],
} as const;

type MaterialName = keyof typeof PALETTE;

interface Materials {
  readonly base: Map<MaterialName, number>;
  readonly gold: number;
  readonly goldTrail: number;
  readonly normalTrail: number;
  readonly highlight: number;
  readonly underside: number;
  readonly glint: number;
  readonly impact: number;
  readonly preview: number;
  readonly shadow: number;
  readonly scuff: number;
  readonly segment: number;
  readonly flash: number;
}

const buildMaterials = (): Materials => {
  const base = new Map<MaterialName, number>();
  for (const name of Object.keys(PALETTE) as MaterialName[]) {
    base.set(name, createMaterial({ baseColor: PALETTE[name] as Rgba }));
  }
  return {
    base,
    flash: createMaterial({ baseColor: [1, 1, 1, 1], emissive: [1, 0.95, 0.85, 1] }),
    glint: createMaterial({ baseColor: [1, 0.97, 0.8, 1], emissive: [1, 0.9, 0.5, 1] }),
    gold: createMaterial({ baseColor: [1, 0.83, 0.28, 1], emissive: [0.55, 0.4, 0.08, 1] }),
    goldTrail: createMaterial({ baseColor: [1, 0.9, 0.45, 1], emissive: [0.9, 0.7, 0.2, 1], opacity: 0.55 }),
    highlight: createMaterial({ baseColor: [1, 0.9, 0.75, 1], emissive: [0.25, 0.2, 0.12, 1], opacity: 0.32 }),
    impact: createMaterial({ baseColor: [1, 0.85, 0.5, 1], emissive: [1, 0.7, 0.25, 1], opacity: 0.7 }),
    normalTrail: createMaterial({ baseColor: [0.95, 0.55, 0.25, 1], emissive: [0.35, 0.16, 0.05, 1], opacity: 0.35 }),
    preview: createMaterial({ baseColor: [0.4, 0.95, 0.6, 1], emissive: [0.2, 0.7, 0.35, 1] }),
    scuff: createMaterial({ baseColor: [0.5, 0.37, 0.22, 1], opacity: 0.55 }),
    segment: createMaterial({ baseColor: [1, 0.72, 0.25, 1], emissive: [0.9, 0.55, 0.12, 1] }),
    shadow: createMaterial({ baseColor: [0.02, 0.02, 0.03, 1], opacity: 0.4 }),
    underside: createMaterial({ baseColor: [0.25, 0.1, 0.04, 1], opacity: 0.3 }),
  };
};

// ── static build: court + wear ────────────────────────────────────────────────

const buildCourt = (box: number, mats: Materials): void => {
  const m = mats.base;
  spawnRenderable(box, m.get("CourtApron")!, xform(vec3(0, -0.07, 4.2), boxScale(vec3(19, 0.1, 19.5))));
  spawnRenderable(box, m.get("Court")!, xform(vec3(0, -0.05, 4.2), boxScale(vec3(16.5, 0.11, 17.2))));
  // Plank variation: a few slightly-tinted boards break up the hardwood.
  for (let i = 0; i < 6; i += 1) {
    const x = -6.5 + i * 2.6;
    spawnRenderable(box, m.get("Plank")!, xform(vec3(x, 0.0035, 4.2 + (i % 3) * 1.4 - 1.4), boxScale(vec3(0.9, 0.012, 14))));
  }
  spawnRenderable(box, m.get("Key")!, xform(vec3(0, 0.006, 1.55), boxScale(vec3(3.6, 0.02, 5.7))));
  // Baseline, free-throw line, key rails.
  spawnRenderable(box, m.get("Line")!, xform(vec3(0, 0.014, -1.2), boxScale(vec3(13, 0.02, 0.1))));
  spawnRenderable(box, m.get("Line")!, xform(vec3(0, 0.014, 4.4), boxScale(vec3(3.6, 0.02, 0.1))));
  for (const sx of [-1.8, 1.8]) {
    spawnRenderable(box, m.get("Line")!, xform(vec3(sx, 0.014, 1.55), boxScale(vec3(0.1, 0.02, 5.7))));
  }
  // Free-throw circle (segments).
  for (let k = 0; k < 24; k += 1) {
    const a = (k / 24) * Math.PI * 2;
    spawnRenderable(
      box,
      m.get("Line")!,
      xform(vec3(Math.cos(a) * 1.8, 0.014, 4.4 + Math.sin(a) * 1.8), boxScale(vec3(0.12, 0.02, 0.12))),
    );
  }
  // The three-point arc the stations stand on, drawn as small segments.
  const segs = 48;
  for (let k = 0; k <= segs; k += 1) {
    const a = -1.05 + (2.1 * k) / segs;
    spawnRenderable(
      box,
      m.get("Line")!,
      xform(vec3(Math.sin(a) * 6.75, 0.014, Math.cos(a) * 6.75), boxScale(vec3(0.14, 0.02, 0.14))),
    );
  }
  // Corner-three straight lines out to the sidelines.
  for (const side of [-1, 1]) {
    spawnRenderable(box, m.get("Line")!, xform(vec3(side * 5.86, 0.014, -0.1), boxScale(vec3(0.1, 0.02, 2.2))));
  }
  // Court wear: scuffs in the key and under the shooting spots, plus worn
  // patches on the busiest line segments. Static decor — zero runtime cost.
  const scuffs: readonly (readonly [number, number, number, number])[] = [
    [0.4, 1.1, 0.5, 0.18],
    [-0.7, 2.2, 0.4, 0.14],
    [0.2, 3.1, 0.6, 0.2],
    [-4.2, 5.0, 0.5, 0.16],
    [4.3, 5.1, 0.45, 0.15],
    [0.1, 6.4, 0.55, 0.2],
    [-2.6, 0.6, 0.35, 0.12],
    [2.2, 1.8, 0.4, 0.14],
  ];
  for (const [x, z, w, d] of scuffs) {
    spawnRenderable(box, mats.scuff, xform(vec3(x, 0.0105, z), boxScale(vec3(w, 0.008, d))));
  }
  for (const [x, z] of [
    [-0.9, 4.4],
    [1.2, 4.4],
    [0, -0.2],
  ] as const) {
    spawnRenderable(box, mats.scuff, xform(vec3(x, 0.0165, z), boxScale(vec3(0.5, 0.006, 0.08))));
  }
};

// ── static build: arena shell + crowd + banners + scoreboard ─────────────────

interface CrowdFigure {
  readonly body: Entity;
  readonly head: Entity;
  readonly x: number;
  readonly baseY: number;
  readonly z: number;
  readonly phase: number;
}

interface Banner {
  readonly entity: Entity;
  readonly position: Vec3;
  readonly size: Vec3;
  readonly phase: number;
}

const buildArena = (box: number, mats: Materials): void => {
  const m = mats.base;
  spawnRenderable(box, m.get("ArenaWall")!, xform(vec3(0, 4.5, -3.6), boxScale(vec3(26, 12, 0.5))));
  for (const side of [-1, 1]) {
    spawnRenderable(box, m.get("ArenaWall")!, xform(vec3(side * 11.5, 4.5, 5), boxScale(vec3(0.5, 12, 20))));
  }
  spawnRenderable(box, m.get("ArenaWall")!, xform(vec3(0, 4.5, 14.5), boxScale(vec3(26, 12, 0.5))));
  for (const side of [-1, 1]) {
    for (let row = 0; row < 4; row += 1) {
      spawnRenderable(
        box,
        m.get(row % 2 === 0 ? "StandDark" : "StandLight")!,
        xform(vec3(side * (9.6 + row * 0.5), 0.55 + row * 0.55, 5), boxScale(vec3(0.5, 1.1, 18.4))),
      );
    }
  }
  for (let row = 0; row < 4; row += 1) {
    spawnRenderable(
      box,
      m.get(row % 2 === 0 ? "StandDark" : "StandLight")!,
      xform(vec3(0, 0.55 + row * 0.55, -2.4 - row * 0.28), boxScale(vec3(22, 1.1, 0.4))),
    );
  }
};

const buildCrowd = (box: number, sphere: number, mats: Materials): { figures: CrowdFigure[]; arms: Entity[]; flashes: Entity[] } => {
  const m = mats.base;
  const figures: CrowdFigure[] = [];
  const seat = (x: number, y: number, z: number, phase: number, light: boolean): void => {
    figures.push({
      baseY: y,
      body: spawnRenderable(box, m.get(light ? "CrowdLight" : "CrowdDark")!, parked()),
      head: spawnRenderable(sphere, m.get(light ? "CrowdLight" : "CrowdDark")!, parked()),
      phase,
      x,
      z,
    });
  };
  // Two rows behind the hoop, one row down each sideline — grouped bob phases
  // (three groups), no per-figure AI.
  for (let i = 0; i < 10; i += 1) {
    seat(-6.3 + i * 1.4, 1.35, -2.55, (i % 3) * 2.1, i % 2 === 0);
    seat(-5.6 + i * 1.3, 1.9, -2.85, (i % 3) * 2.1 + 1, i % 2 === 1);
  }
  for (const side of [-1, 1]) {
    for (let i = 0; i < 8; i += 1) {
      seat(side * 9.4, 1.35, -0.5 + i * 2.1, ((i + (side === 1 ? 1 : 0)) % 3) * 2.1, (i + side) % 2 === 0);
    }
  }
  // A few dedicated raised arms (deterministic schedule; no AI).
  const arms = Array.from({ length: 4 }, () => spawnRenderable(box, m.get("CrowdLight")!, parked()));
  // Rare camera flashes in the stands.
  const flashes = Array.from({ length: 3 }, () => spawnRenderable(sphere, mats.flash, parked()));
  return { arms, figures, flashes };
};

const buildBanners = (box: number, mats: Materials): Banner[] => {
  const m = mats.base;
  return [
    { entity: spawnRenderable(box, m.get("BannerA")!, parked()), phase: 0, position: vec3(-5.4, 6.4, -3.3), size: vec3(0.9, 1.5, 0.06) },
    { entity: spawnRenderable(box, m.get("BannerB")!, parked()), phase: 2.4, position: vec3(5.4, 6.4, -3.3), size: vec3(0.9, 1.5, 0.06) },
  ];
};

/** 7-segment truth table (a, b, c, d, e, f, g) for digits 0–9. */
const DIGIT_SEGS: readonly (readonly number[])[] = [
  [1, 1, 1, 1, 1, 1, 0],
  [0, 1, 1, 0, 0, 0, 0],
  [1, 1, 0, 1, 1, 0, 1],
  [1, 1, 1, 1, 0, 0, 1],
  [0, 1, 1, 0, 0, 1, 1],
  [1, 0, 1, 1, 0, 1, 1],
  [1, 0, 1, 1, 1, 1, 1],
  [1, 1, 1, 0, 0, 0, 0],
  [1, 1, 1, 1, 1, 1, 1],
  [1, 1, 1, 1, 0, 1, 1],
];

interface Scoreboard {
  /** [digit][segment] entities with their fixed transforms. */
  readonly segs: readonly (readonly { entity: Entity; at: Transform }[])[];
  readonly pips: readonly Entity[];
}

const buildScoreboard = (box: number, mats: Materials): Scoreboard => {
  const m = mats.base;
  const cx = 0;
  const cy = 5.55;
  const cz = -3.28;
  spawnRenderable(box, m.get("Scoreboard")!, xform(vec3(cx, cy, cz), boxScale(vec3(2.4, 1.15, 0.12))));
  spawnRenderable(box, m.get("Pole")!, xform(vec3(cx, cy + 0.85, cz), boxScale(vec3(0.08, 0.6, 0.08))));
  const segs: { entity: Entity; at: Transform }[][] = [];
  const w = 0.34;
  const h = 0.5;
  const th = 0.05;
  const zf = cz + 0.09;
  for (let d = 0; d < 3; d += 1) {
    const dx = cx - 0.62 + d * 0.62;
    const dy = cy + 0.13;
    const place = (ox: number, oy: number, horizontal: boolean): { entity: Entity; at: Transform } => ({
      at: xform(vec3(dx + ox, dy + oy, zf), boxScale(horizontal ? vec3(w * 0.66, th, 0.03) : vec3(th, h * 0.42, 0.03))),
      entity: spawnRenderable(box, mats.segment, parked()),
    });
    segs.push([
      place(0, h / 2, true), // a
      place(w / 2, h / 4, false), // b
      place(w / 2, -h / 4, false), // c
      place(0, -h / 2, true), // d
      place(-w / 2, -h / 4, false), // e
      place(-w / 2, h / 4, false), // f
      place(0, 0, true), // g
    ]);
  }
  const pips = Array.from({ length: 6 }, (_, i) =>
    spawnRenderable(box, mats.segment, xform(vec3(cx - 0.45 + i * 0.18, cy - 0.42, zf), TINY)),
  );
  return { pips, segs };
};

// ── static build: hoop + racks ────────────────────────────────────────────────

interface NetStrand {
  readonly entity: Entity;
  readonly x: number;
  readonly z: number;
}

const NET_DROP = 0.36;

interface HoopHandles {
  readonly rimTorus: Entity;
  readonly rimBracket: Entity;
  readonly board: Entity;
  readonly boardTrim: readonly { entity: Entity; offset: Vec3; size: Vec3 }[];
  readonly netStrands: readonly NetStrand[];
  readonly netRing: Entity;
}

const buildHoop = (box: number, mats: Materials): HoopHandles => {
  const m = mats.base;
  spawnRenderable(box, m.get("Pole")!, xform(POLE_CENTER, boxScale(vec3(POLE_HALF.x * 2, POLE_HALF.y * 2, POLE_HALF.z * 2))));
  spawnRenderable(box, m.get("Pole")!, xform(vec3(0, 3.4, -0.82), boxScale(vec3(0.1, 0.1, 0.62))));
  const board = spawnRenderable(
    box,
    m.get("Backboard")!,
    xform(BACKBOARD_CENTER, boxScale(vec3(BACKBOARD_HALF.x * 2, BACKBOARD_HALF.y * 2, BACKBOARD_HALF.z * 2))),
  );
  // Shooter's square, kept as offsets so it shakes with the board.
  const squareZ = BACKBOARD_HALF.z + 0.006;
  const trimSpec: readonly (readonly [number, number, number, number])[] = [
    [0, 3.42 - BACKBOARD_CENTER.y, 0.6, 0.05],
    [0, 3.05 - BACKBOARD_CENTER.y, 0.6, 0.05],
    [-0.3, 3.235 - BACKBOARD_CENTER.y, 0.05, 0.42],
    [0.3, 3.235 - BACKBOARD_CENTER.y, 0.05, 0.42],
  ];
  const boardTrim = trimSpec.map(([ox, oy, w, h]) => ({
    entity: spawnRenderable(box, m.get("BoardFrame")!, parked()),
    offset: vec3(ox, oy, squareZ),
    size: vec3(w, h, 0.012),
  }));
  const rim = createMeshData(torusY(RIM_RADIUS, RIM_TUBE, 40, 10));
  const rimTorus = spawnRenderable(rim, m.get("Rim")!, xform(vec3(RIM_X, RIM_Y, RIM_Z), vec3(1, 1, 1)));
  const rimBracket = spawnRenderable(
    box,
    m.get("Rim")!,
    xform(vec3(0, RIM_Y - 0.02, -(RIM_RADIUS + 0.09)), boxScale(vec3(0.12, 0.05, 0.2))),
  );
  const netStrands: NetStrand[] = RIM_COLLIDER_CENTERS.map((c) => ({
    entity: spawnRenderable(box, m.get("Net")!, parked()),
    x: c.x,
    z: c.z,
  }));
  const ring = createMeshData(torusY(RIM_RADIUS * 0.62, 0.011, 24, 6));
  const netRing = spawnRenderable(ring, m.get("Net")!, xform(vec3(RIM_X, RIM_Y - NET_DROP, RIM_Z), vec3(1, 1, 1)));
  return { board, boardTrim, netRing, netStrands, rimBracket, rimTorus };
};

interface RackFrame {
  readonly entity: Entity;
  readonly position: Vec3;
  readonly scale: Vec3;
}

const buildRacks = (
  box: number,
  cylinder: number,
  sphere: number,
  mats: Materials,
): { rackBalls: Entity[]; rackFrames: RackFrame[][] } => {
  const m = mats.base;
  const rackBalls: Entity[] = [];
  const rackFrames: RackFrame[][] = [];
  for (const station of STATIONS) {
    const center = rackCenter(station);
    const fwd = yawForward(station.baseYaw);
    const right = yawRight(station.baseYaw);
    const railLength = RACK_SLOT_SPACING * BALLS_PER_RACK + 0.12;
    const frames: RackFrame[] = [];
    for (const lateral of [-0.09, 0.09]) {
      const position = vec3(center.x + right.x * lateral, RACK_BALL_Y - 0.1, center.z + right.z * lateral);
      const scale = boxScale(vec3(Math.abs(fwd.x) * railLength + 0.05, 0.05, Math.abs(fwd.z) * railLength + 0.05));
      frames.push({ entity: spawnRenderable(box, m.get("RackFrame")!, xform(position, scale)), position, scale });
    }
    for (const endOffset of [-railLength / 2, railLength / 2]) {
      const position = vec3(center.x + fwd.x * endOffset, (RACK_BALL_Y - 0.12) / 2, center.z + fwd.z * endOffset);
      const scale = vec3(0.05, RACK_BALL_Y - 0.12, 0.05);
      frames.push({ entity: spawnRenderable(cylinder, m.get("RackFrame")!, xform(position, scale)), position, scale });
    }
    rackFrames.push(frames);
    for (let slot = 0; slot < BALLS_PER_RACK; slot += 1) {
      const material = slot === GOLDEN_BALL_INDEX ? mats.gold : m.get("BallOrange")!;
      rackBalls.push(spawnRenderable(sphere, material, xform(rackSlotPosition(station, slot), sphereScale(BALL_RADIUS))));
    }
  }
  return { rackBalls, rackFrames };
};

// ── handles ───────────────────────────────────────────────────────────────────

/** Pooled ball-detail nodes (seams ×2 + light highlight + dark underside),
 * enough for the held ball plus every airborne ball worth detailing. */
const DETAIL_POOL = 5;
const ORANGE_POOL = 6;
const GOLD_TRAIL_POOL = POLISH_TUNING.goldenTrailSamples;
const NORMAL_TRAIL_POOL = POLISH_TUNING.maxPooledParticles - GOLD_TRAIL_POOL;

interface DetailSet {
  readonly seamA: Entity;
  readonly seamB: Entity;
  readonly highlight: Entity;
  readonly underside: Entity;
}

export interface SceneHandles {
  readonly rackBalls: readonly Entity[];
  readonly rackFrames: readonly (readonly RackFrame[])[];
  readonly orangePool: readonly Entity[];
  readonly goldBall: Entity;
  readonly details: readonly DetailSet[];
  readonly glint: Entity;
  readonly shadows: readonly Entity[];
  readonly goldTrail: readonly Entity[];
  readonly normalTrail: readonly Entity[];
  readonly impactFlash: Entity;
  readonly hoop: HoopHandles;
  readonly scoreboard: Scoreboard;
  readonly crowd: { readonly figures: readonly CrowdFigure[]; readonly arms: readonly Entity[]; readonly flashes: readonly Entity[] };
  readonly banners: readonly Banner[];
  readonly preview: readonly Entity[];
}

/** Build the whole scene, set the lights, and return the dynamic handles. */
export const buildScene = (): SceneHandles => {
  clearScene();
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const cylinder = createMesh("cylinder");
  const mats = buildMaterials();

  buildCourt(box, mats);
  buildArena(box, mats);
  const crowd = buildCrowd(box, sphere, mats);
  const banners = buildBanners(box, mats);
  const scoreboard = buildScoreboard(box, mats);
  const hoop = buildHoop(box, mats);
  const { rackBalls, rackFrames } = buildRacks(box, cylinder, sphere, mats);

  const orangePool = Array.from({ length: ORANGE_POOL }, () => spawnRenderable(sphere, mats.base.get("BallOrange")!, parked()));
  const goldBall = spawnRenderable(sphere, mats.gold, parked());
  const details = Array.from({ length: DETAIL_POOL }, () => ({
    highlight: spawnRenderable(sphere, mats.highlight, parked()),
    seamA: spawnRenderable(sphere, mats.base.get("BallSeam")!, parked()),
    seamB: spawnRenderable(sphere, mats.base.get("BallSeam")!, parked()),
    underside: spawnRenderable(sphere, mats.underside, parked()),
  }));
  const glint = spawnRenderable(sphere, mats.glint, parked());
  const shadows = Array.from({ length: ORANGE_POOL + 1 }, () => spawnRenderable(cylinder, mats.shadow, parked()));
  const goldTrail = Array.from({ length: GOLD_TRAIL_POOL }, () => spawnRenderable(sphere, mats.goldTrail, parked()));
  const normalTrail = Array.from({ length: NORMAL_TRAIL_POOL }, () => spawnRenderable(sphere, mats.normalTrail, parked()));
  const impactFlash = spawnRenderable(sphere, mats.impact, parked());
  const preview = Array.from({ length: PREVIEW_POINTS }, () => spawnRenderable(sphere, mats.preview, parked()));

  addLight({ color: [1, 0.95, 0.86, 1], direction: sdk(vec3(-0.25, -0.8, -0.45)), intensity: 2.1, kind: "directional" });
  addLight({ color: [0.62, 0.7, 0.92, 1], direction: sdk(vec3(0.35, -0.5, 0.6)), intensity: 1.05, kind: "directional" });
  addLight({ color: [0.65, 0.65, 0.75, 1], direction: sdk(vec3(0, 1, -0.15)), intensity: 0.7, kind: "directional" });
  addLight({ color: [1, 0.9, 0.75, 1], intensity: 2.2, kind: "point", position: sdk(vec3(0, 5.6, 0.6)) });

  return {
    banners,
    crowd,
    details,
    glint,
    goldBall,
    goldTrail,
    hoop,
    impactFlash,
    normalTrail,
    orangePool,
    preview,
    rackBalls,
    rackFrames,
    scoreboard,
    shadows,
  };
};

// ── per-frame dynamic update ──────────────────────────────────────────────────

const applyCamera = (view: SceneView): void => {
  setCamera3D({
    far: CAMERA_FAR,
    fovY: CAMERA_FOV_Y,
    near: CAMERA_NEAR,
    position: sdk(view.cameraPosition),
    target: sdk(view.cameraTarget),
  });
};

/** Racks: slot balls (with the current station's settle offsets), frame dip. */
const applyRacks = (h: SceneHandles, view: SceneView, currentStation: number): void => {
  for (let i = 0; i < h.rackBalls.length; i += 1) {
    const stationIndex = Math.floor(i / BALLS_PER_RACK);
    const station = STATIONS[stationIndex]!;
    const slot = i % BALLS_PER_RACK;
    if (!view.rackFilled[i]) {
      show(h.rackBalls[i]!, null);
      continue;
    }
    const pos = rackSlotPosition(station, slot);
    const settle = stationIndex === currentStation ? (view.slotSettle[slot] ?? 0) : 0;
    show(h.rackBalls[i]!, xform(vec3(pos.x, pos.y + settle, pos.z), sphereScale(BALL_RADIUS)));
  }
  for (let s = 0; s < h.rackFrames.length; s += 1) {
    const dip = s === currentStation ? view.rackDip : 0;
    for (const frame of h.rackFrames[s]!) {
      show(frame.entity, xform(vec3(frame.position.x, frame.position.y - dip, frame.position.z), frame.scale));
    }
  }
};

/** The light direction the stable ball highlight faces (the key light, inverted). */
const HIGHLIGHT_DIR = (() => {
  const d = vec3(0.25, 0.8, 0.45);
  const len = Math.hypot(d.x, d.y, d.z);
  return vec3(d.x / len, d.y / len, d.z / len);
})();

const applyBalls = (h: SceneHandles, view: SceneView): void => {
  const balls: BallView[] = [...(view.heldBall === null ? [] : [view.heldBall]), ...view.flying];
  let orangeUsed = 0;
  let goldUsed = false;
  let shadowUsed = 0;
  let detailUsed = 0;
  let goldTrailUsed = 0;
  let normalTrailUsed = 0;
  let glintShown = false;
  for (const b of balls) {
    const entity = b.golden ? (goldUsed ? null : h.goldBall) : (h.orangePool[orangeUsed] ?? null);
    if (entity === null) continue;
    if (b.golden) goldUsed = true;
    else orangeUsed += 1;
    // Squash preserves silhouette volume: y compresses, xz bulge slightly.
    const bulge = 1 + (1 - b.squash) * 0.5;
    show(entity, {
      position: sdk(b.position),
      rotation: b.orientation,
      scale: sdk(vec3(BALL_RADIUS * 2 * bulge, BALL_RADIUS * 2 * b.squash, BALL_RADIUS * 2 * bulge)),
    });
    // Ball detail: two rotating seam dots (spin legibility), a stable
    // light-facing highlight, and a darker underside.
    const detail = h.details[detailUsed];
    if (detail !== undefined) {
      detailUsed += 1;
      const seamOffset = quatRotate(b.orientation, vec3(0, BALL_RADIUS * 0.82, 0));
      const dotAt = (sign: number): Vec3 =>
        vec3(b.position.x + seamOffset.x * sign, b.position.y + seamOffset.y * sign, b.position.z + seamOffset.z * sign);
      show(detail.seamA, xform(dotAt(1), sphereScale(BALL_RADIUS * 0.34)));
      show(detail.seamB, xform(dotAt(-1), sphereScale(BALL_RADIUS * 0.34)));
      show(
        detail.highlight,
        xform(
          vec3(
            b.position.x + HIGHLIGHT_DIR.x * BALL_RADIUS * 0.62,
            b.position.y + HIGHLIGHT_DIR.y * BALL_RADIUS * 0.62,
            b.position.z + HIGHLIGHT_DIR.z * BALL_RADIUS * 0.62,
          ),
          sphereScale(BALL_RADIUS * 0.5),
        ),
      );
      show(detail.underside, xform(vec3(b.position.x, b.position.y - BALL_RADIUS * 0.58, b.position.z), sphereScale(BALL_RADIUS * 0.56)));
    }
    // Golden glint: a tiny bright star that blinks on deterministically.
    if (b.golden && b.glint && !glintShown) {
      glintShown = true;
      show(h.glint, xform(vec3(b.position.x + BALL_RADIUS * 0.5, b.position.y + BALL_RADIUS * 0.7, b.position.z), sphereScale(0.035)));
    }
    // Ground shadow: a flat disc that tightens as the ball comes down.
    const shadow = h.shadows[shadowUsed];
    if (shadow !== undefined) {
      shadowUsed += 1;
      const height = Math.max(0, b.position.y - BALL_RADIUS);
      const spread = 1 / (1 + height * 0.45);
      const d = BALL_RADIUS * 2 * (0.7 + 0.6 * spread);
      show(shadow, xform(vec3(b.position.x, 0.006, b.position.z), vec3(d, 0.012, d)));
    }
    // Bounded pooled trails (golden = longer + brighter; normal = fast shots).
    const pool = b.golden ? h.goldTrail : h.normalTrail;
    let used = b.golden ? goldTrailUsed : normalTrailUsed;
    for (let i = b.trail.length - 1; i >= 0 && used < pool.length; i -= 1) {
      const p = b.trail[i]!;
      const fade = (i + 1) / b.trail.length;
      show(pool[used]!, xform(p, sphereScale(BALL_RADIUS * (b.golden ? 0.3 + 0.45 * fade : 0.2 + 0.3 * fade))));
      used += 1;
    }
    if (b.golden) goldTrailUsed = used;
    else normalTrailUsed = used;
  }
  for (let i = orangeUsed; i < h.orangePool.length; i += 1) show(h.orangePool[i]!, null);
  if (!goldUsed) show(h.goldBall, null);
  if (!glintShown) show(h.glint, null);
  for (let i = detailUsed; i < h.details.length; i += 1) {
    const d = h.details[i]!;
    show(d.seamA, null);
    show(d.seamB, null);
    show(d.highlight, null);
    show(d.underside, null);
  }
  for (let i = shadowUsed; i < h.shadows.length; i += 1) show(h.shadows[i]!, null);
  for (let i = goldTrailUsed; i < h.goldTrail.length; i += 1) show(h.goldTrail[i]!, null);
  for (let i = normalTrailUsed; i < h.normalTrail.length; i += 1) show(h.normalTrail[i]!, null);
};

/** Hoop reactions: the VISIBLE rim/board move by the view's damped offsets and
 * return to exact rest (colliders never move). */
const applyHoop = (h: SceneHandles, view: SceneView): void => {
  const ro = view.rimOffset;
  show(h.hoop.rimTorus, xform(vec3(RIM_X + ro.x, RIM_Y + ro.y, RIM_Z + ro.z), vec3(1, 1, 1)));
  show(h.hoop.rimBracket, xform(vec3(ro.x, RIM_Y - 0.02 + ro.y, -(RIM_RADIUS + 0.09) + ro.z), boxScale(vec3(0.12, 0.05, 0.2))));
  const bo = view.boardOffset;
  const bc = vec3(BACKBOARD_CENTER.x + bo.x, BACKBOARD_CENTER.y + bo.y, BACKBOARD_CENTER.z + bo.z);
  show(h.hoop.board, xform(bc, boxScale(vec3(BACKBOARD_HALF.x * 2, BACKBOARD_HALF.y * 2, BACKBOARD_HALF.z * 2))));
  for (const trim of h.hoop.boardTrim) {
    show(trim.entity, xform(vec3(bc.x + trim.offset.x, bc.y + trim.offset.y, bc.z + trim.offset.z), boxScale(trim.size)));
  }
};

/** The net: strands lean from the rim circle to the gather ring; the reaction
 * stretches them down (drop), widens the mouth (flare), and pushes the bottoms
 * sideways near the crossed section — then everything returns to exact rest. */
const applyNet = (h: SceneHandles, view: SceneView): void => {
  const { drop, flare, lateralX, lateralZ } = view.net;
  const stretch = 1 + 0.55 * drop;
  const flareK = 1 + 0.45 * flare;
  const latLen = Math.hypot(lateralX, lateralZ);
  for (const strand of h.hoop.netStrands) {
    const topX = strand.x + view.rimOffset.x;
    const topZ = strand.z + view.rimOffset.z;
    // Strands nearer the crossed section displace more.
    const weight =
      latLen < 1e-6 ? 0 : Math.max(0, ((strand.x - RIM_X) * lateralX + (strand.z - RIM_Z) * lateralZ) / (RIM_RADIUS * latLen));
    const botX = RIM_X + (strand.x - RIM_X) * 0.62 * flareK + lateralX * (0.4 + 0.6 * weight);
    const botZ = RIM_Z + (strand.z - RIM_Z) * 0.62 * flareK + lateralZ * (0.4 + 0.6 * weight);
    const midX = (topX + botX) / 2;
    const midZ = (topZ + botZ) / 2;
    const dropLen = NET_DROP * stretch;
    show(strand.entity, xform(vec3(midX, RIM_Y - dropLen / 2, midZ), boxScale(vec3(0.014, dropLen, 0.014))));
  }
  show(
    h.hoop.netRing,
    xform(vec3(RIM_X + lateralX * 0.7, RIM_Y - NET_DROP * stretch, RIM_Z + lateralZ * 0.7), vec3(flareK, 1, flareK)),
  );
};

const applyImpact = (h: SceneHandles, view: SceneView): void => {
  const i = view.impact;
  show(h.impactFlash, i !== null ? xform(i.position, sphereScale(0.05 + 0.16 * i.strength)) : null);
};

/** The arena scoreboard: 7-segment score + streak pips, mirroring the HUD. */
const applyScoreboard = (h: SceneHandles, view: SceneView): void => {
  const score = Math.max(0, Math.min(999, view.score));
  const digits = [Math.floor(score / 100), Math.floor(score / 10) % 10, score % 10];
  const leading = (d: number): boolean => (d === 0 && score < 100) || (d === 1 && score < 10);
  for (let d = 0; d < 3; d += 1) {
    const mask = DIGIT_SEGS[digits[d]!]!;
    const blank = leading(d);
    const column = h.scoreboard.segs[d]!;
    for (let s = 0; s < 7; s += 1) {
      const seg = column[s]!;
      show(seg.entity, !blank && mask[s] === 1 ? seg.at : null);
    }
  }
  for (let i = 0; i < h.scoreboard.pips.length; i += 1) {
    const pip = h.scoreboard.pips[i]!;
    const lit = i < view.streak;
    const t = setPipTransforms[i]!;
    show(pip, lit ? t : null);
  }
};

/** Fixed pip transforms (computed once; scoreboard geometry never moves). */
const setPipTransforms: Transform[] = Array.from({ length: 6 }, (_, i) =>
  xform(vec3(-0.45 + i * 0.18, 5.13, -3.19), boxScale(vec3(0.12, 0.1, 0.03))),
);

/** Crowd idle bob + reaction lift, four scheduled arms, rare camera flashes —
 * all bounded loops keyed off the view's tick and crowd pulse. */
const applyArenaLife = (h: SceneHandles, view: SceneView): void => {
  const pulse = view.crowdPulse;
  const amp = 0.012 + 0.05 * pulse;
  for (const fig of h.crowd.figures) {
    const bob = Math.sin(view.tick * 0.055 + fig.phase) * amp + (pulse > 0 ? pulse * 0.02 : 0);
    show(fig.body, xform(vec3(fig.x, fig.baseY + 0.31 + bob, fig.z), boxScale(vec3(0.34, 0.62, 0.22))));
    show(fig.head, xform(vec3(fig.x, fig.baseY + 0.74 + bob, fig.z), sphereScale(0.11)));
  }
  for (let i = 0; i < h.crowd.arms.length; i += 1) {
    const fig = h.crowd.figures[i * 7];
    if (fig === undefined) continue;
    const cycle = (view.tick + i * 211) % 600;
    const up = cycle < 90 || pulse > 0.55;
    show(
      h.crowd.arms[i]!,
      up ? xform(vec3(fig.x + 0.22, fig.baseY + 0.85 + Math.sin(view.tick * 0.2 + i) * 0.03, fig.z), boxScale(vec3(0.08, 0.4, 0.08))) : null,
    );
  }
  for (let i = 0; i < h.crowd.flashes.length; i += 1) {
    const fig = h.crowd.figures[5 + i * 9];
    if (fig === undefined) continue;
    const on = (view.tick + i * 397) % 1113 < 3;
    show(h.crowd.flashes[i]!, on ? xform(vec3(fig.x, fig.baseY + 0.9, fig.z + 0.15), sphereScale(0.06)) : null);
  }
  for (const banner of h.banners) {
    const sway = Math.sin(view.tick * 0.012 + banner.phase) * 0.055;
    show(banner.entity, xform(banner.position, boxScale(banner.size), quatFromEulerXyz(0, 0, sway)));
  }
};

const applyPreview = (h: SceneHandles, view: SceneView): void => {
  for (let i = 0; i < h.preview.length; i += 1) {
    const p = view.preview[i];
    show(h.preview[i]!, p !== undefined ? xform(p, sphereScale(0.035)) : null);
  }
};

/** Move every dynamic node to match the session view. Called once per tick. */
export const applyFrame = (h: SceneHandles, view: SceneView): void => {
  applyCamera(view);
  applyRacks(h, view, currentStationOf(view));
  applyBalls(h, view);
  applyHoop(h, view);
  applyNet(h, view);
  applyImpact(h, view);
  applyScoreboard(h, view);
  applyArenaLife(h, view);
  applyPreview(h, view);
};

/** The station whose rack reacts: nearest to the camera (exact during play,
 * and it hands off naturally mid-glide). */
const currentStationOf = (view: SceneView): number => {
  let best = 0;
  let bestD = Infinity;
  for (let i = 0; i < STATIONS.length; i += 1) {
    const p = STATIONS[i]!.position;
    const dx = p.x - view.cameraPosition.x;
    const dz = p.z - view.cameraPosition.z;
    const d = dx * dx + dz * dz;
    if (d < bestD) {
      bestD = d;
      best = i;
    }
  }
  return best;
};
