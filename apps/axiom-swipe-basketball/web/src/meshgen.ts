/*
 * meshgen.ts — small procedural mesh builders (a torus, and a mesh-merge). Kept
 * SDK-free by producing plain vertex arrays in the SDK's `MeshData` SHAPE without
 * importing the SDK; `scene.ts` hands the result straight to `createMeshData`. This
 * is how the hoop rim (a real torus, since the primitive catalog has none) and the
 * basketball's dark seam rings are generated — no external assets.
 */

import { type Quat, type Vec3, quatRotate, vec3 } from "./vec.ts";

/** Plain vertex geometry, structurally the SDK's `MeshData`. */
export interface Geometry {
  positions: Vec3[];
  normals: Vec3[];
  indices: number[];
}

/** A torus about the +Y axis (ring in the XZ plane), centred at the origin. */
export const torusY = (majorRadius: number, minorRadius: number, majorSegs: number, minorSegs: number): Geometry => {
  const positions: Vec3[] = [];
  const normals: Vec3[] = [];
  const indices: number[] = [];
  for (let i = 0; i <= majorSegs; i += 1) {
    const u = (2 * Math.PI * i) / majorSegs;
    const cu = Math.cos(u);
    const su = Math.sin(u);
    const radial = vec3(cu, 0, su);
    for (let j = 0; j <= minorSegs; j += 1) {
      const v = (2 * Math.PI * j) / minorSegs;
      const cv = Math.cos(v);
      const sv = Math.sin(v);
      const normal = vec3(cv * radial.x, sv, cv * radial.z);
      positions.push(vec3(
        (majorRadius + minorRadius * cv) * radial.x,
        minorRadius * sv,
        (majorRadius + minorRadius * cv) * radial.z,
      ));
      normals.push(normal);
    }
  }
  const stride = minorSegs + 1;
  for (let i = 0; i < majorSegs; i += 1) {
    for (let j = 0; j < minorSegs; j += 1) {
      const a = i * stride + j;
      const b = (i + 1) * stride + j;
      const c = (i + 1) * stride + (j + 1);
      const d = i * stride + (j + 1);
      indices.push(a, b, c, a, c, d);
    }
  }
  return { indices, normals, positions };
};

/** Rotate a whole geometry (positions + normals) by a quaternion. */
export const rotateGeometry = (geo: Geometry, q: Quat): Geometry => ({
  indices: geo.indices,
  normals: geo.normals.map((n) => quatRotate(q, n)),
  positions: geo.positions.map((p) => quatRotate(q, p)),
});

/** Merge several geometries into one, offsetting indices as vertices concatenate. */
export const mergeGeometry = (parts: readonly Geometry[]): Geometry => {
  const positions: Vec3[] = [];
  const normals: Vec3[] = [];
  const indices: number[] = [];
  for (const part of parts) {
    const base = positions.length;
    positions.push(...part.positions);
    normals.push(...part.normals);
    for (const idx of part.indices) {
      indices.push(idx + base);
    }
  }
  return { indices, normals, positions };
};
