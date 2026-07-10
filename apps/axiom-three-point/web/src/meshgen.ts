/*
 * meshgen.ts — procedural geometry the SDK's primitive set (box / sphere /
 * cylinder) can't express: the hoop rim + the net's gather ring, both tori.
 * SDK-free (returns a plain `MeshData`-shaped object of
 * `{positions, normals, indices}`), so it stays testable and `scene.ts` hands the
 * result straight to `createMeshData`.
 */

import { type Vec3, vec3 } from "./vec.ts";

/** A `MeshData`-shaped mesh: structurally the SDK's `createMeshData` input. */
export interface Geometry {
  readonly positions: readonly Vec3[];
  readonly normals: readonly Vec3[];
  readonly indices: readonly number[];
}

/**
 * A torus lying in the XZ plane (its axis is +Y) — the shape of a basketball rim
 * and the net's gather ring. `ringR` is the big radius, `tubeR` the tube thickness.
 */
export const torusY = (ringR: number, tubeR: number, ringSeg: number, tubeSeg: number): Geometry => {
  const positions: Vec3[] = [];
  const normals: Vec3[] = [];
  const indices: number[] = [];

  for (let i = 0; i <= ringSeg; i += 1) {
    const u = (i / ringSeg) * Math.PI * 2;
    const cu = Math.cos(u);
    const su = Math.sin(u);
    for (let j = 0; j <= tubeSeg; j += 1) {
      const v = (j / tubeSeg) * Math.PI * 2;
      const cv = Math.cos(v);
      const sv = Math.sin(v);
      const r = ringR + tubeR * cv;
      positions.push(vec3(r * cu, tubeR * sv, r * su));
      normals.push(vec3(cv * cu, sv, cv * su));
    }
  }

  const stride = tubeSeg + 1;
  for (let i = 0; i < ringSeg; i += 1) {
    for (let j = 0; j < tubeSeg; j += 1) {
      const a = i * stride + j;
      const b = a + stride;
      indices.push(a, b, a + 1, a + 1, b, b + 1);
    }
  }

  return { indices, normals, positions };
};
