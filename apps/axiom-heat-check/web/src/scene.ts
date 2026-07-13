/*
 * scene.ts — the ONE gameplay file that touches the engine. It builds every visible
 * thing procedurally (no external assets) through the SDK's 3D scene surface, sets the
 * fixed elevated camera + light rig, and each frame moves the dynamic nodes to match
 * the SDK-free `SceneView` the session hands it: the two symbolic figures (player +
 * defender), the dribbling / arcing ball, the heat glow, the shot trail, the rhythm
 * meter under the player's feet, the hoop make-flash, and the crowd/court heat pulse.
 *
 * Mesh conventions (as in the sibling swipe-basketball demo): the `box` mesh is a UNIT
 * CUBE (scale = full extents); the `sphere` mesh is UNIT DIAMETER (scale = 2·radius).
 * The rim + net ring have no primitive, so they're generated as real meshes
 * (`meshgen.ts`). A node's material is fixed at spawn, so the heat glow / pulse are
 * animated by scaling emissive nodes, not by re-coloring.
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
import { type Quat, type Vec3, IDENTITY_QUAT, add, clamp, mix, quatFromEulerXyz, vec3 } from "./vec.ts";
import { torusY } from "./meshgen.ts";
import type { SceneView } from "./types.ts";
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

// ── palette ───────────────────────────────────────────────────────────────────

const PALETTE = {
  Backboard: [0.94, 0.95, 1, 1],
  BallOrange: [0.98, 0.5, 0.16, 1],
  BallSeam: [0.1, 0.06, 0.04, 1],
  Court: [0.14, 0.17, 0.26, 1],
  CrowdDark: [0.06, 0.07, 0.12, 1],
  DefenderMain: [0.5, 0.12, 0.16, 1],
  DefenderAccent: [0.82, 0.22, 0.24, 1],
  Lane: [0.2, 0.24, 0.36, 1],
  Line: [0.86, 0.9, 0.98, 1],
  Net: [0.9, 0.92, 0.96, 1],
  PlayerMain: [0.2, 0.62, 0.86, 1],
  PlayerAccent: [0.86, 0.92, 0.98, 1],
  Rim: [1, 0.44, 0.14, 1],
} as const;

type MaterialName = keyof typeof PALETTE;

interface Materials {
  readonly base: Map<MaterialName, number>;
  readonly glow: number;
  readonly trail: number;
  readonly flash: number;
  readonly pulse: number;
  readonly rhythmTrack: number;
  readonly rhythmPerfect: number;
  readonly rhythmMarker: number;
  readonly contest: number;
  readonly openWindow: number;
}

const buildMaterials = (): Materials => {
  const base = new Map<MaterialName, number>();
  for (const name of Object.keys(PALETTE) as MaterialName[]) {
    base.set(name, createMaterial({ baseColor: PALETTE[name] as Rgba }));
  }
  return {
    base,
    contest: createMaterial({ baseColor: [0.85, 0.2, 0.22, 1], emissive: [0.5, 0.08, 0.08, 1], opacity: 0.26 }),
    flash: createMaterial({ baseColor: [1, 0.9, 0.5, 1], emissive: [1, 0.75, 0.3, 1] }),
    openWindow: createMaterial({ baseColor: [0.35, 1, 0.55, 1], emissive: [0.25, 0.9, 0.42, 1], opacity: 0.3 }),
    glow: createMaterial({ baseColor: [1, 0.55, 0.2, 1], emissive: [1, 0.5, 0.15, 1], opacity: 0.5 }),
    pulse: createMaterial({ baseColor: [1, 0.4, 0.1, 1], emissive: [1, 0.42, 0.12, 1] }),
    rhythmMarker: createMaterial({ baseColor: [1, 0.95, 0.6, 1], emissive: [1, 0.9, 0.5, 1] }),
    rhythmPerfect: createMaterial({ baseColor: [0.4, 1, 0.6, 1], emissive: [0.3, 0.9, 0.45, 1] }),
    rhythmTrack: createMaterial({ baseColor: [0.16, 0.2, 0.3, 1] }),
    trail: createMaterial({ baseColor: [1, 0.7, 0.3, 1], emissive: [1, 0.6, 0.2, 1] }),
  };
};

// ── the symbolic figure (player / defender) ───────────────────────────────────

interface Figure {
  readonly head: Entity;
  readonly torso: Entity;
  readonly hips: Entity;
  readonly armL: Entity;
  readonly armR: Entity;
  readonly legL: Entity;
  readonly legR: Entity;
  readonly z: number;
}

const makeFigure = (box: number, sphere: number, main: number, accent: number, z: number): Figure => ({
  armL: spawnRenderable(box, accent, parked()),
  armR: spawnRenderable(box, accent, parked()),
  head: spawnRenderable(sphere, accent, parked()),
  hips: spawnRenderable(box, main, parked()),
  legL: spawnRenderable(box, main, parked()),
  legR: spawnRenderable(box, main, parked()),
  torso: spawnRenderable(box, main, parked()),
  z,
});

/** Pose a figure at lateral `x`, leaning by `lean` (-1..1), mid-shot by `pose` (0..1). */
const poseFigure = (fig: Figure, x: number, lean: number, pose: number, stumble: number): void => {
  const leanTotal = lean * 0.22 + stumble;
  const rot = quatFromEulerXyz(0, 0, -leanTotal);
  const legBend = pose <= 0.3 ? pose / 0.3 : Math.max(0, 1 - (pose - 0.3) / 0.5);
  const bodyY = -0.2 * legBend - stumble * 0.22;
  const armRaise = clamp((pose - 0.1) / 0.35, 0, 1);
  // Lean shifts upper parts horizontally (small-angle rotate-about-feet).
  const wx = (bx: number, by: number): number => x + bx - by * Math.sin(leanTotal);

  show(fig.head, xform(vec3(wx(0, 1.62), 1.62 + bodyY, fig.z), sphereScale(0.16), rot));
  show(fig.torso, xform(vec3(wx(0, 1.15), 1.15 + bodyY, fig.z), boxScale(vec3(0.42, 0.6, 0.26)), rot));
  show(fig.hips, xform(vec3(wx(0, 0.78), 0.78 + bodyY, fig.z), boxScale(vec3(0.4, 0.3, 0.26)), rot));

  const legY = 0.4 - 0.12 * legBend;
  const legH = 0.74 - 0.12 * legBend;
  show(fig.legL, xform(vec3(x - 0.12, legY, fig.z - 0.02), boxScale(vec3(0.16, legH, 0.18)), rot));
  show(fig.legR, xform(vec3(x + 0.12, legY, fig.z + 0.02), boxScale(vec3(0.16, legH, 0.18)), rot));

  // Shooting arm (right) rises and reaches forward on release; guide arm follows less.
  const ayR = mix(1.15, 1.74, armRaise);
  const axR = mix(0.34, 0.12, armRaise);
  const azR = mix(0, 0.2, armRaise);
  show(fig.armR, xform(vec3(wx(axR, ayR), ayR + bodyY, fig.z + azR), boxScale(vec3(0.14, mix(0.55, 0.5, armRaise), 0.14)), rot));
  const ayL = mix(1.15, 1.52, armRaise * 0.6);
  show(fig.armL, xform(vec3(wx(-mix(0.34, 0.18, armRaise * 0.6), ayL), ayL + bodyY, fig.z + azR * 0.5), boxScale(vec3(0.14, 0.5, 0.14)), rot));
};

// ── pulse panels (crowd/court heat pulse) ─────────────────────────────────────

interface PulsePanel {
  readonly entity: Entity;
  readonly base: Vec3;
  readonly size: Vec3;
}

export interface SceneHandles {
  readonly player: Figure;
  readonly defender: Figure;
  readonly ball: Entity;
  readonly ballSeam: Entity;
  readonly glow: Entity;
  readonly trail: readonly Entity[];
  readonly hoopFlash: Entity;
  readonly rhythmTrack: Entity;
  readonly rhythmPerfect: Entity;
  readonly rhythmMarker: Entity;
  readonly pulsePanels: readonly PulsePanel[];
  readonly courtGlow: Entity;
  readonly contestDisc: Entity;
  readonly openRing: Entity;
}

const TRAIL_POOL = 16;

// ── static build ──────────────────────────────────────────────────────────────

const buildCourt = (box: number, mats: Materials): void => {
  const m = mats.base;
  spawnRenderable(box, m.get("Court")!, xform(vec3(0, -0.05, 6.5), boxScale(vec3(16, 0.1, 22))));
  spawnRenderable(box, m.get("Lane")!, xform(vec3(0, 0.005, 10.6), boxScale(vec3(3.6, 0.02, 5.8))));
  // Key + baselines + half line, as thin flat boxes.
  spawnRenderable(box, m.get("Line")!, xform(vec3(0, 0.012, C.HOOP_Z + 0.5), boxScale(vec3(12, 0.02, 0.12))));
  spawnRenderable(box, m.get("Line")!, xform(vec3(0, 0.012, 1.2), boxScale(vec3(12, 0.02, 0.12))));
  spawnRenderable(box, m.get("Line")!, xform(vec3(0, 0.012, 7.8), boxScale(vec3(3.6, 0.02, 0.12))));
  for (const sx of [-1.8, 1.8]) {
    spawnRenderable(box, m.get("Line")!, xform(vec3(sx, 0.012, 10.6), boxScale(vec3(0.1, 0.02, 5.8))));
  }
  // Simplified 3-point arc, small boxes on a semicircle centered under the hoop.
  const arcR = 6.4;
  const segs = 20;
  for (let k = 0; k <= segs; k += 1) {
    const a = mix(-1.2, 1.2, k / segs);
    spawnRenderable(box, m.get("Line")!, xform(vec3(Math.sin(a) * arcR, 0.012, C.HOOP_Z - Math.cos(a) * arcR), boxScale(vec3(0.18, 0.02, 0.18))));
  }
};

const buildCrowd = (box: number, mats: Materials): void => {
  const m = mats.base;
  // Back wall behind the hoop.
  spawnRenderable(box, m.get("CrowdDark")!, xform(vec3(0, 3.2, C.HOOP_Z + 3.4), boxScale(vec3(22, 9, 0.6))));
  // Flanking silhouettes on both sidelines, a few rows deep.
  for (const side of [-1, 1]) {
    for (let row = 0; row < 3; row += 1) {
      for (let i = 0; i < 8; i += 1) {
        const x = side * (8.4 + row * 0.95);
        const z = 1.5 + i * 1.7;
        const y = 1 + row * 0.55;
        spawnRenderable(box, m.get("CrowdDark")!, xform(vec3(x, y, z), boxScale(vec3(0.5, 1.2, 0.5))));
      }
    }
  }
};

const buildHoop = (box: number, mats: Materials): void => {
  const m = mats.base;
  spawnRenderable(box, m.get("Backboard")!, xform(vec3(0, C.BACKBOARD_Y, C.HOOP_Z + 0.4), boxScale(vec3(C.BACKBOARD_HALF_W * 2, C.BACKBOARD_HALF_H * 2, C.BACKBOARD_HALF_D * 2))));
  spawnRenderable(box, m.get("Rim")!, xform(vec3(0, C.BACKBOARD_Y - 0.02, C.HOOP_Z + 0.38), boxScale(vec3(0.34, 0.22, 0.02))));
  // Post.
  spawnRenderable(box, m.get("CrowdDark")!, xform(vec3(0, 1.8, C.HOOP_Z + 0.8), boxScale(vec3(0.14, 3.6, 0.14))));
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

const buildPulsePanels = (box: number, mats: Materials): PulsePanel[] => {
  const panels: PulsePanel[] = [];
  const add_ = (base: Vec3, size: Vec3): void => {
    panels.push({ base, entity: spawnRenderable(box, mats.pulse, parked()), size });
  };
  // A back strip and two side strips that grow with the crowd pulse.
  add_(vec3(0, 7.4, C.HOOP_Z + 3.1), vec3(20, 0.4, 0.2));
  for (const side of [-1, 1]) {
    add_(vec3(side * 9.2, 2.6, 6), vec3(0.2, 0.4, 12));
  }
  return panels;
};

/** Build the whole scene, set the lights, and return the dynamic handles. */
export const buildScene = (): SceneHandles => {
  clearScene();
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const mats = buildMaterials();

  buildCourt(box, mats);
  buildCrowd(box, mats);
  buildHoop(box, mats);
  const pulsePanels = buildPulsePanels(box, mats);

  const player = makeFigure(box, sphere, mats.base.get("PlayerMain")!, mats.base.get("PlayerAccent")!, C.PLAYER_Z);
  const defender = makeFigure(box, sphere, mats.base.get("DefenderMain")!, mats.base.get("DefenderAccent")!, C.DEFENDER_Z);

  const ball = spawnRenderable(sphere, mats.base.get("BallOrange")!, parked());
  const ballSeam = spawnRenderable(sphere, mats.base.get("BallSeam")!, parked());
  const glow = spawnRenderable(sphere, mats.glow, parked());
  const trail = Array.from({ length: TRAIL_POOL }, () => spawnRenderable(sphere, mats.trail, parked()));
  const hoopFlash = spawnRenderable(sphere, mats.flash, parked());
  const rhythmTrack = spawnRenderable(box, mats.rhythmTrack, parked());
  const rhythmPerfect = spawnRenderable(box, mats.rhythmPerfect, parked());
  const rhythmMarker = spawnRenderable(box, mats.rhythmMarker, parked());
  const courtGlow = spawnRenderable(box, mats.pulse, parked());
  const contestDisc = spawnRenderable(sphere, mats.contest, parked());
  const openRing = spawnRenderable(sphere, mats.openWindow, parked());

  addLight({ color: [1, 0.96, 0.9, 1], direction: sdk(vec3(-0.3, -0.72, 0.6)), intensity: 1.7, kind: "directional" });
  addLight({ color: [0.6, 0.72, 0.95, 1], direction: sdk(vec3(0.5, -0.4, -0.5)), intensity: 0.7, kind: "directional" });
  addLight({ color: [0.7, 0.72, 0.85, 1], direction: sdk(vec3(0, 1, -0.2)), intensity: 0.65, kind: "directional" });

  return {
    ball,
    ballSeam,
    contestDisc,
    courtGlow,
    defender,
    glow,
    hoopFlash,
    openRing,
    player,
    pulsePanels,
    rhythmMarker,
    rhythmPerfect,
    rhythmTrack,
    trail,
  };
};

// ── per-frame dynamic update ──────────────────────────────────────────────────

const applyCamera = (shake: Vec3): void => {
  setCamera3D({
    far: C.CAMERA_FAR,
    fovY: C.CAMERA_FOV_Y,
    near: C.CAMERA_NEAR,
    position: sdk(add(C.CAMERA_POS, shake)),
    target: sdk(add(C.CAMERA_TARGET, shake)),
  });
};

const applyBall = (h: SceneHandles, view: SceneView): void => {
  show(h.ball, xform(view.ball, sphereScale(C.BALL_RADIUS)));
  show(h.ballSeam, xform(view.ball, sphereScale(C.BALL_RADIUS * 1.04)));
};

const applyGlow = (h: SceneHandles, view: SceneView): void => {
  const g = view.glow;
  show(h.glow, g > 0 ? xform(vec3(view.playerX, 1.1, C.PLAYER_Z), sphereScale(0.7 + g * 0.9)) : null);
};

const applyTrail = (h: SceneHandles, view: SceneView): void => {
  const n = view.trail.length;
  for (let i = 0; i < h.trail.length; i += 1) {
    const idx = n - h.trail.length + i;
    const p = idx >= 0 ? view.trail[idx] : undefined;
    const fade = (i + 1) / h.trail.length;
    const size = (0.06 + 0.1 * fade) * (1 + view.heat * 0.12);
    show(h.trail[i]!, view.ballInFlight && p !== undefined ? xform(p, sphereScale(size)) : null);
  }
};

const applyRhythm = (h: SceneHandles, view: SceneView): void => {
  if (!view.rhythmActive) {
    show(h.rhythmTrack, null);
    show(h.rhythmPerfect, null);
    show(h.rhythmMarker, null);
    return;
  }
  const y = 0.03;
  const z = C.PLAYER_Z - 0.9;
  const halfW = 1;
  show(h.rhythmTrack, xform(vec3(view.playerX, y, z), boxScale(vec3(halfW * 2, 0.03, 0.16))));
  show(h.rhythmPerfect, xform(vec3(view.playerX, y + 0.005, z), boxScale(vec3(C.RHYTHM_PERFECT_HALF * 4 * halfW, 0.04, 0.2))));
  const markerX = view.playerX + mix(-halfW, halfW, view.rhythmPhase);
  show(h.rhythmMarker, xform(vec3(markerX, y + 0.01, z), boxScale(vec3(0.08, 0.08, 0.26))));
};

const applyPulse = (h: SceneHandles, view: SceneView): void => {
  const p = view.pulse;
  for (const panel of h.pulsePanels) {
    show(panel.entity, xform(panel.base, boxScale(vec3(panel.size.x, panel.size.y * (1 + p * 3), panel.size.z))));
  }
  show(h.courtGlow, p > 0.03 ? xform(vec3(0, 0.02, 7), boxScale(vec3(6 + p * 6, 0.02, 8 + p * 6))) : null);
};

const applyHoopFlash = (h: SceneHandles, view: SceneView): void => {
  const f = view.scoreFlash;
  show(h.hoopFlash, f > 0 ? xform(vec3(0, C.HOOP_Y, C.HOOP_Z), sphereScale(0.3 + f * 0.5)) : null);
};

// The defender's contest zone — a faint flat disc that shrinks when they're off balance,
// so the player can read at a glance whether they're inside the contest (readiness drops).
const applyContest = (h: SceneHandles, view: SceneView): void => {
  const r = view.contestRadius;
  show(h.contestDisc, xform(vec3(view.defenderX, 0.04, C.DEFENDER_Z), vec3(r * 2, 0.06, r * 2)));
};

// A green "open window" ring under the player while a real advantage window is open,
// growing with the advantage — the cue to shoot NOW before it closes.
const applyWindow = (h: SceneHandles, view: SceneView): void => {
  const d = 1.6 + view.advantage * 1.4;
  show(h.openRing, view.windowActive ? xform(vec3(view.playerX, 0.05, C.PLAYER_Z), vec3(d, 0.05, d)) : null);
};

/** Move every dynamic node to match the session view. Called once per rendered frame. */
export const applyFrame = (h: SceneHandles, view: SceneView): void => {
  applyCamera(view.cameraShake);
  applyContest(h, view);
  applyWindow(h, view);
  poseFigure(h.player, view.playerX, view.playerLean, view.shotPose, 0);
  poseFigure(h.defender, view.defenderX, 0, 0, (1 - view.defenderBalance) * 0.5);
  applyBall(h, view);
  applyGlow(h, view);
  applyTrail(h, view);
  applyRhythm(h, view);
  applyPulse(h, view);
  applyHoopFlash(h, view);
};
