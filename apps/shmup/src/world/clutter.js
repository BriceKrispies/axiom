/**
 * WHAT THE LEVEL DOES NOT PUT ON THE FLOOR — the arena-shooter policy.
 *
 * This level was dressed like a modern-military campaign map: a burnt-out
 * saloon, drum clusters, tyre piles, pallets, market stalls, and a continuous
 * scatter of bricks, rocks, litter and slab shards over every square metre of
 * ground. That reads as lived-in, and it is exactly wrong for an arena shooter,
 * where the floor is a surface you fight across and the ARCHITECTURE is the
 * cover. Scattered junk in an arena costs readability twice: it hides the
 * silhouette of a player against the ground, and it makes every sightline a
 * negotiation with waist-high debris nobody placed on purpose.
 *
 * So the floor is cleared. Everything that stands on the ground is suppressed;
 * everything attached to the architecture — street lamps, wall signs, roof
 * units, sat dishes, water tanks — stays, because those read as building, not
 * as clutter, and they keep the skyline and the wall surfaces from going flat.
 *
 * HOW IT IS DONE, AND WHY THERE. The suppression happens in one place —
 * `Assembler.place()` — rather than by deleting the ~1800 lines of placement
 * logic in dressing.js. Three reasons:
 *
 *   1. The dressing code still knows how to furnish this level. Turning an id
 *      back on is moving one line out of the set below, not an archaeology
 *      exercise in git history.
 *   2. Every placement decision still runs, so it still draws the same random
 *      numbers in the same order. The buildings, the layout and the level's
 *      whole architecture are therefore BYTE-IDENTICAL to before — only the
 *      props stop being instanced. Deleting the calls instead would reshuffle
 *      the RNG stream and rebuild the level into a different shape, which is
 *      not what "remove the barrels" should mean.
 *   3. One choke point cannot be half-applied. There is no path that places a
 *      prototype without going through `place()`.
 *
 * The two pieces of the wrecked car that are NOT prototypes — its body slab and
 * the sand drift piled against it — are gated at their own site in dressing.js,
 * because raw geometry has no id to suppress. `isSuppressed()` is exported for
 * exactly that case; if you find yourself needing it a third time, the thing
 * you are placing probably wants to be a prototype.
 */

/**
 * Props that stand on the ground. All suppressed.
 *
 * Grouped by what they are, so a decision to bring a category back — cover, or
 * vegetation for silhouette — is one edit rather than a scavenger hunt.
 */
export const GROUND_CLUTTER = new Set([
  // The wrecked saloon and its wheel.
  'wreck', 'tyre', 'tyre_small',

  // Drums and containers.
  'barrel_blue', 'barrel_rust', 'barrel_wood',
  'bucket', 'jerry_can', 'gas_bottle',
  'box_card_a', 'box_card_b',

  // Crates, pallets and stacked cover. An arena gets its cover from the
  // building shells; a crate on an open floor is the thing this is removing.
  'crate_a', 'crate_b', 'crate_c', 'crate_flat', 'pallet',

  // Sandbags and concrete barriers — same reasoning as the crates.
  'sandbag_a', 'sandbag_b', 'sandbag_c',
  'jersey', 'block_big', 'block_small',

  // Rubble and rock.
  'rock_a', 'rock_b', 'brick_a', 'brick_b', 'slab_shard', 'rebar',
  'plank_a', 'plank_b', 'pock',

  // Litter.
  'litter', 'can', 'bottle', 'glass_shards',

  // Cinder blocks — the single most numerous thing on the floor, 163 of them.
  'cinder',

  // The wheel shed by the wrecked saloon.
  'wheel_flat',

  // Street furniture and market dressing that sits on the floor.
  'stall', 'table', 'table_small', 'chair', 'shelf', 'cabinet',
  'mattress', 'planter', 'stool', 'tray', 'produce',

  // Vegetation. Palms are ground-planted, so they go with the rest; the trunk
  // and frond are separate prototypes and both have to be named.
  'shrub', 'weeds', 'palm_trunk', 'palm_frond',

  // The dust fillet that grounds a prop against the floor. With nothing left to
  // ground it is a stain with no object, which is worse than either.
  'dust_skirt',
]);

/**
 * Kept on purpose, listed so the intent is legible and a future edit can see
 * what was decided rather than what was merely forgotten:
 *
 *   lamp_post, lamp_glass   vertical, and the only thing lighting the street
 *                           after dusk
 *   sign_board, sign_hang   wall-mounted; they break up blank facades
 *   ac_unit, sat_dish,      roof and wall furniture; they carry the skyline
 *   roof_vent, water_tank
 */
/**
 * `?clutter=1` puts every suppressed prop back, for comparing the two dressings
 * side by side. Read once at module load: `place()` runs thousands of times per
 * build and has no business parsing a query string.
 */
const RESTORE_CLUTTER =
  typeof location !== 'undefined' &&
  new URLSearchParams(location.search).get('clutter') === '1';

/**
 * The categories of floor dressing this level no longer draws.
 *
 * Named rather than boolean-per-call-site so the policy reads as a set of
 * decisions, and so turning one back on is one edit here.
 */
export const ARENA_FLOOR = {
  /** Prototypes in GROUND_CLUTTER. */
  props: true,

  /**
   * The dust rings and swept grit at the foot of a prop. They exist to hide the
   * seam where an object meets the ground; with the objects gone they are
   * stains with no object — pale ellipses on an otherwise clean road that read
   * as decals floating over it.
   */
  skirts: true,

  /**
   * Road marks that imply traffic: tyre ruts polished into the dust, the dust
   * drifted along them, and the scuffs where vehicles swung across the road.
   *
   * Two reasons, and the second is the one that matters. Thematically there are
   * no vehicles in an arena, so tyre tracks are describing something that does
   * not happen here. Visually they are pale patches lifted ~4 cm off the road
   * surface — the decal offset every road mark uses — which was invisible under
   * a floor covered in rubble and reads as a hovering disc on a clean one.
   */
  vehicleMarks: true,
};

export function isSuppressed(id) {
  return !RESTORE_CLUTTER && ARENA_FLOOR.props && GROUND_CLUTTER.has(id);
}

/** True when the named floor-dressing category is switched off. */
export function suppresses(category) {
  return !RESTORE_CLUTTER && ARENA_FLOOR[category] === true;
}

/**
 * Warn about names in the set that no prototype answers to.
 *
 * A misspelt id here fails SILENTLY — it simply never matches, and the prop it
 * was meant to remove goes on being placed while the list says otherwise. That
 * is the one failure mode a policy file like this has, so the level checks
 * itself once at build time. Called from `Assembler.finalize()`.
 */
export function auditClutter(knownIds) {
  const unknown = [...GROUND_CLUTTER].filter((id) => !knownIds.has(id));
  if (unknown.length) {
    console.warn(
      `[world] ${unknown.length} ids in GROUND_CLUTTER match no prototype and ` +
        `suppress nothing: ${unknown.join(', ')}`
    );
  }
  return unknown;
}
