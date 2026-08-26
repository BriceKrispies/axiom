import * as THREE from 'three';

/**
 * PRE-BAKED SURFACE TEXTURES.
 *
 * The surface textures are a pure function of (glsl, seed, params, size), so on
 * a first visit the app was spending ~14 s of serial GPU shader compilation to
 * recompute something that never varies. `tools/bake-textures.mjs` computes them
 * once, at build time, with the same forge on a real GPU; this loads the result.
 *
 * Absence is not an error. A checkout that has never run the bake tool, or a
 * surface added since the last bake, simply falls through to the procedural
 * path — which is still the source of truth, and still what the tool runs to
 * produce these files. `?baked=0` forces that path for comparison.
 */

/** Where `tools/bake-textures.mjs` writes. Served from `public/` at the root. */
const BAKED_ROOT = 'baked';

/**
 * Fetch the manifest, or null if this build has no baked textures.
 *
 * Deliberately quiet: a 404 here is the ordinary state of a fresh checkout, and
 * an error in the console that every developer learns to ignore is worse than
 * no error at all.
 */
export async function loadBakedManifest() {
  try {
    const response = await fetch(`${BAKED_ROOT}/manifest.json`, { cache: 'force-cache' });
    if (!response.ok) return null;
    const manifest = await response.json();
    return manifest?.sets ? manifest : null;
  } catch {
    return null;
  }
}

/**
 * Decode one surface's maps into textures ready to be copied into the render
 * targets `TextureForge.allocate()` already handed out.
 *
 * `createImageBitmap` rather than an <img>: it decodes off the main thread,
 * which is the entire reason this path is cheaper than the one it replaces. A
 * decode that blocked the main thread would trade 14 s of shader compile for a
 * different stall and be no better.
 *
 * Every texture is marked `NoColorSpace`. The bytes in the file are the bytes
 * that were read back off the target — already in whatever encoding that target
 * used — so the copy must be a raw passthrough. Marking the albedo `sRGB` here
 * would linearise on sample and re-encode on write, and the surface would come
 * back visibly washed out.
 */
export async function loadBakedSet(entry) {
  const channels = ['albedo', 'orm', 'normal'];
  const loaded = await Promise.all(
    channels.map(async (channel) => {
      const file = entry.files?.[channel];
      if (!file) return null;
      const response = await fetch(`${BAKED_ROOT}/${file}`, { cache: 'force-cache' });
      if (!response.ok) return null;
      const bitmap = await createImageBitmap(await response.blob());
      const texture = new THREE.Texture(bitmap);
      texture.colorSpace = THREE.NoColorSpace;
      texture.flipY = false;
      texture.needsUpdate = true;
      return texture;
    })
  );
  return Object.fromEntries(channels.map((channel, i) => [channel, loaded[i]]));
}
