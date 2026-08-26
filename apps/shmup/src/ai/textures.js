import * as THREE from 'three';
import { LEAN } from '../core/fidelity.js';

/**
 * The one detail configuration every character surface uses at lean fidelity.
 * `nylon` because it is the set most of the body already uses; the cloth
 * surfaces lose their distinct weave, which is the trade.
 */
const LEAN_DETAIL = { set: 'nylon', scale: 0.45 };

import { bakeSoldierSets, RIM } from './bake.js';

/**
 * AI — the character material set: baked pixels in, MeshStandardMaterials out.
 *
 * The baking itself lives in bake.js, which imports no THREE and therefore runs
 * inside a bakery worker (src/core/bakery.js). At ~2.5 s it is the single
 * largest block of JavaScript in this app's boot, so `src/ai/index.js` queues it
 * in `prepare()` — before any subsystem starts building object graphs — and this
 * class is handed the finished buffers. Everything here needs the main thread:
 * it uploads textures and compiles the `onBeforeCompile` variants.
 *
 * See bake.js for the two-scale camouflage design (a large base tile carrying
 * the macro blotch, a small detail tile carrying the weave) — that split is what
 * keeps a 300:1 frequency ratio out of a single 2 k map.
 */

function dataTexture(buf, size, srgbSpace, aniso) {
  const t = new THREE.DataTexture(buf, size, size, THREE.RGBAFormat);
  t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.colorSpace = srgbSpace ? THREE.SRGBColorSpace : THREE.NoColorSpace;
  t.generateMipmaps = true;
  t.minFilter = THREE.LinearMipmapLinearFilter;
  t.magFilter = THREE.LinearFilter;
  t.anisotropy = aniso;
  t.needsUpdate = true;
  return t;
}

/* ------------------------------------------------------------------ */
/* Public: the material set                                            */
/* ------------------------------------------------------------------ */

export class SoldierMaterials {
  /**
   * @param baked  the result of `bakeSoldierSets()` — typed arrays, from a
   *   worker or from a synchronous call. Pass a NUMBER instead to bake inline
   *   from that seed, which is what the standalone preview page does.
   * @param opts   { anisotropy }
   */
  constructor(baked, opts = {}) {
    const aniso = opts.anisotropy ?? 8;
    // A seed rather than a bake result: bake it here and now. Same function the
    // worker calls, so the two paths cannot produce different pixels.
    const data = typeof baked === 'number'
      ? bakeSoldierSets({ nzSeed: baked, size: opts.size ?? 512, camo: opts.camo ?? ['arid', 'woodland'] })
      : baked;

    this.materials = new Map();
    this._disposables = [];
    this.camoStats = data.camoStats;
    this.bakeMs = data.bakeMs;

    const size = data.size;
    this.sets = {};
    for (const k in data.sets) {
      const s = data.sets[k];
      this.sets[k] = {
        albedo: dataTexture(s.albedo, size, true, aniso),
        orm: dataTexture(s.orm, size, false, aniso),
        normal: dataTexture(s.normal, size, false, aniso),
      };
    }

    this.details = {};
    for (const k in data.details) {
      const d = data.details[k];
      this.details[k] = dataTexture(d.normal, d.size, false, aniso);
    }

    for (const k in this.sets) {
      const s = this.sets[k];
      this._disposables.push(s.albedo, s.normal, s.orm);
    }
    for (const k in this.details) this._disposables.push(this.details[k]);
  }

  /**
   * Build (and cache) a MeshStandardMaterial for a set.
   * opts: { tint:[r,g,b], rough, metal, normalScale, key, side, transparent,
   *         detail: { set, scale, normal, rough } }
   *
   * Everything here stays a plain MeshStandardMaterial, which is what lets
   * render's MaterialPatcher inject the CSM shadow, the contact shadow, GTAO and
   * SSR into it. The detail layer is added through `onBeforeCompile`, and the
   * patcher chains our hook (it calls the previous one first), so the two
   * coexist. `customProgramCacheKey` is mandatory: without it three would hand
   * the detail-blended program to the skin material, which shares every define.
   */
  get(setName, opts = {}) {
    // LEAN: ONE DETAIL CONFIG FOR EVERY CHARACTER SURFACE.
    //
    // The program cache tag below is built from `d.set`, `d.scale` and the rim
    // strength — a texture and two uniform VALUES, none of which change a line
    // of generated code. Measured: three of these came out byte-identical in
    // translated HLSL and still cost three separate ~100 KB programs. Four
    // detail configs (cloth/nylon x 0.45/0.5) is what turns one soldier shader
    // into a quarter of the app's entire shader volume.
    //
    // Canonicalising the VALUES rather than just the key matters: three runs
    // `onBeforeCompile` once per PROGRAM, so materials sharing a program share
    // the first one's uniforms. Collapsing the key alone would give every
    // character the first material's detail texture. Collapsing the values makes
    // that sharing correct by construction.
    const d = LEAN
      ? (opts.detail ? { ...opts.detail, ...LEAN_DETAIL } : null)
      : opts.detail;
    const key = `${setName}|${opts.key ?? ''}|${(opts.tint ?? []).join(',')}|${opts.rough ?? ''}|${
      opts.metal ?? ''
    }|${d ? `${d.set},${d.scale},${d.normal},${d.rough}` : ''}`;
    let m = this.materials.get(key);
    if (m) return m;
    const set = this.sets[setName];
    if (!set) throw new Error(`[ai] unknown material set "${setName}"`);
    m = new THREE.MeshStandardMaterial({
      map: set.albedo,
      normalMap: set.normal,
      roughnessMap: set.orm,
      metalnessMap: set.orm,
      aoMap: set.orm,
      vertexColors: true,
      roughness: opts.rough ?? 1,
      metalness: opts.metal ?? 1,
      color: opts.tint ? new THREE.Color(opts.tint[0], opts.tint[1], opts.tint[2]) : 0xffffff,
      side: opts.side ?? THREE.FrontSide,
      dithering: true,
    });
    m.normalScale.set(opts.normalScale ?? 1, opts.normalScale ?? 1);
    m.aoMapIntensity = opts.ao ?? 0.85;
    m.name = `ai_${setName}`;
    this._attachShader(m, d && this.details[d.set] ? d : null, LEAN ? 1 : opts.rim);
    this.materials.set(key, m);
    return m;
  }

  /**
   * Install the character shader hooks: the high-frequency detail tile (when the
   * set has one) and the silhouette edge-darkening term (always).
   *
   * Both live in ONE onBeforeCompile because render's MaterialPatcher chains
   * whatever hook it finds — it calls ours first, then injects the CSM shadow,
   * contact shadow, GTAO and bounce fill. `customProgramCacheKey` must describe
   * every branch below or three hands the detail-blended program to the skin
   * material, which shares every define.
   */
  _attachShader(m, d, rimScale = 1) {
    const rim = new THREE.Vector4(
      RIM.strength * rimScale,
      RIM.edge,
      RIM.power,
      0
    );
    const uni = {
      owDetailTex: { value: d ? this.details[d.set] : null },
      owDetailParams: {
        value: new THREE.Vector3(d?.scale ?? 8, d?.normal ?? 0.7, d?.rough ?? 0.2),
      },
      owCharRim: { value: rim },
    };
    m.userData.owDetailUniforms = uni;
    m.userData.owCharRim = uni.owCharRim;
    const tag = `ai-${d ? `detail-${d.set}-${d.scale}` : 'plain'}-rim${rim.x.toFixed(2)}`;
    m.customProgramCacheKey = () => tag;
    m.onBeforeCompile = (shader) => {
      shader.uniforms.owCharRim = uni.owCharRim;
      shader.fragmentShader = 'uniform vec4 owCharRim;\n' + shader.fragmentShader;
      if (d) {
        shader.uniforms.owDetailTex = uni.owDetailTex;
        shader.uniforms.owDetailParams = uni.owDetailParams;
        shader.fragmentShader =
          'uniform sampler2D owDetailTex;\nuniform vec3 owDetailParams;\n' + shader.fragmentShader;
        // roughness: the detail alpha is a signed delta around 0.5
        shader.fragmentShader = shader.fragmentShader.replace(
          '#include <roughnessmap_fragment>',
          `#include <roughnessmap_fragment>
          roughnessFactor = clamp( roughnessFactor +
            ( texture2D( owDetailTex, vNormalMapUv * owDetailParams.x ).w - 0.5 ) * owDetailParams.z,
            0.04, 1.0 );`
        );
        // normal: add the detail tangent slope to the base one before the TBN
        shader.fragmentShader = shader.fragmentShader.replace(
          '#include <normal_fragment_maps>',
          `vec3 owMapN = texture2D( normalMap, vNormalMapUv ).xyz * 2.0 - 1.0;
          owMapN.xy *= normalScale;
          owMapN.xy += ( texture2D( owDetailTex, vNormalMapUv * owDetailParams.x ).xy * 2.0 - 1.0 )
            * owDetailParams.y;
          normal = normalize( tbn * normalize( owMapN ) );`
        );
      }
      // silhouette: darken the grazing sliver of every closed surface, using the
      // geometric normal so the band cannot crawl with the detail tile.
      shader.fragmentShader = shader.fragmentShader.replace(
        '#include <opaque_fragment>',
        `{
          float owF = 1.0 - abs( dot( normalize( vViewPosition ), nonPerturbedNormal ) );
          float owEdge = pow( smoothstep( owCharRim.y, 1.0, owF ), owCharRim.z );
          outgoingLight *= 1.0 - owCharRim.x * owEdge;
        }
        #include <opaque_fragment>`
      );
    };
  }

  /** Flat material for goggle lenses / optic glass. */
  glass(tint = [0.06, 0.07, 0.08]) {
    let m = this.materials.get('glass');
    if (m) return m;
    m = new THREE.MeshStandardMaterial({
      color: new THREE.Color(tint[0], tint[1], tint[2]),
      roughness: 0.11,
      metalness: 0.0,
      vertexColors: true,
      envMapIntensity: 1.4,
    });
    m.name = 'ai_glass';
    // A goggle lens is the one place a *bright* grazing highlight is correct, so
    // the edge term runs at half strength: enough that the lens rim does not
    // bloom into the sky, not enough to kill the sheen that makes it read glass.
    this._attachShader(m, null, 0.5);
    this.materials.set('glass', m);
    return m;
  }

  dispose() {
    for (const t of this._disposables) t.dispose();
    for (const m of this.materials.values()) m.dispose();
    this.materials.clear();
    this._disposables.length = 0;
  }
}
