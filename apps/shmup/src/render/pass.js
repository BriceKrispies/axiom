import * as THREE from 'three';
import { FS_VERT } from './glsl.js';

/**
 * Full-screen triangle infrastructure. One shared geometry, one shared scene,
 * one shared camera — a pass is just a material we swap in. No allocation per
 * frame, no examples/jsm EffectComposer.
 */

const _geometry = new THREE.BufferGeometry();
_geometry.setAttribute(
  'position',
  new THREE.BufferAttribute(new Float32Array([-1, -1, 0, 3, -1, 0, -1, 3, 0]), 3)
);
_geometry.setAttribute('uv', new THREE.BufferAttribute(new Float32Array([0, 0, 2, 0, 0, 2]), 2));
_geometry.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e8);

const _scene = new THREE.Scene();
_scene.matrixAutoUpdate = false;
const _camera = new THREE.Camera();
const _mesh = new THREE.Mesh(_geometry, null);
_mesh.frustumCulled = false;
_mesh.matrixAutoUpdate = false;
_scene.add(_mesh);

/** Draw `material` over `target` (null = canvas). */
export function blit(renderer, material, target, clear = false, layer = 0) {
  _mesh.material = material;
  renderer.setRenderTarget(target, layer);
  if (clear) renderer.clear(true, false, false);
  renderer.render(_scene, _camera);
}

export function disposeFullScreen() {
  _geometry.dispose();
}

/**
 * Hand a full-screen material's program to the driver WITHOUT drawing it.
 *
 * The point is the asymmetry between the two halves of creating a program.
 * `compile()` issues the link and returns immediately; the driver then works
 * through its queue on its own thread and blocks nobody. The expensive half is
 * the REFLECTION the renderer does the first time it draws with the program —
 * `getUniformLocation` and friends — which blocks the main thread until the
 * driver has finished not just this program but everything queued ahead of it.
 * Measured cold on this app: 6 023 ms charged to a pass whose own program costs
 * 108 ms.
 *
 * So warming is not an optimisation of the pass, it is what lets the frame loop
 * draw the pass without ever paying that. Warm early, check `materialReady`,
 * and only put the pass in the frame when the answer is yes.
 *
 * It must be compiled against the SAME full-screen scene `blit()` draws with,
 * or three's program cache key differs and the warm compiles a program nothing
 * will ever use.
 */
export function warmFullScreen(renderer, material) {
  _mesh.material = material;
  renderer.compile(_scene, _camera);
}

/** Is this material's program linked and safe to draw without blocking? */
export function materialReady(renderer, material) {
  const program = renderer.properties?.get(material)?.currentProgram;
  // No program yet means nothing has been issued for it — not ready. A driver
  // without KHR_parallel_shader_compile has no isReady(), and on that driver a
  // linked program genuinely is ready.
  return !!program && (typeof program.isReady !== 'function' || program.isReady());
}

/** A post-processing pass: a ShaderMaterial plus the uniforms it owns. */
export class Pass {
  constructor(name, fragmentShader, uniforms, opts = {}) {
    this.name = name;
    this.uniforms = uniforms;
    this.material = new THREE.ShaderMaterial({
      name,
      uniforms,
      vertexShader: FS_VERT,
      fragmentShader,
      depthTest: false,
      depthWrite: false,
      blending: opts.blending ?? THREE.NoBlending,
      defines: opts.defines ?? {},
      glslVersion: opts.glslVersion ?? null,
      transparent: opts.blending !== undefined && opts.blending !== THREE.NoBlending,
    });
  }
  render(renderer, target, clear = false) {
    blit(renderer, this.material, target, clear);
  }
  dispose() {
    this.material.dispose();
  }
}

/** Half-float colour target with sane defaults for HDR post. */
export function hdrTarget(w, h, opts = {}) {
  const rt = new THREE.WebGLRenderTarget(Math.max(1, w), Math.max(1, h), {
    type: THREE.HalfFloatType,
    format: THREE.RGBAFormat,
    minFilter: THREE.LinearFilter,
    magFilter: THREE.LinearFilter,
    wrapS: THREE.ClampToEdgeWrapping,
    wrapT: THREE.ClampToEdgeWrapping,
    depthBuffer: false,
    stencilBuffer: false,
    generateMipmaps: false,
    ...opts,
  });
  rt.texture.name = opts.name ?? 'hdr';
  return rt;
}
