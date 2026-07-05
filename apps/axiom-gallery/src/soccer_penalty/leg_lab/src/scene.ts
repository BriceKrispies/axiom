/*
 * The side-on 3D scene: turn each `LegLabFrame` into engine draws through the
 * `@axiom/game` SDK. TypeScript twin of the Rust lab's `leg_lab_scene.rs`, but
 * where the Rust path re-authored the whole scene each frame, here the leg parts
 * are spawned ONCE (`buildScene`) and moved each frame with `setNodeTransform`
 * (`updateScene`) — the SDK's retained-node model.
 *
 * The leg is drawn from the same primitives as the Rust lab: thigh + shin are
 * cylinders, the foot is a box, each joint + marker is a sphere. The one
 * difference is the ground: the SDK's `createMesh` has no `plane` kind (only
 * box/cylinder/sphere), so the ground is a thin box.
 *
 * World → view: the sim advances the hip forward forever in +X; we re-centre every
 * position by the hip's X so the leg walks in place under the fixed side camera (a
 * treadmill). The planted foot therefore slides backward on screen — correct: it
 * is glued to the ground while the body moves over it.
 */

import {
  addLight,
  clearScene,
  createMaterial,
  createMesh,
  setCamera3D,
  setNodeTransform,
  spawnRenderable,
} from "@axiom/game";
import type { Entity, Rgba, Transform } from "@axiom/game";

import { type Vec3, add, length, scale, sub, vec3 } from "./vec3.ts";
import type { LegLabFrame } from "./leg-lab-sim.ts";
import type { LegRig } from "./leg-rig.ts";

/** A rotation quaternion as the SDK's `[x, y, z, w]`. */
type Quat = readonly [number, number, number, number];

const DEG_TO_RAD = Math.PI / 180;

/** Push a marker 0.16 toward the camera (+Z) so it reads as a distinct indicator in front of the leg. */
const TOWARD_CAMERA: Vec3 = vec3(0, 0, 0.16);

const rgb = (r: number, g: number, b: number): Rgba => [r, g, b, 1];

/** Rotation mapping the cylinder's local +Y axis onto unit `dir`, so a unit-height cylinder lies along a bone. */
const quatFromUnitYTo = (dir: Vec3): Quat => {
  const d = Math.min(Math.max(dir.y, -1), 1);
  // axis = cross((0,1,0), dir) = (dir.z, 0, -dir.x).
  const axis = vec3(dir.z, 0, -dir.x);
  const axisLen = length(axis);
  if (axisLen < 1e-6) {
    // Parallel to ±Y: identity when aligned, a half-turn about X when opposed.
    return d < 0 ? [1, 0, 0, 0] : [0, 0, 0, 1];
  }
  const angle = Math.acos(d);
  const s = Math.sin(angle / 2) / axisLen;
  return [axis.x * s, axis.y * s, axis.z * s, Math.cos(angle / 2)];
};

/** The transform for a bone cylinder spanning `a`→`b` (catalog cylinder: height 1, diameter 1, along +Y). */
const boneTransform = (a: Vec3, b: Vec3, diameter: number): Transform => {
  const seg = sub(b, a);
  const len = Math.max(length(seg), 1e-4);
  const dir = scale(seg, 1 / len);
  return { position: scale(add(a, b), 0.5), rotation: quatFromUnitYTo(dir), scale: vec3(diameter, len, diameter) };
};

/** The transform for a sphere of the given `diameter` centred at `at` (catalog sphere: diameter 1). */
const ballTransform = (at: Vec3, diameter: number): Transform => ({
  position: at,
  rotation: [0, 0, 0, 1],
  scale: vec3(diameter, diameter, diameter),
});

/** Average cross-section diameter of a limb render box (used to size its cylinder). */
const limbDiameter = (box: Vec3): number => (box.x + box.z) * 0.5;

/** The transform for the foot box (catalog box spans ±1, so scale is half-extents), nudged forward of the ankle. */
const footTransform = (rig: LegRig, foot: Vec3): Transform => ({
  position: add(foot, vec3(rig.footBox.z * 0.25, rig.footBox.y * 0.5, 0)),
  rotation: [0, 0, 0, 1],
  scale: vec3(rig.footBox.z * 0.5, rig.footBox.y * 0.5, rig.footBox.x * 0.5),
});

/** The leg's joint positions in VIEW space (re-centred so the hip stays at x = 0). */
interface ViewPose {
  readonly hip: Vec3;
  readonly knee: Vec3;
  readonly foot: Vec3;
  readonly target: Vec3;
}

const viewPose = (frame: LegLabFrame): ViewPose => {
  const shift = vec3(frame.hipWorld.x, 0, 0);
  return {
    hip: sub(frame.hipWorld, shift),
    knee: sub(frame.pose.knee, shift),
    foot: sub(frame.pose.foot, shift),
    target: sub(frame.rawTargetWorld, shift),
  };
};

/** The movable scene nodes plus the rig they are sized from. */
export interface LegLabScene {
  readonly rig: LegRig;
  readonly thigh: Entity;
  readonly shin: Entity;
  readonly foot: Entity;
  readonly hipJoint: Entity;
  readonly kneeJoint: Entity;
  readonly ankleJoint: Entity;
  readonly hipMarker: Entity;
  readonly targetMarker: Entity;
}

/**
 * Build the scene once: register the meshes + materials, spawn the ground and the
 * eight leg parts at `frame0`'s pose, and set the fixed side camera + key light.
 * Returns the movable node handles for `updateScene` to drive.
 */
export const buildScene = (rig: LegRig, frame0: LegLabFrame): LegLabScene => {
  clearScene();

  const cylinder = createMesh("cylinder");
  const sphere = createMesh("sphere");
  const box = createMesh("box");

  const skin = createMaterial({ baseColor: rgb(0.8, 0.6, 0.46) });
  const sock = createMaterial({ baseColor: rgb(0.16, 0.17, 0.22) });
  const boot = createMaterial({ baseColor: rgb(0.06, 0.06, 0.09) });
  const joint = createMaterial({ baseColor: rgb(0.92, 0.92, 0.86) });
  const hipMarkerMat = createMaterial({ baseColor: rgb(0.15, 0.85, 0.95) });
  const targetMarkerMat = createMaterial({ baseColor: rgb(0.98, 0.25, 0.55) });
  const ground = createMaterial({ baseColor: rgb(0.2, 0.32, 0.22) });

  // Ground: a thin box (the SDK has no plane primitive) with its top at y = 0.
  spawnRenderable(box, ground, { position: vec3(0, -0.05, 0), rotation: [0, 0, 0, 1], scale: vec3(12, 0.05, 12) });

  const v = viewPose(frame0);
  const thighD = limbDiameter(rig.thighBox);
  const shinD = limbDiameter(rig.shinBox);

  return {
    rig,
    thigh: spawnRenderable(cylinder, skin, boneTransform(v.hip, v.knee, thighD)),
    shin: spawnRenderable(cylinder, sock, boneTransform(v.knee, v.foot, shinD)),
    foot: spawnRenderable(box, boot, footTransform(rig, v.foot)),
    hipJoint: spawnRenderable(sphere, joint, ballTransform(v.hip, 0.13)),
    kneeJoint: spawnRenderable(sphere, joint, ballTransform(v.knee, 0.13)),
    ankleJoint: spawnRenderable(sphere, joint, ballTransform(v.foot, 0.13)),
    hipMarker: spawnRenderable(sphere, hipMarkerMat, ballTransform(add(v.hip, TOWARD_CAMERA), 0.12)),
    targetMarker: spawnRenderable(sphere, targetMarkerMat, ballTransform(add(v.target, TOWARD_CAMERA), 0.1)),
  };
};

/** Re-place the fixed side camera + key light. Called after `buildScene` and (defensively) each frame. */
export const setView = (): void => {
  setCamera3D({
    position: vec3(-0.15, 0.42, 2.4),
    target: vec3(-0.15, 0.36, 0),
    fovY: 42 * DEG_TO_RAD,
    near: 0.05,
    far: 50,
  });
  addLight({ kind: "directional", direction: vec3(-0.4, -0.8, -0.45), color: [1, 1, 1, 1], intensity: 1.15 });
};

/** Move every leg node to this frame's pose (the per-frame `setNodeTransform` sweep). */
export const updateScene = (scene: LegLabScene, frame: LegLabFrame): void => {
  const v = viewPose(frame);
  setNodeTransform(scene.thigh, boneTransform(v.hip, v.knee, limbDiameter(scene.rig.thighBox)));
  setNodeTransform(scene.shin, boneTransform(v.knee, v.foot, limbDiameter(scene.rig.shinBox)));
  setNodeTransform(scene.foot, footTransform(scene.rig, v.foot));
  setNodeTransform(scene.hipJoint, ballTransform(v.hip, 0.13));
  setNodeTransform(scene.kneeJoint, ballTransform(v.knee, 0.13));
  setNodeTransform(scene.ankleJoint, ballTransform(v.foot, 0.13));
  setNodeTransform(scene.hipMarker, ballTransform(add(v.hip, TOWARD_CAMERA), 0.12));
  setNodeTransform(scene.targetMarker, ballTransform(add(v.target, TOWARD_CAMERA), 0.1));
};
