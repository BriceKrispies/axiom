/*
 * gold-coin.ts — the common prize: one fat struck coin.
 *
 * This is what a small win looks like, so it has to be instantly legible at a
 * glance. Two decisions carry the whole object (and a third, what is struck ON
 * it, is covered under "the struck device" below):
 *
 * 1. It is STRUCK, not a puck. A single cylinder reads as a plastic checker no
 *    matter how good the gold is. So the coin is two coaxial discs — a wider,
 *    thinner blank in the SIDE rung whose exposed annulus is the recessed rim,
 *    and a narrower, thicker field in the TOP rung standing proud of it. The
 *    step between them is a real geometric ledge, and it lands the palette's
 *    brightest gold on the big camera-facing surface with a darker ring around
 *    it — which is what "minted" looks like without a texture.
 *
 * 2. It always FACES the camera. A slow yaw turn is right for a solid object
 *    like the gold bar and fatal for a coin: half of every revolution would be
 *    spent edge-on, and the prize would thin to a line and disappear. So it
 *    declares
 *    `presentation: "faces-camera"` and the staging leans it into the lens and
 *    rocks it instead of revolving it. A treasure the player cannot see for half
 *    its cycle is a bug, not a flourish.
 */

import type { SceneInstance } from "@axiom/web-engine";
import { quatPitch, quatRoll } from "../../../presentation/stage/vectors.ts";
import type { Prize, PrizeFrame, PrizePlace } from "./prize.ts";
import { sparkleAt, v3 } from "./prize.ts";

// ── the blank ───────────────────────────────────────────────────────────────

/** Coin diameter in prize-local units, and the half-thickness of the blank it
 * is cut from. Sized to fill the unit box the way a hand-sized object should:
 * a coin noticeably wider than the gold bar is tall, because a coin held up to
 * the camera IS its face. */
const COIN_DIAMETER = 1.4;
const COIN_THICKNESS = 0.2;
/** The proud struck field: narrower than the blank, and thicker, so it stands
 * clear on BOTH faces and leaves an annulus of the blank showing as the rim. */
const FIELD_DIAMETER = 1.24;
const FIELD_THICKNESS = 0.28;
/** Where the blank's exposed rim annulus sits — the surface the milling and the
 * glints stand on. */
const RIM_Z = COIN_THICKNESS / 2;

/** A cylinder's axis is +Y, so this is the rotation that stands one on its edge
 * with its flat face toward the camera. Every disc in this file uses it. */
const FACE_CAMERA = quatPitch(Math.PI / 2);

// ── the rim milling ─────────────────────────────────────────────────────────

/*
 * True edge milling runs around the cylindrical side, where from this camera it
 * is one pixel of silhouette and reads as nothing. So the ticks go where the
 * camera can actually see them: standing on the exposed rim annulus, running
 * radially off the field's ledge — denticles, which is what a struck coin
 * carries on the face anyway. Twelve is the count where they still read as
 * separate teeth at prize scale; more turns the rim to a dotted mush.
 */
const MILL_TICKS = 12;
const MILL_RADIUS = 0.66;
const MILL_LENGTH = 0.08;
const MILL_WIDTH = 0.05;
const MILL_RELIEF = 0.05;

// ── the struck device ───────────────────────────────────────────────────────

/*
 * The coin carries NO FACE.
 *
 * It used to: two round eye studs, cocked brows, a nose and a stepped-arc smile,
 * all held in relief. It read as a face, and a small gold face staring out of a
 * treasure chest is unsettling rather than charming — so it is gone, and nothing
 * face-shaped replaces it. That rules out the obvious substitutes too: a central
 * boss inside a ring of rays is a sunburst to a designer and an EYE to everyone
 * else, and this coin has had enough of being looked at.
 *
 * What it gets instead is the plainest real mint device there is — a single
 * concentric struck ring inside the denticles. It is built the same way the
 * coin's rim already is, as a two-tier step rather than a drawn line: a slightly
 * larger disc in the dark `Etch` rung with the bright field's disc struck proud
 * in front of it, leaving an annulus of shadow showing. Two instances, no
 * texture, and it reads as minted from the first frame.
 *
 * The relief argument the face was built on still holds and is worth keeping
 * written down, because it governs anything struck on this coin later: a raised
 * part catches the key on its own facet and throws its own value break, so it
 * survives the software backend, which has no texture and only flat per-facet
 * light to work with. The dark rungs then make a raised part read as a struck
 * RECESS holding shadow, which is how a real mint mark looks — and why nothing
 * here can be mistaken for a sticker.
 */
const DEVICE_DIAMETER = 0.96;
const DEVICE_DEPTH = 0.3;

/** Three glints riding the rim, at fixed angles so the coin twinkles where its
 * edge would actually catch light. */
const GLINT_ANGLES = [0.6, 2.5, 4.3];
const GLINT_SIZE = 0.14;

const build = (place: PrizePlace, frame: PrizeFrame): readonly SceneInstance[] => {
  // Nothing to compose here. This coin declares `presentation: "faces-camera"`,
  // and the staging honours that: it leans the whole prize into the camera's own
  // elevation and rocks it gently instead of revolving it (see
  // `PrizePresentation`). So the coin is authored square in its local XY plane
  // and simply arrives facing the lens — no counter-rotation, no cancelling of
  // the reveal's turn, and one place to change if the shot ever changes.

  // The two-tier body: a wide dark blank, and the bright field struck proud of it.
  const blank = place("blank", "cylinder", "PrizeGoldSide", v3(0, 0, 0), v3(COIN_DIAMETER, COIN_THICKNESS, COIN_DIAMETER), FACE_CAMERA);
  const field = place("field", "cylinder", "PrizeGoldTop", v3(0, 0, 0), v3(FIELD_DIAMETER, FIELD_THICKNESS, FIELD_DIAMETER), FACE_CAMERA);

  // Denticles around the rim. `quatRoll(angle)` sends a box's local +X along the
  // radius, so each tick's length runs outward and its width runs tangentially —
  // one rotation places the whole ring with no per-tick trigonometry.
  const milling = Array.from({ length: MILL_TICKS }, (_, i): SceneInstance => {
    const angle = (i / MILL_TICKS) * Math.PI * 2;
    return place(
      `mill${i}`,
      "box",
      "PrizeGoldEtch",
      v3(Math.cos(angle) * MILL_RADIUS, Math.sin(angle) * MILL_RADIUS, RIM_Z + MILL_RELIEF / 2),
      v3(MILL_LENGTH, MILL_WIDTH, MILL_RELIEF),
      quatRoll(angle),
    );
  });

  // The struck concentric ring: a dark disc, and the field's own bright disc
  // struck proud in front of it so an annulus of shadow shows between the two.
  // Both are DEEPER than the field they sit inside, which is what keeps the ring
  // a ring from every angle the rock carries the coin through — a shallower pair
  // would be swallowed by the field as soon as the coin tipped.
  const device = [
    place("device", "cylinder", "PrizeGoldEtch", v3(0, 0, 0), v3(DEVICE_DIAMETER, DEVICE_DEPTH, DEVICE_DIAMETER), FACE_CAMERA),
    place("devicefield", "cylinder", "PrizeGoldTop", v3(0, 0, 0), v3(DEVICE_DIAMETER - 0.13, DEVICE_DEPTH + 0.02, DEVICE_DIAMETER - 0.13), FACE_CAMERA),
  ];

  // Glints, once the coin has settled. Each is dropped entirely below its
  // threshold rather than drawn at zero scale — a sparkle is on for a handful of
  // frames out of forty, so this is three instances most of the time and none of
  // them the rest.
  const glints = GLINT_ANGLES.flatMap((angle, i): readonly SceneInstance[] => {
    const twinkle = sparkleAt(i, frame.tick, frame.settle);
    const size = GLINT_SIZE * twinkle;
    return twinkle < 0.02
      ? []
      : [place(`glint${i}`, "sphere", "PrizeSparkle", v3(Math.cos(angle) * MILL_RADIUS, Math.sin(angle) * MILL_RADIUS, RIM_Z + MILL_RELIEF), v3(size, size, size))];
  });

  return [blank, field, ...milling, ...device, ...glints];
};

/**
 * The coin's true reach, and the two parts that set it: a rim glint at full
 * twinkle (hypot(0.66, 0.15) + 0.07 ≈ 0.747) and the outer corner of a milling
 * tick (hypot(0.700, 0.15) ≈ 0.716). The blank itself only reaches 0.707, so
 * declaring the disc alone would under-report the object. The sway cannot widen
 * this — a rotation about the origin leaves every part's distance from it
 * unchanged, which is exactly why the presentation is built as one rotation
 * rather than as per-part offsets.
 */
export const GOLD_COIN: Prize = {
  // A struck coin is nothing but its face, so it holds that face to the lens and
  // rocks. The staging owns the lean now (see `PrizePresentation`), so the coin
  // no longer has to cancel the turn itself.
  presentation: "faces-camera",
  lean: 1,
  build,
  extent: 0.75,
  // Nothing new: the coin is cut entirely from the shared metal ladder, which is
  // the point of that ladder existing — a coin and a bar in the same chest must
  // be the same substance.
  materials: {},
};
