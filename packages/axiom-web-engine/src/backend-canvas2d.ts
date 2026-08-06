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
 * Softening the workload keeps it real-time: low-detail primitive meshes (half
 * the GPU path's facet budget) and whole-node culls — behind the camera, or a
 * bounding sphere projecting under half a pixel. Translucent triangles
 * rasterize after opaque ones with depth TEST but no depth WRITE, alpha-blended
 * in software.
 *
 * RESOLUTION is the other half of that budget, and it is the one the player can
 * see. This backend used to rasterize at a hard-coded HALF of the canvas backing
 * store, itself a fixed 960x600 regardless of how large the canvas was actually
 * displayed — so every edge in the scene was drawn at 480x300 and then stretched
 * across the real element, which is what made diagonals stair-step. The size now
 * comes from `render-quality.ts`: the canvas backing store is resolved from its
 * CSS box, the display's pixel ratio and the quality's render scale, and the
 * framebuffer matches it exactly, so the blit is 1:1 and a sample is a pixel.
 * Supersampling (`renderScale > 1`) is therefore real supersampling — more
 * samples across the same CSS box, downsampled by the browser on display.
 *
 * Note on `imageSmoothingEnabled`: it is gone, and it was never anti-aliasing.
 * It only ever interpolated the upscale of the finished half-res bitmap — it
 * cannot smooth the edges of polygons this rasterizer draws, because those edges
 * are already resolved into the pixel buffer by the time it would apply.
 */

import type { Handle, MeshData } from "./api.ts";
import type { RenderBackend, SceneFrame } from "./backend.ts";
import { type Mat4, lookAt, multiply, perspective } from "./mat4.ts";
import type { RenderQuality } from "./render-quality.ts";
import { diffuseOnly, shadeSurface, tonemap } from "./shading.ts";
import { SOFTWARE_DETAIL_SCALE } from "./tessellation.ts";

interface CpuMesh {
  /** xyz-interleaved model-space positions. */
  readonly positions: Float32Array;
  readonly indices: Uint32Array;
  /** One ambient-occlusion scalar per vertex (defaults to 1.0 when the mesh
   * carries no `ao`), averaged over a triangle's 3 verts at shade time. */
  readonly ao: Float32Array;
  /** Model-space bounding-sphere radius (for whole-node culling). */
  readonly radius: number;
  /** Model-space axis-aligned bounds, as [minX, minY, minZ, maxX, maxY, maxZ].
   * Kept alongside the sphere because the two answer different questions: the
   * sphere is the cheap conservative test for "could this be on screen at all",
   * the box is the tight test for "is the camera INSIDE this solid" — and for a
   * wide flat mesh those differ enormously (see `cullBackFaces` below). */
  readonly bounds: Float32Array;
  /** Whether the geometry is a closed solid, so its back faces can be skipped
   * (see `MeshData.closed`). Absent on custom geometry, which stays two-sided. */
  readonly closed: boolean;
}

/** The specular bucket for a matte material — identically zero, so the diffuse-only
 * fast path can reuse one frozen triple instead of allocating per triangle. */
const ZERO_SPECULAR: readonly [number, number, number] = [0, 0, 0];

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
export const createCanvas2dBackend = (canvas: HTMLCanvasElement, quality: RenderQuality): RenderBackend => {
  const ctx = canvas.getContext("2d");
  if (ctx === null) {
    throw new Error("renderer: the 2D canvas context is unavailable");
  }
  const meshes = new Map<Handle, CpuMesh>();

  // The framebuffer, rebuilt only when the canvas backing store changes size.
  let fbWidth = 0;
  let fbHeight = 0;
  let image: ImageData | null = null;
  let pixels = new Uint32Array(0);
  let depth = new Float32Array(0);
  // Packed ABGR background pixel, recomputed each frame from `frame.clearColor`
  // (the store's `setClearColor`, default `CLEAR_COLOR`).
  const clearPixelOf = (rgb: readonly [number, number, number]): number =>
    (255 << 24) | (channel(rgb[2]) << 16) | (channel(rgb[1]) << 8) | channel(rgb[0]);

  /** Match the framebuffer to the canvas backing store. Allocation happens here
   * and only on a real size change, never per frame. */
  const ensureFramebuffer = (): void => {
    const width = Math.max(1, canvas.width);
    const height = Math.max(1, canvas.height);
    if (fbWidth === width && fbHeight === height && image !== null) {
      return;
    }
    fbWidth = width;
    fbHeight = height;
    image = ctx.createImageData(width, height);
    pixels = new Uint32Array(image.data.buffer);
    depth = new Float32Array(width * height);
  };

  // Scratch buffer, grown on demand, reused across nodes and frames.
  let world = new Float32Array(3 * 1024);

  /**
   * Rasterize one flat-shaded triangle with a 1/w depth test (perspective-
   * correct: 1/w interpolates linearly in screen space; bigger = nearer).
   * Solid triangles write depth; translucent ones test it and alpha-blend.
   *
   * The two barycentrics are AFFINE in the pixel centre, so a row does not need
   * to re-derive them per pixel — which is what this loop used to do, at eight
   * multiplies and ten add/subs per pixel, in the hottest function of the
   * software path. Expanding the weight for the first vertex,
   *
   *   l0·area = (x1·y2 − x2·y1) + px·(y1 − y2) + py·(x2 − x1)
   *
   * shows the per-pixel change is the constant `(y1 − y2)/area`, so stepping x
   * costs one add per barycentric. Each ROW re-derives its own starting value
   * from the closed form rather than carrying an accumulator down the whole
   * triangle, so rounding cannot compound past a single row — the rendered frame
   * is bit-identical to the per-pixel form (verified against a frozen capture).
   *
   * Two more consequences of that same affinity carry the loop. A measured
   * decomposition of the treasure-chest scene put 47% of the whole frame in the
   * pixel walk and a further 14% in per-row setup — between them, three fifths of
   * the renderer — so both are worth expressing exactly rather than repeatedly:
   *
   *  - **1/w is affine in x as well**, because `invW = w2 + l0·(w0−w2) + l1·(w1−w2)`
   *    is a linear combination of affine terms. The depth value therefore needs ONE
   *    add per pixel, not three multiplies and two adds.
   *  - **the row's span is trimmed to its EXACT integer range once**, by walking the
   *    two outward-rounded ends inward until all three weights are non-negative.
   *    The covered set is contiguous — it is the intersection of three half-lines —
   *    so every pixel strictly between the trimmed ends is inside the triangle, and
   *    the three sign tests the walk used to pay PER PIXEL are retired. Coverage is
   *    unchanged: the same exact test decides the same pixels, asked about the two
   *    ends of a row instead of all of it. (Measured: 15% of stepped pixels were
   *    failing that test — about two per row, one at each end.)
   *  - **the span bounds cost no division.** Each bound is `−baseᵢ/stepLᵢ`, and both
   *    numerator and denominator carry the same `1/area` factor, which cancels:
   *    `−(cᵢ + aᵢ/2 + bᵢ·py)/aᵢ` is affine in `py`, so the bound is `Kᵢ + Mᵢ·py` for
   *    two constants resolved once per triangle. Six divisions per row become three
   *    multiply-adds.
   *
   * What is left per pixel is one add, one depth read, one compare and — only for a
   * pixel that wins the depth test — two stores. Verified output-preserving by
   * rendering the same frozen scene both ways in one page and comparing the two
   * framebuffers: 0 of 137,124 pixels differ, on the idle board, mid-reveal and
   * during the intro flight.
   */
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

    // Affine coefficients of the two barycentrics, before the 1/area scale.
    const a0 = y1 - y2;
    const b0 = x2 - x1;
    const c0 = x1 * y2 - x2 * y1;
    const a1 = y2 - y0;
    const b1 = x0 - x2;
    const c1 = x2 * y0 - x0 * y2;
    const stepL0 = a0 * inv;
    const stepL1 = a1 * inv;
    const stepL2 = -(stepL0 + stepL1);
    const HALF_PIXEL = 0.5;
    // 1/w as an affine function of the two free barycentrics, and its per-pixel step.
    const dw0 = w0 - w2;
    const dw1 = w1 - w2;
    const stepW = stepL0 * dw0 + stepL1 * dw1;
    // Each span bound as `K + M·py` (see the doc comment). The third weight is
    // `1 − l0 − l1`, so its coefficients are the negated sums of the other two, with
    // the `area` term the constant 1 contributes.
    //
    // A horizontal edge makes its `aᵢ` zero and these two constants infinite — which
    // is exactly the case the row loop below routes to the `stepLᵢ === 0` arm and
    // never reads them in. `stepLᵢ` IS `aᵢ · inv`, so the two agree by construction.
    const a2 = -(a0 + a1);
    const b2 = -(b0 + b1);
    const c2 = area - c0 - c1;
    const boundK0 = -(c0 + a0 * HALF_PIXEL) / a0;
    const boundM0 = -b0 / a0;
    const boundK1 = -(c1 + a1 * HALF_PIXEL) / a1;
    const boundM1 = -b1 / a1;
    const boundK2 = -(c2 + a2 * HALF_PIXEL) / a2;
    const boundM2 = -b2 / a2;

    for (let y = minY; y <= maxY; y += 1) {
      const py = y + HALF_PIXEL;
      const rowBase = y * fbWidth;
      // Each barycentric is AFFINE in x on this row: `li(x) = stepLi*x + basei`.
      // Solving `li(x) >= 0` for all three gives the row's SPAN — the pixels the
      // triangle can actually cover — so the loop below walks the span instead of
      // the bounding box. For a thin diagonal (a chest lid's edge, a palm frond)
      // the box is mostly empty, and walking it meant computing and rejecting far
      // more pixels than were ever filled.
      //
      // The bounds are rounded OUTWARD, and the exact per-pixel sign test is kept
      // below, so the span is a conservative skip and never a coverage decision:
      // the rendered frame is bit-identical to walking the whole box.
      const base0 = (c0 + a0 * HALF_PIXEL + b0 * py) * inv;
      const base1 = (c1 + a1 * HALF_PIXEL + b1 * py) * inv;
      const base2 = 1 - base0 - base1;
      let lo = minX;
      let hi = maxX;
      if (stepL0 > 0) lo = Math.max(lo, Math.floor(boundK0 + boundM0 * py));
      else if (stepL0 < 0) hi = Math.min(hi, Math.ceil(boundK0 + boundM0 * py));
      else if (base0 < 0) continue;
      if (stepL1 > 0) lo = Math.max(lo, Math.floor(boundK1 + boundM1 * py));
      else if (stepL1 < 0) hi = Math.min(hi, Math.ceil(boundK1 + boundM1 * py));
      else if (base1 < 0) continue;
      if (stepL2 > 0) lo = Math.max(lo, Math.floor(boundK2 + boundM2 * py));
      else if (stepL2 < 0) hi = Math.min(hi, Math.ceil(boundK2 + boundM2 * py));
      else if (base2 < 0) continue;
      if (lo > hi) continue;

      // Trim the outward-rounded ends inward to the EXACT covered range, so the
      // walk below owes no coverage test. Each step asks the same `l < 0` question
      // the per-pixel form asked; because the covered set is contiguous, deciding it
      // at the two ends decides it everywhere between them.
      let l0 = stepL0 * lo + base0;
      let l1 = stepL1 * lo + base1;
      while (lo <= hi && (l0 < 0 || l1 < 0 || 1 - l0 - l1 < 0)) {
        lo += 1;
        l0 += stepL0;
        l1 += stepL1;
      }
      let h0 = stepL0 * hi + base0;
      let h1 = stepL1 * hi + base1;
      while (hi >= lo && (h0 < 0 || h1 < 0 || 1 - h0 - h1 < 0)) {
        hi -= 1;
        h0 -= stepL0;
        h1 -= stepL1;
      }
      if (lo > hi) continue;

      let invW = w2 + l0 * dw0 + l1 * dw1;
      let index = rowBase + lo;
      const end = rowBase + hi;
      // Two walks rather than one with a per-pixel `solid` test: opacity is a
      // property of the TRIANGLE, and hoisting it out of the loop is the point of
      // having got the per-pixel work down to a handful of operations.
      if (solid) {
        for (; index <= end; index += 1, invW += stepW) {
          if (invW > depth[index]!) {
            depth[index] = invW;
            pixels[index] = packed;
          }
        }
      } else {
        // Translucent: depth TEST, no depth write; blend in software.
        for (; index <= end; index += 1, invW += stepW) {
          if (invW > depth[index]!) {
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
    }
  };

  return {
    dropMeshes: (): void => {
      meshes.clear();
    },
    detailScale: SOFTWARE_DETAIL_SCALE * quality.curveDetail,
    name: "Canvas2D",
    render: (frame: SceneFrame): void => {
      ensureFramebuffer();
      if (image === null) {
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

        // CACHED on the node by the store, rebuilt only on a re-pose — this used
        // to be a fresh `fromTrs` per node per frame here, in the GL backend and
        // in the DOM backend, three times over for the same numbers.
        const model: Mat4 = node.model;
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
        // Matte materials (the default, roughness >= 1) have identically-zero
        // specular + Fresnel, so every triangle takes the diffuse-only fast path:
        // byte-identical, but skips the eye vector, per-light Blinn-Phong lobe, and
        // Fresnel rim — the bulk of the per-triangle shading cost in software.
        const matte = roughness >= 1;
        const meshAo = mesh.ao;
        const indices = mesh.indices;
        // Back faces of a CLOSED, opaque solid are never the nearest surface along
        // any ray from outside it — the front face in front of them always wins the
        // depth test — so drawing them is pure waste: a shade, a near-plane clip and
        // a fill, all overdrawn. Skipping them removes roughly half the triangles of
        // every primitive in the scene.
        //
        // Three conditions, each guarding a case where a back face IS visible:
        //  - `mesh.closed`: open geometry (a sheet, a decal, custom data) is
        //    legitimately two-sided, so it keeps the normal-flip behaviour below.
        //  - opaque: a translucent solid shows its far wall through the near one,
        //    which is exactly what the GPU path blends, so it must keep both.
        //  - camera OUTSIDE the solid: standing inside a mesh (a room, an
        //    enclosing backdrop box) you see nothing BUT its back faces.
        //
        // That last test is against the mesh's BOX, not its bounding sphere, and
        // the difference is not a micro-optimization. A sphere around a wide, flat
        // mesh — a ground plane, a water slab, the scenery a top-down scene is
        // mostly made of — has a radius of half its DIAGONAL, so a camera looking
        // down at it from any normal height sits "inside" that sphere while being
        // nowhere near inside the solid. The sphere test therefore switched back-
        // face culling off for precisely the largest objects in the frame, the
        // ones whose hidden faces cost the most to draw: measured on the treasure-
        // chest scene, two ground/water slabs alone accounted for 72% of all pixel
        // coverage, roughly half of it faces that could never be seen.
        // The eye in MODEL space. `model`'s upper 3x3 is R·S, so its inverse is
        // S⁻¹·Rᵀ: dotting with a column and dividing by that axis' scale SQUARED
        // (once to undo the scale baked into the column, once for S⁻¹) inverts it
        // without building a second matrix.
        const ex = eye.x - t.position.x;
        const ey = eye.y - t.position.y;
        const ez = eye.z - t.position.z;
        const sx2 = t.scale.x * t.scale.x;
        const sy2 = t.scale.y * t.scale.y;
        const sz2 = t.scale.z * t.scale.z;
        const lx = (model[0]! * ex + model[1]! * ey + model[2]! * ez) / sx2;
        const ly = (model[4]! * ex + model[5]! * ey + model[6]! * ez) / sy2;
        const lz = (model[8]! * ex + model[9]! * ey + model[10]! * ez) / sz2;
        const bb = mesh.bounds;
        const insideSolid =
          lx >= bb[0]! && lx <= bb[3]! && ly >= bb[1]! && ly <= bb[4]! && lz >= bb[2]! && lz <= bb[5]!;
        const cullBackFaces = mesh.closed && opacity >= 1 && !insideSolid;
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
            // Facing away. On a closed opaque solid seen from outside, drop it
            // here — before the shade, the clip and the fill it would have paid.
            if (cullBackFaces) continue;
            nx = -nx;
            ny = -ny;
            nz = -nz;
          }

          // Same shading truth as the WebGL2 shader: albedo-tinted, AO-attenuated
          // diffuse + neutral white specular/Fresnel, then the highlight tonemap.
          // Matte nodes skip the specular half entirely (see `matte` above).
          const shaded = matte
            ? { diffuse: diffuseOnly(nx, ny, nz, mx, my, mz, frame), specular: ZERO_SPECULAR }
            : shadeSurface(nx, ny, nz, mx, my, mz, eye.x, eye.y, eye.z, roughness, frame);
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

      // The framebuffer IS the backing store, so this is a 1:1 blit: no
      // intermediate canvas, no scaling draw, no interpolation. `putImageData`
      // ignores the context transform by definition, which is exactly right —
      // these are already device pixels.
      ctx.putImageData(image, 0, 0);
    },
    resize: (): void => {
      // The framebuffer follows canvas.width/height on the next render, so there
      // is nothing to do here: `renderer.ts` owns the backing store and has
      // already resized it by the time this is called.
    },
    uploadMesh: (handle: Handle, data: MeshData): void => {
      const count = data.positions.length;
      const positions = new Float32Array(count * 3);
      // Per-vertex AO: absent -> 1.0 everywhere (a no-op multiply at shade time).
      const ao = new Float32Array(count).fill(1);
      const aoSrc = data.ao;
      let radius = 0;
      // [minX, minY, minZ, maxX, maxY, maxZ]. Seeded at +/-Infinity so an empty
      // mesh yields an inverted box that contains nothing — which is the honest
      // answer for "is the camera inside this?" when there is no geometry.
      const bounds = new Float32Array([
        Number.POSITIVE_INFINITY,
        Number.POSITIVE_INFINITY,
        Number.POSITIVE_INFINITY,
        Number.NEGATIVE_INFINITY,
        Number.NEGATIVE_INFINITY,
        Number.NEGATIVE_INFINITY,
      ]);
      for (let i = 0; i < count; i += 1) {
        const p = data.positions[i]!;
        positions[i * 3] = p.x;
        positions[i * 3 + 1] = p.y;
        positions[i * 3 + 2] = p.z;
        ao[i] = aoSrc?.[i] ?? 1;
        radius = Math.max(radius, Math.sqrt(p.x * p.x + p.y * p.y + p.z * p.z));
        bounds[0] = Math.min(bounds[0]!, p.x);
        bounds[1] = Math.min(bounds[1]!, p.y);
        bounds[2] = Math.min(bounds[2]!, p.z);
        bounds[3] = Math.max(bounds[3]!, p.x);
        bounds[4] = Math.max(bounds[4]!, p.y);
        bounds[5] = Math.max(bounds[5]!, p.z);
      }
      meshes.set(handle, { ao, bounds, closed: data.closed === true, indices: new Uint32Array(data.indices), positions, radius });
    },
  };
};
