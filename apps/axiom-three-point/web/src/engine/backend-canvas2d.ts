/*
 * engine/backend-canvas2d.ts — the SOFTWARE drawing fallback: a flat-shaded
 * painter's-algorithm rasterizer over the plain 2D canvas API, auto-selected
 * when WebGL2 is unavailable (or forced with `?backend=canvas2d`). Per frame it
 * transforms every visible node's vertices to world space, lights each triangle
 * ONCE at its centroid with the SAME Lambert model as the WebGL2 backend
 * (shared ambient / falloff constants from `backend.ts`), projects to screen,
 * sorts every triangle back-to-front, and fills them as 2D paths.
 *
 * Softening the workload keeps it real-time: the store feeds this backend
 * low-detail primitive meshes (`meshDetail: "low"`), nodes are skipped whole
 * when their bounding sphere projects smaller than half a pixel or sits behind
 * the camera (this is what parks off-court pool entities for free), back-facing
 * triangles of CLOSED primitives are culled, and sub-pixel triangles are
 * dropped with a threshold small enough that finely-tessellated distant meshes
 * still keep coverage.
 */

import type { Handle, MeshData } from "./api.ts";
import { type FrameNode, type RenderBackend, type SceneFrame, AMBIENT, CLEAR_COLOR } from "./backend.ts";
import { type Mat4, fromTrs, lookAt, multiply, perspective } from "./mat4.ts";

interface CpuMesh {
  /** xyz-interleaved model-space positions. */
  readonly positions: Float32Array;
  readonly indices: Uint32Array;
  /** Model-space bounding-sphere radius (for whole-node culling). */
  readonly radius: number;
}

interface DrawTri {
  /** Painter key: view depth at the centroid (bigger = farther, drawn first). */
  readonly depth: number;
  readonly x0: number;
  readonly y0: number;
  readonly x1: number;
  readonly y1: number;
  readonly x2: number;
  readonly y2: number;
  readonly style: string;
  readonly opacity: number;
}

/** Skip triangles with screen area below this (px²) — small enough that dense
 * distant tessellation keeps coverage instead of eroding away. */
const MIN_TRI_AREA = 0.15;

/**
 * The shared Lambert term: ambient + Σ directional + Σ point (soft
 * 1/(1+0.08·d²) falloff), exactly the WebGL2 fragment math evaluated once per
 * triangle. Exported for the render tests, so both backends provably match.
 */
export const lambertLight = (
  nx: number,
  ny: number,
  nz: number,
  px: number,
  py: number,
  pz: number,
  frame: Pick<SceneFrame, "dirLights" | "pointLights">,
): readonly [number, number, number] => {
  let r = AMBIENT;
  let g = AMBIENT;
  let b = AMBIENT;
  for (const light of frame.dirLights) {
    const lambert = Math.max(0, -(nx * light.direction[0] + ny * light.direction[1] + nz * light.direction[2]));
    r += lambert * light.color[0];
    g += lambert * light.color[1];
    b += lambert * light.color[2];
  }
  for (const light of frame.pointLights) {
    const tx = light.position[0] - px;
    const ty = light.position[1] - py;
    const tz = light.position[2] - pz;
    const d = Math.sqrt(tx * tx + ty * ty + tz * tz);
    const inv = 1 / Math.max(d, 1e-5);
    const lambert = Math.max(0, (nx * tx + ny * ty + nz * tz) * inv) / (1 + 0.08 * d * d);
    r += lambert * light.color[0];
    g += lambert * light.color[1];
    b += lambert * light.color[2];
  }
  return [r, g, b];
};

const channel = (v: number): number => Math.max(0, Math.min(255, Math.round(v * 255)));

/** Create the Canvas2D software backend (always available). */
export const createCanvas2dBackend = (canvas: HTMLCanvasElement): RenderBackend => {
  const ctx = canvas.getContext("2d");
  if (ctx === null) {
    throw new Error("renderer: the 2D canvas context is unavailable");
  }
  const meshes = new Map<Handle, CpuMesh>();

  // Scratch buffers, grown on demand, reused across nodes and frames.
  let world = new Float32Array(3 * 1024);
  let screen = new Float32Array(3 * 1024);

  return {
    dropMeshes: (): void => {
      meshes.clear();
    },
    meshDetail: "low",
    name: "Canvas2D",
    render: (frame: SceneFrame): void => {
      const w = canvas.width;
      const h = canvas.height;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.globalAlpha = 1;
      ctx.fillStyle = `rgb(${channel(CLEAR_COLOR[0])} ${channel(CLEAR_COLOR[1])} ${channel(CLEAR_COLOR[2])})`;
      ctx.fillRect(0, 0, w, h);

      const aspect = w / Math.max(1, h);
      const proj = perspective(frame.camera.fovY, aspect, frame.camera.near, frame.camera.far);
      const view = lookAt(frame.camera.position, frame.camera.target, { x: 0, y: 1, z: 0 });
      const viewProj = multiply(proj, view);
      const eye = frame.camera.position;
      // Forward axis of the camera (for whole-node behind-camera culling).
      let fx = frame.camera.target.x - eye.x;
      let fy = frame.camera.target.y - eye.y;
      let fz = frame.camera.target.z - eye.z;
      const flen = Math.sqrt(fx * fx + fy * fy + fz * fz) || 1;
      fx /= flen;
      fy /= flen;
      fz /= flen;
      // Pixels per world unit at unit distance (for projected-size culling).
      const pxPerUnit = h / (2 * Math.tan(frame.camera.fovY / 2));

      const tris: DrawTri[] = [];

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
          screen = new Float32Array(vertexCount * 3);
        }
        for (let i = 0; i < vertexCount; i += 1) {
          const x = mesh.positions[i * 3]!;
          const y = mesh.positions[i * 3 + 1]!;
          const z = mesh.positions[i * 3 + 2]!;
          const wx = model[0]! * x + model[4]! * y + model[8]! * z + model[12]!;
          const wy = model[1]! * x + model[5]! * y + model[9]! * z + model[13]!;
          const wz = model[2]! * x + model[6]! * y + model[10]! * z + model[14]!;
          world[i * 3] = wx;
          world[i * 3 + 1] = wy;
          world[i * 3 + 2] = wz;
          const clipX = viewProj[0]! * wx + viewProj[4]! * wy + viewProj[8]! * wz + viewProj[12]!;
          const clipY = viewProj[1]! * wx + viewProj[5]! * wy + viewProj[9]! * wz + viewProj[13]!;
          const clipW = viewProj[3]! * wx + viewProj[7]! * wy + viewProj[11]! * wz + viewProj[15]!;
          // w_clip = distance along the view axis; ≤ near means unusable.
          screen[i * 3 + 2] = clipW;
          if (clipW > 1e-5) {
            screen[i * 3] = ((clipX / clipW) * 0.5 + 0.5) * w;
            screen[i * 3 + 1] = (0.5 - (clipY / clipW) * 0.5) * h;
          }
        }

        const base = material.baseColor;
        const emissive = material.emissive;
        const indices = mesh.indices;
        for (let i = 0; i < indices.length; i += 3) {
          const a = indices[i]! * 3;
          const b = indices[i + 1]! * 3;
          const c = indices[i + 2]! * 3;
          const wa = screen[a + 2]!;
          const wb = screen[b + 2]!;
          const wc = screen[c + 2]!;
          if (wa <= 1e-5 || wb <= 1e-5 || wc <= 1e-5) continue; // clipped at/behind the eye
          const x0 = screen[a]!;
          const y0 = screen[a + 1]!;
          const x1 = screen[b]!;
          const y1 = screen[b + 1]!;
          const x2 = screen[c]!;
          const y2 = screen[c + 1]!;
          const area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
          if (Math.abs(area) < MIN_TRI_AREA * 2) continue;

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

          const lit = lambertLight(nx, ny, nz, mx, my, mz, frame);
          tris.push({
            depth: (wa + wb + wc) / 3,
            opacity: material.opacity,
            style: `rgb(${channel(base[0] * lit[0] + emissive[0])} ${channel(base[1] * lit[1] + emissive[1])} ${channel(
              base[2] * lit[2] + emissive[2],
            )})`,
            x0,
            y0,
            x1,
            y1,
            x2,
            y2,
          });
        }
      }

      // Painter's algorithm: farthest first.
      tris.sort((p, q) => q.depth - p.depth);
      let alpha = 1;
      for (const tri of tris) {
        if (tri.opacity !== alpha) {
          alpha = tri.opacity;
          ctx.globalAlpha = alpha;
        }
        ctx.fillStyle = tri.style;
        ctx.beginPath();
        ctx.moveTo(tri.x0, tri.y0);
        ctx.lineTo(tri.x1, tri.y1);
        ctx.lineTo(tri.x2, tri.y2);
        ctx.closePath();
        ctx.fill();
      }
      ctx.globalAlpha = 1;
    },
    resize: (): void => {
      // Nothing retained — the next render reads canvas.width/height directly.
    },
    uploadMesh: (handle: Handle, data: MeshData): void => {
      const count = data.positions.length;
      const positions = new Float32Array(count * 3);
      let radius = 0;
      for (let i = 0; i < count; i += 1) {
        const p = data.positions[i]!;
        positions[i * 3] = p.x;
        positions[i * 3 + 1] = p.y;
        positions[i * 3 + 2] = p.z;
        radius = Math.max(radius, Math.sqrt(p.x * p.x + p.y * p.y + p.z * p.z));
      }
      meshes.set(handle, { indices: new Uint32Array(data.indices), positions, radius });
    },
  };
};
