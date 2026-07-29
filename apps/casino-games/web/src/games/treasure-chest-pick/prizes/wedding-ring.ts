/*
 * wedding-ring.ts — the jackpot: a gold band carrying a brilliant-cut diamond.
 *
 * This is the best thing a chest can hold, so it is the one treasure that has to
 * read instantly at hero size: a ring silhouette (a hole you can see through)
 * with a big faceted stone standing on top of it.
 *
 * ── why the band is an arc of flat segments ────────────────────────────────
 * The engine's mesh vocabulary is box / sphere / cylinder. There is no torus and
 * no CSG, so a ring cannot be subtracted out of anything — the hole has to be
 * built rather than cut. The honest answer is the same one the chest's barrel
 * lid makes for its dome (`lidArc` in scene.ts): a curve the primitives cannot
 * express becomes a ring of flat facets, each placed by its OWN chord so the
 * facets meet edge to edge at any segment count with no seam to tune. Fourteen
 * segments is where the silhouette stops reading as a polygon, and the faceting
 * is not a compromise — each segment's outer face meets the reveal key at its
 * own angle, so the band reads as round from SHADING as well as from outline,
 * which is exactly the chunky faceted look everything else in this scene has.
 *
 * The band is three concentric rings, not one: a shank, a narrower rim standing
 * proud of it, and a dark bore lining the hole. That value step is what stops a
 * ring from reading as a bead — and it is stepped by GEOMETRY (outside / face /
 * inside), never by angle around the circle. Angle-stepped materials would fake
 * a light direction, and this prize turns on `frame.spin`, so a faked highlight
 * would rotate with the object and read as painted stripes.
 *
 * ── why the whole assembly leans back ──────────────────────────────────────
 * The ring stands UP, hole toward the camera, because a ring lying flat reads as
 * a disc. But the reveal camera looks slightly DOWN, so a perfectly upright ring
 * aims its hole below the viewer. The whole assembly therefore leans back a few
 * degrees, which tips the hole up into the camera's eye-line and, more
 * importantly, tips the diamond's table up to face it. It is the pose a ring is
 * displayed in for exactly this reason.
 */

import type { EngineQuat, EngineVec3, MaterialSpec, SceneInstance } from "@axiom/web-engine";
import { QUAT_IDENTITY, quatMul, quatPitch, quatRoll, quatYaw, rotateByQuat } from "../../../presentation/stage/vectors.ts";
import type { Prize, PrizeFrame, PrizePlace } from "./prize.ts";
import { solid, sparkleAt, v3 } from "./prize.ts";

// ── the diamond palette ────────────────────────────────────────────────────
/*
 * A diamond is the one thing in this chest that WANTS to be white, and white is
 * the trap. The reveal rig sums to ~1.25 on an up-facing surface, so an albedo
 * anywhere near 1.0 multiplies past the tone curve's knee; once two channels
 * clip together the hue is gone by construction and the stone renders as a
 * formless bright blob with no facets in it. The gem this catalog replaced
 * measured (250, 253, 252) on screen — pure paper white, no shape at all.
 *
 * So the ladder tops out at 0.72, below even the gold's brightest rung, and what
 * makes it read as a diamond is not brightness but the STEP between the facets:
 * a bright table, a mid crown alternating with a darker one, a bright thin
 * girdle line, and a deep pavilion. Every rung is pulled toward blue, because a
 * pale icy blue-white that is genuinely under the clip point reads as diamond
 * where a neutral white just reads as blown-out.
 *
 * The only emissive is on the PAVILION — the underside, which faces away from
 * every light and would otherwise go dead. A little fire glowing out of the
 * shadowed cone is exactly what a real stone does, and it is safe there
 * precisely because that rung is dark: it lands around 0.4 lit, nowhere near
 * clipping. Putting the same glow on the table would push its blue channel
 * straight back over 1.0 and re-create the bug this whole palette exists to fix.
 */
const DIAMOND_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  /** The flat top facet — the brightest rung, and the biggest single face. */
  PrizeDiamondTable: solid([0.6, 0.68, 0.72, 1]),
  /** The kite facets of the crown, alternating with `…Deep` so the crown reads
   * as a ring of separate faces rather than one smooth cone. */
  PrizeDiamondCrown: solid([0.46, 0.55, 0.62, 1]),
  PrizeDiamondCrownDeep: solid([0.33, 0.41, 0.48, 1]),
  /** The thin bright line at the stone's widest point, where crown meets pavilion. */
  PrizeDiamondGirdle: solid([0.55, 0.62, 0.68, 1]),
  /** The pointed underside: the deepest rung, carrying the stone's inner fire. */
  PrizeDiamondPavilion: { baseColor: [0.2, 0.27, 0.35, 1], emissive: [0.06, 0.09, 0.14, 1] },
};

// ── the band ───────────────────────────────────────────────────────────────

/** Segments in one ring of the band. Below ~12 the silhouette reads as a
 * polygon; above ~16 the extra facets cost instances and change nothing. */
const BAND_SEGMENTS = 14;
/** Half the angle one segment spans — the chord geometry hangs off this. */
const HALF_STEP = Math.PI / BAND_SEGMENTS;
/** Chord length, per unit of ring radius. */
const CHORD = 2 * Math.sin(HALF_STEP);
/** How far a chord's midpoint sits inside its circle, per unit of radius. */
const CHORD_INSET = Math.cos(HALF_STEP);
/** A hair of overlap on each chord so adjacent facets cannot open a hairline
 * gap at the mid-radius circle where they meet. */
const CHORD_WELD = 0.006;

/** The band's centre, pushed below the prize origin so the assembly — band plus
 * the stone standing on top of it — is balanced around the origin rather than
 * hanging low in the unit box. */
const BAND_Y = -0.18;
/** Mid-line radius of the shank: a band ~1.04 local units across. */
const BAND_RADIUS = 0.52;
/** The hole is most of what makes this read as a ring instead of a bead, so the
 * shank stays slim in section and the bore is left wide: an opening ~0.91 across
 * inside an outer diameter of ~1.18. */
const SHANK_THICK = 0.115;
const SHANK_WIDTH = 0.185;

/**
 * One ring of chord-placed facets in the band's plane (the XY plane, so the
 * hole faces the camera). Each segment is placed by its own chord: the chord
 * gives its centre, its length, and its roll directly, which is why the facets
 * abut at any radius and any segment count.
 *
 * A box's local +X is rolled onto the chord's tangent, so its local +Y ends up
 * radial — that is the axis `thickness` runs along, and `width` runs along the
 * ring's axis (local Z), which is the band's width on a finger.
 */
const bandRing = (
  place: PrizePlace,
  prefix: string,
  material: string,
  radius: number,
  thickness: number,
  width: number,
): readonly SceneInstance[] =>
  Array.from({ length: BAND_SEGMENTS }, (_, i): SceneInstance => {
    const mid = (i * 2 + 1) * HALF_STEP;
    return place(
      `${prefix}${i}`,
      "box",
      material,
      v3(Math.cos(mid) * radius * CHORD_INSET, BAND_Y + Math.sin(mid) * radius * CHORD_INSET, 0),
      v3(radius * CHORD + CHORD_WELD, thickness, width),
      quatRoll(mid + Math.PI / 2),
    );
  });

// ── the stone ──────────────────────────────────────────────────────────────
/*
 * A brilliant cut, built from the outside in: a girdle disc at the widest point,
 * a crown of angled kite facets rising from it to a flat table, and a pointed
 * pavilion hanging beneath. Everything here is a body of revolution about the
 * prize's +Y, so unlike the band the stone stays fully legible through every
 * part of the presentation turn — which is why the stone, not the band, is what
 * the eye is asked to hold onto.
 */

/** The stone's widest plane. Everything else is measured from it. */
const GIRDLE_Y = 0.635;
const GIRDLE_RADIUS = 0.215;
const GIRDLE_HEIGHT = 0.05;

/** The table sits high enough that the crown has real height to slope through —
 * a shallow crown reads as a bead with a lid on it, not as a cut stone. */
const TABLE_Y = 0.812;
const TABLE_RADIUS = 0.125;
const TABLE_HEIGHT = 0.034;
const TABLE_TOP = TABLE_Y + TABLE_HEIGHT / 2;

/** Kite facets around the crown. Eight is the count at which the crown still
 * reads as separate faces catching the key at separate angles; more and it
 * smooths into a cone, which is the thing a faceted stone must not look like. */
const CROWN_FACETS = 8;
const FACET_STEP = (Math.PI * 2) / CROWN_FACETS;
const CROWN_BOTTOM = GIRDLE_Y + GIRDLE_HEIGHT / 2;
const CROWN_TOP = TABLE_Y - TABLE_HEIGHT / 2;
const CROWN_RISE = CROWN_TOP - CROWN_BOTTOM;
/** How far the crown draws in from the girdle to the table. */
const CROWN_RUN = GIRDLE_RADIUS - TABLE_RADIUS;
const CROWN_MID_Y = (CROWN_BOTTOM + CROWN_TOP) / 2;
const CROWN_MID_RADIUS = (GIRDLE_RADIUS + TABLE_RADIUS) / 2;
/** One kite plate spans the whole run from girdle to table, so the crown is a
 * single ring of faces with no horizontal seam cutting across it. */
const KITE_LENGTH = Math.hypot(CROWN_RISE, CROWN_RUN);
/** Yaw sends a kite's local +Z radially outward; this pitch then lays the plate
 * along the run from girdle out-and-down to table, so its local +Y — the plate's
 * face — becomes the kite's normal. */
const KITE_SLOPE = Math.atan2(CROWN_RISE, CROWN_RUN);
/** The chord that abuts its neighbours at the crown's mid radius, welded by the
 * same hair the band's facets use. */
const KITE_WIDTH = 2 * CROWN_MID_RADIUS * Math.tan(FACET_STEP / 2) + CHORD_WELD;
/** Alternating rungs around the crown. Two neighbouring kites never share a
 * value, so the crown reads as separate faces even in the frames where the key
 * happens to hit them all alike — and because the alternation is fixed to the
 * geometry rather than to an angle, the turn cannot smear it into stripes. */
const CROWN_LADDER: readonly string[] = ["PrizeDiamondCrown", "PrizeDiamondCrownDeep"];

/**
 * The pavilion is a single box tipped onto a corner: a 45° roll puts one EDGE
 * down, and a further atan(1/√2) tips that edge until one VERTEX is straight
 * down — the cube's body diagonal standing on the −Y axis. That gives the
 * pointed underside a brilliant cut needs from a mesh vocabulary with no cone
 * in it, and its hexagonal waist is a genuinely faceted flank rather than the
 * smooth taper a squashed sphere would give.
 *
 * The box's widest cross-section is parked just BELOW the girdle so the girdle
 * disc overhangs it (which is what a real stone's girdle does) and so the
 * upper half of the tipped box — the part that is not pavilion at all — stays
 * hidden inside the crown, with its top vertex buried in the table.
 */
const PAVILION_EDGE = 0.26;
const PAVILION_DROP = 0.05;
const PAVILION_Y = GIRDLE_Y - PAVILION_DROP;
const PAVILION_TIP = quatMul(quatRoll(Math.PI / 4), quatPitch(Math.atan(Math.SQRT1_2)));

/** The collet: the narrow neck the stone's point plunges into, which is what
 * lifts the stone clear of the shank instead of letting it sit on it like a
 * lump. Deliberately slimmer than the pavilion's waist, so the cone is the
 * silhouette and the metal is just what it stands on. */
const COLLET_RADIUS = 0.095;
const COLLET_Y = 0.47;
const COLLET_HEIGHT = 0.26;

/** Four claws gripping the girdle. They are what says "set" rather than
 * "glued": each one leans inward at the top so it curls over the girdle onto
 * the crown, the way a prong actually holds a stone. */
const PRONGS = 4;

// ── the glints ─────────────────────────────────────────────────────────────

/** A glint's diameter at full envelope. Each one is scaled by its OWN envelope,
 * so one at rest scales to nothing and vanishes rather than sitting there as a
 * dot — the sparkles have to fire out of unison or they read as a string of
 * lights instead of facets catching the key. */
const GLINT_SIZE = 0.13;

const crownGlint = (facet: number): EngineVec3 => {
  const at = (facet + 0.5) * FACET_STEP;
  return v3(Math.sin(at) * (CROWN_MID_RADIUS + 0.012), CROWN_MID_Y + 0.008, Math.cos(at) * (CROWN_MID_RADIUS + 0.012));
};

/** Where the stone catches light: its table, three of its eight kites (spread
 * around the crown, never two neighbours), and the girdle's edge. Five is as
 * many as this stone can carry — more and the twinkles overlap often enough to
 * read as a steady glow, which is the opposite of what a facet does. */
const GLINTS: readonly EngineVec3[] = [
  v3(0.04, TABLE_TOP + 0.01, 0.06),
  crownGlint(0),
  crownGlint(3),
  crownGlint(6),
  v3(Math.sin(2.1) * (GIRDLE_RADIUS + 0.01), GIRDLE_Y, Math.cos(2.1) * (GIRDLE_RADIUS + 0.01)),
];

// ── the lean ───────────────────────────────────────────────────────────────

/** The display lean (see the header). Negative pitch tips the ring's axis UP
 * toward the raised camera, which leans the band's top away and brings the
 * stone's table around to face the viewer. */
const RING_LEAN: EngineQuat = quatPitch(-0.3);

/**
 * `place`, with the lean folded in — so every part below is authored upright,
 * in the plain readable numbers the prize contract asks for, and exactly one
 * place in this file knows about the pose. The offset is rotated by the lean and
 * the part's own rotation is composed with it, which is the same weld `lidArc`
 * uses to keep an arc of slats attached to the lid board that carries it.
 *
 * A rotation preserves length, so the lean cannot change how far anything
 * reaches from the origin — `extent` is measured on the upright assembly.
 */
const leaning = (place: PrizePlace): PrizePlace =>
  (suffix, mesh, material, offset, scale, rotation = QUAT_IDENTITY): SceneInstance =>
    place(suffix, mesh, material, rotateByQuat(offset, RING_LEAN), scale, quatMul(RING_LEAN, rotation));

const build = (rawPlace: PrizePlace, frame: PrizeFrame): readonly SceneInstance[] => {
  const place = leaning(rawPlace);

  // The three concentric rings of the band. The rim stands proud of the shank
  // so the band has a rounded crown in section rather than a flat strap edge,
  // and the bore lines the hole in the darkest gold there is: the inside of a
  // ring never sees the key, and that dark tunnel is what proves the hole is a
  // hole rather than a disc painted to look like one.
  const shank = bandRing(place, "shank", "PrizeGold", BAND_RADIUS, SHANK_THICK, SHANK_WIDTH);
  const rim = bandRing(place, "rim", "PrizeGoldTop", BAND_RADIUS + 0.045, 0.045, 0.105);
  const bore = bandRing(place, "bore", "PrizeGoldDeep", BAND_RADIUS - 0.05, 0.03, 0.155);

  // The shoulders: two blocks where the shank swells up into the setting,
  // leaning inward to carry the eye off the band and onto the stone.
  const shoulders = [-1, 1].map((side): SceneInstance =>
    place(`shoulder${side}`, "box", "PrizeGoldTop", v3(0.14 * side, 0.36, 0), v3(0.13, 0.22, 0.19), quatRoll(0.45 * side)),
  );

  const collet = place("collet", "cylinder", "PrizeGoldSide", v3(0, COLLET_Y, 0), v3(COLLET_RADIUS * 2, COLLET_HEIGHT, COLLET_RADIUS * 2));

  const prongs = Array.from({ length: PRONGS }, (_, i): SceneInstance => {
    // Offset half a step off the crown facets' own spacing so a claw sits over a
    // facet's edge, where a real prong grips, not flat across the middle of one.
    const at = (i + 0.5) * ((Math.PI * 2) / PRONGS);
    return place(
      `prong${i}`,
      "box",
      "PrizeGoldTop",
      v3(Math.sin(at) * 0.205, GIRDLE_Y + 0.02, Math.cos(at) * 0.205),
      v3(0.055, 0.19, 0.055),
      quatMul(quatYaw(at), quatPitch(-0.28)),
    );
  });

  const pavilion = place("pavilion", "box", "PrizeDiamondPavilion", v3(0, PAVILION_Y, 0), v3(PAVILION_EDGE, PAVILION_EDGE, PAVILION_EDGE), PAVILION_TIP);

  const girdle = place("girdle", "cylinder", "PrizeDiamondGirdle", v3(0, GIRDLE_Y, 0), v3(GIRDLE_RADIUS * 2, GIRDLE_HEIGHT, GIRDLE_RADIUS * 2));

  const crown = Array.from({ length: CROWN_FACETS }, (_, i): SceneInstance => {
    const at = (i + 0.5) * FACET_STEP;
    return place(
      `kite${i}`,
      "box",
      CROWN_LADDER[i % CROWN_LADDER.length],
      v3(Math.sin(at) * CROWN_MID_RADIUS, CROWN_MID_Y, Math.cos(at) * CROWN_MID_RADIUS),
      v3(KITE_WIDTH, 0.05, KITE_LENGTH),
      quatMul(quatYaw(at), quatPitch(KITE_SLOPE)),
    );
  });

  const table = place("table", "cylinder", "PrizeDiamondTable", v3(0, TABLE_Y, 0), v3(TABLE_RADIUS * 2, TABLE_HEIGHT, TABLE_RADIUS * 2));

  // The glints. `sparkleAt` is pure in (index, tick) and already folds in
  // `settle`, so nothing twinkles while the prize is still climbing out — and a
  // glint whose envelope is at rest scales to zero rather than lingering.
  const glints = GLINTS.map((at, i): SceneInstance => {
    const flash = GLINT_SIZE * sparkleAt(i, frame.tick, frame.settle);
    return place(`glint${i}`, "sphere", "PrizeSparkle", at, v3(flash, flash, flash));
  });

  return [...bore, ...shank, ...rim, ...shoulders, collet, pavilion, girdle, ...crown, table, ...prongs, ...glints];
};

/**
 * The jackpot treasure.
 *
 * `extent` is set by the table's glint at full envelope — the single highest
 * point the assembly ever reaches (~0.84 up the axis plus half a glint). The
 * band's bottom outer corner is the next furthest at ~0.77, so the declared
 * reach holds for the whole turn, in every phase of the sparkle cycle.
 */
export const WEDDING_RING: Prize = {
  // The subject IS the hole and the stone above it, so the ring stands square to
  // the lens and rocks rather than revolving — a full turn would carry the band
  // edge-on and the ring would vanish twice a revolution.
  presentation: "faces-camera",
  lean: 1,
  build,
  extent: 0.92,
  materials: DIAMOND_MATERIALS,
};
