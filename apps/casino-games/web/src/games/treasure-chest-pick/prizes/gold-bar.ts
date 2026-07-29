/*
 * gold-bar.ts — the cast ingot: the "real haul" treasure (see `CANONICAL`).
 *
 * The whole object is one silhouette: a bar whose BASE is wider AND longer than
 * its top, so its four sides slope inward as they rise. That trapezoid is what
 * separates a cast ingot from a gold-painted box, and it is the only thing the
 * player has to read at this size — so it is built to be unmistakable (a 0.48
 * drop in width and 0.34 in depth over 0.55 of height, ~24° off vertical on the
 * long faces), not hinted at.
 *
 * The engine's mesh vocabulary is box / sphere / cylinder — there is no tapered
 * primitive and no CSG, so the slope is four thin FLANK plates, each pitched
 * (and, on the ends, yawed first) onto the true slope plane between the top
 * section and the base footprint — the same move the chest lid's dome makes when
 * it becomes an honest arc of flat slats rather than a half-cylinder the engine
 * cannot draw.
 *
 * ── why not a stack of slabs ───────────────────────────────────────────────
 * The obvious build — a staircase of four boxes, each narrower than the one
 * below — is what this file did first, and it fails on THIS camera specifically.
 * The reveal looks ~50° DOWN at the prize, so every one of those slabs presents
 * its up-facing ledge straight at the lens: the bar rendered as four bright
 * bands separated by three shadow lines, reading as a stack of pancakes rather
 * than as one cast ingot. A staircase hides its steps when you look along it and
 * shows every one of them when you look down on it.
 *
 * So the body is a single CORE box sized to the bar's TOP section, whose own top
 * face is the bar's top — one clean, unbroken, best-lit surface, which is what a
 * real ingot presents and what the reference shows. The core is the top section
 * at every height, so it can never poke through the flanks that slope outward
 * past it; a thin base slab under it carries the wide footprint, and the four
 * flanks span between the two. Seen from above, the core's face and the four
 * sloped plates tile the whole silhouette with no ledge anywhere. The plates
 * overlap at the four corners on purpose — that intersection IS the chunky cast
 * corner, and it costs nothing because the plates meet at an angle rather than
 * sharing a plane.
 *
 * ── seating the value ladder ───────────────────────────────────────────────
 * The reveal camera sits ~50° ABOVE the prize, and the key arrives from above it
 * too — (0.62, 0.60, 0.51), with the warm lamp riding just off that same
 * direction. So an UP-facing face both fills more of the frame and receives
 * roughly 1.4, while a face turned away from that direction gets little more
 * than the 0.19 ambient. The first pass of this file ignored that and painted the slabs —
 * whose big up-facing ledges are most of what the camera actually sees — with
 * `PrizeGoldSide`, the second-darkest rung, saving `PrizeGoldTop` for one small
 * inset plate. The bar's entire visible mass was therefore the darkest gold in
 * the palette and it rendered BROWN.
 *
 * So the ladder is now seated by what a face receives, not by where it sits on
 * the object: the core's top face takes `PrizeGoldTop` (0.78 × ~1.4 lands
 * ~236/255 — bright gold with its hue intact, which is what the shared palette's
 * brightest rung is authored for), the sloped long faces take `PrizeGold`, and
 * only the two short ENDS — which swing away from the key for most of the
 * turntable revolution — step down to `PrizeGoldSide`. The step between facings
 * is what makes it read as metal; it just has to be the right way up.
 */

import type { EngineVec3 } from "@axiom/web-engine";
import type { Prize } from "./prize.ts";
import { sparkleAt, v3 } from "./prize.ts";
import { quatMul, quatPitch, quatYaw, rotateByQuat } from "../../../presentation/stage/vectors.ts";

/** The cast footprint at the base and at the shoulder, in prize-local units. */
const BASE_W = 1.5;
const BASE_D = 0.9;
const TOP_W = 1.02;
const TOP_D = 0.56;
/** The bar spans ±this in Y: the underside of the base slab to the core's top
 * face, which IS the bar's top. */
const BAR_HALF_H = 0.275;

/** How thick the base slab is. It is the cast foot the flanks land on, and it
 * carries the full base section — so the four corner wedges the flank plates
 * cannot reach (see `across` below) show ITS top face rather than open air,
 * reading as the chamfered corners a cast bar actually has. */
const BASE_H = 0.14;

/** The two sections the slope runs between: the base footprint at the very
 * bottom, and the core's top face at the very top. The flanks are derived from
 * exactly these, so the slope cannot drift out of step with either end. */
const BASE_Y = -BAR_HALF_H + BASE_H / 2;
const TOP_Y = BAR_HALF_H;

interface Flank {
  readonly suffix: string;
  /** Yaw that swings the plate onto this side. The outward direction is the
   * yaw's own +Z, so one pitched plate serves all four faces. */
  readonly yaw: number;
  /** True for the two LONG faces (normal along ±Z, 1.5 across). Those are the
   * broad sloped faces the camera reads the taper off, and their normals tip up
   * into the key, so they hold the mid rung; the two short ENDS turn away from
   * it and step down. */
  readonly long: boolean;
  readonly material: string;
}

const FLANKS: readonly Flank[] = [
  { long: true, material: "PrizeGold", suffix: "faceF", yaw: 0 },
  { long: true, material: "PrizeGold", suffix: "faceB", yaw: Math.PI },
  { long: false, material: "PrizeGoldSide", suffix: "faceR", yaw: Math.PI / 2 },
  { long: false, material: "PrizeGoldSide", suffix: "faceL", yaw: -Math.PI / 2 },
];

/** Plate thickness. It straddles the slope plane — half proud, half buried — so
 * the plate can never float off the staircase or sink behind it. */
const FLANK_THICK = 0.05;

/**
 * Glint sites: the four cast corners of the shoulder and two points along the
 * top edges — where a real bar's light lives, on an edge rather than in the
 * middle of a face. Each runs its own `sparkleAt` cycle, so they pop out of
 * unison; one at envelope ~0 scales to ~0 and is simply invisible that frame.
 */
const GLINTS: readonly EngineVec3[] = [
  v3(-0.5, 0.272, 0.275),
  v3(0.5, 0.272, 0.275),
  v3(0.5, 0.272, -0.275),
  v3(-0.5, 0.272, -0.275),
  v3(-0.3, 0.278, 0.275),
  v3(0.3, 0.278, -0.275),
];
const GLINT_SIZE = 0.16;

export const GOLD_BAR: Prize = {
  // A solid ingot reads from every side, so it turns. It leans back only a
  // little: a bar lying flat to the lens loses the tapered profile that is the
  // whole point of it.
  presentation: "turntable",
  lean: 0.34,
  /** Nothing new: the bar is exactly the shared metal ladder (`PrizeGoldTop` →
   * `PrizeGold` → `PrizeGoldSide`), which is the point of that palette — a bar
   * and a coin that disagreed about gold would read as two substances. */
  materials: {},
  /** The base slab's far bottom corner, (0.75, −0.275, 0.45), reaches 0.917; the
   * flank plates (0.83) and the brightest glint (0.71) stay inside it. */
  extent: 0.92,
  build: (place, frame) => {
    // The cast body: ONE box at the bar's top section, in the palette's brightest
    // rung. Its top face is the single largest, best-lit, most camera-facing
    // surface on the object (see "seating the value ladder" and "why not a stack
    // of slabs"), so it is what has to carry the gold — unbroken.
    const core = place("core", "box", "PrizeGoldTop", v3(0, 0, 0), v3(TOP_W, BAR_HALF_H * 2, TOP_D));
    // The cast foot, carrying the wide base footprint. It wears the mid rung
    // because nothing above ever sees its top face — the flanks cover that band
    // completely — and what the camera does catch is its outer edge.
    const foot = place("foot", "box", "PrizeGold", v3(0, BASE_Y, 0), v3(BASE_W, BASE_H, BASE_D));

    // One sloped plate per side, derived from the two sections it spans exactly
    // the way a lid slat is derived from its own chord: the rise and the inward
    // run give the plate's length, its tilt, and its centre with nothing left
    // over.
    const faces = FLANKS.map((flank) => {
      const reachBase = flank.long ? BASE_D / 2 : BASE_W / 2;
      const reachTop = flank.long ? TOP_D / 2 : TOP_W / 2;
      const rise = TOP_Y - BASE_Y;
      const run = reachBase - reachTop;
      // The plate's local +Y runs up the slope and its local +Z is the outward
      // normal. quatPitch(a) sends +Y to (0, cos a, sin a), so the tilt that
      // leans the plate's top inward — and therefore tips its normal outward and
      // UP, into the key — is this, negative.
      const tilt = -Math.atan2(run, rise);
      const turn = quatYaw(flank.yaw);
      // A sloped face of a frustum is a TRAPEZOID — wider at the base than at the
      // top — and a box is a rectangle, so one plate cannot be both. Cut to the
      // base width it overhangs the top section by a quarter-unit on each side,
      // sticking out into open air; on a turntable those overhangs swing past the
      // camera as thin unlit flags, which is exactly what they looked like. Cut
      // to the TOP width it instead falls short at the bottom — and that shortfall
      // lands on the foot slab, which is a solid box of the full base section, so
      // what the camera sees there is more gold rather than a hole. Falling short
      // onto something is always better than overhanging into nothing.
      const across = flank.long ? TOP_W : TOP_D;
      return place(
        flank.suffix,
        "box",
        flank.material,
        rotateByQuat(v3(0, (BASE_Y + TOP_Y) / 2, (reachBase + reachTop) / 2), turn),
        v3(across, Math.hypot(rise, run), FLANK_THICK),
        quatMul(turn, quatPitch(tilt)),
      );
    });

    // No struck lettering. `PrizeGoldEtch` is 0.26 albedo — a third of the top
    // rung — so at the size this bar occupies, two stamped rows did not read as
    // lettering at all: they read as wide dark bars laid across the brightest
    // surface on the object, and they were most of why the first pass looked
    // like a grill instead of gold. The reference ingot carries no markings
    // either, and it is cleaner for it.

    const glints = GLINTS.map((site, i) => {
      const s = GLINT_SIZE * sparkleAt(i, frame.tick, frame.settle);
      return place(`glint${i}`, "sphere", "PrizeSparkle", site, v3(s, s, s));
    });

    return [core, foot, ...faces, ...glints];
  },
};
