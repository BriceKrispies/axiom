import * as THREE from 'three';

/**
 * READ THE BAKED SURFACE TEXTURES BACK OFF THE GPU, so they can be baked once
 * at build time and shipped instead of generated on every first visit.
 *
 * WHY THIS EXISTS. The nineteen procedural surface shaders are the single
 * largest block of work in a cold boot: ~14 s of serial GPU shader compilation,
 * for programs that each run four full-screen draws and are then thrown away.
 * Measured by skipping them outright, removing them is worth ~14 s off "settled"
 * and ~17 s off the weapon reaching the player's hands. Nothing else on the list
 * is close.
 *
 * The textures themselves are a pure function of (glsl, seed, params, size), so
 * they can be produced once. The only honest way to produce them is with the
 * same forge, on a real GPU, in a real browser — a CPU reimplementation would be
 * a second source of truth that could drift from the shader without anyone
 * noticing.
 *
 * So: drive the app's own `TextureForge`, read the render targets back, and hand
 * the bytes to the build tool. Dev-only, loaded behind `?bakedump=1`; it is not
 * part of the game.
 */

/** A full-screen triangle that samples one texture, for the readback pass. */
const READBACK_FRAG = /* glsl */ `
precision highp float;
uniform sampler2D tSrc;
varying vec2 vUv;
void main() { gl_FragColor = texture2D( tSrc, vUv ); }
`;

const READBACK_VERT = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = vec4( position.xy, 0.0, 1.0 );
}
`;

/**
 * Copy `texture` into a fresh RGBA8 target and read the pixels back.
 *
 * Round-tripping through a draw rather than reading the source target directly
 * is deliberate: the forge's targets differ in colour space (albedo is
 * sRGB-encoded by the hardware, ORM and normal are linear) and in whether they
 * carry mips, and `readRenderTargetPixels` on a target that is currently bound
 * to a material is a good way to read a stale mip. One neutral copy makes every
 * surface come back in the same, known form.
 */
function readTexture(renderer, texture, size) {
  const rt = new THREE.WebGLRenderTarget(size, size, {
    type: THREE.UnsignedByteType,
    format: THREE.RGBAFormat,
    colorSpace: texture.colorSpace ?? THREE.NoColorSpace,
    depthBuffer: false,
    stencilBuffer: false,
    generateMipmaps: false,
  });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute(
    'position',
    new THREE.BufferAttribute(new Float32Array([-1, -1, 0, 3, -1, 0, -1, 3, 0]), 3)
  );
  geometry.setAttribute('uv', new THREE.BufferAttribute(new Float32Array([0, 0, 2, 0, 0, 2]), 2));
  const material = new THREE.ShaderMaterial({
    uniforms: { tSrc: { value: texture } },
    vertexShader: READBACK_VERT,
    fragmentShader: READBACK_FRAG,
    depthTest: false,
    depthWrite: false,
  });
  const scene = new THREE.Scene();
  scene.add(new THREE.Mesh(geometry, material));
  const camera = new THREE.Camera();

  const prev = renderer.getRenderTarget();
  renderer.setRenderTarget(rt);
  renderer.render(scene, camera);
  const pixels = new Uint8Array(size * size * 4);
  renderer.readRenderTargetPixels(rt, 0, 0, size, size, pixels);
  renderer.setRenderTarget(prev);

  rt.dispose();
  geometry.dispose();
  material.dispose();
  return pixels;
}

/**
 * Encode raw RGBA into an image blob.
 *
 * WebP, not PNG. These are high-entropy noise fields, which is the worst case
 * for PNG's filters — and the encoder is the browser's own, so this needs no
 * build dependency at all. Lossless by default because the ORM and normal maps
 * are DATA: a lossy roughness channel is a visibly different surface, and a
 * lossy normal map is a visibly wrong one.
 */
async function encode(pixels, size, { type = 'image/webp', quality = 1 } = {}) {
  const canvas = new OffscreenCanvas(size, size);
  const ctx = canvas.getContext('2d');
  const image = ctx.createImageData(size, size);
  // WebGL reads bottom-up, canvas writes top-down.
  const stride = size * 4;
  for (let y = 0; y < size; y++) {
    const src = (size - 1 - y) * stride;
    image.data.set(pixels.subarray(src, src + stride), y * stride);
  }
  ctx.putImageData(image, 0, 0);
  const blob = await canvas.convertToBlob({ type, quality });
  const buffer = await blob.arrayBuffer();
  return { bytes: new Uint8Array(buffer), type: blob.type };
}

/**
 * Install `window.__DUMPBAKES__(opts)` — returns one entry per baked surface,
 * each carrying its albedo/orm/normal as encoded image bytes plus the metadata
 * the runtime loader needs to rebuild the set without the forge.
 */
export function installBakeDump(engine) {
  const dump = async ({ type = 'image/webp', quality = 1, only = null } = {}) => {
    const materials = engine.ctx.peek('materials');
    const renderer = engine.ctx.peek('render')?.renderer;
    if (!materials || !renderer) return { ok: false, reason: 'no materials/renderer' };

    const out = [];
    for (const [key, set] of materials._sets) {
      if (only && !only.includes(key)) continue;
      const maps = {};
      for (const channel of ['albedo', 'orm', 'normal']) {
        const texture = set[channel];
        if (!texture) continue;
        const pixels = readTexture(renderer, texture, set.size);
        const encoded = await encode(pixels, set.size, { type, quality });
        maps[channel] = encoded;
      }
      out.push({
        // The SIZE-INDEPENDENT identity, not the runtime cache key: a shipped
        // asset is a recipe, and it has to be findable by a game that asked for
        // a different resolution. See MaterialSystem._bakeIdentity().
        key: set.bakedId ?? key,
        name: set.name ?? key,
        size: set.size,
        worldSize: set.worldSize,
        relief: set.relief,
        maps,
      });
    }
    return { ok: true, sets: out };
  };

  // The bytes cannot cross the CDP boundary as a Uint8Array; the tool asks for
  // base64 and pays the 33% on a channel that is not the game's.
  window.__DUMPBAKES__ = async (opts) => {
    const result = await dump(opts);
    if (!result.ok) return result;
    const toB64 = (u8) => {
      let s = '';
      for (let i = 0; i < u8.length; i += 0x8000) {
        s += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
      }
      return btoa(s);
    };
    return {
      ok: true,
      sets: result.sets.map((entry) => ({
        ...entry,
        maps: Object.fromEntries(
          Object.entries(entry.maps).map(([k, v]) => [
            k,
            { type: v.type, bytes: v.bytes.length, b64: toB64(v.bytes) },
          ])
        ),
      })),
    };
  };

  /** Sizes only — for deciding format and resolution without moving the bytes. */
  window.__DUMPBAKESIZES__ = async (opts) => {
    const result = await dump(opts);
    if (!result.ok) return result;
    return {
      ok: true,
      sets: result.sets.map((e) => ({
        key: e.key,
        size: e.size,
        bytes: Object.fromEntries(Object.entries(e.maps).map(([k, v]) => [k, v.bytes.length])),
      })),
    };
  };
}
