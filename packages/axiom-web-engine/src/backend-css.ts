/*
 * backend-css.ts — the DOM drawing backend: the scene is rendered as real HTML
 * elements positioned by CSS 3D transforms, with NO canvas and no rasterizer of
 * our own. The browser's compositor is the renderer; we only hand it geometry as
 * `matrix3d` and flat-shaded `background` colors. Selected with `?backend=css`.
 *
 * How a triangle-list mesh becomes DOM (the whole idea):
 *
 *   1. `uploadMesh` MERGES COPLANAR TRIANGLES into convex polygon FACES. A unit
 *      box's 12 triangles collapse to 6 quads; a cylinder cap's 12 triangles to
 *      one 12-gon. This is the single most important step — a DOM renderer pays
 *      per ELEMENT, not per pixel, so halving the element count doubles the frame
 *      rate. Emitting one element per triangle is 2-8x slower for identical output.
 *   2. Each face becomes ONE absolutely-positioned element, sized to the face's
 *      bounding box in its own plane and mapped into 3D by a `matrix3d` whose
 *      basis columns are the plane's (u, v, n). Non-rectangular faces additionally
 *      carry a `clip-path` polygon; rectangles skip it (the fast path).
 *   3. Each scene node is a wrapper element carrying the model matrix, so a
 *      re-pose is ONE style write regardless of face count, and the faces below it
 *      are never touched again unless the lighting changes.
 *   4. The camera is the container's `perspective` plus a view matrix on the world
 *      root. See `cssViewMatrix` for the exact CSS <-> engine projection identity.
 *
 * Shading is per FACE, evaluated with the SAME `shading.ts` truth the WebGL2 and
 * Canvas2D backends use (flat Lambert at the face centroid, tonemapped), so colors
 * match the other backends exactly. Depth order is the browser's `preserve-3d`
 * sorting; alpha is native CSS `rgba`.
 *
 * THE BUDGET, and why there is a LOD. CSS compositing cost scales with the total
 * PAINTED element count, and re-posing any node invalidates the whole
 * `preserve-3d` sorting context — so cost tracks the whole scene, not the moving
 * part of it. Measured on a real GPU browser: ~300 elements holds 60fps, ~900
 * gives ~14fps, and ~2400 sits at ~5fps even when nothing moves at all.
 *
 * A scene authored for WebGL2 blows straight past that (the casino chest scene
 * is 359 nodes / ~3.6k faces), so this backend does what a renderer facing a hard
 * budget must: it drops what cannot be seen, then what cannot be read. Backface
 * + screen-space culling (see NODE_MIN_PX / FACE_MIN_PX2) takes that scene from
 * ~3600 elements at ~2fps to ~390 at a locked 60.
 *
 * The LOD is CONTINUOUS, not a fixed decimation, and that matters: thresholds are
 * evaluated per frame against each node's real projected size. The chest
 * nameplate's welded letter strokes measure ~8px on the board and are dropped, so
 * the plaque reads blank — but when the chosen chest flies to its hero framing the
 * same strokes measure ten times that and come back, spelling the brand exactly
 * when the shot is about it. Detail below the threshold is gone, not shrunk; that
 * is the honest cost of the budget, and it is the same trade `backend-canvas2d.ts`
 * already makes with its sub-pixel node cull.
 *
 * As a browser-DOM boundary this file is coverage-exempt (test-exempt.json) and
 * outside the Branchless Law, exactly like the other two backends.
 */

import type { Handle, MeshData } from "./api.ts";
import type { RenderBackend, ResolvedMaterial, SceneFrame } from "./backend.ts";
import { type Mat4, fromTrs, lookAt, multiply } from "./mat4.ts";
import { diffuseOnly, shadeSurface, tonemap } from "./shading.ts";

/** CSS pixels per world unit. The projection is mathematically independent of
 * this (it cancels in the perspective divide — see `cssViewMatrix`), so it is
 * purely a PRECISION choice: big enough that sub-unit detail lands on distinct
 * subpixels rather than collapsing into rounding noise. */
const WORLD_PX = 100;

/** Plane-bucketing quantum for the coplanar merge. Two triangles join a face when
 * their normal and plane offset agree to this many decimals. Loose enough to
 * absorb float noise in generated primitives, tight enough that a low-segment
 * cylinder's adjacent side quads stay separate faces. */
const PLANE_QUANTUM = 1e4;

/** A hull point is treated as ON the bounding box (making the face a plain
 * rectangle, so `clip-path` can be skipped) within this fraction of its size. */
const RECT_EPSILON = 1e-3;

/**
 * Merging coplanar triangles has one real cost: a face is FLAT-shaded at a single
 * point, so a merged face loses the positional shading variation its triangles
 * had. That is invisible for chest-sized geometry and glaring for a 48-unit floor
 * slab: POINT lights fall off as 1/(1 + 0.08·d²), so shading the whole slab at its
 * centre (right under the lamp) floods it, where the per-triangle backends sample
 * far out and get almost nothing.
 *
 * So a rectangular face wider than this many world units is re-emitted as a grid
 * of cells, each shaded at its own centre — restoring the falloff gradient. The
 * grid is capped so a big slab costs tens of elements, not hundreds.
 */
const MAX_FACE_SPAN = 3;
const MAX_FACE_GRID = 4;

/**
 * Subdivision cells are grown by this many CSS px so neighbours OVERLAP instead
 * of butting edge to edge. Two abutting quads share an exact edge in theory, but
 * after the perspective divide and sub-pixel rounding the seam does not close,
 * and on a 48-unit floor slab it shows as a hairline of background straight
 * across the frame. The cells differ only slightly in shade, so a fraction of a
 * pixel of overlap is invisible — where the seam is not.
 */
const CELL_OVERLAP_PX = 1;

/**
 * SCREEN-SPACE LEVEL OF DETAIL — the reason this backend is usable at all.
 *
 * `backend-canvas2d.ts` already culls whole nodes whose bounding sphere projects
 * under half a pixel, because a software rasterizer pays per pixel. A DOM
 * renderer pays per ELEMENT — a 3-pixel gold strap costs exactly as much to
 * composite and depth-sort as the chest behind it — so the same idea has to bite
 * far, far earlier. These are the thresholds, in CSS pixels of the real viewport:
 *
 *   - a NODE whose bounding sphere projects smaller than `NODE_MIN_PX` across is
 *     dropped whole (the chest's plank grooves, band slats, letter strokes);
 *   - a FACE whose projected area — foreshortening included, so grazing faces go
 *     first — is under `FACE_MIN_PX2` is dropped;
 *   - a BACK-FACING face is dropped outright. This one is free: it is exact, not
 *     an approximation, and it removes ~half of every box in the scene. CSS
 *     `backface-visibility` hides such faces at paint time but they still cost
 *     layout and depth sorting, so hiding them via `display` is the real win.
 *
 * Culling is folded into the existing per-face loop, which already computes each
 * face's world normal and centroid to shade it — so LOD costs no extra pass, and
 * a culled face additionally skips the (much more expensive) shading call.
 */
const NODE_MIN_PX = 26;
const FACE_MIN_PX2 = 110;

/** Meshes with at least this many vertices AND a constant radius about their
 * centroid render as a single shaded impostor disc instead of one element per
 * quad — a low-detail unit sphere is 96 quads, which alone would blow the whole
 * element budget for one ball. */
const IMPOSTOR_MIN_VERTICES = 40;
const IMPOSTOR_RADIUS_TOLERANCE = 0.02;

/** A face of a merged coplanar triangle group, in MESH-LOCAL space. */
interface CssFace {
  /** Plane origin: the bounding box's (minU, minV) corner. */
  readonly ox: number;
  readonly oy: number;
  readonly oz: number;
  /** Orthonormal plane basis; `(u, v, n)` is right-handed so the element's CSS
   * front face points along the outward normal and `backface-visibility` culls. */
  readonly ux: number;
  readonly uy: number;
  readonly uz: number;
  readonly vx: number;
  readonly vy: number;
  readonly vz: number;
  readonly nx: number;
  readonly ny: number;
  readonly nz: number;
  /** Element size in CSS px. */
  readonly width: number;
  readonly height: number;
  /** `clip-path` polygon, or "" when the face fills its box (a rectangle). */
  readonly clip: string;
  /** Face centroid in mesh-local space (the point lighting is evaluated at). */
  readonly cx: number;
  readonly cy: number;
  readonly cz: number;
  /** Mean baked ambient occlusion over the face's vertices. */
  readonly ao: number;
}

/** An uploaded mesh: either merged polygon faces, or an impostor sphere. */
interface CssMesh {
  readonly faces: readonly CssFace[];
  /** Set when the mesh is a ball: local-space centre and radius. */
  readonly impostor: { readonly cx: number; readonly cy: number; readonly cz: number; readonly r: number } | null;
  /** Mesh-local bounding-sphere radius about the origin, for node-level LOD. */
  readonly radius: number;
}

/** The live DOM for one scene node. */
interface NodeDom {
  readonly root: HTMLElement;
  /** This node's faces AFTER scale-aware subdivision — parallel to `faceEls`. */
  readonly faces: readonly CssFace[];
  readonly faceEls: readonly HTMLElement[];
  /** True while the node is LOD-culled, so becoming visible forces a re-shade. */
  hidden: boolean;
  /** Last applied values, so an unchanged node costs zero style writes. */
  lastTransform: unknown;
  lastMaterial: Handle;
  lastMesh: Handle;
  lastLightEpoch: number;
}

const clamp255 = (v: number): number => Math.max(0, Math.min(255, Math.round(v * 255)));

/** Detach a node's DOM subtree (the node left the scene, or changed mesh). */
const disposeNode = (dom: NodeDom): void => {
  dom.root.remove();
};

/** One lit surface point, as an argument bag — the shading inputs are a genuine
 * data clump (normal + position + material + frame), not a refactorable list. */
interface FaceShadeInput {
  /** Unit surface normal. */
  readonly n: readonly [number, number, number];
  /** World-space point to light at (the face centroid). */
  readonly p: readonly [number, number, number];
  /** Baked ambient occlusion for the face. */
  readonly ao: number;
  readonly material: ResolvedMaterial;
  readonly frame: SceneFrame;
}

/**
 * Shade one face and return a CSS color. Mirrors the Canvas2D/WebGL2 truth
 * exactly: albedo-tinted, AO-attenuated diffuse + neutral specular + emissive,
 * then the shared highlight tonemap — so a face here matches the same face on
 * the other two backends channel for channel.
 */
const shadeFace = ({ n, p, ao, material, frame }: FaceShadeInput): string => {
  const eye = frame.camera.position;
  // Two-sided, exactly like the software backend: flip the normal toward the eye.
  const facing = (eye.x - p[0]) * n[0] + (eye.y - p[1]) * n[1] + (eye.z - p[2]) * n[2];
  const s = facing < 0 ? -1 : 1;
  const fx = n[0] * s;
  const fy = n[1] * s;
  const fz = n[2] * s;
  const shaded =
    material.roughness >= 1
      ? { diffuse: diffuseOnly(fx, fy, fz, p[0], p[1], p[2], frame), specular: [0, 0, 0] as const }
      : shadeSurface(fx, fy, fz, p[0], p[1], p[2], eye.x, eye.y, eye.z, material.roughness, frame);
  const base = material.baseColor;
  const em = material.emissive;
  const r = clamp255(tonemap(shaded.diffuse[0] * ao * base[0] + shaded.specular[0] + em[0]));
  const g = clamp255(tonemap(shaded.diffuse[1] * ao * base[1] + shaded.specular[1] + em[1]));
  const b = clamp255(tonemap(shaded.diffuse[2] * ao * base[2] + shaded.specular[2] + em[2]));
  return material.opacity >= 1 ? `rgb(${r},${g},${b})` : `rgba(${r},${g},${b},${material.opacity})`;
};

const mat = (m: Mat4, i: number): number => m[i]!;

const matrix3d = (m: Mat4): string =>
  `matrix3d(${mat(m, 0)},${mat(m, 1)},${mat(m, 2)},${mat(m, 3)},${mat(m, 4)},${mat(m, 5)},${mat(m, 6)},${mat(m, 7)},${mat(m, 8)},${mat(m, 9)},${mat(m, 10)},${mat(m, 11)},${mat(m, 12)},${mat(m, 13)},${mat(m, 14)},${mat(m, 15)})`;

/**
 * The engine camera as a CSS transform, and the identity that makes it exact.
 *
 * The engine projects a view-space point to screen as `sx = d·xv/(-zv)` with
 * `d = (H/2)/tan(fovY/2)`. CSS, given `perspective: d`, projects a point in the
 * container's space as `sx = X·d/(d - Z)`. Matching the two gives
 *
 *     X = xv        Y = -yv        Z = zv + d
 *
 * i.e. `CSS = Translate(0, 0, d) · diag(1, -1, 1) · View`. Scaling the world by
 * WORLD_PX cancels: numerator and denominator scale together, so `sx` is
 * unchanged. That is why `WORLD_PX` is free to choose for precision alone.
 */
const cssViewMatrix = (frame: SceneFrame, perspectiveDistance: number): Mat4 => {
  const cam = frame.camera;
  const eye = { x: cam.position.x * WORLD_PX, y: cam.position.y * WORLD_PX, z: cam.position.z * WORLD_PX };
  const at = { x: cam.target.x * WORLD_PX, y: cam.target.y * WORLD_PX, z: cam.target.z * WORLD_PX };
  const view = lookAt(eye, at, { x: 0, y: 1, z: 0 });
  // diag(1, -1, 1) — CSS y grows downward.
  const flipY: Mat4 = new Float32Array([1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
  const push: Mat4 = new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, perspectiveDistance, 1]);
  return multiply(push, multiply(flipY, view));
};

/** A point in a face's own 2D plane space. */
type PlanePoint = readonly [number, number];

/** Cross product of OA x OB, the monotone-chain turn test. */
const hullCross = (o: PlanePoint, a: PlanePoint, b: PlanePoint): number =>
  (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);

/** One monotone chain (lower or upper hull) over x-sorted points. */
const hullChain = (src: readonly PlanePoint[]): PlanePoint[] => {
  const out: PlanePoint[] = [];
  for (const p of src) {
    while (out.length >= 2 && hullCross(out.at(-2)!, out.at(-1)!, p) <= 0) out.pop();
    out.push(p);
  }
  out.pop();
  return out;
};

/** Andrew's monotone chain convex hull over plane-space points, CCW. */
const convexHull = (pts: readonly PlanePoint[]): PlanePoint[] => {
  const sorted = pts.toSorted((a, b) => a[0] - b[0] || a[1] - b[1]);
  if (sorted.length < 3) return [...sorted];
  return [...hullChain(sorted), ...hullChain(sorted.toReversed())];
};

/** Merge a triangle list into coplanar convex polygon faces. */
const buildFaces = (data: MeshData): CssFace[] => {
  const pos = data.positions;
  const nrm = data.normals;
  const ao = data.ao;
  const groups = new Map<string, { tris: number[][]; nx: number; ny: number; nz: number }>();

  for (let i = 0; i < data.indices.length; i += 3) {
    const ia = data.indices[i]!;
    const ib = data.indices[i + 1]!;
    const ic = data.indices[i + 2]!;
    const a = pos[ia]!;
    const b = pos[ib]!;
    const c = pos[ic]!;
    let nx = (b.y - a.y) * (c.z - a.z) - (b.z - a.z) * (c.y - a.y);
    let ny = (b.z - a.z) * (c.x - a.x) - (b.x - a.x) * (c.z - a.z);
    let nz = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    const len = Math.hypot(nx, ny, nz);
    if (len < 1e-12) continue; // degenerate (sphere pole fans)
    nx /= len;
    ny /= len;
    nz /= len;
    // Trust the authored vertex normal for outward orientation.
    const vn = nrm[ia]!;
    if (nx * vn.x + ny * vn.y + nz * vn.z < 0) {
      nx = -nx;
      ny = -ny;
      nz = -nz;
    }
    const d = nx * a.x + ny * a.y + nz * a.z;
    const key = `${Math.round(nx * PLANE_QUANTUM)},${Math.round(ny * PLANE_QUANTUM)},${Math.round(nz * PLANE_QUANTUM)},${Math.round(d * PLANE_QUANTUM)}`;
    const g = groups.get(key);
    if (g === undefined) groups.set(key, { nx, ny, nz, tris: [[ia, ib, ic]] });
    else g.tris.push([ia, ib, ic]);
  }

  const faces: CssFace[] = [];
  for (const g of groups.values()) {
    const verts = [...new Set(g.tris.flat())];
    const ref = pos[verts[0]!]!;
    // An orthonormal plane basis; (u, v, n) right-handed.
    const seed = Math.abs(g.nx) > 0.9 ? { x: 0, y: 1, z: 0 } : { x: 1, y: 0, z: 0 };
    let ux = seed.y * g.nz - seed.z * g.ny;
    let uy = seed.z * g.nx - seed.x * g.nz;
    let uz = seed.x * g.ny - seed.y * g.nx;
    const ul = Math.hypot(ux, uy, uz) || 1;
    ux /= ul;
    uy /= ul;
    uz /= ul;
    const vx = g.ny * uz - g.nz * uy;
    const vy = g.nz * ux - g.nx * uz;
    const vz = g.nx * uy - g.ny * ux;

    const planePts = verts.map((vi): readonly [number, number] => {
      const p = pos[vi]!;
      const dx = p.x - ref.x;
      const dy = p.y - ref.y;
      const dz = p.z - ref.z;
      return [dx * ux + dy * uy + dz * uz, dx * vx + dy * vy + dz * vz];
    });
    const hull = convexHull(planePts);
    if (hull.length < 3) continue;
    const minU = Math.min(...hull.map((p) => p[0]));
    const maxU = Math.max(...hull.map((p) => p[0]));
    const minV = Math.min(...hull.map((p) => p[1]));
    const maxV = Math.max(...hull.map((p) => p[1]));
    const width = (maxU - minU) * WORLD_PX;
    const height = (maxV - minV) * WORLD_PX;
    if (width < 0.01 || height < 0.01) continue;

    // A 4-point hull sitting exactly on its bounding box needs no clip-path.
    const epsU = (maxU - minU) * RECT_EPSILON;
    const epsV = (maxV - minV) * RECT_EPSILON;
    const isRect =
      hull.length === 4 &&
      hull.every(
        (p) =>
          (Math.abs(p[0] - minU) < epsU || Math.abs(p[0] - maxU) < epsU) &&
          (Math.abs(p[1] - minV) < epsV || Math.abs(p[1] - maxV) < epsV),
      );
    const clip = isRect
      ? ""
      : `polygon(${hull.map((p) => `${((p[0] - minU) * WORLD_PX).toFixed(2)}px ${((p[1] - minV) * WORLD_PX).toFixed(2)}px`).join(",")})`;

    let aoSum = 0;
    for (const vi of verts) aoSum += ao === undefined ? 1 : (ao[vi] ?? 1);
    const faceAo = aoSum / verts.length;

    // The face spans its whole bounding box. Note that the shading subdivision
    // happens per NODE, not here — a mesh is authored at unit size and the
    // node's scale is what makes a face physically huge (the floor slab is a
    // UNIT box scaled 48x). See `subdivideForScale`.
    const cU = (minU + maxU) / 2;
    const cV = (minV + maxV) / 2;
    faces.push({
      ao: faceAo,
      clip,
      cx: ref.x + ux * cU + vx * cV,
      cy: ref.y + uy * cU + vy * cV,
      cz: ref.z + uz * cU + vz * cV,
      height,
      nx: g.nx,
      ny: g.ny,
      nz: g.nz,
      ox: ref.x + ux * minU + vx * minV,
      oy: ref.y + uy * minU + vy * minV,
      oz: ref.z + uz * minU + vz * minV,
      ux,
      uy,
      uz,
      vx,
      vy,
      vz,
      width,
    });
  }
  return faces;
};

/**
 * Split rectangular faces that the node's scale makes physically large into a
 * shading grid, so point-light falloff varies across them (see MAX_FACE_SPAN).
 * Only rectangles subdivide — a cell of a rectangle is still a rectangle, so no
 * polygon clipping is needed, and the big offenders (floor slabs, backdrop
 * sheets) are always rectangles. `clip === ""` is exactly the rectangle marker.
 */
const subdivideForScale = (faces: readonly CssFace[], scale: { x: number; y: number; z: number }): CssFace[] => {
  const out: CssFace[] = [];
  for (const f of faces) {
    // World length of the face's u/v spans once the node scale is applied.
    const spanU = (f.width / WORLD_PX) * Math.hypot(f.ux * scale.x, f.uy * scale.y, f.uz * scale.z);
    const spanV = (f.height / WORLD_PX) * Math.hypot(f.vx * scale.x, f.vy * scale.y, f.vz * scale.z);
    const cols = f.clip === "" ? Math.min(MAX_FACE_GRID, Math.max(1, Math.ceil(spanU / MAX_FACE_SPAN))) : 1;
    const rows = f.clip === "" ? Math.min(MAX_FACE_GRID, Math.max(1, Math.ceil(spanV / MAX_FACE_SPAN))) : 1;
    if (cols === 1 && rows === 1) {
      out.push(f);
      continue;
    }
    const stepU = f.width / WORLD_PX / cols;
    const stepV = f.height / WORLD_PX / rows;
    for (let i = 0; i < cols; i += 1) {
      for (let j = 0; j < rows; j += 1) {
        const du = i * stepU;
        const dv = j * stepV;
        const ox = f.ox + f.ux * du + f.vx * dv;
        const oy = f.oy + f.uy * du + f.vy * dv;
        const oz = f.oz + f.uz * du + f.vz * dv;
        out.push({
          ao: f.ao,
          clip: f.clip,
          cx: ox + f.ux * (stepU / 2) + f.vx * (stepV / 2),
          cy: oy + f.uy * (stepU / 2) + f.vy * (stepV / 2),
          cz: oz + f.uz * (stepU / 2) + f.vz * (stepV / 2),
          height: f.height / rows + CELL_OVERLAP_PX,
          nx: f.nx,
          ny: f.ny,
          nz: f.nz,
          ox,
          oy,
          oz,
          ux: f.ux,
          uy: f.uy,
          uz: f.uz,
          vx: f.vx,
          vy: f.vy,
          vz: f.vz,
          width: f.width / cols + CELL_OVERLAP_PX,
        });
      }
    }
  }
  return out;
};

/** Detect a ball: every vertex the same distance from the centroid. */
const detectImpostor = (data: MeshData): CssMesh["impostor"] => {
  const pts = data.positions;
  if (pts.length < IMPOSTOR_MIN_VERTICES) return null;
  let cx = 0;
  let cy = 0;
  let cz = 0;
  for (const p of pts) {
    cx += p.x;
    cy += p.y;
    cz += p.z;
  }
  cx /= pts.length;
  cy /= pts.length;
  cz /= pts.length;
  const radii = pts.map((p) => Math.hypot(p.x - cx, p.y - cy, p.z - cz));
  const mean = radii.reduce((s, r) => s + r, 0) / radii.length;
  if (mean < 1e-6) return null;
  const spread = Math.max(...radii.map((r) => Math.abs(r - mean))) / mean;
  return spread <= IMPOSTOR_RADIUS_TOLERANCE ? { cx, cy, cz, r: mean } : null;
};

/** Build the CSS3D backend over `canvas`. The canvas element itself is NEVER
 * drawn into: it stays a transparent layout + pointer-input anchor, and the DOM
 * scene is mounted as a sibling layer pinned on top of it. */
export const createCssBackend = (canvas: HTMLCanvasElement): RenderBackend => {
  const meshes = new Map<Handle, CssMesh>();
  const nodeDom = new Map<object, NodeDom>();

  const root = document.createElement("div");
  root.className = "axiom-css3d";
  root.setAttribute("aria-hidden", "true");
  root.style.cssText =
    "position:absolute;left:0;top:0;width:100%;height:100%;pointer-events:none;overflow:hidden;transform-style:flat";
  const world = document.createElement("div");
  world.style.cssText = "position:absolute;left:50%;top:50%;width:0;height:0;transform-style:preserve-3d";
  root.append(world);
  canvas.parentElement?.append(root);

  /** Fallback viewport, used only before the layer has been laid out. The LIVE
   * size comes from `root.clientWidth/Height`: `resize` reports the canvas
   * BACKING STORE (e.g. 960x600), but this layer is displayed at whatever CSS
   * size the parent stretches it to, and the perspective distance must be in the
   * displayed pixels or the whole scene renders at the wrong scale. */
  let fallbackSize = { height: Math.max(1, canvas.height), width: Math.max(1, canvas.width) };
  /** Bumped whenever lights/camera change, to invalidate cached face colors. */
  let lightEpoch = 0;
  let lastLightKey = "";

  const buildNodeDom = (mesh: CssMesh, scale: { x: number; y: number; z: number }, opaque: boolean): NodeDom => {
    const el = document.createElement("div");
    el.style.cssText = "position:absolute;left:0;top:0;width:0;height:0;transform-style:preserve-3d";
    const faceEls: HTMLElement[] = [];
    // Only OPAQUE faces subdivide. The cells overlap by a hair to close seams
    // (CELL_OVERLAP_PX), and on a translucent surface that overlap double-blends
    // into a visible dark band — which is exactly what the reveal veil showed.
    // A translucent wash (veil, shadow disc, glow pool) is also the case that
    // needs per-cell shading variation least, so it stays one whole face.
    const subdivided = opaque ? subdivideForScale(mesh.faces, scale) : [...mesh.faces];
    const faces = mesh.impostor === null ? subdivided : [];
    if (mesh.impostor === null) {
      for (const f of faces) {
        const fe = document.createElement("i");
        const m: Mat4 = new Float32Array([
          f.ux, f.uy, f.uz, 0,
          f.vx, f.vy, f.vz, 0,
          f.nx, f.ny, f.nz, 0,
          f.ox * WORLD_PX, f.oy * WORLD_PX, f.oz * WORLD_PX, 1,
        ]);
        const clipRule = f.clip === "" ? "" : `clip-path:${f.clip};`;
        fe.style.cssText = `position:absolute;left:0;top:0;display:block;transform-origin:0 0;backface-visibility:hidden;width:${f.width.toFixed(3)}px;height:${f.height.toFixed(3)}px;transform:${matrix3d(m)};${clipRule}`;
        el.append(fe);
        faceEls.push(fe);
      }
    } else {
      // Impostor: one camera-facing disc, shaded by a radial gradient.
      const fe = document.createElement("i");
      const d = mesh.impostor.r * 2 * WORLD_PX;
      fe.style.cssText =
        `position:absolute;left:0;top:0;display:block;transform-origin:50% 50%;border-radius:50%;` +
        `width:${d.toFixed(3)}px;height:${d.toFixed(3)}px;margin-left:${(-d / 2).toFixed(3)}px;margin-top:${(-d / 2).toFixed(3)}px;`;
      el.append(fe);
      faceEls.push(fe);
    }
    return { faceEls, faces, hidden: false, lastLightEpoch: -1, lastMaterial: -1, lastMesh: -1, lastTransform: null, root: el };
  };

  return {
    dropMeshes: (): void => {
      meshes.clear();
      for (const dom of nodeDom.values()) disposeNode(dom);
      nodeDom.clear();
    },
    meshDetail: "low",
    name: "CSS3D",
    render: (frame: SceneFrame): void => {
      const [cr, cg, cb] = frame.clearColor;
      root.style.background = `rgb(${clamp255(cr)},${clamp255(cg)},${clamp255(cb)})`;

      // Camera: container perspective + the view matrix on the world root.
      const viewW = root.clientWidth || fallbackSize.width;
      const viewH = root.clientHeight || fallbackSize.height;
      const dist = viewH / 2 / Math.tan(frame.camera.fovY / 2);
      root.style.perspective = `${dist.toFixed(3)}px`;
      root.style.perspectiveOrigin = `${(viewW / 2).toFixed(2)}px ${(viewH / 2).toFixed(2)}px`;
      const viewMatrix = cssViewMatrix(frame, dist);
      world.style.transform = matrix3d(viewMatrix);
      // Transpose of the view's rotation 3x3 — the counter-rotation an impostor
      // applies so its disc always faces the camera (see the impostor branch).
      const invViewRot: Mat4 = new Float32Array([
        mat(viewMatrix, 0), mat(viewMatrix, 4), mat(viewMatrix, 8), 0,
        mat(viewMatrix, 1), mat(viewMatrix, 5), mat(viewMatrix, 9), 0,
        mat(viewMatrix, 2), mat(viewMatrix, 6), mat(viewMatrix, 10), 0,
        0, 0, 0, 1,
      ]);

      // Lighting/camera change invalidates every cached face color.
      const lightKey = `${frame.dirLights.map((l) => `${l.direction.join(",")}|${l.color.join(",")}`).join(";")}/${frame.pointLights.map((l) => `${l.position.join(",")}|${l.color.join(",")}`).join(";")}/${frame.camera.position.x},${frame.camera.position.y},${frame.camera.position.z}`;
      if (lightKey !== lastLightKey) {
        lastLightKey = lightKey;
        lightEpoch += 1;
      }

      // LOD basis: pixels per world unit at unit distance, and the camera's
      // forward axis (distances are measured ALONG it, not radially, so a node
      // off to the side is not spuriously treated as far away).
      const eye = frame.camera.position;
      const pxPerUnit = viewH / (2 * Math.tan(frame.camera.fovY / 2));
      let fwdX = frame.camera.target.x - eye.x;
      let fwdY = frame.camera.target.y - eye.y;
      let fwdZ = frame.camera.target.z - eye.z;
      const fwdLen = Math.hypot(fwdX, fwdY, fwdZ) || 1;
      fwdX /= fwdLen;
      fwdY /= fwdLen;
      fwdZ /= fwdLen;

      const seen = new Set<object>();
      for (const node of frame.nodes) {
        const mesh = meshes.get(node.mesh);
        const material = frame.materials.get(node.material);
        if (mesh === undefined || material === undefined) continue;
        seen.add(node);

        const t = node.transform;
        let dom = nodeDom.get(node);
        if (dom === undefined || dom.lastMesh !== node.mesh) {
          if (dom !== undefined) disposeNode(dom);
          dom = buildNodeDom(mesh, t.scale, material.opacity >= 1);
          dom.lastMesh = node.mesh;
          nodeDom.set(node, dom);
          world.append(dom.root);
        }

        // ── node-level LOD: drop anything behind the camera or too small to read
        const maxScale = Math.max(Math.abs(t.scale.x), Math.abs(t.scale.y), Math.abs(t.scale.z));
        const boundRadius = mesh.radius * maxScale;
        const along =
          (t.position.x - eye.x) * fwdX + (t.position.y - eye.y) * fwdY + (t.position.z - eye.z) * fwdZ;
        const projectedPx = (2 * boundRadius * pxPerUnit) / Math.max(along, frame.camera.near);
        const cull = along + boundRadius < frame.camera.near || projectedPx < NODE_MIN_PX;
        if (cull) {
          if (!dom.hidden) {
            dom.hidden = true;
            dom.root.style.display = "none";
          }
          continue;
        }
        if (dom.hidden) {
          dom.hidden = false;
          dom.root.style.display = "";
          // It was culled, so its faces hold stale colors — force a re-shade.
          dom.lastLightEpoch = -1;
        }
        // Screen-space scale factor for this node, reused by the per-face area cull.
        const nodePxPerUnit = pxPerUnit / Math.max(along, frame.camera.near);

        const posed = dom.lastTransform !== t;
        if (posed) {
          dom.lastTransform = t;
          const m = fromTrs(
            { x: t.position.x * WORLD_PX, y: t.position.y * WORLD_PX, z: t.position.z * WORLD_PX },
            t.rotation,
            t.scale,
          );
          dom.root.style.transform = matrix3d(m);
        }

        // Re-shade only when the pose, material, or lighting actually changed.
        if (!posed && dom.lastMaterial === node.material && dom.lastLightEpoch === lightEpoch) continue;
        dom.lastMaterial = node.material;
        dom.lastLightEpoch = lightEpoch;

        const model = fromTrs(t.position, t.rotation, t.scale);
        const lx = (x: number, y: number, z: number): readonly [number, number, number] => [
          mat(model, 0) * x + mat(model, 4) * y + mat(model, 8) * z,
          mat(model, 1) * x + mat(model, 5) * y + mat(model, 9) * z,
          mat(model, 2) * x + mat(model, 6) * y + mat(model, 10) * z,
        ];
        const wp = (x: number, y: number, z: number): readonly [number, number, number] => {
          const l = lx(x, y, z);
          return [l[0] + mat(model, 12), l[1] + mat(model, 13), l[2] + mat(model, 14)];
        };

        if (mesh.impostor === null) {
          dom.faces.forEach((f, i) => {
            const el = dom.faceEls[i]!;
            // Exact world normal under any linear transform: normalize(Mu x Mv).
            const mu = lx(f.ux, f.uy, f.uz);
            const mv = lx(f.vx, f.vy, f.vz);
            let nx = mu[1] * mv[2] - mu[2] * mv[1];
            let ny = mu[2] * mv[0] - mu[0] * mv[2];
            let nz = mu[0] * mv[1] - mu[1] * mv[0];
            const nl = Math.hypot(nx, ny, nz) || 1;
            nx /= nl;
            ny /= nl;
            nz /= nl;
            const c = wp(f.cx, f.cy, f.cz);

            // ── face-level LOD, using the normal + centroid shading needs anyway.
            // Back-facing: the outward normal points away from the eye. Exact for
            // opaque solids. Translucent faces are kept two-sided, because a
            // shadow disc or a glass pane is authored to be seen from both sides.
            const towardEye = (eye.x - c[0]) * nx + (eye.y - c[1]) * ny + (eye.z - c[2]) * nz;
            const backFacing = material.opacity >= 1 && towardEye <= 0;
            // Projected area, foreshortening included: world area scaled by the
            // node's px-per-unit, times |cos| between the normal and the view ray.
            const worldW = (f.width / WORLD_PX) * Math.hypot(mu[0], mu[1], mu[2]);
            const worldH = (f.height / WORLD_PX) * Math.hypot(mv[0], mv[1], mv[2]);
            const viewLen = Math.hypot(eye.x - c[0], eye.y - c[1], eye.z - c[2]) || 1;
            const facing = Math.abs(towardEye) / viewLen;
            const areaPx = worldW * worldH * nodePxPerUnit * nodePxPerUnit * facing;
            const drop = backFacing || areaPx < FACE_MIN_PX2;
            el.style.display = drop ? "none" : "";
            // A dropped face also skips the much more expensive shading call.
            if (drop) return;
            el.style.background = shadeFace({ ao: f.ao, frame, material, n: [nx, ny, nz], p: c });
          });
        } else {
          // A ball drawn as ONE camera-facing disc. A low-detail unit sphere is 96
          // quads; at 13 balls that is 1248 elements — more than the entire CSS
          // element budget — for a shape whose flat-shaded silhouette is a circle
          // and whose shading is a smooth gradient. So: shade the point facing the
          // key light and the point facing away, and let a radial gradient
          // interpolate. Visually near-identical, 96x cheaper.
          const imp = mesh.impostor;
          const c = wp(imp.cx, imp.cy, imp.cz);
          const key = frame.dirLights[0];
          const toLight: readonly [number, number, number] =
            key === undefined ? [0.4, 1, 0.3] : [-key.direction[0], -key.direction[1], -key.direction[2]];
          const lit = shadeFace({ ao: 1, frame, material, n: toLight, p: c });
          const dark = shadeFace({ ao: 1, frame, material, n: [-toLight[0], -toLight[1], -toLight[2]], p: c });

          // Put the gradient's highlight where the light actually is on screen:
          // rotate the light direction into view space (x right, y down).
          const sx = mat(viewMatrix, 0) * toLight[0] + mat(viewMatrix, 4) * toLight[1] + mat(viewMatrix, 8) * toLight[2];
          const sy = mat(viewMatrix, 1) * toLight[0] + mat(viewMatrix, 5) * toLight[1] + mat(viewMatrix, 9) * toLight[2];
          const sl = Math.hypot(sx, sy) || 1;
          const hx = (50 + (sx / sl) * 30).toFixed(1);
          const hy = (50 + (sy / sl) * 30).toFixed(1);

          // Billboard: the disc counter-rotates by the transpose of (view . node)
          // so the composed transform leaves it square to the screen. For the
          // uniform scale a sphere always carries, S commutes out exactly.
          const invNodeRot = fromTrs(
            { x: 0, y: 0, z: 0 },
            [-t.rotation[0], -t.rotation[1], -t.rotation[2], t.rotation[3]],
            { x: 1, y: 1, z: 1 },
          );
          const el = dom.faceEls[0]!;
          el.style.background = `radial-gradient(circle at ${hx}% ${hy}%, ${lit} 0%, ${lit} 22%, ${dark} 100%)`;
          el.style.transform = matrix3d(multiply(invNodeRot, invViewRot));
        }
      }

      for (const [node, dom] of nodeDom) {
        if (seen.has(node)) continue;
        disposeNode(dom);
        nodeDom.delete(node);
      }
    },
    resize: (w: number, h: number): void => {
      fallbackSize = { height: Math.max(1, h), width: Math.max(1, w) };
    },
    uploadMesh: (handle: Handle, data: MeshData): void => {
      const impostor = detectImpostor(data);
      // Bounding-sphere radius about the mesh origin, for the node-level LOD.
      let radius = 0;
      for (const p of data.positions) {
        radius = Math.max(radius, Math.hypot(p.x, p.y, p.z));
      }
      meshes.set(handle, { faces: impostor === null ? buildFaces(data) : [], impostor, radius });
    },
  };
};
