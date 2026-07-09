/*
 * projection.ts — a small pure-TypeScript camera-math kit. The SDK's own `mat4`
 * helpers forward to the native host (`boundHost()`), so they are no-ops in a bare
 * `node --test`; this module re-implements exactly the matrix math the game needs
 * (RH perspective + look-at, 4×4 multiply/invert, project, unproject, ray∩plane) in
 * pure f64, so on-screen ball selection and pointer-drag unprojection are both
 * deterministic AND testable. The convention (right-handed, `−Z` forward, NDC z in
 * `[−1, 1]`) mirrors the engine's `mat4Perspective`/`mat4LookAt`, so a ball's
 * projected screen position agrees with where the engine draws it; any residual is
 * absorbed by the generous selection radius and calibrated against a screenshot.
 *
 * Matrices are column-major `number[16]` (glm layout: `m[col*4 + row]`).
 */

import { type Vec2, type Vec3, cross, dot, normalize, sub, vec2, vec3 } from "./vec.ts";

export type Mat4 = readonly number[];

/** A fixed perspective camera (eye + look-at + intrinsics). */
export interface Camera {
  readonly position: Vec3;
  readonly target: Vec3;
  readonly up: Vec3;
  readonly fovY: number;
  readonly near: number;
  readonly far: number;
}

/** Right-handed perspective projection, NDC z in `[−1, 1]` (glm `perspectiveRH_NO`). */
export const perspective = (fovY: number, aspect: number, near: number, far: number): Mat4 => {
  const t = Math.tan(fovY / 2);
  const m = new Array<number>(16).fill(0);
  m[0] = 1 / (aspect * t);
  m[5] = 1 / t;
  m[10] = -(far + near) / (far - near);
  m[11] = -1;
  m[14] = -(2 * far * near) / (far - near);
  return m;
};

/** Right-handed look-at view matrix (glm `lookAtRH`). */
export const lookAt = (eye: Vec3, target: Vec3, up: Vec3): Mat4 => {
  const f = normalize(sub(target, eye));
  const s = normalize(cross(f, up));
  const u = cross(s, f);
  return [
    s.x, u.x, -f.x, 0,
    s.y, u.y, -f.y, 0,
    s.z, u.z, -f.z, 0,
    -dot(s, eye), -dot(u, eye), dot(f, eye), 1,
  ];
};

/** Column-major 4×4 multiply `a·b`. */
export const multiply = (a: Mat4, b: Mat4): Mat4 => {
  const out = new Array<number>(16).fill(0);
  for (let col = 0; col < 4; col += 1) {
    for (let row = 0; row < 4; row += 1) {
      let acc = 0;
      for (let k = 0; k < 4; k += 1) {
        acc += a[k * 4 + row]! * b[col * 4 + k]!;
      }
      out[col * 4 + row] = acc;
    }
  }
  return out;
};

/** General 4×4 inverse (adjugate / determinant). Returns the identity for a singular matrix. */
export const invert = (m: Mat4): Mat4 => {
  const a00 = m[0]!, a01 = m[1]!, a02 = m[2]!, a03 = m[3]!;
  const a10 = m[4]!, a11 = m[5]!, a12 = m[6]!, a13 = m[7]!;
  const a20 = m[8]!, a21 = m[9]!, a22 = m[10]!, a23 = m[11]!;
  const a30 = m[12]!, a31 = m[13]!, a32 = m[14]!, a33 = m[15]!;

  const b00 = a00 * a11 - a01 * a10;
  const b01 = a00 * a12 - a02 * a10;
  const b02 = a00 * a13 - a03 * a10;
  const b03 = a01 * a12 - a02 * a11;
  const b04 = a01 * a13 - a03 * a11;
  const b05 = a02 * a13 - a03 * a12;
  const b06 = a20 * a31 - a21 * a30;
  const b07 = a20 * a32 - a22 * a30;
  const b08 = a20 * a33 - a23 * a30;
  const b09 = a21 * a32 - a22 * a31;
  const b10 = a21 * a33 - a23 * a31;
  const b11 = a22 * a33 - a23 * a32;

  const det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
  if (Math.abs(det) < 1e-12) {
    return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  }
  const inv = 1 / det;
  return [
    (a11 * b11 - a12 * b10 + a13 * b09) * inv,
    (a02 * b10 - a01 * b11 - a03 * b09) * inv,
    (a31 * b05 - a32 * b04 + a33 * b03) * inv,
    (a22 * b04 - a21 * b05 - a23 * b03) * inv,
    (a12 * b08 - a10 * b11 - a13 * b07) * inv,
    (a00 * b11 - a02 * b08 + a03 * b07) * inv,
    (a32 * b02 - a30 * b05 - a33 * b01) * inv,
    (a20 * b05 - a22 * b02 + a23 * b01) * inv,
    (a10 * b10 - a11 * b08 + a13 * b06) * inv,
    (a01 * b08 - a00 * b10 - a03 * b06) * inv,
    (a30 * b04 - a31 * b02 + a33 * b00) * inv,
    (a21 * b02 - a20 * b04 - a23 * b00) * inv,
    (a11 * b07 - a10 * b09 - a12 * b06) * inv,
    (a00 * b09 - a01 * b07 + a02 * b06) * inv,
    (a31 * b01 - a30 * b03 - a32 * b00) * inv,
    (a20 * b03 - a21 * b01 + a22 * b00) * inv,
  ];
};

/** Transform a homogeneous point `(v, 1)` by `m`, returning clip `{x,y,z,w}`. */
const transformPoint = (m: Mat4, v: Vec3): { x: number; y: number; z: number; w: number } => ({
  x: m[0]! * v.x + m[4]! * v.y + m[8]! * v.z + m[12]!,
  y: m[1]! * v.x + m[5]! * v.y + m[9]! * v.z + m[13]!,
  z: m[2]! * v.x + m[6]! * v.y + m[10]! * v.z + m[14]!,
  w: m[3]! * v.x + m[7]! * v.y + m[11]! * v.z + m[15]!,
});

/** The combined view-projection for a camera at a given viewport aspect ratio. */
export const viewProjection = (camera: Camera, aspect: number): Mat4 =>
  multiply(
    perspective(camera.fovY, aspect, camera.near, camera.far),
    lookAt(camera.position, camera.target, camera.up),
  );

/** A projected screen sample: pixel position plus clip `w` (`w > 0` ⇔ in front of the camera). */
export interface Projected {
  readonly pos: Vec2;
  readonly w: number;
  /** Half a metre at the point's depth, expressed in screen pixels — a rough on-screen scale. */
  readonly pixelsPerMetre: number;
}

/** Project a world point to canvas pixels (top-left origin, y-down). */
export const project = (world: Vec3, viewProj: Mat4, viewport: Vec2): Projected => {
  const clip = transformPoint(viewProj, world);
  const w = clip.w;
  const invW = 1 / (Math.abs(w) < 1e-9 ? 1e-9 : w);
  const ndcX = clip.x * invW;
  const ndcY = clip.y * invW;
  const px = (ndcX * 0.5 + 0.5) * viewport.x;
  const py = (1 - (ndcY * 0.5 + 0.5)) * viewport.y;
  // A crude on-screen metre scale: the projection scales screen-x by 1/(aspect·tan)
  // and divides by w, so pixels-per-metre ≈ (m00 · viewport.x/2) / w.
  const pixelsPerMetre = (viewProj[0]! * viewport.x * 0.5) * invW;
  return { pixelsPerMetre: Math.abs(pixelsPerMetre), pos: vec2(px, py), w };
};

/** A world-space ray (origin + unit direction). */
export interface Ray {
  readonly origin: Vec3;
  readonly dir: Vec3;
}

/** Unproject a canvas-pixel position into a world ray, given the inverse view-projection. */
export const unprojectRay = (px: number, py: number, viewport: Vec2, invViewProj: Mat4): Ray => {
  const ndcX = (px / viewport.x) * 2 - 1;
  const ndcY = 1 - (py / viewport.y) * 2;
  const near = transformPoint(invViewProj, vec3(ndcX, ndcY, -1));
  const far = transformPoint(invViewProj, vec3(ndcX, ndcY, 1));
  const nearW = 1 / (Math.abs(near.w) < 1e-9 ? 1e-9 : near.w);
  const farW = 1 / (Math.abs(far.w) < 1e-9 ? 1e-9 : far.w);
  const n = vec3(near.x * nearW, near.y * nearW, near.z * nearW);
  const f = vec3(far.x * farW, far.y * farW, far.z * farW);
  return { dir: normalize(sub(f, n)), origin: n };
};

/** Intersect a ray with the plane `z = planeZ`; `null` if the ray is parallel to it. */
export const rayPlaneZ = (ray: Ray, planeZ: number): Vec3 | null => {
  if (Math.abs(ray.dir.z) < 1e-6) {
    return null;
  }
  const t = (planeZ - ray.origin.z) / ray.dir.z;
  return vec3(ray.origin.x + ray.dir.x * t, ray.origin.y + ray.dir.y * t, ray.origin.z + ray.dir.z * t);
};
