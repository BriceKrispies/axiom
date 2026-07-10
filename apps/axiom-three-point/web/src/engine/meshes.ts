/*
 * engine/meshes.ts — the procedural unit primitives behind `createMesh`. Each
 * builder returns the `MeshData` triangle-list shape from `api.ts`
 * (`{positions, normals, indices}`), pure and DOM-free so it is testable under
 * bare `node --test`. Conventions (the game's scale math depends on them):
 * `unitBox` is a UNIT CUBE centered at the origin (scale = full extents, flat
 * per-face normals), `unitSphere` is UNIT DIAMETER (radius 0.5, smooth
 * normals; scale = 2·radius), and `unitCylinderY` is UNIT DIAMETER × UNIT
 * HEIGHT around +Y with caps (scale = (diameter, height, diameter); smooth
 * side normals, flat cap normals).
 */

import type { EngineVec3, MeshData } from "./api.ts";

const v3 = (x: number, y: number, z: number): EngineVec3 => ({ x, y, z });

const TAU = Math.PI * 2;

/** A unit cube centered at the origin (extents ±0.5), 24 vertices with flat
 * per-face normals, 12 triangles. */
export const unitBox = (): MeshData => {
  const positions: EngineVec3[] = [];
  const normals: EngineVec3[] = [];
  const indices: number[] = [];

  // Each face is (normal, u-axis, v-axis) with u × v = normal, so the corner
  // order below is counter-clockwise seen from outside.
  const faces: readonly (readonly [EngineVec3, EngineVec3, EngineVec3])[] = [
    [v3(1, 0, 0), v3(0, 1, 0), v3(0, 0, 1)],
    [v3(-1, 0, 0), v3(0, 0, 1), v3(0, 1, 0)],
    [v3(0, 1, 0), v3(0, 0, 1), v3(1, 0, 0)],
    [v3(0, -1, 0), v3(1, 0, 0), v3(0, 0, 1)],
    [v3(0, 0, 1), v3(1, 0, 0), v3(0, 1, 0)],
    [v3(0, 0, -1), v3(0, 1, 0), v3(1, 0, 0)],
  ];
  const corners: readonly (readonly [number, number])[] = [
    [-1, -1],
    [1, -1],
    [1, 1],
    [-1, 1],
  ];

  for (const [n, u, w] of faces) {
    const base = positions.length;
    for (const [su, sv] of corners) {
      positions.push(
        v3(
          0.5 * (n.x + su * u.x + sv * w.x),
          0.5 * (n.y + su * u.y + sv * w.y),
          0.5 * (n.z + su * u.z + sv * w.z),
        ),
      );
      normals.push(n);
    }
    indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
  }

  return { indices, normals, positions };
};

/** A unit-diameter sphere (radius 0.5) centered at the origin, latitude/
 * longitude grid with smooth normals (normal = normalized position). */
export const unitSphere = (latSegments = 16, lonSegments = 24): MeshData => {
  const positions: EngineVec3[] = [];
  const normals: EngineVec3[] = [];
  const indices: number[] = [];

  for (let lat = 0; lat <= latSegments; lat += 1) {
    const phi = (lat / latSegments) * Math.PI;
    const y = Math.cos(phi);
    const ring = Math.sin(phi);
    for (let lon = 0; lon <= lonSegments; lon += 1) {
      const theta = (lon / lonSegments) * TAU;
      const n = v3(ring * Math.cos(theta), y, ring * Math.sin(theta));
      normals.push(n);
      positions.push(v3(n.x * 0.5, n.y * 0.5, n.z * 0.5));
    }
  }

  const stride = lonSegments + 1;
  for (let lat = 0; lat < latSegments; lat += 1) {
    for (let lon = 0; lon < lonSegments; lon += 1) {
      const a = lat * stride + lon;
      const b = a + stride;
      indices.push(a, b, a + 1, a + 1, b, b + 1);
    }
  }

  return { indices, normals, positions };
};

/** A unit-diameter, unit-height capped cylinder around +Y, centered at the
 * origin (radius 0.5, y ∈ [-0.5, 0.5]); smooth radial side normals, flat ±Y
 * cap normals. */
export const unitCylinderY = (segments = 24): MeshData => {
  const positions: EngineVec3[] = [];
  const normals: EngineVec3[] = [];
  const indices: number[] = [];

  // Side wall: (top, bottom) vertex pairs sharing a smooth radial normal.
  for (let i = 0; i <= segments; i += 1) {
    const theta = (i / segments) * TAU;
    const c = Math.cos(theta);
    const s = Math.sin(theta);
    const n = v3(c, 0, s);
    positions.push(v3(c * 0.5, 0.5, s * 0.5), v3(c * 0.5, -0.5, s * 0.5));
    normals.push(n, n);
  }
  for (let i = 0; i < segments; i += 1) {
    const t0 = i * 2;
    const b0 = t0 + 1;
    const t1 = t0 + 2;
    const b1 = t0 + 3;
    indices.push(t0, b0, t1, t1, b0, b1);
  }

  // Caps: a center vertex plus its own flat-normal ring (normals must not be
  // shared with the side wall, so the rim stays crisp).
  const cap = (yTop: boolean): void => {
    const y = yTop ? 0.5 : -0.5;
    const n = v3(0, yTop ? 1 : -1, 0);
    const center = positions.length;
    positions.push(v3(0, y, 0));
    normals.push(n);
    const ring = positions.length;
    for (let i = 0; i <= segments; i += 1) {
      const theta = (i / segments) * TAU;
      positions.push(v3(Math.cos(theta) * 0.5, y, Math.sin(theta) * 0.5));
      normals.push(n);
    }
    for (let i = 0; i < segments; i += 1) {
      // Wind the top cap outward-CCW and the bottom cap the opposite way.
      const a = ring + i;
      const b = ring + i + 1;
      indices.push(center, ...(yTop ? [b, a] : [a, b]));
    }
  };
  cap(true);
  cap(false);

  return { indices, normals, positions };
};
