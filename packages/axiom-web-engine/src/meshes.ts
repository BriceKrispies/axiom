/*
 * meshes.ts — the procedural unit primitives behind `createMesh`. Each
 * builder returns the `MeshData` triangle-list shape from `api.ts`
 * (`{positions, normals, indices}`), pure and DOM-free so it is testable under
 * bare `node --test`. Conventions (consumer scale math depends on them):
 * `unitBox` is a UNIT CUBE centered at the origin (scale = full extents, flat
 * per-face normals), `unitSphere` is UNIT DIAMETER (radius 0.5, smooth
 * normals; scale = 2·radius), and `unitCylinderY` is UNIT DIAMETER × UNIT
 * HEIGHT around +Y with caps (scale = (diameter, height, diameter); smooth
 * side normals, flat cap normals).
 *
 * The builders are branchless (an engine-spine invariant): every mesh is
 * assembled by mapping/flat-mapping over index ranges instead of accumulating
 * inside imperative loops, and the cap winding is selected arithmetically.
 */

import type { EngineVec3, MeshData } from "./api.ts";

const v3 = (x: number, y: number, z: number): EngineVec3 => ({ x, y, z });

const HALF = 0.5;
const TWO = 2;
const THREE = 3;
const TAU = Math.PI * TWO;
const DEFAULT_LAT_SEGMENTS = 16;
const DEFAULT_LON_SEGMENTS = 24;
const DEFAULT_SEGMENTS = 24;

/** `[0, 1, …, count - 1]`, a branchless counting-index list that replaces a loop. */
const range = (count: number): number[] => Array.from({ length: count }, (value, index) => index);

/** One flat-normal end cap of `unitCylinderY`: a center vertex plus its own
 * ring, wound so `topFlag` (1 selects +Y, 0 selects -Y) picks the outward triangle
 * order arithmetically. Its vertices start at `centerIndex` in the merged mesh. */
interface CapSpec {
  readonly segments: number;
  readonly y: number;
  readonly ny: number;
  readonly centerIndex: number;
  readonly topFlag: number;
}

const buildCap = (
  spec: CapSpec,
): { readonly positions: EngineVec3[]; readonly normals: EngineVec3[]; readonly indices: number[] } => {
  const { segments, y, ny, centerIndex, topFlag } = spec;
  const normal = v3(0, ny, 0);
  const ringStart = centerIndex + 1;
  const ringPositions = range(segments + 1).map((seg) => {
    const theta = (seg / segments) * TAU;
    return v3(Math.cos(theta) * HALF, y, Math.sin(theta) * HALF);
  });
  const positions = [v3(0, y, 0), ...ringPositions];
  const normals = positions.map(() => normal);
  const indices = range(segments).flatMap((seg) => {
    const va = ringStart + seg;
    const vb = ringStart + seg + 1;
    const second = topFlag * vb + (1 - topFlag) * va;
    const third = topFlag * va + (1 - topFlag) * vb;
    return [centerIndex, second, third];
  });
  return { indices, normals, positions };
};

/** A unit cube centered at the origin (extents ±0.5), 24 vertices with flat
 * per-face normals, 12 triangles. */
export const unitBox = (): MeshData => {
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

  const positions = faces.flatMap(([normal, uAxis, vAxis]) =>
    corners.map(([su, sv]) =>
      v3(
        HALF * (normal.x + su * uAxis.x + sv * vAxis.x),
        HALF * (normal.y + su * uAxis.y + sv * vAxis.y),
        HALF * (normal.z + su * uAxis.z + sv * vAxis.z),
      ),
    ),
  );
  const normals = faces.flatMap(([normal]) => corners.map(() => normal));
  const indices = range(faces.length).flatMap((faceIdx) => {
    const base = faceIdx * corners.length;
    return [base, base + 1, base + TWO, base, base + TWO, base + THREE];
  });

  return { indices, normals, positions };
};

/** A unit-diameter sphere (radius 0.5) centered at the origin, latitude/
 * longitude grid with smooth normals (normal = normalized position). */
export const unitSphere = (latSegments = DEFAULT_LAT_SEGMENTS, lonSegments = DEFAULT_LON_SEGMENTS): MeshData => {
  const normals = range(latSegments + 1).flatMap((lat) => {
    const phi = (lat / latSegments) * Math.PI;
    const y = Math.cos(phi);
    const ring = Math.sin(phi);
    return range(lonSegments + 1).map((lon) => {
      const theta = (lon / lonSegments) * TAU;
      return v3(ring * Math.cos(theta), y, ring * Math.sin(theta));
    });
  });
  const positions = normals.map((normal) => v3(normal.x * HALF, normal.y * HALF, normal.z * HALF));

  const stride = lonSegments + 1;
  const indices = range(latSegments).flatMap((lat) =>
    range(lonSegments).flatMap((lon) => {
      const va = lat * stride + lon;
      const vb = va + stride;
      return [va, vb, va + 1, va + 1, vb, vb + 1];
    }),
  );

  return { indices, normals, positions };
};

/** A unit-diameter, unit-height capped cylinder around +Y, centered at the
 * origin (radius 0.5, y ∈ [-0.5, 0.5]); smooth radial side normals, flat ±Y
 * cap normals. */
export const unitCylinderY = (segments = DEFAULT_SEGMENTS): MeshData => {
  const rim = range(segments + 1);
  // Side wall: (top, bottom) vertex pairs sharing a smooth radial normal.
  const sideNormals = rim.flatMap((seg) => {
    const theta = (seg / segments) * TAU;
    const normal = v3(Math.cos(theta), 0, Math.sin(theta));
    return [normal, normal];
  });
  const sidePositions = rim.flatMap((seg) => {
    const theta = (seg / segments) * TAU;
    const cosT = Math.cos(theta);
    const sinT = Math.sin(theta);
    return [v3(cosT * HALF, HALF, sinT * HALF), v3(cosT * HALF, -HALF, sinT * HALF)];
  });
  const sideIndices = range(segments).flatMap((seg) => {
    const topA = seg * TWO;
    const botA = topA + 1;
    const topB = topA + TWO;
    const botB = topA + THREE;
    return [topA, botA, topB, topB, botA, botB];
  });

  // Caps: a center vertex plus its own flat-normal ring (normals must not be
  // shared with the side wall, so the rim stays crisp). The top cap winds
  // outward-CCW; the bottom cap the opposite way (selected by the flag).
  const sideVerts = (segments + 1) * TWO;
  const capVerts = segments + TWO;
  const top = buildCap({ centerIndex: sideVerts, ny: 1, segments, topFlag: 1, y: HALF });
  const bottom = buildCap({ centerIndex: sideVerts + capVerts, ny: -1, segments, topFlag: 0, y: -HALF });

  return {
    indices: [...sideIndices, ...top.indices, ...bottom.indices],
    normals: [...sideNormals, ...top.normals, ...bottom.normals],
    positions: [...sidePositions, ...top.positions, ...bottom.positions],
  };
};
