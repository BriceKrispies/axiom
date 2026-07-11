/*
 * scene.ts — the ONE gameplay file that touches the engine. It builds the whole
 * toy-tabletop stadium procedurally (no external assets) through the SDK's 3D
 * scene surface — striped grass diamond, infield dirt + base paths + bases, foul
 * lines, mound, warning track, blue outfield walls with orange trim, tiered
 * seating, the home-plate deck with its scoreboard panels, the batter, the
 * mechanical pitcher, ten toy fielders on their patrol circles — and each frame
 * moves the dynamic nodes to match the SDK-free `SceneView` the session hands it.
 *
 * Mesh conventions: `box` is a UNIT CUBE (scale = full extents); `sphere` is UNIT
 * DIAMETER (scale = 2·radius); `cylinder` is UNIT (radius 0.5, height 1, Y axis —
 * scale = (diameter, height, diameter)). A node's material is fixed at spawn, so
 * flashes are animated by scaling emissive nodes, never by re-coloring.
 */

import {
  type Entity,
  type Rgba,
  type Transform,
  addLight,
  clearScene,
  createMaterial,
  createMesh,
  setCamera3D,
  setClearColor,
  setNodeTransform,
  spawnRenderable,
} from "@axiom/game";
import { type Quat, type Vec3, IDENTITY_QUAT, clamp01, mix, quatFromEulerXyz, vec3 } from "./vec.ts";
import { batDir, batPlaneY } from "./swing.ts";
import type { SceneView, SwingState } from "./types.ts";
import * as C from "./constants.ts";

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

const YAW_POS = quatFromEulerXyz(0, Math.PI / 4, 0);
const YAW_NEG = quatFromEulerXyz(0, -Math.PI / 4, 0);

// ── palette ───────────────────────────────────────────────────────────────────

const PALETTE = {
  BallWhite: [1, 1, 0.98, 1],
  BaseWhite: [1, 1, 0.98, 1],
  BatterBlue: [0.22, 0.46, 1, 1],
  BatterHelmet: [0.14, 0.3, 0.85, 1],
  BatterPuck: [0.55, 0.85, 1, 1],
  BatYellow: [1, 0.88, 0.25, 1],
  BatKnob: [0.55, 0.4, 0.16, 1],
  CornerBlue: [0.24, 0.3, 0.8, 1],
  DeckBrown: [0.72, 0.5, 0.3, 1],
  Dirt: [0.82, 0.58, 0.34, 1],
  DirtLight: [0.95, 0.72, 0.44, 1],
  DotBlue: [0.2, 0.35, 0.95, 1],
  DotRed: [0.9, 0.15, 0.12, 1],
  DotYellow: [0.95, 0.8, 0.15, 1],
  FielderBase: [1, 0.6, 0.3, 1],
  FielderCap: [1, 0.22, 0.18, 1],
  FielderWhite: [1, 0.98, 0.95, 1],
  GrassDark: [0.4, 0.82, 0.24, 1],
  GrassLight: [0.55, 1, 0.34, 1],
  GroundGreen: [0.38, 0.7, 0.24, 1],
  Line: [1, 1, 0.96, 1],
  MachineDark: [0.3, 0.3, 0.36, 1],
  MachineOrange: [1, 0.6, 0.34, 1],
  PanelNavy: [0.1, 0.13, 0.38, 1],
  PatrolDirt: [0.68, 0.46, 0.26, 1],
  PatrolGreen: [0.36, 0.72, 0.22, 1],
  SeatBlue: [0.42, 0.54, 1, 1],
  SeatBlueDark: [0.3, 0.39, 0.92, 1],
  SkyBowl: [0.72, 0.76, 1, 1],
  WallBlue: [0.32, 0.44, 1, 1],
  WallTrim: [1, 0.68, 0.16, 1],
} as const;

type MaterialName = keyof typeof PALETTE;

interface Materials {
  readonly base: Map<MaterialName, number>;
  readonly bat: number;
  readonly flash: number;
  readonly trail: number;
  readonly shadow: number;
  readonly digit: number;
  readonly impact: number;
}

const buildMaterials = (): Materials => {
  const base = new Map<MaterialName, number>();
  for (const name of Object.keys(PALETTE) as MaterialName[]) {
    base.set(name, createMaterial({ baseColor: PALETTE[name] as Rgba }));
  }
  // Painted field markings glow faintly so they read WHITE at grazing angles
  // (the canvas2d hemisphere shading otherwise grays them out).
  base.set("Line", createMaterial({ baseColor: PALETTE.Line as Rgba, emissive: [0.3, 0.3, 0.28, 1] }));
  base.set("BaseWhite", createMaterial({ baseColor: PALETTE.BaseWhite as Rgba, emissive: [0.3, 0.3, 0.28, 1] }));
  return {
    base,
    bat: createMaterial({ baseColor: [1, 0.88, 0.25, 1], emissive: [0.45, 0.36, 0.08, 1] }),
    digit: createMaterial({ baseColor: [1, 0.3, 0.15, 1], emissive: [0.9, 0.2, 0.08, 1] }),
    flash: createMaterial({ baseColor: [1, 0.95, 0.6, 1], emissive: [1, 0.85, 0.4, 1] }),
    impact: createMaterial({ baseColor: [1, 0.9, 0.5, 1], emissive: [1, 0.8, 0.35, 1], opacity: 0.55 }),
    shadow: createMaterial({ baseColor: [0.05, 0.12, 0.05, 1], opacity: 0.35 }),
    trail: createMaterial({ baseColor: [1, 0.9, 0.6, 1], emissive: [1, 0.75, 0.35, 1] }),
  };
};

// ── static field build ────────────────────────────────────────────────────────

const buildGround = (box: number, cyl: number, m: Map<MaterialName, number>): void => {
  // Neutral ground under everything, and the brown home-plate deck near the
  // camera — wide enough to own the whole bottom of the frame like the reference.
  spawnRenderable(box, m.get("GroundGreen")!, xform(vec3(0, -0.07, 14), boxScale(vec3(76, 0.1, 64))));
  spawnRenderable(box, m.get("DeckBrown")!, xform(vec3(0, -0.005, -2), boxScale(vec3(46, 0.06, 15))));
  // The pale walkway seams framing the plate area (the reference's deck panels).
  for (const side of [1, -1]) {
    spawnRenderable(box, m.get("DirtLight")!, xform(vec3(side * 2.6, 0.028, -1.8), boxScale(vec3(0.35, 0.02, 8))));
  }

  // Striped outfield/infield grass: horizontal bands clipped to the diamond width.
  for (let k = 0; k < 14; k += 1) {
    const zc = 1.2 + k * 2.4;
    const halfW = Math.min(zc + 1.2, C.WALL_LINE - zc + 1.2);
    if (halfW <= 0.4) {
      continue;
    }
    const mat = k % 2 === 0 ? m.get("GrassLight")! : m.get("GrassDark")!;
    spawnRenderable(box, mat, xform(vec3(0, 0.002, zc), boxScale(vec3(halfW * 2, 0.03, 2.4))));
  }

  // Infield dirt square (rotated 45°) with a striped grass diamond inside it.
  spawnRenderable(box, m.get("Dirt")!, xform(vec3(0, 0.03, 7.5), boxScale(vec3(10.6, 0.05, 10.6)), YAW_POS));
  spawnRenderable(box, m.get("GrassLight")!, xform(vec3(0, 0.045, 7.5), boxScale(vec3(8, 0.04, 8)), YAW_POS));
  for (let k = 0; k < 4; k += 1) {
    const zc = 3.3 + k * 3.2;
    const halfW = 5.66 - Math.abs(zc - 7.5) - 0.35;
    if (halfW <= 0.3) {
      continue;
    }
    spawnRenderable(box, m.get("GrassDark")!, xform(vec3(0, 0.068, zc), boxScale(vec3(halfW * 2, 0.012, 1.6))));
  }

  // Mound + home-plate dirt circle.
  spawnRenderable(cyl, m.get("DirtLight")!, xform(vec3(C.MOUND.x, 0.075, C.MOUND.z), vec3(3.6, 0.14, 3.6)));
  spawnRenderable(cyl, m.get("Dirt")!, xform(vec3(0, 0.045, 0), vec3(5.4, 0.09, 5.4)));

  // Home plate + batter boxes. Ground decals near the plate live on a strict
  // height LADDER (dirt circle top 0.09 < foul lines < batter boxes < plate):
  // exact coplanarity z-fights, and at this grazing angle a strip narrower than
  // ~0.14u projects to a sub-2px sliver the software rasterizer fills
  // inconsistently frame-to-frame (the "flickering lines" artifact) — so the
  // painted markings are deliberately chunky, toy-style.
  spawnRenderable(box, m.get("BaseWhite")!, xform(vec3(0, 0.13, 0), boxScale(vec3(0.5, 0.02, 0.5)), YAW_POS));
  for (const side of [1, -1]) {
    spawnRenderable(box, m.get("Line")!, xform(vec3(side * 0.5, 0.125, 0), boxScale(vec3(0.14, 0.012, 1.33))));
    spawnRenderable(box, m.get("Line")!, xform(vec3(side * 1.0, 0.125, 0.6), boxScale(vec3(1.14, 0.012, 0.14))));
    spawnRenderable(box, m.get("Line")!, xform(vec3(side * 1.0, 0.125, -0.6), boxScale(vec3(1.14, 0.012, 0.14))));
    spawnRenderable(box, m.get("Line")!, xform(vec3(side * 1.5, 0.125, 0), boxScale(vec3(0.14, 0.012, 1.33))));
  }

  // Bases (1B, 2B, 3B).
  const b = C.BASE_CORNER;
  for (const [bx, bz] of [
    [-b, b],
    [0, 2 * b],
    [b, b],
  ] as const) {
    spawnRenderable(box, m.get("BaseWhite")!, xform(vec3(bx, 0.12, bz), boxScale(vec3(0.6, 0.14, 0.6)), YAW_POS));
  }

  // Foul lines home → corners (chunky + above the dirt-circle top, see the
  // ladder note below), and the warning track inside the walls.
  spawnRenderable(box, m.get("Line")!, xform(vec3(8.5, 0.105, 8.5), boxScale(vec3(24, 0.012, 0.32)), YAW_NEG));
  spawnRenderable(box, m.get("Line")!, xform(vec3(-8.5, 0.105, 8.5), boxScale(vec3(24, 0.012, 0.32)), YAW_POS));
  spawnRenderable(box, m.get("DirtLight")!, xform(vec3(7.86, 0.028, 24.86), boxScale(vec3(24.5, 0.02, 1.7)), YAW_POS));
  spawnRenderable(box, m.get("DirtLight")!, xform(vec3(-7.86, 0.028, 24.86), boxScale(vec3(24.5, 0.02, 1.7)), YAW_NEG));
};

const buildStadium = (box: number, m: Map<MaterialName, number>): void => {
  // The upper stadium bowl — a wide backdrop so the frame never shows black void.
  spawnRenderable(box, m.get("SkyBowl")!, xform(vec3(0, 16, 52), boxScale(vec3(150, 34, 1.5))));
  for (const side of [1, -1]) {
    spawnRenderable(box, m.get("SkyBowl")!, xform(vec3(side * 42, 16, 18), boxScale(vec3(1.5, 34, 80))));
  }
  // Outfield walls along the two upper diagonals, with orange trim on top.
  for (const side of [1, -1]) {
    const yaw = side > 0 ? YAW_POS : YAW_NEG;
    const cx = side * 8.9;
    spawnRenderable(box, m.get("WallBlue")!, xform(vec3(cx, C.WALL_HEIGHT / 2, 25.9), boxScale(vec3(25.8, C.WALL_HEIGHT, 0.9)), yaw));
    spawnRenderable(box, m.get("WallTrim")!, xform(vec3(cx, C.WALL_HEIGHT + 0.12, 25.9), boxScale(vec3(25.8, 0.26, 1.04)), yaw));
    // Tiered seating rising behind each wall.
    for (let k = 0; k < 4; k += 1) {
      const off = 1.4 + k * 1.55;
      const mat = k % 2 === 0 ? m.get("SeatBlue")! : m.get("SeatBlueDark")!;
      spawnRenderable(box, mat, xform(vec3(cx + side * off * 0.707, 1.3 + k * 0.85, 25.9 + off * 0.707), boxScale(vec3(27.5 + k * 1.4, 1.7, 1.6)), yaw));
    }
    // Foul-side fences + side seating running toward the camera.
    spawnRenderable(box, m.get("WallBlue")!, xform(vec3(side * 17.6, 1.1, 5), boxScale(vec3(0.9, 2.2, 25))));
    spawnRenderable(box, m.get("WallTrim")!, xform(vec3(side * 17.6, 2.32, 5), boxScale(vec3(1.04, 0.24, 25))));
    for (let k = 0; k < 3; k += 1) {
      const mat = k % 2 === 0 ? m.get("SeatBlue")! : m.get("SeatBlueDark")!;
      spawnRenderable(box, mat, xform(vec3(side * (19 + k * 1.5), 0.95 + k * 0.8, 5), boxScale(vec3(1.6, 1.5, 25))));
    }
    // Blue corner blocks framing the bottom of the composition.
    spawnRenderable(box, m.get("CornerBlue")!, xform(vec3(side * 14.2, 1.2, -5.2), boxScale(vec3(6.5, 2.6, 6))));
    spawnRenderable(box, m.get("SeatBlueDark")!, xform(vec3(side * 15.4, 2.9, -5.6), boxScale(vec3(4.5, 1.2, 5))));
  }
};

const buildScorePanels = (box: number, sphere: number, m: Map<MaterialName, number>, digit: number): void => {
  // Screen-left (+X): the score panel with glowing digit bars; screen-right: B/S/O dots.
  spawnRenderable(box, m.get("PanelNavy")!, xform(vec3(4.7, 0.045, -2.7), boxScale(vec3(3.4, 0.08, 2.1))));
  spawnRenderable(box, m.get("Line")!, xform(vec3(4.7, 0.05, -1.78), boxScale(vec3(3.4, 0.09, 0.18))));
  for (let k = 0; k < 2; k += 1) {
    spawnRenderable(box, digit, xform(vec3(5.25 - k * 1.15, 0.1, -2.85), boxScale(vec3(0.62, 0.02, 1.05))));
  }
  spawnRenderable(box, m.get("PanelNavy")!, xform(vec3(-4.7, 0.045, -2.7), boxScale(vec3(3.4, 0.08, 2.1))));
  const dotRows: readonly (readonly [MaterialName, number])[] = [
    ["DotBlue", -2.15],
    ["DotYellow", -2.7],
    ["DotRed", -3.25],
  ];
  for (const [mat, rz] of dotRows) {
    spawnRenderable(box, m.get("Line")!, xform(vec3(-3.6, 0.09, rz), boxScale(vec3(0.3, 0.02, 0.3))));
    for (let k = 0; k < 3; k += 1) {
      spawnRenderable(sphere, m.get(mat)!, xform(vec3(-4.35 - k * 0.62, 0.11, rz), sphereScale(0.13)));
    }
  }
};

const buildPatrolCircles = (cyl: number, m: Map<MaterialName, number>): void => {
  for (const spot of C.FIELDER_SPOTS) {
    const infield = spot.z < 13.5;
    const mat = infield ? m.get("PatrolDirt")! : m.get("PatrolGreen")!;
    const y = infield ? 0.062 : 0.026;
    const d = spot.radius * 1.9;
    spawnRenderable(cyl, mat, xform(vec3(spot.x, y, spot.z), vec3(d, 0.015, d)));
  }
};

// ── dynamic actors ────────────────────────────────────────────────────────────

interface MachineNodes {
  readonly body: Entity;
  readonly barrel: Entity;
  readonly hopper: Entity;
  readonly flash: Entity;
}

const buildMachine = (box: number, cyl: number, sphere: number, m: Map<MaterialName, number>, flashMat: number): MachineNodes => {
  const mz = C.MOUND.z;
  spawnRenderable(box, m.get("MachineDark")!, xform(vec3(0, 0.28, mz), boxScale(vec3(1.15, 0.26, 0.9))));
  for (const side of [1, -1]) {
    spawnRenderable(cyl, m.get("MachineDark")!, xform(vec3(side * 0.62, 0.3, mz), vec3(0.4, 0.14, 0.4), quatFromEulerXyz(0, 0, Math.PI / 2)));
  }
  const body = spawnRenderable(box, m.get("MachineOrange")!, xform(vec3(0, 0.72, mz), boxScale(vec3(0.9, 0.62, 0.78))));
  const barrel = spawnRenderable(cyl, m.get("MachineDark")!, xform(vec3(0, 1.16, mz - 0.35), vec3(0.26, 1.1, 0.26), quatFromEulerXyz(Math.PI / 2, 0, 0)));
  // A bright muzzle ring so the launcher's mouth (the thing to watch) pops.
  spawnRenderable(cyl, m.get("BaseWhite")!, xform(vec3(0, 1.16, mz - 0.88), vec3(0.3, 0.06, 0.3), quatFromEulerXyz(Math.PI / 2, 0, 0)));
  const hopper = spawnRenderable(box, m.get("MachineOrange")!, xform(vec3(0, 1.14, mz + 0.42), boxScale(vec3(0.56, 0.34, 0.46))));
  const flash = spawnRenderable(sphere, flashMat, parked());
  return { barrel, body, flash, hopper };
};

interface FigureNodes {
  readonly puck: Entity;
  readonly legL: Entity;
  readonly legR: Entity;
  readonly hips: Entity;
  readonly torso: Entity;
  readonly armL: Entity;
  readonly armR: Entity;
  readonly head: Entity;
  readonly cap: Entity;
}

const buildFielderFigure = (box: number, cyl: number, sphere: number, m: Map<MaterialName, number>): FigureNodes => ({
  armL: spawnRenderable(box, m.get("FielderCap")!, parked()),
  armR: spawnRenderable(box, m.get("FielderCap")!, parked()),
  cap: spawnRenderable(sphere, m.get("FielderCap")!, parked()),
  head: spawnRenderable(sphere, m.get("FielderWhite")!, parked()),
  hips: spawnRenderable(box, m.get("FielderWhite")!, parked()),
  legL: spawnRenderable(box, m.get("FielderWhite")!, parked()),
  legR: spawnRenderable(box, m.get("FielderWhite")!, parked()),
  puck: spawnRenderable(cyl, m.get("FielderBase")!, parked()),
  torso: spawnRenderable(box, m.get("FielderWhite")!, parked()),
});

const buildBatterFigure = (box: number, cyl: number, sphere: number, m: Map<MaterialName, number>): FigureNodes => ({
  armL: spawnRenderable(box, m.get("BatterBlue")!, parked()),
  armR: spawnRenderable(box, m.get("BatterBlue")!, parked()),
  cap: spawnRenderable(sphere, m.get("BatterHelmet")!, parked()),
  head: spawnRenderable(sphere, m.get("BatterHelmet")!, parked()),
  hips: spawnRenderable(box, m.get("BatterBlue")!, parked()),
  legL: spawnRenderable(box, m.get("BatterBlue")!, parked()),
  legR: spawnRenderable(box, m.get("BatterBlue")!, parked()),
  puck: spawnRenderable(cyl, m.get("BatterPuck")!, parked()),
  torso: spawnRenderable(box, m.get("BatterBlue")!, parked()),
});

const TRAIL_POOL = 14;

/** The oversized bat's stepped taper: [innerR, outerR, width] per segment — thin
 * handle, fat barrel, fattest at the very tip. */
const BAT_SEGMENTS: readonly (readonly [number, number, number])[] = [
  [C.BAT_GRIP_R, C.BAT_BARREL_R, C.BAT_HANDLE_W],
  [C.BAT_BARREL_R, (C.BAT_BARREL_R + C.BAT_TIP_R) / 2, C.BAT_BARREL_W],
  [(C.BAT_BARREL_R + C.BAT_TIP_R) / 2, C.BAT_TIP_R, C.BAT_TIP_W],
];

export interface SceneHandles {
  readonly batter: FigureNodes;
  readonly bat: readonly Entity[];
  readonly batKnob: Entity;
  readonly machine: MachineNodes;
  readonly fielders: readonly FigureNodes[];
  readonly ball: Entity;
  readonly ballShadow: Entity;
  readonly trail: readonly Entity[];
  readonly impactRing: Entity;
}

/** Build the whole stadium, set the lights, and return the dynamic handles. */
export const buildScene = (): SceneHandles => {
  clearScene();
  // Daylight horizon: the clear colour is also what depth fog fades toward.
  setClearColor([0.62, 0.72, 0.95, 1]);
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const cyl = createMesh("cylinder");
  const mats = buildMaterials();
  const m = mats.base;

  buildGround(box, cyl, m);
  buildStadium(box, m);
  buildScorePanels(box, sphere, m, mats.digit);
  buildPatrolCircles(cyl, m);

  const machine = buildMachine(box, cyl, sphere, m, mats.flash);
  const batter = buildBatterFigure(box, cyl, sphere, m);
  const bat = BAT_SEGMENTS.map(() => spawnRenderable(box, mats.bat, parked()));
  const batKnob = spawnRenderable(box, m.get("BatKnob")!, parked());
  const fielders = C.FIELDER_SPOTS.map(() => buildFielderFigure(box, cyl, sphere, m));
  const ball = spawnRenderable(sphere, m.get("BallWhite")!, parked());
  const ballShadow = spawnRenderable(cyl, mats.shadow, parked());
  const trail = Array.from({ length: TRAIL_POOL }, () => spawnRenderable(sphere, mats.trail, parked()));
  const impactRing = spawnRenderable(sphere, mats.impact, parked());

  // NOTE: a directional light's `direction` is authored as the direction the
  // light TRAVELS (the engine negates it into the frame's to-light vector).
  // The SUN: a strong, warm, high key — the daylight the toy stadium sits in.
  addLight({ color: [1, 0.97, 0.88, 1], direction: sdk(vec3(-0.25, -0.9, 0.3)), intensity: 3.6, kind: "directional" });
  addLight({ color: [0.75, 0.82, 1, 1], direction: sdk(vec3(0.5, -0.55, -0.35)), intensity: 1.2, kind: "directional" });
  addLight({ color: [0.85, 0.87, 0.95, 1], direction: sdk(vec3(0, 1, 0.1)), intensity: 0.8, kind: "directional" });

  return { ball, ballShadow, bat, batKnob, batter, fielders, impactRing, machine, trail };
};

// ── per-frame dynamic update ──────────────────────────────────────────────────

const applyCamera = (view: SceneView): void => {
  setCamera3D({
    far: C.CAMERA_FAR,
    fovY: C.CAMERA_FOV_Y,
    near: C.CAMERA_NEAR,
    position: sdk(view.cameraPos),
    target: sdk(view.cameraTarget),
  });
};

/** The bat's visual raise (radians) per swing state — cocked up when armed, flat mid-strike. */
const batTilt = (state: SwingState, readiness: number): number => {
  if (state === "swing" || state === "follow") {
    return 0.1;
  }
  // Rewind eases the bat back up as the batter re-arms; ready holds it cocked.
  return mix(0.1, 0.68, readiness);
};

const applyBatter = (h: SceneHandles, view: SceneView): void => {
  const bx = view.batterX;
  const bz = C.BATTER_Z;
  const s = view.swing;
  // The stance coil: fully wound while armed, whipped open through the strike.
  const coil = s.state === "swing" || s.state === "follow" ? 0 : s.readiness;
  // Torso twist follows the bat backward when coiled and whips through the strike.
  const twist = clamp01(1 - Math.abs(s.theta - C.THETA_SWEET) / 2.4);
  const yawAngle = mix(-0.55, 0.5, twist) + coil * -0.35;
  const crouch = coil * 0.07;
  const yaw = quatFromEulerXyz(0, yawAngle, coil * 0.12);

  // A wide, planted athletic stance: feet apart along the pitch line, knees bent.
  show(h.batter.puck, xform(vec3(bx, 0.16, bz), vec3(1.05, 0.12, 0.78)));
  show(h.batter.legL, xform(vec3(bx, 0.42 - crouch * 0.5, bz - 0.16), boxScale(vec3(0.17, 0.42 - crouch, 0.17))));
  show(h.batter.legR, xform(vec3(bx, 0.42 - crouch * 0.5, bz + 0.16), boxScale(vec3(0.17, 0.42 - crouch, 0.17))));
  show(h.batter.hips, xform(vec3(bx, 0.68 - crouch, bz), boxScale(vec3(0.3, 0.18, 0.42)), yaw));
  show(h.batter.torso, xform(vec3(bx, 0.98 - crouch, bz), boxScale(vec3(0.32, 0.46, 0.4)), yaw));
  show(h.batter.head, xform(vec3(bx, 1.4 - crouch, bz), sphereScale(0.14)));
  show(h.batter.cap, xform(vec3(bx, 1.47 - crouch, bz), sphereScale(0.15)));

  // Bat: pivot at the hands; each stepped segment tilts up about the grip, then
  // yaws with θ — thin handle out to the oversized barrel, widest at the tip.
  const tilt = batTilt(s.state, s.readiness);
  const d = batDir(s.theta);
  const pivotY = batPlaneY(s.theta) + 0.05;
  const reach = Math.cos(tilt);
  const rot = quatFromEulerXyz(0, s.theta + Math.PI / 2, tilt);
  for (let i = 0; i < h.bat.length; i += 1) {
    const [r0, r1, w] = BAT_SEGMENTS[i]!;
    const rc = (r0 + r1) / 2;
    const center = vec3(bx + d.x * rc * reach, pivotY + Math.sin(tilt) * rc, bz + d.z * rc * reach);
    show(h.bat[i]!, xform(center, boxScale(vec3(r1 - r0 + 0.02, w, w)), rot));
  }
  show(h.batKnob, xform(vec3(bx + d.x * C.BAT_GRIP_R, pivotY, bz + d.z * C.BAT_GRIP_R), boxScale(vec3(0.15, 0.15, 0.15)), rot));

  // Arms reach from the shoulders toward the grip.
  const handX = bx + d.x * (C.BAT_GRIP_R + 0.12) * reach;
  const handZ = bz + d.z * (C.BAT_GRIP_R + 0.12) * reach;
  for (const [node, sideX] of [
    [h.batter.armL, -0.2],
    [h.batter.armR, 0.2],
  ] as const) {
    const ax = mix(bx + sideX, handX, 0.55);
    const az = mix(bz, handZ, 0.55);
    show(node, xform(vec3(ax, 1.02 - crouch, az), boxScale(vec3(0.11, 0.3, 0.11)), yaw));
  }
};

const applyMachine = (h: SceneHandles, view: SceneView): void => {
  const mz = C.MOUND.z;
  const squash = 1 - 0.2 * view.windup;
  const recoil = 0.26 * view.windup - 0.34 * view.muzzleFlash;
  show(h.machine.body, xform(vec3(0, 0.41 + 0.62 * squash * 0.5, mz), boxScale(vec3(0.9 * (1 + 0.12 * view.windup), 0.62 * squash, 0.78))));
  show(h.machine.barrel, xform(vec3(0, 1.16, mz - 0.35 + recoil), vec3(0.26, 1.1, 0.26), quatFromEulerXyz(Math.PI / 2, 0, 0)));
  show(h.machine.hopper, xform(vec3(0, 0.98 + 0.16 * squash, mz + 0.42), boxScale(vec3(0.56, 0.34, 0.46))));
  // Launch cue: a blink as the wind-up crests, then the muzzle flash at release.
  const blink = view.windup > 0.82 ? 0.12 + 0.07 * Math.sin(view.tick * 0.9) : 0;
  const flash = Math.max(view.muzzleFlash * 0.4, blink);
  show(h.machine.flash, flash > 0.01 ? xform(vec3(0, 1.16, mz - 0.95), sphereScale(flash)) : null);
};

/** Ground height under XZ (infield dirt sits higher than the striped grass). */
const groundYAt = (x: number, z: number): number => {
  const onInfieldDirt = Math.abs(x) + Math.abs(z - 7.5) <= 7.6;
  const onHomeCircle = Math.hypot(x, z) <= 2.7;
  return onHomeCircle ? 0.1 : onInfieldDirt ? 0.066 : 0.03;
};

const applyBall = (h: SceneHandles, view: SceneView): void => {
  if (!view.ballVisible) {
    show(h.ball, null);
    show(h.ballShadow, null);
  } else {
    show(h.ball, xform(view.ball, sphereScale(C.BALL_RADIUS * 1.15)));
    const gy = groundYAt(view.ball.x, view.ball.z);
    const s = 0.36 * (1 - clamp01(view.ball.y / 14) * 0.6);
    show(h.ballShadow, xform(vec3(view.ball.x, gy + 0.006, view.ball.z), vec3(s, 0.01, s)));
  }
  const n = view.trail.length;
  for (let i = 0; i < h.trail.length; i += 1) {
    const idx = n - h.trail.length + i;
    const p = idx >= 0 ? view.trail[idx] : undefined;
    const fade = (i + 1) / h.trail.length;
    show(h.trail[i]!, view.ballInPlay && p !== undefined ? xform(p, sphereScale(0.04 + 0.09 * fade)) : null);
  }
  // A brief expanding ring at the contact point on strong hits.
  const f = view.impactFlash;
  const anchor = view.trail.length > 0 ? view.trail[0]! : view.ball;
  show(h.impactRing, f > 0.02 && view.ballInPlay ? xform(anchor, sphereScale(0.2 + (1 - f) * 0.9)) : null);
};

const applyFielders = (h: SceneHandles, view: SceneView): void => {
  for (let i = 0; i < h.fielders.length; i += 1) {
    const f = view.fielders[i]!;
    const nodes = h.fielders[i]!;
    const bob = Math.abs(Math.sin(view.tick * (f.chasing ? 0.24 : 0.11) + i * 1.7)) * (f.chasing ? 0.07 : 0.035);
    const lean = f.chasing ? 0.18 : 0;
    const rot = quatFromEulerXyz(lean, 0, 0);
    const x = f.x;
    const z = f.z;
    show(nodes.puck, xform(vec3(x, 0.07, z), vec3(0.95, 0.12, 0.68)));
    show(nodes.legL, xform(vec3(x - 0.09, 0.3, z), boxScale(vec3(0.13, 0.34, 0.15))));
    show(nodes.legR, xform(vec3(x + 0.09, 0.3, z), boxScale(vec3(0.13, 0.34, 0.15))));
    show(nodes.hips, xform(vec3(x, 0.52 + bob, z), boxScale(vec3(0.28, 0.14, 0.19)), rot));
    show(nodes.torso, xform(vec3(x, 0.74 + bob, z), boxScale(vec3(0.3, 0.32, 0.2)), rot));
    show(nodes.armL, xform(vec3(x - 0.2, 0.74 + bob, z), boxScale(vec3(0.09, 0.28, 0.09)), rot));
    show(nodes.armR, xform(vec3(x + 0.2, 0.74 + bob, z), boxScale(vec3(0.09, 0.28, 0.09)), rot));
    show(nodes.head, xform(vec3(x, 1.02 + bob, z + lean * 0.1), sphereScale(0.11)));
    show(nodes.cap, xform(vec3(x, 1.08 + bob, z + lean * 0.1), sphereScale(0.12)));
  }
};

/** Move every dynamic node to match the session view. Called once per rendered frame. */
export const applyFrame = (h: SceneHandles, view: SceneView): void => {
  applyCamera(view);
  applyBatter(h, view);
  applyMachine(h, view);
  applyBall(h, view);
  applyFielders(h, view);
};
