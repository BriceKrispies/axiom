/**
 * FIDELITY RAMP — paint the level before its shaders exist.
 *
 * THE PROBLEM THIS SOLVES. On a first-ever visit the GPU driver's shader cache
 * is empty and every program has to be compiled from source. ANGLE's D3D11
 * path — what Chrome on Windows actually uses — advertises
 * KHR_parallel_shader_compile and then compiles SERIALLY: measured at 27
 * programs over 10.4 s, completions arriving in an even trickle, half of them
 * done at 58% of the phase. Wall time is a sum, so no amount of scheduling,
 * batching or earlier issuing changes it. The boot profiler prints that
 * measurement directly (`SHADER COMPILE PARALLELISM`).
 *
 * That 10.4 s was 64% of the time to first paint, and the app spent all of it
 * on a loading screen because the frame loop cannot draw a material whose
 * program is not linked — try it and the driver links it synchronously on the
 * draw call, which is worse (`?prewarm=0` measured 26.8 s to first paint).
 *
 * THE WAY OUT is to notice WHICH programs are expensive. The scene's lit,
 * shadow-receiving, 14-point-light materials cost ~390 ms each. The post chain
 * costs ~40 ms each. So the first frames are drawn with a small set of UNLIT
 * stand-ins — a handful of programs instead of 27 — the game goes on screen,
 * and the real materials are compiled behind it while the player is already
 * moving. When they land, they are swapped back in one go.
 *
 * WHAT THE PLAYER SEES. The stand-in carries the real material's albedo map and
 * colour, so the level appears in its own colours, flat-lit, and then gains its
 * lighting a few seconds later. That is a deliberate trade: a legible level you
 * can move through beats a progress bar in front of a black screen. It is also
 * why the stand-in is not magenta or grey — a placeholder that reads as an
 * error is worse than the wait.
 *
 * WHY SO FEW PROGRAMS. three folds material FEATURES into the program cache
 * key, so the stand-ins collapse to one program per distinct combination of
 * (map, vertex colours, skinning, alpha test, side). A level made of hundreds
 * of meshes lands on a handful of programs, because the expensive axes — light
 * counts, shadow cascades, the whole lit pipeline — are gone.
 *
 * WHAT IT DOES NOT TOUCH. Materials whose program is already linked cost
 * nothing to draw, so nothing is gained by standing them in and there is a real
 * risk of changing a frame the pixel gate checks. The sky, the post chain and
 * the depth pre-passes are all left alone — the ramp only ever replaces
 * scene-graph mesh materials, and only until the real ones are ready.
 */
import * as THREE from 'three';

/**
 * The stand-in's feature set, kept as SMALL as it can be.
 *
 * Every feature three folds into the program cache key costs another program,
 * and another program is another ~390 ms of serial driver compile — which is
 * the entire thing this exists to avoid. A first attempt copied `map`,
 * `transparent`, `alphaTest` and `toneMapped` from the real material to make
 * the stand-in look closer to the finished article, and produced TWENTY
 * programs against the real set's twenty-seven. It saved nothing.
 *
 * So the stand-in copies only what changes the SHAPE of what you see:
 *   - vertexColors, because a mesh that encodes its whole look in vertex colour
 *     renders as one flat blob without it;
 *   - alphaTest, because a cut-out leaf or grate without it is a solid quad,
 *     which reads as broken geometry rather than as missing detail;
 *   - side, because a single-sided mesh drawn double-sided (or the reverse)
 *     changes the silhouette.
 *
 * Colour is copied per material and costs nothing — it is a uniform, not a
 * define. The albedo MAP is deliberately dropped: at this point in boot the
 * texture bakes have not run, so every map is still primed to its surface's
 * flat base tint, and sampling it produces the same image as the colour
 * uniform for one extra program per variant.
 */
const keyOf = (obj, m) =>
  `${m.vertexColors ? 'vc' : '-'}|${m.alphaTest > 0 ? `at${m.alphaTest}` : '-'}|${m.side}`;

export class FidelityRamp {
  constructor() {
    /** mesh -> the material (or material array) it had before. */
    this._saved = new Map();
    /** cache key -> the one stand-in material every mesh with that key shares. */
    this._stand = new Map();
    this._engaged = false;
  }

  /** How many distinct stand-in programs the first frames will need. */
  get programs() {
    return this._stand.size;
  }

  /**
   * A stand-in for one real material. Shared by cache key, so a hundred meshes
   * that differ only in which texture they sample still compile one program.
   *
   * `map` is the one thing copied per material rather than per key, which would
   * be a bug — two meshes sharing a key but not a texture would swap textures.
   * They do not: the map lives on the stand-in, so a distinct map means a
   * distinct stand-in. The key stays coarse; the cache is keyed on the map too.
   */
  _standIn(obj, m) {
    // Colour is a uniform, so it varies per material for free; only the feature
    // key can add a program, and it is deliberately coarse. See keyOf().
    const key = `${keyOf(obj, m)}|${m.color?.getHex?.() ?? '-'}`;
    let s = this._stand.get(key);
    if (!s) {
      s = new THREE.MeshBasicMaterial({
        color: m.color ? m.color.clone() : new THREE.Color(0xffffff),
        vertexColors: !!m.vertexColors,
        side: m.side,
        alphaTest: m.alphaTest ?? 0,
      });
      s.name = `ramp:${key}`;
      this._stand.set(key, s);
    }
    return s;
  }

  /**
   * Hand the real materials back to the scene just long enough for
   * `renderer.compile()` to see them, then take them away again.
   *
   * THIS IS THE WHOLE TRICK, and getting it wrong is silent. `compile()`
   * traverses the scene and compiles the materials it finds there — so with the
   * stand-ins installed it compiles stand-ins, reports success, and the real
   * programs are never linked at all. The first attempt did exactly that:
   * "compiled: 0" in 4 ms, and the real cost simply moved to the first draw
   * after the swap, which is the serial stall the ramp exists to avoid.
   *
   * `restore` and `reengage` run inside ONE synchronous callback, so no frame
   * can render in between and the frame loop never sees a material whose
   * program is still linking. Everything after that — the polling, the waiting
   * — happens with the stand-ins safely back in place.
   */
  withRealMaterials(fn) {
    const saved = new Map(this._saved);
    saved.forEach((mat, obj) => { obj.material = mat; });
    try {
      return fn();
    } finally {
      saved.forEach((mat, obj) => {
        obj.material = Array.isArray(mat)
          ? mat.map((sub) => this._standIn(obj, sub))
          : this._standIn(obj, mat);
      });
    }
  }

  /**
   * Swap every mesh in each scene to its stand-in. Both the world and the
   * viewmodel scene must be passed: the pre-warm compiles both, and a scene
   * left un-ramped contributes its real (expensive) programs to the phase that
   * gates first paint — which measured as 6 s of the viewmodel alone.
   *
   * Returns the number of distinct programs the result needs.
   */
  engage(...scenes) {
    if (this._engaged) return this.programs;
    this._engaged = true;
    scenes.forEach((scene) => scene.traverse((obj) => {
      const m = obj.material;
      if (!m || obj.isMesh !== true) return;
      this._saved.set(obj, m);
      obj.material = Array.isArray(m)
        ? m.map((sub) => this._standIn(obj, sub))
        : this._standIn(obj, m);
    }));
    return this.programs;
  }

  /**
   * Put the real materials back.
   *
   * A mesh added to the scene AFTER engage() never had its material saved and
   * is left exactly as it is — it was built with the real material and its
   * program is either linked or about to be, which is the same position it
   * would have been in without the ramp.
   */
  release() {
    if (!this._engaged) return 0;
    this._engaged = false;
    this._saved.forEach((mat, obj) => { obj.material = mat; });
    const n = this._saved.size;
    this._saved.clear();
    this._stand.forEach((m) => m.dispose());
    this._stand.clear();
    return n;
  }
}
