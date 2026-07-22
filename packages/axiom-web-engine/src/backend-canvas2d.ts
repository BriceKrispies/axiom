/*
 * backend-canvas2d.ts — the SOFTWARE drawing fallback: a z-buffered scanline
 * rasterizer over the plain 2D canvas API, auto-selected when WebGL2 is
 * unavailable (or forced with `?backend=canvas2d`). Per frame it transforms
 * every visible node's vertices to world space, lights each triangle ONCE at
 * its centroid with the SAME Lambert model as the WebGL2 backend (the shared
 * `lambertLight` in `shading.ts`), clips triangles crossing the near plane
 * (Sutherland–Hodgman — a camera standing INSIDE a large box would otherwise
 * have that box dropped whole, punching a hole in the scene), and rasterizes
 * flat-shaded spans into a reduced-resolution framebuffer with a
 * perspective-correct 1/w depth buffer — per-PIXEL occlusion, so coplanar
 * decals stacked millimetres apart and large enclosing surfaces resolve
 * exactly like the hardware path, with no painter's-sort artifacts. The
 * framebuffer is blitted up to the canvas each frame.
 *
 * Softening the workload keeps it real-time: half-resolution internally (the
 * chunky look of a software fallback is embraced), low-detail primitive meshes
 * (`meshDetail: "low"`), and whole-node culls — behind the camera, or a
 * bounding sphere projecting under half a pixel. Translucent triangles
 * rasterize after opaque ones with depth TEST but no depth WRITE, alpha-blended
 * in software.
 */

import type { Handle, MeshData } from "./api.ts";
import type { RenderBackend, SceneFrame } from "./backend.ts";
import { type Mat4, fromTrs, lookAt, multiply, perspective } from "./mat4.ts";
import { shadeSurface, tonemap } from "./shading.ts";

interface CpuMesh {
  /** xyz-interleaved model-space positions. */
  readonly positions: Float32Array;
  readonly indices: Uint32Array;
  /** One ambient-occlusion scalar per vertex (defaults to 1.0 when the mesh
   * carries no `ao`), averaged over a triangle's 3 verts at shade time. */
  readonly ao: Float32Array;
  /** Model-space bounding-sphere radius (for whole-node culling). */
  readonly radius: number;
}

/** Internal framebuffer scale (the software fallback renders at half res). */
const INTERNAL_SCALE = 0.5;

const channel = (v: number): number => Math.max(0, Math.min(255, Math.round(v * 255)));

/** One triangle queued for rasterization (screen space + 1/w depth). */
interface RasterTri {
  readonly x0: number;
  readonly y0: number;
  readonly w0: number;
  readonly x1: number;
  readonly y1: number;
  readonly w1: number;
  readonly x2: number;
  readonly y2: number;
  readonly w2: number;
  readonly r: number;
  readonly g: number;
  readonly b: number;
  readonly opacity: number;
}

/** Create the Canvas2D software backend (always available). */
export const createCanvas2dBackend = (canvas: HTMLCanvasElement): RenderBackend => {
  const ctx = canvas.getContext("2d");
  if (ctx === null) {
    throw new Error("renderer: the 2D canvas context is unavailable");
  }
  const meshes = new Map<Handle, CpuMesh>();

  // The reduced-resolution framebuffer, rebuilt when the canvas size changes.
  let fbWidth = 0;
  let fbHeight = 0;
  let fbCanvas: HTMLCanvasElement | null = null;
  let fbCtx: CanvasRenderingContext2D | null = null;
  let image: ImageData | null = null;
  let pixels = new Uint32Array(0);
  let depth = new Float32Array(0);
  // Packed ABGR background pixel, recomputed each frame from `frame.clearColor`
  // (the store's `setClearColor`, default `CLEAR_COLOR`).
  const clearPixelOf = (rgb: readonly [number, number, number]): number =>
    (255 << 24) | (channel(rgb[2]) << 16) | (channel(rgb[1]) << 8) | channel(rgb[0]);

  const ensureFramebuffer = (): boolean => {
    const width = Math.max(1, Math.round(canvas.width * INTERNAL_SCALE));
    const height = Math.max(1, Math.round(canvas.height * INTERNAL_SCALE));
    if (fbWidth === width && fbHeight === height && image !== null) {
      return true;
    }
    fbWidth = width;
    fbHeight = height;
    fbCanvas = document.createElement("canvas");
    fbCanvas.width = width;
    fbCanvas.height = height;
    fbCtx = fbCanvas.getContext("2d");
    if (fbCtx === null) {
      return false;
    }
    image = fbCtx.createImageData(width, height);
    pixels = new Uint32Array(image.data.buffer);
    depth = new Float32Array(width * height);
    return true;
  };

  // Scratch buffer, grown on demand, reused across nodes and frames.
  let world = new Float32Array(3 * 1024);

  /** Rasterize one flat-shaded triangle with a 1/w depth test (perspective-
   * correct: 1/w interpolates linearly in screen space; bigger = nearer).
   * Solid triangles write depth; translucent ones test it and alpha-blend. */
  const rasterize = (tri: RasterTri): void => {
    const { x0, y0, w0, x1, y1, w1, x2, y2, w2 } = tri;
    const area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
    if (area === 0) return;
    const inv = 1 / area;
    const minX = Math.max(0, Math.floor(Math.min(x0, x1, x2)));
    const maxX = Math.min(fbWidth - 1, Math.ceil(Math.max(x0, x1, x2)));
    const minY = Math.max(0, Math.floor(Math.min(y0, y1, y2)));
    const maxY = Math.min(fbHeight - 1, Math.ceil(Math.max(y0, y1, y2)));
    if (minX > maxX || minY > maxY) return;

    const solid = tri.opacity >= 1;
    const packed = (255 << 24) | (tri.b << 16) | (tri.g << 8) | tri.r;
    const alpha = tri.opacity;

    for (let y = minY; y <= maxY; y += 1) {
      const py = y + 0.5;
      const rowBase = y * fbWidth;
      for (let x = minX; x <= maxX; x += 1) {
        const px = x + 0.5;
        // Barycentric weights (signed, normalized by the full area).
        const l0 = ((x1 - px) * (y2 - py) - (x2 - px) * (y1 - py)) * inv;
        const l1 = ((x2 - px) * (y0 - py) - (x0 - px) * (y2 - py)) * inv;
        const l2 = 1 - l0 - l1;
        if (l0 < 0 || l1 < 0 || l2 < 0) continue;
        const invW = l0 * w0 + l1 * w1 + l2 * w2;
        const index = rowBase + x;
        if (invW <= depth[index]!) continue;
        if (solid) {
          depth[index] = invW;
          pixels[index] = packed;
        } else {
          // Translucent: depth TEST above, no depth write; blend in software.
          const dst = pixels[index]!;
          const dr = dst & 0xff;
          const dg = (dst >> 8) & 0xff;
          const db = (dst >> 16) & 0xff;
          const nr = Math.round(tri.r * alpha + dr * (1 - alpha));
          const ng = Math.round(tri.g * alpha + dg * (1 - alpha));
          const nb = Math.round(tri.b * alpha + db * (1 - alpha));
          pixels[index] = (255 << 24) | (nb << 16) | (ng << 8) | nr;
        }
      }
    }
  };

  return {
    dropMeshes: (): void => {
      meshes.clear();
    },
    meshDetail: "low",
    name: "Canvas2D",
    render: (frame: SceneFrame): void => {
      if (!ensureFramebuffer() || image === null || fbCtx === null || fbCanvas === null) {
        return;
      }
      pixels.fill(clearPixelOf(frame.clearColor));
      depth.fill(0);

      const w = fbWidth;
      const h = fbHeight;
      const aspect = canvas.width / Math.max(1, canvas.height);
      const proj = perspective(frame.camera.fovY, aspect, frame.camera.near, frame.camera.far);
      const view = lookAt(frame.camera.position, frame.camera.target, { x: 0, y: 1, z: 0 });
      const viewProj = multiply(proj, view);
      const eye = frame.camera.position;
      // Forward axis of the camera (for culling and the near clip plane).
      let fx = frame.camera.target.x - eye.x;
      let fy = frame.camera.target.y - eye.y;
      let fz = frame.camera.target.z - eye.z;
      const flen = Math.sqrt(fx * fx + fy * fy + fz * fz) || 1;
      fx /= flen;
      fy /= flen;
      fz /= flen;
      // Pixels per world unit at unit distance (for projected-size culling).
      const pxPerUnit = h / (2 * Math.tan(frame.camera.fovY / 2));
      // The near clip plane in world space: keep points with
      // dot(p − planePoint, forward) ≥ 0.
      const nearDist = Math.max(frame.camera.near, 1e-3) * 1.01;
      const planeX = eye.x + fx * nearDist;
      const planeY = eye.y + fy * nearDist;
      const planeZ = eye.z + fz * nearDist;
      const planeSide = (px: number, py: number, pz: number): number =>
        (px - planeX) * fx + (py - planeY) * fy + (pz - planeZ) * fz;

      /** Project a world point; returns [x, y, 1/w] in framebuffer pixels.
       * Callers guarantee the point is on the visible side of the near plane. */
      const project = (wx: number, wy: number, wz: number): readonly [number, number, number] => {
        const clipX = viewProj[0]! * wx + viewProj[4]! * wy + viewProj[8]! * wz + viewProj[12]!;
        const clipY = viewProj[1]! * wx + viewProj[5]! * wy + viewProj[9]! * wz + viewProj[13]!;
        const clipW = Math.max(viewProj[3]! * wx + viewProj[7]! * wy + viewProj[11]! * wz + viewProj[15]!, 1e-5);
        return [((clipX / clipW) * 0.5 + 0.5) * w, (0.5 - (clipY / clipW) * 0.5) * h, 1 / clipW];
      };

      const translucent: RasterTri[] = [];

      for (const node of frame.nodes) {
        const mesh = meshes.get(node.mesh);
        const material = frame.materials.get(node.material);
        if (mesh === undefined || material === undefined) continue;
        const t = node.transform;
        const maxScale = Math.max(Math.abs(t.scale.x), Math.abs(t.scale.y), Math.abs(t.scale.z));
        // Whole-node cull: behind the camera, or projecting under half a pixel.
        const cx = t.position.x - eye.x;
        const cy = t.position.y - eye.y;
        const cz = t.position.z - eye.z;
        const along = cx * fx + cy * fy + cz * fz;
        const boundRadius = mesh.radius * maxScale;
        if (along + boundRadius < frame.camera.near) continue;
        if ((boundRadius * pxPerUnit) / Math.max(along, frame.camera.near) < 0.5) continue;

        const model: Mat4 = fromTrs(t.position, t.rotation, t.scale);
        const vertexCount = mesh.positions.length / 3;
        if (world.length < vertexCount * 3) {
          world = new Float32Array(vertexCount * 3);
        }
        for (let i = 0; i < vertexCount; i += 1) {
          const x = mesh.positions[i * 3]!;
          const y = mesh.positions[i * 3 + 1]!;
          const z = mesh.positions[i * 3 + 2]!;
          world[i * 3] = model[0]! * x + model[4]! * y + model[8]! * z + model[12]!;
          world[i * 3 + 1] = model[1]! * x + model[5]! * y + model[9]! * z + model[13]!;
          world[i * 3 + 2] = model[2]! * x + model[6]! * y + model[10]! * z + model[14]!;
        }

        const base = material.baseColor;
        const emissive = material.emissive;
        const opacity = material.opacity;
        const roughness = material.roughness;
        const meshAo = mesh.ao;
        const indices = mesh.indices;
        for (let i = 0; i < indices.length; i += 3) {
          const ia = indices[i]!;
          const ib = indices[i + 1]!;
          const ic = indices[i + 2]!;
          const a = ia * 3;
          const b = ib * 3;
          const c = ic * 3;
          // Flat AO for the triangle: the mean of its three vertices' occlusion,
          // the per-triangle analogue of the GPU's per-fragment interpolation.
          const aoTri = (meshAo[ia]! + meshAo[ib]! + meshAo[ic]!) / 3;
          const sa = planeSide(world[a]!, world[a + 1]!, world[a + 2]!);
          const sb = planeSide(world[b]!, world[b + 1]!, world[b + 2]!);
          const sc = planeSide(world[c]!, world[c + 1]!, world[c + 2]!);
          if (sa < 0 && sb < 0 && sc < 0) continue; // fully behind the near plane

          // World-space face normal (from the actual transformed triangle), and
          // the centroid it is lit at. Culling is off in the GL backend for
          // thin two-sided meshes, so instead of dropping back faces we flip
          // their normal toward the eye — the exact gl_FrontFacing behavior.
          let nx =
            (world[b + 1]! - world[a + 1]!) * (world[c + 2]! - world[a + 2]!) -
            (world[b + 2]! - world[a + 2]!) * (world[c + 1]! - world[a + 1]!);
          let ny =
            (world[b + 2]! - world[a + 2]!) * (world[c]! - world[a]!) -
            (world[b]! - world[a]!) * (world[c + 2]! - world[a + 2]!);
          let nz =
            (world[b]! - world[a]!) * (world[c + 1]! - world[a + 1]!) -
            (world[b + 1]! - world[a + 1]!) * (world[c]! - world[a]!);
          const nlen = Math.sqrt(nx * nx + ny * ny + nz * nz) || 1;
          nx /= nlen;
          ny /= nlen;
          nz /= nlen;
          const mx = (world[a]! + world[b]! + world[c]!) / 3;
          const my = (world[a + 1]! + world[b + 1]! + world[c + 1]!) / 3;
          const mz = (world[a + 2]! + world[b + 2]! + world[c + 2]!) / 3;
          const toEye = (eye.x - mx) * nx + (eye.y - my) * ny + (eye.z - mz) * nz;
          if (toEye < 0) {
            nx = -nx;
            ny = -ny;
            nz = -nz;
          }

          // Same shading truth as the WebGL2 shader: albedo-tinted, AO-attenuated
          // diffuse + neutral white specular/Fresnel, then the highlight tonemap.
          const shaded = shadeSurface(nx, ny, nz, mx, my, mz, eye.x, eye.y, eye.z, roughness, frame);
          const dif = shaded.diffuse;
          const spc = shaded.specular;
          const r = channel(tonemap(dif[0] * aoTri * base[0] + spc[0] + emissive[0]));
          const g = channel(tonemap(dif[1] * aoTri * base[1] + spc[1] + emissive[1]));
          const bl = channel(tonemap(dif[2] * aoTri * base[2] + spc[2] + emissive[2]));

          // Clip against the near plane (Sutherland–Hodgman) into 0–2 triangles.
          const src: readonly (readonly [number, number, number])[] = [
            [world[a]!, world[a + 1]!, world[a + 2]!],
            [world[b]!, world[b + 1]!, world[b + 2]!],
            [world[c]!, world[c + 1]!, world[c + 2]!],
          ];
          const out: [number, number, number][] = [];
          for (let k = 0; k < 3; k += 1) {
            const cur = src[k]!;
            const nxt = src[(k + 1) % 3]!;
            const curSide = planeSide(cur[0], cur[1], cur[2]);
            const nxtSide = planeSide(nxt[0], nxt[1], nxt[2]);
            if (curSide >= 0) out.push([cur[0], cur[1], cur[2]]);
            if ((curSide >= 0) !== (nxtSide >= 0)) {
              const tt = curSide / (curSide - nxtSide);
              out.push([
                cur[0] + (nxt[0] - cur[0]) * tt,
                cur[1] + (nxt[1] - cur[1]) * tt,
                cur[2] + (nxt[2] - cur[2]) * tt,
              ]);
            }
          }
          if (out.length < 3) continue;
          const p0 = project(out[0]![0], out[0]![1], out[0]![2]);
          let prev = project(out[1]![0], out[1]![1], out[1]![2]);
          for (let k = 2; k < out.length; k += 1) {
            const cur = project(out[k]![0], out[k]![1], out[k]![2]);
            const tri: RasterTri = {
              b: bl,
              g,
              opacity,
              r,
              w0: p0[2],
              w1: prev[2],
              w2: cur[2],
              x0: p0[0],
              x1: prev[0],
              x2: cur[0],
              y0: p0[1],
              y1: prev[1],
              y2: cur[1],
            };
            if (opacity >= 1) {
              rasterize(tri);
            } else {
              translucent.push(tri);
            }
            prev = cur;
          }
        }
      }

      // Translucent pass: depth-tested (against the opaque scene), no depth
      // writes, farthest first among themselves.
      translucent.sort((p, q) => Math.min(p.w0, p.w1, p.w2) - Math.min(q.w0, q.w1, q.w2));
      for (const tri of translucent) {
        rasterize(tri);
      }

      fbCtx.putImageData(image, 0, 0);
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(fbCanvas, 0, 0, canvas.width, canvas.height);
    },
    resize: (): void => {
      // The framebuffer follows canvas.width/height on the next render.
    },
    uploadMesh: (handle: Handle, data: MeshData): void => {
      const count = data.positions.length;
      const positions = new Float32Array(count * 3);
      // Per-vertex AO: absent -> 1.0 everywhere (a no-op multiply at shade time).
      const ao = new Float32Array(count).fill(1);
      const aoSrc = data.ao;
      let radius = 0;
      for (let i = 0; i < count; i += 1) {
        const p = data.positions[i]!;
        positions[i * 3] = p.x;
        positions[i * 3 + 1] = p.y;
        positions[i * 3 + 2] = p.z;
        ao[i] = aoSrc?.[i] ?? 1;
        radius = Math.max(radius, Math.sqrt(p.x * p.x + p.y * p.y + p.z * p.z));
      }
      meshes.set(handle, { ao, indices: new Uint32Array(data.indices), positions, radius });
    },
  };
};
