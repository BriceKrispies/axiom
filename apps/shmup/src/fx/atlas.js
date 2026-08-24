import * as THREE from 'three';
import {
  P,
  D,
  paintParticleAtlas,
  paintDecalAtlas,
  paintBrass,
} from './atlasbake.js';

/**
 * FX ATLAS TEXTURES — the main-thread half: pixels in, GPU textures out.
 *
 * The painting itself lives in atlasbake.js, which imports no THREE and is
 * therefore runnable inside a bakery worker (src/core/bakery.js). This file is
 * everything that can only happen where the WebGL context is: wrapping the
 * bytes in `THREE.DataTexture` with the right colour space, wrap mode and mip
 * settings.
 *
 * The split is why `FxSystem.init` can hand ~1.1 s of value-noise evaluation to
 * a worker and get back transferable buffers, instead of blocking boot on it.
 *
 * Each builder comes in two forms:
 *   `buildXFromPixels(painted, ...)`  wrap an already-painted result
 *   `buildX(seed, size)`              paint synchronously, then wrap
 * The synchronous form is what runs when no worker is available, and it calls
 * the same painter, so the two paths cannot produce different pixels.
 *
 * `P` and `D` (the tile-index tables) are re-exported so the seven fx modules
 * that pick sprites by name did not have to change their imports.
 */

export { P, D };

function makeTexture(data, size, { srgb, mips = true, name }) {
  const t = new THREE.DataTexture(data, size, size, THREE.RGBAFormat, THREE.UnsignedByteType);
  t.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
  t.wrapS = t.wrapT = THREE.ClampToEdgeWrapping;
  t.minFilter = mips ? THREE.LinearMipmapLinearFilter : THREE.LinearFilter;
  t.magFilter = THREE.LinearFilter;
  t.generateMipmaps = mips;
  t.anisotropy = 4;
  t.name = name;
  t.needsUpdate = true;
  return t;
}

/* ------------------------------------------------------------- particles -- */

export function buildParticleAtlasFromPixels({ pixels, cols, size }) {
  return {
    texture: makeTexture(pixels, size, { srgb: true, name: 'fx-particles' }),
    cols,
    size,
  };
}

export function buildParticleAtlas(seed, size = 1024) {
  return buildParticleAtlasFromPixels(paintParticleAtlas({ seed, size }));
}

/* ---------------------------------------------------------------- decals -- */

export function buildDecalAtlasFromPixels({ albedo, normal, orm, cols, size }) {
  return {
    albedo: makeTexture(albedo, size, { srgb: true, name: 'fx-decal-albedo' }),
    normal: makeTexture(normal, size, { srgb: false, name: 'fx-decal-normal' }),
    orm: makeTexture(orm, size, { srgb: false, name: 'fx-decal-orm' }),
    cols,
    size,
  };
}

export function buildDecalAtlas(seed, size = 1024) {
  return buildDecalAtlasFromPixels(paintDecalAtlas({ seed, size }));
}

/* ----------------------------------------------------------------- brass -- */

export function buildBrassTexturesFromPixels({ normal, orm, size }) {
  const nt = makeTexture(normal, size, { srgb: false, name: 'fx-brass-normal' });
  const ot = makeTexture(orm, size, { srgb: false, name: 'fx-brass-orm' });
  // A casing is a tube: the maps tile around it, unlike the atlases above.
  nt.wrapS = nt.wrapT = THREE.RepeatWrapping;
  ot.wrapS = ot.wrapT = THREE.RepeatWrapping;
  return { normal: nt, orm: ot };
}

export function buildBrassTextures(seed, size = 256) {
  return buildBrassTexturesFromPixels(paintBrass({ seed, size }));
}
