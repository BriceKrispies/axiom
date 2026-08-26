/**
 * FIDELITY — how much shader this build is willing to compile.
 *
 * This is a different axis from `quality` (`?q=low|medium|ultra`, see
 * config.js), and confusing the two wastes time. Quality scales things that
 * cost FRAME time: render scale, shadow map size, cascade count, particle
 * budgets. Fidelity scales things that cost COLD BOOT time: how much shader
 * text exists, and therefore how long the GPU driver spends compiling it on a
 * player's first ever visit.
 *
 * They are independent because the costs are independent. Measured on this app:
 * dropping from `ultra` to `low` barely moved the cold boot at all — it compiled
 * MORE programs, not fewer — because the presets never touched the material
 * shaders, which are where the text is.
 *
 * WHY LEAN IS THE DEFAULT. A cold first visit is dominated by the GPU driver
 * linking programs — serially, one at a time, with the GPU otherwise idle. The
 * governing arithmetic is boringly linear:
 *
 *     cold boot  ~=  (number of lit programs)  x  (~100 KB of translated HLSL each)
 *
 * The second factor is immovable. Three separate attempts to shrink it — fewer
 * lights, fewer shadow cascades, swapping the material class to Lambert —
 * bought 4%, 13% and "renders nothing" respectively, because the bulk of a
 * program is three's shared lighting and shadow plumbing, not anything this app
 * chose. The first factor is freely reducible, and reducing it works: 101 -> 83
 * programs measured -16% time, 83 -> 60 measured -18%, 60 -> 53 measured -15%.
 *
 * So `lean` is a programs budget, and everything in it is one decision about
 * what the game stops having:
 *
 *   surface ornament   parallax occlusion, procedural weathering, patch, cloth,
 *                      detile and macro-relief layers come out of the material
 *                      shader. What decides WHAT a surface is — projection,
 *                      albedo, tint, roughness, metalness, normal map, vertex
 *                      masks — all stays.
 *   surface variety    nineteen library surfaces fold onto four (LEAN_SURFACE
 *                      in materials/index.js). Walls stop differing from walls.
 *   character variants one detail set and a fixed rim for every AI soldier,
 *                      canonicalised by VALUE — `onBeforeCompile` runs once per
 *                      PROGRAM, so two materials must agree on the numbers, not
 *                      merely on the cache key.
 *   the post chain     SSR, GTAO, contact shadows, TAA, motion blur, ADS depth
 *                      of field, bloom and FXAA. The composite still tone-maps
 *                      and grades.
 *   the sky            the dome, the volumetric march, four LUT bakes and the
 *                      equirect IBL — nine programs. Replaced by a flat colour
 *                      driven from the same CPU atmosphere that drives the
 *                      ambient, so the sky and the light on the level agree.
 *                      The level runs at a fixed 16:30 with `timeRate = 0`, so
 *                      only cloud drift is actually lost.
 *   fx                 particles, tracers, muzzle flash, impacts, decals,
 *                      shells, explosions, haze, ambience. The system is not
 *                      registered at all (see main.js).
 *
 * The weapon is deliberately NOT on that list. The gun in your hands is this
 * game's identity in a way the sparks around it are not.
 *
 *   MEASURED, cold, "settled" (see CLAUDE.md), 60 live programs against 101:
 *     lean    15 583 ms      median of three, 53 programs
 *     full    ~26 000 ms     paired A/B, four pairs, all agreeing in sign
 *
 * `?fidelity=full` restores the original look. Capture mode forces it, because
 * the pixel gate's reference images are of the full renderer — see the note in
 * CLAUDE.md about that being a decision to revisit rather than a settled one.
 */

/**
 * Read once, at module load. Deliberately not routed through `config`: the
 * material shader builder and the sky dome both need this while constructing
 * GLSL, which happens before and below anywhere a config object is in scope.
 * One module so there is one answer, rather than three files each parsing the
 * query string slightly differently.
 */
const params = (() => {
  try {
    return new URLSearchParams(location.search);
  } catch {
    return new URLSearchParams('');
  }
})();

/**
 * `lean` (default) or `full`. Capture mode is always `full`.
 *
 * A THIRD RUNG WAS TRIED AND REMOVED. `minimal` swapped world surfaces to
 * `MeshLambertMaterial` and skipped the OW chunk, on the theory that three's
 * PBR shader was the bulk of the ~144 KB every lit program carries. Measured:
 * 144.5 KB -> 99.6 KB, a 31% cut, not the ~10x predicted — because the bulk is
 * NOT the PBR BRDF, it is the shared lighting and shadow plumbing (unrolled
 * light loops, shadow-map PCF, fog, packing), which is the same whatever
 * material class you choose. The PBR-specific part is only ~45 KB of the 144.
 *
 * It also rendered nothing: this app's surfaces depend on the OW chunk for
 * their triplanar projection, tint and ORM unpacking, so `owNoPatch` does not
 * make a material plainer, it removes what draws it.
 *
 * The conclusion worth keeping: shader text here is dominated by LIGHT COUNT
 * and SHADOW CASTERS, not by material class. Cutting it means fewer lights, or
 * hand-written lighting outside three's material system entirely.
 */
export const FIDELITY = params.get('capture') === '1'
  ? 'full'
  : params.get('fidelity') === 'full'
    ? 'full'
    : 'lean';

/** True when the expensive per-pixel material features are compiled out. */
export const LEAN = FIDELITY !== 'full';
