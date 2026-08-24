import * as THREE from 'three';
import { boot } from '../core/profile.js';
import { TextureForge } from './generator.js';
import { LIBRARY, resolveName } from './library.js';
import { extendMaterial, DEFAULT_PARAMS } from './shader.js';
import { bakeMasks, setMask } from './masks.js';

/**
 * Procedural PBR texture generation and the shared material library.
 *
 * There are no art assets in this project: every texel is rendered on the GPU
 * at boot from the noise stack in glsl/, packed into three 8-bit textures per
 * surface (albedo+height / ORM / tangent normal) and handed to a
 * MeshStandardMaterial extended with projection, parallax, detail, macro
 * variation and weathering (see shader.js).
 *
 * Public API — reach it with `ctx.get('materials')`:
 *
 *   get(name, opts?)          -> THREE.Material (cached; same opts, same instance)
 *   getTextureSet(name, opts?)-> { albedo, normal, orm, size, worldSize }
 *   variant(name, opts)       -> alias for get() with a fresh cache entry
 *   names()                   -> string[]
 *   surfaceOf(name)           -> one of the ARCHITECTURE.md surface tags
 *   bakeMasks(geometry, opts) -> geometry with wear/grime/AO vertex masks
 *   setGroundLevel(y)         -> where the ground-splash weathering starts
 *   detailNormal / macroTexture -> the shared micro/macro maps
 *
 * `opts` accepts anything in DEFAULT_PARAMS (scale, tint, uvMode, parallax,
 * weather, …) plus `three` for raw THREE material properties and `bake` to
 * force a distinct texture bake (a different paint colour, for example).
 */
export class MaterialSystem {
  static id = 'materials';
  static deps = ['render'];

  constructor(opts = {}) {
    /** Allows a standalone harness to drive the system without the engine. */
    this._injectedRenderer = opts.renderer ?? null;
    this._sets = new Map(); // bakeKey -> texture set
    this._materials = new Map(); // matKey  -> THREE.Material
    this._forge = null;
    this._shared = null;
    this._groundY = 0;
    this._built = false;
    this._warned = false;
    this._quality = 1;
    /** seconds since the last bake, for the scratch-target release below */
    this._idle = 0;
    this._scratchFreed = false;
    /** Surfaces allocated but not yet painted; drained by stream(). */
    this._pendingBakes = [];
    /** Set once stream() has finished: later requests bake synchronously. */
    this._streamClosed = false;
  }

  async init(ctx) {
    this.ctx = ctx;
    const q = ctx?.config?.q;
    this._anisotropy = q?.anisotropy ?? 8;
    // Texture budget scales with the quality preset; 1K is the reference.
    this._quality =
      ctx?.config?.quality === 'low' ? 0.5 : ctx?.config?.quality === 'medium' ? 0.75 : 1;
    this._tryBuild();
  }

  // ------------------------------------------------------------- internals --
  _renderer() {
    if (this._injectedRenderer) return this._injectedRenderer;
    const r = this.ctx?.peek?.('render');
    return r?.renderer ?? r?.getRenderer?.() ?? null;
  }

  _tryBuild() {
    if (this._built) return true;
    const renderer = this._renderer();
    if (!renderer) {
      if (!this._warned) {
        console.warn('[materials] no WebGLRenderer available yet — deferring texture bake');
        this._warned = true;
      }
      return false;
    }
    const t0 = performance.now();
    this._forge = new TextureForge(renderer, { anisotropy: this._anisotropy });
    // 1K, not 512: the micro tooth is 1.6-4 mm over a 0.25 m tile, which needs
    // ~6 texels per grain to survive mip 1 instead of averaging to flat grey.
    const detail = boot.time('mat:sharedDetail', () => this._forge.buildDetail(this._size(1024)));
    const macro = boot.time('mat:sharedMacro', () => this._forge.buildMacro(256));
    this._shared = {
      detailNormal: detail.normal,
      detailAlbedo: detail.albedo,
      macro: macro.albedo,
    };
    this._built = true;
    const ms = performance.now() - t0;
    if (ms > 30) console.info(`[materials] shared maps ${ms.toFixed(0)}ms`);
    return true;
  }

  _size(base) {
    const s = Math.max(128, Math.round((base * this._quality) / 128) * 128);
    // keep it a power of two so mip chains stay clean
    return 1 << Math.round(Math.log2(s));
  }

  /**
   * Names resolve through the alias table. An unknown name warns and falls back
   * to concrete rather than throwing — a typo in one subsystem must not take
   * the whole boot down.
   */
  _resolve(name) {
    const key = resolveName(name);
    if (LIBRARY[key]) return key;
    if (!this._missing) this._missing = new Set();
    if (!this._missing.has(name)) {
      this._missing.add(name);
      console.warn(`[materials] unknown surface "${name}" — falling back to concrete`);
    }
    return 'concrete';
  }

  _bakeKey(name, bake) {
    return `${name}|${bake.size}|${bake.seed}|${bake.tintA ?? ''}|${bake.tintB ?? ''}|${(
      bake.param ?? []
    ).join('_')}`;
  }

  /** Build (or fetch) the three packed textures for a surface. */
  getTextureSet(name, opts = {}) {
    const key = this._resolve(name);
    const def = LIBRARY[key];
    if (!this._tryBuild()) return null;

    const bake = { ...def.bake, ...(opts.bake ?? {}) };
    bake.size = this._size(bake.size);
    const cacheKey = this._bakeKey(key, bake);
    let set = this._sets.get(cacheKey);
    if (set) return set;

    const t0 = performance.now();
    this._idle = 0;
    this._scratchFreed = false;

    const forgeDef = {
      key,
      glsl: def.glsl,
      size: bake.size,
      seed: bake.seed ?? 1,
      worldSize: bake.worldSize,
      relief: bake.relief,
      tintA: bake.tintA !== undefined ? new THREE.Color(bake.tintA) : undefined,
      tintB: bake.tintB !== undefined ? new THREE.Color(bake.tintB) : undefined,
      param: bake.param ? new THREE.Vector4().fromArray(bake.param) : undefined,
    };

    // ALLOCATE NOW, PAINT LATER.
    //
    // The level asks for its sixteen surfaces while it is being assembled, and
    // baking them there costs ~0.6 s of the boot's critical path — a shader
    // compile and four 1K full-screen passes each. None of it has to happen
    // before the first frame: `allocate()` hands back the real texture objects,
    // primed to the surface's flat base colour, and `stream()` paints them over
    // the frames after the game is already on screen. Nothing rebinds, because
    // the texture objects never change.
    //
    // Measured with `node tools/bootprofile.mjs --samples`; the bakes show up
    // as `mat:bake:*` under `world:finalize`.
    const alloc = boot.time(`mat:alloc:${key}@${bake.size}`, () => this._forge.allocate(forgeDef));
    set = alloc.set;
    set.name = key;
    set.painted = false;
    this._sets.set(cacheKey, set);

    if (this._streamClosed) {
      // A surface requested after boot finished streaming (a late prop, a dev
      // reload). Nothing is going to drain the queue, so bake it here.
      this._paint(forgeDef, alloc, key, bake.size);
    } else {
      this._pendingBakes.push({ def: forgeDef, alloc, key, size: bake.size, set });
    }

    const ms = performance.now() - t0;
    if (ms > 40) console.info(`[materials] alloc ${key} ${bake.size}px ${ms.toFixed(0)}ms`);
    return set;
  }

  /** Run one queued bake into the targets `getTextureSet()` already handed out. */
  _paint(def, alloc, key, size) {
    boot.time(`mat:bake:${key}@${size}`, () => this._forge.paint(def, alloc.rts));
    alloc.set.painted = true;
  }

  /**
   * DEFERRED CONSTRUCTION — see src/core/streaming.js.
   *
   * One surface bake per yield. Ordering is request order, which is roughly
   * the order the level assembler needed them, so the ground and the walls the
   * player is looking at are painted before the dressing.
   *
   * `materials` is early in dependency order, so this generator is drained
   * first — the level gains its texture detail before the weapons and the
   * garrison stream in behind it.
   */
  *stream() {
    while (this._pendingBakes.length) {
      const job = this._pendingBakes.shift();
      this._paint(job.def, job.alloc, job.key, job.size);
      yield job.key;
    }
    // From here on there is nobody left to drain the queue.
    this._streamClosed = true;
  }

  /**
   * Every bake happens while the level is loading, but the half-float scratch
   * height targets the Sobel pass reads were being held for the whole session
   * (~10.5 MB of VRAM for 1K/512/256). Release them once the bake burst has
   * clearly finished; `TextureForge._heightRT()` recreates on demand, so a late
   * bake still produces exactly the same texture, it just re-allocates first.
   *
   * Nothing here touches a material, a uniform or a texture that is sampled, so
   * it cannot move a pixel — it only changes when a scratch buffer is freed.
   */
  update(dt) {
    if (this._scratchFreed || !this._forge) return;
    this._idle += dt > 0.25 ? 0.25 : dt; // ignore load-hitch dt spikes
    if (this._idle < 5) return;
    this._scratchFreed = true;
    this._forge.releaseScratch();
  }

  // ------------------------------------------------------------------ API --
  /**
   * Fetch a material. Identical (name, opts) return the identical instance so
   * meshes batch; pass any override to get a distinct variant.
   */
  get(name, opts = {}) {
    const key = this._resolve(name);
    const def = LIBRARY[key];

    const matKey = key + '|' + stableKey(opts);
    const cached = this._materials.get(matKey);
    if (cached) return cached;

    const set = this.getTextureSet(key, opts);
    const p = { ...DEFAULT_PARAMS, ...def.mat, ...opts };
    delete p.three;
    delete p.bake;
    p.groundY = opts.groundY ?? this._groundY;

    const threeProps = { ...(def.three ?? {}), ...(opts.three ?? {}) };
    const usePhysical = threeProps.physical === true;
    delete threeProps.physical;

    const Ctor = usePhysical ? THREE.MeshPhysicalMaterial : THREE.MeshStandardMaterial;
    const mat = new Ctor({
      color: 0xffffff,
      roughness: 1,
      metalness: 1,
      dithering: true,
    });
    mat.name = matKey;

    if (set) {
      mat.map = set.albedo;
      mat.normalMap = set.normal;
      mat.normalScale.set(1, 1);
      mat.roughnessMap = set.orm;
      // The height in albedo.a is only meaningful with the extension; keep the
      // stock alpha path off unless the surface is actually alpha-masked.
      if (!(p.alphaMask || threeProps.transparent)) mat.transparent = false;
    } else if (!this._warned) {
      console.warn(`[materials] "${key}" built without textures (no renderer)`);
    }

    if (p.vertexMasks) mat.vertexColors = true;
    applyProps(mat, threeProps);

    if (set) extendMaterial(mat, p, this._shared);

    this._materials.set(matKey, mat);
    return mat;
  }

  /** Explicit variant request — same as get(), reads better at the call site. */
  variant(name, opts = {}) {
    return this.get(name, opts);
  }

  /** All library names (aliases excluded). */
  names() {
    return Object.keys(LIBRARY);
  }

  /** The ARCHITECTURE.md surface tag for impact FX / audio / footsteps. */
  surfaceOf(name) {
    return LIBRARY[resolveName(name)]?.surface ?? 'concrete';
  }

  /** Live-update a material's uniforms after creation. */
  tune(material, changes = {}) {
    const u = material.userData?.owUniforms;
    if (!u) return material;
    if (changes.scale !== undefined) {
      const s = material.userData.owParams.uvMode === 'mesh' ? changes.scale : 1 / changes.scale;
      u.owTile.value.x = s;
      u.owTile.value.y = s;
    }
    if (changes.tint !== undefined) u.owTintCol.value.set(changes.tint);
    if (changes.parallax !== undefined) u.owParallaxP.value.x = changes.parallax;
    if (changes.groundY !== undefined) u.owGroundY.value = changes.groundY;
    if (changes.normalStrength !== undefined) u.owNormalAmp.value = changes.normalStrength;
    if (changes.weather !== undefined) u.owWeatherP.value.fromArray(changes.weather);
    return material;
  }

  /** Where the ground-splash weathering band sits, in world Y. */
  setGroundLevel(y) {
    this._groundY = y;
    for (const m of this._materials.values()) {
      const u = m.userData?.owUniforms;
      if (u) u.owGroundY.value = y;
    }
  }

  get detailNormal() {
    return this._shared?.detailNormal ?? null;
  }

  get macroTexture() {
    return this._shared?.macro ?? null;
  }

  bakeMasks(geometry, opts) {
    return bakeMasks(geometry, opts);
  }

  setMask(geometry, opts) {
    return setMask(geometry, opts);
  }

  /** Debug: a grid of spheres/panels showing every surface in the library. */
  debugBoard(opts = {}) {
    return buildDebugBoard(this, opts);
  }

  dispose() {
    for (const m of this._materials.values()) m.dispose();
    this._materials.clear();
    this._sets.clear();
    this._forge?.dispose();
    this._forge = null;
    this._shared = null;
    this._built = false;
  }
}

/**
 * Assigning a hex number over a THREE.Color property silently replaces the
 * Color object and produces NaN uniforms (a black material), so colour-valued
 * properties have to go through .set().
 */
function applyProps(mat, props) {
  for (const k in props) {
    const cur = mat[k];
    const v = props[k];
    if (cur && cur.isColor && !(v && v.isColor)) cur.set(v);
    else if (cur && cur.isVector2 && Array.isArray(v)) cur.fromArray(v);
    else mat[k] = v;
  }
  return mat;
}

function stableKey(opts) {
  const keys = Object.keys(opts).sort();
  if (!keys.length) return '';
  return keys.map((k) => `${k}=${JSON.stringify(opts[k])}`).join(',');
}

/**
 * A material test board — one sphere plus one bevelled panel per surface.
 * Lives here rather than in a test file so the capture harness and any other
 * subsystem can ask for it.
 */
function buildDebugBoard(system, { columns = 6, spacing = 1.25, radius = 0.42 } = {}) {
  const group = new THREE.Group();
  const names = system.names();
  const sphere = new THREE.SphereGeometry(radius, 64, 48);
  const panel = new THREE.BoxGeometry(0.92, 0.92, 0.14, 8, 8, 2);
  system.bakeMasks(panel, { wear: 1, grime: 0.9 });

  names.forEach((name, i) => {
    const x = (i % columns) * spacing;
    const y = -Math.floor(i / columns) * spacing;
    const mat = system.get(name, { vertexMasks: false });
    const s = new THREE.Mesh(sphere, mat);
    s.position.set(x, y, 0);
    s.castShadow = s.receiveShadow = true;
    group.add(s);

    const pm = system.get(name, { vertexMasks: true, localSpace: true });
    const b = new THREE.Mesh(panel, pm);
    b.position.set(x, y, -0.9);
    b.castShadow = b.receiveShadow = true;
    group.add(b);
  });
  group.userData.names = names;
  return group;
}

export { bakeMasks, setMask, LIBRARY };
