/*
 * scene.ts — the ONE gameplay file that touches the engine. It builds the whole
 * arena procedurally (no external assets) through the SDK's 3D scene surface —
 * hardwood court with painted key / free-throw line / three-point arc, glass
 * backboard with a shooter's square, the real torus rim (the SAME
 * RIM_RADIUS/RIM_TUBE constants the physics collider ring uses, so the rim you
 * see is the rim you hit), a procedural net, the pole, three ball racks, and an
 * arena backdrop — then `applyFrame` re-poses every dynamic node each frame from
 * the SDK-free `SceneView` the session hands it.
 *
 * Mesh conventions (as in the sibling heat-check app): `box` is a UNIT CUBE
 * (scale = full extents); `sphere` is UNIT DIAMETER (scale = 2·radius). A node's
 * material is fixed at spawn, so the orange and golden live balls are two
 * pre-spawned entities and every glow/flash is an emissive node that gets scaled,
 * never re-colored.
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
import { type Quat, type Vec3, IDENTITY_QUAT, quatRotate, vec3 } from "./vec.ts";
import { torusY } from "./meshgen.ts";
import { RIM_COLLIDER_CENTERS } from "./physics.ts";
import type { SceneView } from "./types.ts";
import {
  BACKBOARD_CENTER,
  BACKBOARD_HALF,
  BALL_RADIUS,
  CAMERA_FAR,
  CAMERA_FOV_Y,
  CAMERA_NEAR,
  PREVIEW_POINTS,
  RACK_BALL_Y,
  RACK_SLOT_SPACING,
  BALLS_PER_RACK,
  GOLDEN_BALL_INDEX,
  POLE_CENTER,
  POLE_HALF,
  RIM_RADIUS,
  RIM_TUBE,
  RIM_X,
  RIM_Y,
  RIM_Z,
  STATIONS,
  TRAIL_POOL,
  rackCenter,
  rackSlotPosition,
  yawForward,
  yawRight,
} from "./constants.ts";

// ── SDK transform adapters ────────────────────────────────────────────────────

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

// ── palette ───────────────────────────────────────────────────────────────────

const PALETTE = {
  ArenaWall: [0.12, 0.14, 0.21, 1],
  Backboard: [0.92, 0.94, 0.98, 1],
  BallGoldBase: [1, 0.83, 0.28, 1],
  BallOrange: [0.92, 0.44, 0.13, 1],
  BallSeam: [0.16, 0.08, 0.04, 1],
  BoardFrame: [0.85, 0.3, 0.15, 1],
  Court: [0.76, 0.55, 0.32, 1],
  CourtApron: [0.5, 0.2, 0.17, 1],
  Key: [0.58, 0.24, 0.2, 1],
  Line: [0.92, 0.93, 0.96, 1],
  Net: [0.93, 0.94, 0.97, 1],
  Pole: [0.22, 0.24, 0.3, 1],
  RackFrame: [0.28, 0.3, 0.38, 1],
  Rim: [0.95, 0.4, 0.12, 1],
  StandDark: [0.13, 0.14, 0.22, 1],
  StandLight: [0.17, 0.19, 0.28, 1],
} as const;

type MaterialName = keyof typeof PALETTE;

interface Materials {
  readonly base: Map<MaterialName, number>;
  readonly gold: number;
  readonly goldTrail: number;
  readonly impact: number;
  readonly preview: number;
  readonly shadow: number;
}

const buildMaterials = (): Materials => {
  const base = new Map<MaterialName, number>();
  for (const name of Object.keys(PALETTE) as MaterialName[]) {
    base.set(name, createMaterial({ baseColor: PALETTE[name] as Rgba }));
  }
  return {
    base,
    gold: createMaterial({ baseColor: [1, 0.83, 0.28, 1], emissive: [0.55, 0.4, 0.08, 1] }),
    goldTrail: createMaterial({ baseColor: [1, 0.9, 0.45, 1], emissive: [0.9, 0.7, 0.2, 1], opacity: 0.55 }),
    impact: createMaterial({ baseColor: [1, 0.85, 0.5, 1], emissive: [1, 0.7, 0.25, 1], opacity: 0.7 }),
    preview: createMaterial({ baseColor: [0.4, 0.95, 0.6, 1], emissive: [0.2, 0.7, 0.35, 1] }),
    shadow: createMaterial({ baseColor: [0.02, 0.02, 0.03, 1], opacity: 0.4 }),
  };
};

// ── static build ──────────────────────────────────────────────────────────────

const buildCourt = (box: number, mats: Materials): void => {
  const m = mats.base;
  // Hardwood inside the apron; a darker apron slab underneath sticks out around it.
  spawnRenderable(box, m.get("CourtApron")!, xform(vec3(0, -0.07, 4.2), boxScale(vec3(19, 0.1, 19.5))));
  spawnRenderable(box, m.get("Court")!, xform(vec3(0, -0.05, 4.2), boxScale(vec3(16.5, 0.11, 17.2))));
  // Painted key (rim axis is z=0; the baseline sits behind the board).
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
};

const buildArena = (box: number, mats: Materials): void => {
  const m = mats.base;
  // Back wall behind the hoop, side walls, and a far wall behind the shooter.
  spawnRenderable(box, m.get("ArenaWall")!, xform(vec3(0, 4.5, -3.6), boxScale(vec3(26, 12, 0.5))));
  for (const side of [-1, 1]) {
    spawnRenderable(box, m.get("ArenaWall")!, xform(vec3(side * 11.5, 4.5, 5), boxScale(vec3(0.5, 12, 20))));
  }
  spawnRenderable(box, m.get("ArenaWall")!, xform(vec3(0, 4.5, 14.5), boxScale(vec3(26, 12, 0.5))));
  // Bleacher rows along both sides and behind the hoop (alternating tones).
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

interface NetStrand {
  readonly entity: Entity;
  readonly x: number;
  readonly z: number;
}

const NET_DROP = 0.36;

const buildHoop = (box: number, mats: Materials): { netStrands: readonly NetStrand[]; netRing: Entity } => {
  const m = mats.base;
  // Pole, angled arm, and board — the physics AABBs drawn at their exact extents.
  spawnRenderable(box, m.get("Pole")!, xform(POLE_CENTER, boxScale(vec3(POLE_HALF.x * 2, POLE_HALF.y * 2, POLE_HALF.z * 2))));
  spawnRenderable(box, m.get("Pole")!, xform(vec3(0, 3.4, -0.82), boxScale(vec3(0.1, 0.1, 0.62))));
  spawnRenderable(
    box,
    m.get("Backboard")!,
    xform(BACKBOARD_CENTER, boxScale(vec3(BACKBOARD_HALF.x * 2, BACKBOARD_HALF.y * 2, BACKBOARD_HALF.z * 2))),
  );
  // Shooter's square on the glass, just in front of the board face.
  const squareZ = BACKBOARD_CENTER.z + BACKBOARD_HALF.z + 0.006;
  spawnRenderable(box, m.get("BoardFrame")!, xform(vec3(0, 3.42, squareZ), boxScale(vec3(0.6, 0.05, 0.012))));
  spawnRenderable(box, m.get("BoardFrame")!, xform(vec3(0, 3.05, squareZ), boxScale(vec3(0.6, 0.05, 0.012))));
  for (const sx of [-0.3, 0.3]) {
    spawnRenderable(box, m.get("BoardFrame")!, xform(vec3(sx, 3.235, squareZ), boxScale(vec3(0.05, 0.42, 0.012))));
  }
  // Rim: the real torus, at the exact collider circle, plus its board bracket.
  const rim = createMeshData(torusY(RIM_RADIUS, RIM_TUBE, 40, 10));
  spawnRenderable(rim, m.get("Rim")!, xform(vec3(RIM_X, RIM_Y, RIM_Z), vec3(1, 1, 1)));
  spawnRenderable(box, m.get("Rim")!, xform(vec3(0, RIM_Y - 0.02, -(RIM_RADIUS + 0.09)), boxScale(vec3(0.12, 0.05, 0.2))));
  // Net: one strand per rim collider sphere, leaning inward, plus a gather ring.
  const netStrands: NetStrand[] = RIM_COLLIDER_CENTERS.map((c) => ({
    entity: spawnRenderable(box, m.get("Net")!, parked()),
    x: c.x,
    z: c.z,
  }));
  const ring = createMeshData(torusY(RIM_RADIUS * 0.62, 0.011, 24, 6));
  const netRing = spawnRenderable(ring, m.get("Net")!, xform(vec3(RIM_X, RIM_Y - NET_DROP, RIM_Z), vec3(1, 1, 1)));
  return { netRing, netStrands };
};

const buildRacks = (box: number, cylinder: number, sphere: number, mats: Materials): Entity[] => {
  const m = mats.base;
  const rackBalls: Entity[] = [];
  for (const station of STATIONS) {
    const center = rackCenter(station);
    const fwd = yawForward(station.baseYaw);
    const right = yawRight(station.baseYaw);
    const railLength = RACK_SLOT_SPACING * BALLS_PER_RACK + 0.12;
    // Two low rails the balls rest between, on four legs.
    for (const lateral of [-0.09, 0.09]) {
      spawnRenderable(
        box,
        m.get("RackFrame")!,
        xform(
          vec3(center.x + right.x * lateral, RACK_BALL_Y - 0.1, center.z + right.z * lateral),
          boxScale(vec3(Math.abs(fwd.x) * railLength + 0.05, 0.05, Math.abs(fwd.z) * railLength + 0.05)),
        ),
      );
    }
    for (const endOffset of [-railLength / 2, railLength / 2]) {
      spawnRenderable(
        cylinder,
        m.get("RackFrame")!,
        xform(vec3(center.x + fwd.x * endOffset, (RACK_BALL_Y - 0.12) / 2, center.z + fwd.z * endOffset), vec3(0.05, RACK_BALL_Y - 0.12, 0.05)),
      );
    }
    // The five balls; the fifth is golden (its own emissive material).
    for (let slot = 0; slot < BALLS_PER_RACK; slot += 1) {
      const material = slot === GOLDEN_BALL_INDEX ? mats.gold : m.get("BallOrange")!;
      rackBalls.push(spawnRenderable(sphere, material, xform(rackSlotPosition(station, slot), sphereScale(BALL_RADIUS))));
    }
  }
  return rackBalls;
};

// ── handles ───────────────────────────────────────────────────────────────────

/** Enough pooled ball entities for the worst case: one held + four airborne
 * orange, plus the (single) golden ball held or airborne, plus fade-outs. */
const ORANGE_POOL = 6;

export interface SceneHandles {
  readonly rackBalls: readonly Entity[];
  readonly orangePool: readonly Entity[];
  readonly goldBall: Entity;
  readonly seamDots: readonly [Entity, Entity];
  readonly shadows: readonly Entity[];
  readonly trail: readonly Entity[];
  readonly impactFlash: Entity;
  readonly netRing: Entity;
  readonly netStrands: readonly NetStrand[];
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
  const { netRing, netStrands } = buildHoop(box, mats);
  const rackBalls = buildRacks(box, cylinder, sphere, mats);

  const orangePool = Array.from({ length: ORANGE_POOL }, () => spawnRenderable(sphere, mats.base.get("BallOrange")!, parked()));
  const goldBall = spawnRenderable(sphere, mats.gold, parked());
  const seamDots: [Entity, Entity] = [
    spawnRenderable(sphere, mats.base.get("BallSeam")!, parked()),
    spawnRenderable(sphere, mats.base.get("BallSeam")!, parked()),
  ];
  const shadows = Array.from({ length: ORANGE_POOL + 1 }, () => spawnRenderable(cylinder, mats.shadow, parked()));
  const trail = Array.from({ length: TRAIL_POOL }, () => spawnRenderable(sphere, mats.goldTrail, parked()));
  const impactFlash = spawnRenderable(sphere, mats.impact, parked());
  const preview = Array.from({ length: PREVIEW_POINTS }, () => spawnRenderable(sphere, mats.preview, parked()));

  // Arena lighting: a warm key aimed at the hoop end, a cool fill from behind the
  // shooter, a bounce from below, and a point light living over the rim.
  addLight({ color: [1, 0.95, 0.86, 1], direction: sdk(vec3(-0.25, -0.8, -0.45)), intensity: 2.1, kind: "directional" });
  addLight({ color: [0.62, 0.7, 0.92, 1], direction: sdk(vec3(0.35, -0.5, 0.6)), intensity: 1.05, kind: "directional" });
  addLight({ color: [0.65, 0.65, 0.75, 1], direction: sdk(vec3(0, 1, -0.15)), intensity: 0.7, kind: "directional" });
  addLight({ color: [1, 0.9, 0.75, 1], intensity: 2.2, kind: "point", position: sdk(vec3(0, 5.6, 0.6)) });

  return { goldBall, impactFlash, netRing, netStrands, orangePool, preview, rackBalls, seamDots, shadows, trail };
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

const applyRacks = (h: SceneHandles, view: SceneView): void => {
  for (let i = 0; i < h.rackBalls.length; i += 1) {
    const station = STATIONS[Math.floor(i / BALLS_PER_RACK)]!;
    const slot = i % BALLS_PER_RACK;
    show(h.rackBalls[i]!, view.rackFilled[i] ? xform(rackSlotPosition(station, slot), sphereScale(BALL_RADIUS)) : null);
  }
};

const applyBalls = (h: SceneHandles, view: SceneView): void => {
  // Every visible basketball this frame: the held ball plus all airborne ones.
  const balls = [...(view.heldBall === null ? [] : [view.heldBall]), ...view.flying];
  let orangeUsed = 0;
  let goldUsed = false;
  let shadowUsed = 0;
  for (const b of balls) {
    const entity = b.golden ? (goldUsed ? null : h.goldBall) : h.orangePool[orangeUsed] ?? null;
    if (entity === null) continue;
    if (b.golden) goldUsed = true;
    else orangeUsed += 1;
    show(entity, xform(b.position, sphereScale(BALL_RADIUS), b.orientation));
    // Ground shadow: a flat disc that tightens as the ball comes down.
    const shadow = h.shadows[shadowUsed];
    if (shadow !== undefined) {
      shadowUsed += 1;
      const height = Math.max(0, b.position.y - BALL_RADIUS);
      const spread = 1 / (1 + height * 0.45);
      const d = BALL_RADIUS * 2 * (0.7 + 0.6 * spread);
      show(shadow, xform(vec3(b.position.x, 0.006, b.position.z), vec3(d, 0.012, d)));
    }
  }
  for (let i = orangeUsed; i < h.orangePool.length; i += 1) show(h.orangePool[i]!, null);
  if (!goldUsed) show(h.goldBall, null);
  for (let i = shadowUsed; i < h.shadows.length; i += 1) show(h.shadows[i]!, null);
  // Two seam dots riding the newest airborne ball make the spin visible.
  const spinBall = view.flying[view.flying.length - 1];
  if (spinBall === undefined) {
    show(h.seamDots[0], null);
    show(h.seamDots[1], null);
    return;
  }
  const seamOffset = quatRotate(spinBall.orientation, vec3(0, BALL_RADIUS * 0.82, 0));
  const dotAt = (sign: number): Vec3 =>
    vec3(spinBall.position.x + seamOffset.x * sign, spinBall.position.y + seamOffset.y * sign, spinBall.position.z + seamOffset.z * sign);
  show(h.seamDots[0], xform(dotAt(1), sphereScale(BALL_RADIUS * 0.34)));
  show(h.seamDots[1], xform(dotAt(-1), sphereScale(BALL_RADIUS * 0.34)));
};

const applyTrail = (h: SceneHandles, view: SceneView): void => {
  for (let i = 0; i < h.trail.length; i += 1) {
    const p = view.trail[i];
    const fade = (i + 1) / h.trail.length;
    show(h.trail[i]!, p !== undefined ? xform(p, sphereScale(BALL_RADIUS * (0.3 + 0.45 * fade))) : null);
  }
};

const applyNet = (h: SceneHandles, view: SceneView): void => {
  const pulse = view.netPulse;
  const stretch = 1 + 0.4 * pulse;
  const flare = 1 + 0.28 * pulse;
  for (const strand of h.netStrands) {
    // Strands lean inward from the rim circle toward the gather ring; a make
    // stretches them down and flares them out.
    const topX = strand.x;
    const topZ = strand.z;
    const botX = RIM_X + (strand.x - RIM_X) * 0.62 * flare;
    const botZ = RIM_Z + (strand.z - RIM_Z) * 0.62 * flare;
    const midX = (topX + botX) / 2;
    const midZ = (topZ + botZ) / 2;
    const drop = NET_DROP * stretch;
    show(strand.entity, xform(vec3(midX, RIM_Y - drop / 2, midZ), boxScale(vec3(0.014, drop, 0.014))));
  }
  show(
    h.netRing,
    xform(vec3(RIM_X, RIM_Y - NET_DROP * stretch, RIM_Z), vec3(flare, 1, flare)),
  );
};

const applyImpact = (h: SceneHandles, view: SceneView): void => {
  const i = view.impact;
  show(h.impactFlash, i !== null ? xform(i.position, sphereScale(0.05 + 0.16 * i.strength)) : null);
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
  applyRacks(h, view);
  applyBalls(h, view);
  applyTrail(h, view);
  applyNet(h, view);
  applyImpact(h, view);
  applyPreview(h, view);
};
