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
 * WHY LEAN IS THE DEFAULT. A cold first visit is ~35 s to settle, and the great
 * majority of that is the driver linking ~110 programs, several of them 130-145
 * KB of generated GLSL. No amount of scheduling moves it; the app spent a long
 * time proving that. The only lever that works is having less shader. `lean`
 * takes the per-pixel ornament out of the surface materials — parallax
 * occlusion, procedural weathering, patch/cloth/detile/macro layers — and keeps
 * everything that decides WHAT a surface is: its projection, albedo, tint,
 * roughness, metalness, normal map and vertex masks.
 *
 *   MEASURED, cold, median of three runs, "settled" (see CLAUDE.md):
 *     full    34 754 ms
 *     lean    26 550 ms      -8.2 s
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

/**
 * The sky dome is NOT part of `lean` yet, and that is deliberate.
 *
 * It is the largest single shader in the app (43 KB of atmosphere, volumetric
 * cloud and star field) and removing it was measured at only -2.1 s — far less
 * than its apparent share, because the driver pipelines and the cost simply
 * moved to the next program. The honest lean sky is a cubemap baked from this
 * exact shader at build time, which keeps the image and loses only the cloud
 * drift; the level runs at a fixed 16:30 with `timeRate = 0`, so nothing else
 * about it is animated. Until that exists, `?sky=flat` is a measurement switch
 * that renders a two-colour gradient and is not a shipping option.
 */
export const LEAN_SKY = params.get('sky') === 'flat';
