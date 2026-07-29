/*
 * gold-coin.ts — the common prize: one fat struck coin with a face on it.
 *
 * This is what a small win looks like, so it has to be instantly legible at a
 * glance and charming on the second glance. Two decisions carry the whole
 * object:
 *
 * 1. It is STRUCK, not a puck. A single cylinder reads as a plastic checker no
 *    matter how good the gold is. So the coin is two coaxial discs — a wider,
 *    thinner blank in the SIDE rung whose exposed annulus is the recessed rim,
 *    and a narrower, thicker field in the TOP rung standing proud of it. The
 *    step between them is a real geometric ledge, and it lands the palette's
 *    brightest gold on the big camera-facing surface with a darker ring around
 *    it — which is what "minted" looks like without a texture.
 *
 * 2. It always FACES the camera. A slow yaw turn is right for a bar or a boot
 *    and fatal for a coin: half of every revolution would be spent edge-on and
 *    the prize would thin to a line and disappear. So this one declares
 *    `presentation: "faces-camera"` and the staging leans it into the lens and
 *    rocks it instead of revolving it. A treasure the player cannot see for half
 *    its cycle is a bug, not a flourish.
 */

import type { EngineVec3, SceneInstance } from "@axiom/web-engine";
import { QUAT_IDENTITY, quatPitch, quatRoll } from "../../../presentation/stage/vectors.ts";
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

// ── the face ────────────────────────────────────────────────────────────────

/*
 * The features are held in RELIEF — small boxes and studs standing ~0.045 proud
 * of the field — rather than painted on as flat decals. Relief is what survives
 * this game's shading: each raised part catches the key on its own facet and
 * throws its own value break, so the face still reads on the software backend,
 * which has no texture and only flat per-facet light to work with. The material
 * then does the opposite job: the darkest rungs (`Deep`, `Etch`) make a raised
 * part read as a STRUCK RECESS holding shadow, which is how a real mint mark
 * looks and how nothing here can be mistaken for a sticker.
 */
const FEATURE_Z = 0.155;
const FEATURE_DEPTH = 0.06;

/** Big eyes, set wide. A coin face has one job at thumbnail size — the eyes and
 * the smile are the whole read, so they are deliberately oversized for the head
 * they sit in. */
const EYE_X = 0.21;
const EYE_Y = 0.19;
const EYE_DIAMETER = 0.19;

/** Brows, cocked up-and-out. A pair of level bars reads as a frown by accident;
 * a few degrees of outward lift is the entire difference between "grumpy coin"
 * and "pleased coin", for two parts. */
const BROW_Y = 0.345;
const BROW_TILT = 0.2;

/*
 * The smile is an ARC of stepped boxes, not a bar. This codebase builds every
 * curve it cannot express as an honest run of flat facets (the chest's barrel
 * lid is eight of them — see `lidArc` in `scene.ts`), and the argument is the
 * same at this scale: a straight bar under two round eyes reads as a slot, and
 * one box rolled to a slant reads as a smirk. Five chords around a circle read
 * as a smile from the first frame.
 *
 * Each chord is placed from its OWN pair of arc points, so the segments meet
 * however many there are. They overlap slightly (`SMILE_JOIN`) because a box has
 * square ends: at this curvature the wedge between two consecutive chords would
 * show as a nick in the mouth, and a smile with gaps in it reads as teeth.
 */
const SMILE_SEGMENTS = 5;
const SMILE_RADIUS = 0.3;
const SMILE_CENTER_Y = 0.06;
const SMILE_FROM = Math.PI * 1.18;
const SMILE_TO = Math.PI * 1.82;
const SMILE_THICKNESS = 0.085;
const SMILE_JOIN = 1.14;

/** A point on the smile's circle, in face-local space. */
const smilePoint = (t: number): EngineVec3 => {
  const angle = SMILE_FROM + (SMILE_TO - SMILE_FROM) * t;
  return v3(Math.cos(angle) * SMILE_RADIUS, SMILE_CENTER_Y + Math.sin(angle) * SMILE_RADIUS, FEATURE_Z);
};

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

  // Eyes are studs (a short cylinder facing the camera), not boxes: a round eye
  // is worth the tessellation, and it is the one place on this object where the
  // difference between a circle and a square is the difference between a face
  // and a robot.
  const eyes = [-1, 1]
    .map((s): readonly SceneInstance[] => [
      place(`eye${s}`, "cylinder", "PrizeGoldEtch", v3(s * EYE_X, EYE_Y, FEATURE_Z), v3(EYE_DIAMETER, FEATURE_DEPTH, EYE_DIAMETER), FACE_CAMERA),
      place(`brow${s}`, "box", "PrizeGoldDeep", v3(s * (EYE_X + 0.01), BROW_Y, FEATURE_Z), v3(0.26, 0.055, FEATURE_DEPTH), quatRoll(s * BROW_TILT)),
    ])
    .flat();

  // A small round nose, kept low and shallow so it sits between the eyes and the
  // smile without competing with either for the silhouette.
  const nose = place("nose", "sphere", "PrizeGoldDeep", v3(0, 0.045, FEATURE_Z), v3(0.13, 0.15, FEATURE_DEPTH * 2), QUAT_IDENTITY);

  const smile = Array.from({ length: SMILE_SEGMENTS }, (_, i): SceneInstance => {
    const from = smilePoint(i / SMILE_SEGMENTS);
    const to = smilePoint((i + 1) / SMILE_SEGMENTS);
    const dx = to.x - from.x;
    const dy = to.y - from.y;
    // A box's local +X runs along its length, so the chord's own angle IS the roll.
    return place(
      `smile${i}`,
      "box",
      "PrizeGoldEtch",
      v3((from.x + to.x) / 2, (from.y + to.y) / 2, FEATURE_Z),
      v3(Math.hypot(dx, dy) * SMILE_JOIN, SMILE_THICKNESS, FEATURE_DEPTH),
      quatRoll(Math.atan2(dy, dx)),
    );
  });

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

  return [blank, field, ...milling, ...eyes, nose, ...smile, ...glints];
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
