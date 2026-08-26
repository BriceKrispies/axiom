import * as THREE from 'three';
import { boot } from '../core/profile.js';
import { WAIT } from '../core/streaming.js';
import { TextureForge } from './generator.js';
import { LIBRARY, resolveName } from './library.js';
import { LEAN } from '../core/fidelity.js';

/**
 * The lean surface map: every library surface folded onto one of four
 * survivors — a wall, a ground, a metal and a glass. Anything absent keeps its
 * own identity (foliage and fabric read as broken if they become concrete).
 */
/**
 * THREE MATERIAL PROPERTIES THAT COST A WHOLE PROGRAM, dropped under `lean`.
 *
 * Folding nineteen library surfaces onto four (LEAN_SURFACE, below) cut the
 * VARIETY but not the program count: measured after that fold, the world
 * surfaces were still nineteen distinct programs out of sixty. The custom cache
 * key here is already canonical — it is the sorted OW defines and nothing else
 * (see shader.js) — so the duplication was coming from three's OWN key.
 *
 * Every name below flips a `USE_*` define or changes the material class, and so
 * forces three to build a separate program even when two surfaces are otherwise
 * identical:
 *
 *   physical                          MeshPhysicalMaterial, a different shader
 *   sheen*                            USE_SHEEN
 *   anisotropy*                       USE_ANISOTROPY
 *   specularIntensity/Color           USE_SPECULAR
 *   clearcoat*                        USE_CLEARCOAT
 *   iridescence*                      USE_IRIDESCENCE
 *   transmission/thickness/attenu*    USE_TRANSMISSION
 *   toneMapped                        adds or drops the tone-mapping chunk
 *   flatShading                       FLAT_SHADED
 *   alphaTest                         USE_ALPHATEST
 *
 * What is NOT here matters as much: colour, tint, opacity, transparency, side,
 * emissive, emissive intensity and envMapIntensity are all uniforms or blend
 * state. They cost nothing at compile time, so a lean surface keeps every one of
 * them and can still be any colour, glow, or be see-through. What it loses is
 * the sheen on the fabric and the anisotropic streak on the brushed metal.
 */
const LEAN_STRIP = [
  'physical',
  'sheen',
  'sheenRoughness',
  'sheenColor',
  'anisotropy',
  'anisotropyRotation',
  'specularIntensity',
  'specularColor',
  'clearcoat',
  'clearcoatRoughness',
  'iridescence',
  'iridescenceIOR',
  'iridescenceThicknessRange',
  'transmission',
  'thickness',
  'attenuationDistance',
  'attenuationColor',
  'toneMapped',
  'flatShading',
  'alphaTest',
];

const LEAN_SURFACE = {
  concrete: 'concrete',
  concrete_floor: 'concrete',
  brick: 'concrete',
  plaster: 'concrete',
  tile: 'concrete',
  wood: 'concrete',
  asphalt: 'asphalt',
  sand: 'asphalt',
  dirt: 'asphalt',
  gravel: 'asphalt',
  metal_rust: 'metal_painted',
  metal_painted: 'metal_painted',
  metal_brushed: 'metal_painted',
  corrugated: 'metal_painted',
  rubber: 'metal_painted',
  burlap: 'fabric',
  fabric: 'fabric',
  glass: 'glass',
  foliage: 'foliage',
};
import { extendMaterial, DEFAULT_PARAMS } from './shader.js';
import { bakeMasks, setMask } from './masks.js';
import { loadBakedManifest, loadBakedSet } from './baked.js';

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
    /** While true, stream() paints nothing — see the note on stream(). */
    this._holdBakes = false;
    /** Set from ctx.config.progressiveBoot in init(). */
  }

  /**
   * Hold or release the queued surface bakes.
   *
   * The composing app owns this, because it is the only place that knows what
   * else is competing for the driver's one shader-compile thread.
   */
  holdBakes(on) {
    this._holdBakes = !!on;
    if (!on) this._idle = 0;
  }

  async init(ctx) {
    this.ctx = ctx;
    // Progressive boot: the surface bakes are the most expensive shader work in
    // the app and the least urgent. See the note on stream().
    this._holdBakes = !!(ctx?.config?.holdBakes ?? ctx?.config?.progressiveBoot);
    this._skipBakes = !!ctx?.config?.skipBakes;
    this._bakeWarm = !!ctx?.config?.bakeWarm;
    // PRE-BAKED TEXTURES, if this build has them. Started here and never
    // awaited on the critical path: the level is assembled from primed
    // textures either way, and stream() is what needs the answer.
    // `?baked=0` forces the procedural path — the comparison that says what
    // shipping them is worth, and the fallback if an asset is ever wrong.
    this._bakedManifest = undefined;
    const wanted = ctx?.config?.bakedTextures !== false;
    (wanted ? loadBakedManifest() : Promise.resolve(null)).then((m) => {
      this._bakedManifest = m ?? null;
      m && console.info(`[materials] pre-baked textures: ${Object.keys(m.sets).length} surfaces`);
    });
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
    // Deferred under progressive boot along with the surface bakes: these two
    // are 418 ms of cold shader compile between the player and a level they
    // could be walking through, and the only thing that samples them is a lit
    // material — which the fidelity ramp has replaced with a stand-in that has
    // no detail input at all. The texture objects are real either way, so
    // nothing rebinds when they are painted later.
    const defer = { defer: this._holdBakes };
    const detail = boot.time('mat:sharedDetail', () => this._forge.buildDetail(this._size(1024), 1, defer));
    const macro = boot.time('mat:sharedMacro', () => this._forge.buildMacro(256, 2, defer));
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
    // LEAN: NINETEEN SURFACES BECOME FOUR.
    //
    // Cold boot is (number of lit materials) x (~100 KB of translated HLSL
    // each), and the second factor is not movable — it is three's PBR core plus
    // the surface composition that makes these materials work at all. The count
    // is. Every surface the level stops using is a whole program the driver
    // never has to translate.
    //
    // This is the largest single reduction available and also the most visible
    // one: brick, plaster and tile all become concrete; sand, dirt and gravel
    // all become asphalt; every metal becomes one metal. The level keeps its
    // shapes, its lighting and its tints — the per-surface texture identity is
    // what goes.
    if (LEAN) return LEAN_SURFACE[key] ?? (LIBRARY[key] ? key : 'concrete');
    if (LIBRARY[key]) return key;
    if (!this._missing) this._missing = new Set();
    if (!this._missing.has(name)) {
      this._missing.add(name);
      console.warn(`[materials] unknown surface "${name}" — falling back to concrete`);
    }
    return 'concrete';
  }

  /**
   * WHAT A BAKED SURFACE IS, INDEPENDENT OF HOW BIG IT WAS BAKED.
   *
   * `_bakeKey` embeds the size, because two resolutions of the same surface are
   * genuinely different texture sets at runtime. A shipped asset is not: it is
   * the recipe, and the runtime blits it into whatever target it allocated, so a
   * 512 bake can serve a 1024 request at the cost of some sharpness. Keying the
   * manifest on the size would mean a smaller bake produced files the game never
   * asks for — which it did, and the level rendered from half-loaded sets.
   */
  _bakeIdentity(name, bake) {
    return `${name}|${bake.seed}|${bake.tintA ?? ''}|${bake.tintB ?? ''}|${(
      bake.param ?? []
    ).join('_')}`;
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
    // Carried on the set so the build-time dump can name the asset the same way
    // the loader looks it up. See _bakeIdentity().
    set.bakedId = this._bakeIdentity(key, bake);
    this._sets.set(cacheKey, set);

    if (this._streamClosed) {
      // A surface requested after boot finished streaming (a late prop, a dev
      // reload). Nothing is going to drain the queue, so bake it here.
      this._paint(forgeDef, alloc, key, bake.size);
    } else {
      this._pendingBakes.push({
        def: forgeDef, alloc, key, cacheKey, bakedId: set.bakedId, size: bake.size, set,
      });
    }

    const ms = performance.now() - t0;
    if (ms > 40) console.info(`[materials] alloc ${key} ${bake.size}px ${ms.toFixed(0)}ms`);
    return set;
  }

  /**
   * Paint the deferred SHARED maps (detail, macro) — the ones every lit
   * material samples.
   *
   * Deliberately not part of `holdBakes(false)`. That is called the instant the
   * lighting's links are handed to the driver, and painting here means drawing
   * with a program the driver has not reached yet, which blocks the main thread
   * until it has drained the ENTIRE queue behind it. Doing that turned the
   * "poll, never block" path into a blocking one without saying so, and it is
   * why forcing `?lighting=block` appeared to change nothing: it was already
   * blocking.
   *
   * The caller paints these once the lighting is READY instead — the queue is
   * empty by then, and the first frame that shows a real material is the first
   * frame that needs them.
   */
  paintShared() {
    return boot.time('mat:sharedDeferred', () => this._forge?.paintDeferred() ?? 0);
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
    // TEXTURE DETAIL YIELDS TO LIGHTING.
    //
    // The GPU driver compiles shaders on ONE serial thread, and four different
    // parts of this app hand it work with no idea the others exist: the post
    // chain, the fidelity ramp's real materials, these bakes, and whatever the
    // streamer builds next. Whoever submits first wins, and each of these bake
    // shaders is 0.3-3.5 s of cold compile — MEASURED at 14.2 s for the set,
    // against 1.4 s for every lit material in the level put together.
    //
    // Submitted first, as they were, they put a level that is merely flat ahead
    // of a level that is lit: the ramp's release moved from 13.5 s to 36.6 s
    // cold once the frame loop stopped blocking and the bakes could run freely.
    // So they wait until the lighting has been handed to the driver. It costs
    // the surface detail a few frames and it is the whole of that difference.
    while (this._holdBakes) yield WAIT;
    // MEASUREMENT MODE. Drops every surface bake on the floor, leaving each set
    // on the flat colour `allocate()` primed it to, and lets the stream finish
    // normally so `loaded` still fires. This is how the question "what would
    // baking these at build time actually buy?" gets a number instead of an
    // argument — the 19 bake shaders are the largest single block of driver
    // compile in the session. Not a shipping mode: the level renders untextured.
    if (this._skipBakes) {
      this._pendingBakes.length = 0;
      this._streamClosed = true;
      return;
    }
    // The manifest is one small fetch started back in init(); nothing here can
    // be decided until it lands, and it lands long before the first surface
    // would have finished baking.
    while (this._bakedManifest === undefined) yield WAIT;
    while (this._pendingBakes.length) {
      const job = this._pendingBakes[0];
      // ---- pre-baked path ------------------------------------------------
      const entry = this._bakedManifest?.sets?.[job.bakedId] ?? null;
      if (entry) {
        if (!job.images) {
          job.images = null;
          loadBakedSet(entry).then((images) => { job.images = images; });
          job.requested = true;
          yield WAIT;
          continue;
        }
        this._pendingBakes.shift();
        boot.time(`mat:load:${job.key}@${job.size}`, () =>
          this._forge.paintFromImages(job.alloc.rts, job.images));
        job.alloc.set.painted = true;
        yield job.key;
        continue;
      }
      // ISSUE, WAIT, THEN PAINT. Painting straight away compiles the surface's
      // program inside the first draw and blocks the main thread for as long as
      // that takes — up to 3.5 s for one surface, and 14.2 s across the set. See
      // TextureForge.issueProgram().
      // ISSUE-WAIT-PAINT WAS A COLD REGRESSION, and a large one.
      //
      // The intent was sound — compiling a 19 KB procedural shader inside its
      // first draw blocks the main thread for up to 3.5 s — but the streamer is
      // STRICTLY SEQUENTIAL, so every `yield WAIT` here costs a whole frame and
      // stops every generator queued behind this one. Across nineteen surfaces
      // that is ~25 s of cold wall time: `loaded` went from 44.5 s to 69.4 s,
      // both medians of three, and it stayed hidden because the pre-baked
      // textures take a different path and were masking it.
      //
      // `?bakewarm=1` restores it for comparison. The real fix is not to make
      // the wait cheaper but to stop the queue being one-at-a-time — and that
      // has its own cold cost, measured and rejected (see CLAUDE.md).
      if (this._bakeWarm && !this._forge.programReady(job.def)) {
        // Issued once, then only polled. Re-issuing every frame asks three to
        // re-walk and re-validate the material for a link the driver is already
        // working on, which is pure overhead on the one thread this is trying
        // to keep free.
        if (!job.issued) {
          job.issued = true;
          this._forge.issueProgram(job.def, job.alloc.rts);
        }
        yield WAIT;
        continue;
      }
      this._pendingBakes.shift();
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
    LEAN && LEAN_STRIP.forEach((k) => delete threeProps[k]);
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
