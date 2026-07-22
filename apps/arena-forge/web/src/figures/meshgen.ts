/*
 * meshgen.ts — procedural geometry the engine's primitive set (box / sphere /
 * cylinder) cannot express: capsule, cone, tapered prism / wedge, beveled plate,
 * ring (torus), segmented appendage (swept tube), and billboard quad. Each returns
 * a plain `{positions, normals, indices}` (`Geometry`) — structurally the engine's
 * `createMeshData` input — so it stays SDK-free and unit-testable, and `scene.ts`
 * hands the result straight to `createMeshData`. Segment counts are parameters so
 * the quality profile can pick cheap counts on the Canvas2D baseline.
 *
 * Conventions match the engine's built-ins so app-side scale math is uniform:
 * every generator is centered at the origin and sized to the given extents/radii
 * (NOT unit-normalized), and normals are outward unit vectors.
 */

import { type Vec3, clamp, normalize, vec3 } from "./vec3.ts";

export interface Geometry {
  readonly positions: readonly Vec3[];
  readonly normals: readonly Vec3[];
  readonly indices: readonly number[];
  /**
   * OPTIONAL per-vertex ambient occlusion in 0..1 (1 = fully lit, lower = more
   * occluded). The engine's `MeshData.ao` consumes this: builders populate it (via
   * `verticalOcclusion`) and `primitives.ts` forwards it to `createMeshData`, so
   * the backends darken the diffuse+ambient term in occluded regions. Absent ⇒ the
   * engine treats every vertex as 1.0 (no occlusion), so it stays optional.
   */
  readonly ao?: readonly number[];
}

/** Floor so a fully-occluded (downward) vertex is darkened, never black. */
const AO_FLOOR = 0.35;

/**
 * A cheap, honest per-vertex occlusion proxy from the surface normal: upward-facing
 * vertices read fully lit, undersides / inner faces darken toward `AO_FLOOR`. It is
 * a pure function of the normals so it stays deterministic and unit-testable, and it
 * gives the AO hook (`Geometry.ao`) real data the moment the engine can consume it.
 */
export const verticalOcclusion = (normals: readonly Vec3[]): number[] =>
  normals.map((n) => clamp(AO_FLOOR + (1 - AO_FLOOR) * (0.5 + 0.5 * n.y), 0, 1));

interface Mutable {
  positions: Vec3[];
  normals: Vec3[];
  indices: number[];
}

const emptyMesh = (): Mutable => ({ positions: [], normals: [], indices: [] });

const quad = (m: Mutable, a: Vec3, b: Vec3, c: Vec3, d: Vec3, n: Vec3): void => {
  const base = m.positions.length;
  m.positions.push(a, b, c, d);
  m.normals.push(n, n, n, n);
  m.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
};

/** A ring of `seg` vertices of radius `r` at height `y`, with the given normals. */
const ring = (m: Mutable, y: number, r: number, seg: number, radial: (nx: number, nz: number) => Vec3): number => {
  const start = m.positions.length;
  for (let i = 0; i <= seg; i += 1) {
    const a = (i / seg) * Math.PI * 2;
    const cx = Math.cos(a);
    const sz = Math.sin(a);
    m.positions.push(vec3(cx * r, y, sz * r));
    m.normals.push(radial(cx, sz));
  }
  return start;
};

const connectRings = (m: Mutable, lower: number, upper: number, seg: number): void => {
  for (let i = 0; i < seg; i += 1) {
    const a = lower + i;
    const b = upper + i;
    m.indices.push(a, b, a + 1, a + 1, b, b + 1);
  }
};

/** A capsule of radius `r` and cylindrical length `len` about +Y (total height
 * len + 2r), the workhorse limb/body primitive. */
export const capsule = (r: number, len: number, radialSeg = 12, capSeg = 4): Geometry => {
  const m = emptyMesh();
  const half = len / 2;
  const side = (nx: number, nz: number): Vec3 => vec3(nx, 0, nz);
  const rings: number[] = [];
  // Bottom hemisphere.
  for (let j = 0; j <= capSeg; j += 1) {
    const phi = (Math.PI / 2) * (j / capSeg); // 0 at equator → π/2 at pole
    const y = -half - r * Math.sin(phi);
    const rr = r * Math.cos(phi);
    rings.push(ring(m, y, rr, radialSeg, (nx, nz) => normalize(vec3(nx * Math.cos(phi), -Math.sin(phi), nz * Math.cos(phi)))));
  }
  rings.reverse();
  // Cylinder equator rings.
  rings.push(ring(m, -half, r, radialSeg, side));
  rings.push(ring(m, half, r, radialSeg, side));
  // Top hemisphere.
  for (let j = 1; j <= capSeg; j += 1) {
    const phi = (Math.PI / 2) * (j / capSeg);
    const y = half + r * Math.sin(phi);
    const rr = r * Math.cos(phi);
    rings.push(ring(m, y, rr, radialSeg, (nx, nz) => normalize(vec3(nx * Math.cos(phi), Math.sin(phi), nz * Math.cos(phi)))));
  }
  for (let k = 0; k + 1 < rings.length; k += 1) {
    connectRings(m, rings[k] as number, rings[k + 1] as number, radialSeg);
  }
  return m;
};

/** A cone with base radius `rBase` at y=-h/2 and apex at y=+h/2. */
export const cone = (rBase: number, h: number, seg = 12): Geometry => {
  const m = emptyMesh();
  const half = h / 2;
  const slope = Math.atan2(rBase, h); // side normal tilt
  const cs = Math.cos(slope);
  const sn = Math.sin(slope);
  const baseRing = ring(m, -half, rBase, seg, (nx, nz) => normalize(vec3(nx * cs, sn, nz * cs)));
  // Apex duplicated per segment so each side triangle has the base-vertex normal.
  for (let i = 0; i < seg; i += 1) {
    const a = baseRing + i;
    const apex = m.positions.length;
    m.positions.push(vec3(0, half, 0));
    m.normals.push(m.normals[a] as Vec3);
    m.indices.push(a, apex, a + 1);
  }
  // Base cap (facing -Y).
  const center = m.positions.length;
  m.positions.push(vec3(0, -half, 0));
  m.normals.push(vec3(0, -1, 0));
  const capStart = m.positions.length;
  for (let i = 0; i <= seg; i += 1) {
    const ang = (i / seg) * Math.PI * 2;
    m.positions.push(vec3(Math.cos(ang) * rBase, -half, Math.sin(ang) * rBase));
    m.normals.push(vec3(0, -1, 0));
  }
  for (let i = 0; i < seg; i += 1) {
    m.indices.push(center, capStart + i + 1, capStart + i);
  }
  return m;
};

/** A frustum box: bottom `bx×bz`, top `tx×tz`, height `h`, centered. A thin top
 * (tx→0) is a wedge/blade; a near-full inset top is a beveled plate/petal. */
export const taperedPrism = (bx: number, bz: number, tx: number, tz: number, h: number): Geometry => {
  const m = emptyMesh();
  const hy = h / 2;
  const bhx = bx / 2;
  const bhz = bz / 2;
  const thx = tx / 2;
  const thz = tz / 2;
  const b0 = vec3(-bhx, -hy, -bhz);
  const b1 = vec3(bhx, -hy, -bhz);
  const b2 = vec3(bhx, -hy, bhz);
  const b3 = vec3(-bhx, -hy, bhz);
  const t0 = vec3(-thx, hy, -thz);
  const t1 = vec3(thx, hy, -thz);
  const t2 = vec3(thx, hy, thz);
  const t3 = vec3(-thx, hy, thz);
  quad(m, b0, b3, b2, b1, vec3(0, -1, 0)); // bottom
  quad(m, t0, t1, t2, t3, vec3(0, 1, 0)); // top
  const faceNormal = (a: Vec3, b: Vec3, c: Vec3): Vec3 =>
    normalize(vec3(
      (b.y - a.y) * (c.z - a.z) - (b.z - a.z) * (c.y - a.y),
      (b.z - a.z) * (c.x - a.x) - (b.x - a.x) * (c.z - a.z),
      (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x),
    ));
  quad(m, b0, b1, t1, t0, faceNormal(b0, b1, t1)); // -Z
  quad(m, b1, b2, t2, t1, faceNormal(b1, b2, t2)); // +X
  quad(m, b2, b3, t3, t2, faceNormal(b2, b3, t3)); // +Z
  quad(m, b3, b0, t0, t3, faceNormal(b3, b0, t0)); // -X
  return m;
};

const tri = (m: Mutable, a: Vec3, b: Vec3, c: Vec3, n: Vec3): void => {
  const base = m.positions.length;
  m.positions.push(a, b, c);
  m.normals.push(n, n, n);
  m.indices.push(base, base + 1, base + 2);
};

/**
 * A genuine chamfered/beveled hard-surface box (unit ±0.5, sized `w×h×d`): the six
 * faces are inset by a bevel `b`, the twelve edges become flat chamfer quads, and the
 * eight corners become chamfer triangles — so every armor-plate edge catches a
 * highlight instead of reading as a razor-sharp box. The segment count is FIXED
 * (44 tris), so it stays a cheap hard-surface bevel, not a subdivided round. `bevel`
 * is a fraction of the smallest half-extent (clamped so a thin plate never
 * self-intersects). Normals are per-face/per-chamfer outward units; carries the
 * dormant AO hook (undersides darkened). Centered at the origin like the built-ins.
 */
export const roundedBox = (w = 1, h = 1, d = 1, bevel = 0.16): Geometry => {
  const hx = w / 2;
  const hy = h / 2;
  const hz = d / 2;
  const b = clamp(bevel, 0, 0.49) * Math.min(hx, hy, hz);
  const ix = hx - b;
  const iy = hy - b;
  const iz = hz - b;
  const m = emptyMesh();

  // Six inset face rectangles.
  quad(m, vec3(hx, -iy, -iz), vec3(hx, -iy, iz), vec3(hx, iy, iz), vec3(hx, iy, -iz), vec3(1, 0, 0));
  quad(m, vec3(-hx, -iy, -iz), vec3(-hx, iy, -iz), vec3(-hx, iy, iz), vec3(-hx, -iy, iz), vec3(-1, 0, 0));
  quad(m, vec3(-ix, hy, -iz), vec3(ix, hy, -iz), vec3(ix, hy, iz), vec3(-ix, hy, iz), vec3(0, 1, 0));
  quad(m, vec3(-ix, -hy, -iz), vec3(-ix, -hy, iz), vec3(ix, -hy, iz), vec3(ix, -hy, -iz), vec3(0, -1, 0));
  quad(m, vec3(-ix, -iy, hz), vec3(ix, -iy, hz), vec3(ix, iy, hz), vec3(-ix, iy, hz), vec3(0, 0, 1));
  quad(m, vec3(-ix, -iy, -hz), vec3(-ix, iy, -hz), vec3(ix, iy, -hz), vec3(ix, -iy, -hz), vec3(0, 0, -1));

  const signs = [-1, 1];
  // Twelve edge chamfer quads: one per (axis-pair, sign, sign). Each connects the
  // two adjacent faces' shared inset edge with a flat chamfer plane.
  for (const sy of signs) {
    for (const sx of signs) {
      // edges parallel to Z, between ±X and ±Y faces.
      quad(m, vec3(sx * hx, sy * iy, -iz), vec3(sx * hx, sy * iy, iz), vec3(sx * ix, sy * hy, iz), vec3(sx * ix, sy * hy, -iz), normalize(vec3(sx, sy, 0)));
    }
  }
  for (const sz of signs) {
    for (const sy of signs) {
      // edges parallel to X, between ±Y and ±Z faces.
      quad(m, vec3(-ix, sy * hy, sz * iz), vec3(ix, sy * hy, sz * iz), vec3(ix, sy * iy, sz * hz), vec3(-ix, sy * iy, sz * hz), normalize(vec3(0, sy, sz)));
    }
  }
  for (const sz of signs) {
    for (const sx of signs) {
      // edges parallel to Y, between ±X and ±Z faces.
      quad(m, vec3(sx * hx, -iy, sz * iz), vec3(sx * hx, iy, sz * iz), vec3(sx * ix, iy, sz * hz), vec3(sx * ix, -iy, sz * hz), normalize(vec3(sx, 0, sz)));
    }
  }

  // Eight corner chamfer triangles.
  for (const sz of signs) {
    for (const sy of signs) {
      for (const sx of signs) {
        tri(m, vec3(sx * hx, sy * iy, sz * iz), vec3(sx * ix, sy * hy, sz * iz), vec3(sx * ix, sy * iy, sz * hz), normalize(vec3(sx, sy, sz)));
      }
    }
  }

  return { positions: m.positions, normals: m.normals, indices: m.indices, ao: verticalOcclusion(m.normals) };
};

/** A wedge (thin-topped tapered prism) — shields, blades, crests, petals. */
export const wedge = (w: number, h: number, d: number): Geometry => taperedPrism(w, d, w * 0.12, d, h);

/** A beveled plate — armored plates, pedestals, banners. */
export const plate = (w: number, d: number, thick: number, bevel = 0.12): Geometry =>
  taperedPrism(w, d, Math.max(0.01, w - 2 * bevel * w), Math.max(0.01, d - 2 * bevel * d), thick);

/** A torus in the XZ plane (axis +Y) — guard rings, halos, orbit rings. */
export const ringTorus = (ringR: number, tubeR: number, ringSeg = 20, tubeSeg = 8): Geometry => {
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
      normals.push(normalize(vec3(cv * cu, sv, cv * su)));
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
  return { positions, normals, indices };
};

/** A swept tube along a bent centerline — tails, vines, spore chains. `links`
 * segments over `len` about +Y, bending `curve` radians total in the XY plane. */
export const segmentedAppendage = (r: number, len: number, links = 5, curve = 0.8, radial = 6): Geometry => {
  const m = emptyMesh();
  const rings: number[] = [];
  let px = 0;
  let py = -len / 2;
  let ang = 0;
  const step = len / links;
  for (let k = 0; k <= links; k += 1) {
    const a = ang;
    const nx = Math.cos(a);
    const ny = Math.sin(a);
    // Ring perpendicular to travel direction (nx,ny) in the XY plane, extruded in Z.
    const start = m.positions.length;
    for (let i = 0; i <= radial; i += 1) {
      const t = (i / radial) * Math.PI * 2;
      const ox = Math.cos(t) * r * -ny; // in-plane perpendicular
      const oy = Math.cos(t) * r * nx;
      const oz = Math.sin(t) * r;
      m.positions.push(vec3(px + ox, py + oy, oz));
      m.normals.push(normalize(vec3(ox, oy, oz)));
    }
    rings.push(start);
    px += nx * step;
    py += ny * step;
    ang += curve / links;
  }
  for (let k = 0; k + 1 < rings.length; k += 1) {
    connectRings(m, rings[k] as number, rings[k + 1] as number, radial);
  }
  return m;
};

/** A flat quad in the XY plane facing +Z — afterimages, spore puffs, cheap accents.
 * (The scene yaws the node toward the camera at pose time for a true billboard.) */
export const billboard = (w: number, h: number): Geometry => {
  const m = emptyMesh();
  quad(m, vec3(-w / 2, -h / 2, 0), vec3(w / 2, -h / 2, 0), vec3(w / 2, h / 2, 0), vec3(-w / 2, h / 2, 0), vec3(0, 0, 1));
  return m;
};
