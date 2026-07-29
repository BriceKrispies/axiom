/*
 * leather-boot.ts — the old boot. The joke prize.
 *
 * Every other treasure in this catalog is trying to be worth something. This one
 * is the punchline: the chest does the whole ceremony — the latch, the seam
 * light, the lid, the burst — and what climbs out is a boot somebody threw in
 * the sea. The gag only lands if the boot is genuinely DULL. So there is no
 * gold on it, no buckle, no stitching in a contrast color, no sparkle, and
 * nothing that twinkles on `settle`: the presentation keeps its promises and the
 * object refuses to pay them off. Anything decorative added here makes the boot
 * look like a prize, which is the one thing it must not look like.
 *
 * ── the read is a PROFILE, and profiles are long ───────────────────────────
 * A boot is an L: a SHAFT rising up +Y meeting a FOOT running out along +Z. But
 * an L only exists when you can see both limbs, and the first version of this
 * file got that wrong in two compounding ways — it pointed the toe at the lens
 * and it made the foot barely longer than the shaft was thick. The result was a
 * dark stubby block. Every recognisable drawing of a boot in the world is a
 * profile, and this one is authored for that fact:
 *
 *   * the FOOT is 1.32 long against a shaft 0.38 wide, and the shaft sits over
 *     the HEEL end of it, so 0.80 of pure toe overhangs the column. That
 *     overhang IS the L. Proportion does the work; no detail can rescue a
 *     silhouette that is square.
 *   * the whole assembly is yawed ~69° off head-on (`BOOT_TOE_YAW`) so it
 *     presents its length, not its toe.
 *
 * The prize also revolves (`presentation: "turntable"`), and its phase comes
 * from the free-running session tick — so the boot WILL pass through head-on
 * once a revolution and no authored yaw can prevent that. What saves those
 * frames is the same long foot: the camera looks down ~50°, so a foot this long
 * still projects most of its length down the screen even when it is pointing
 * straight at the lens. The geometry has to read from everywhere; the yaw only
 * picks the angle it rests at.
 *
 * ── value is authored for a camera that looks DOWN ─────────────────────────
 * There are no textures in this engine, so the boot is carved by a value ladder
 * — but the ladder is chosen by how much light a face is about to RECEIVE, not
 * by how important it is. The rig sums to ~1.25 on an up-facing surface and the
 * camera is above the object, so broad up-facing faces are the brightest thing
 * in the image by a wide margin. Painting the instep and the sole in a "lit"
 * tone (the first version's mistake) therefore produced a cream plinth with a
 * dark boot perched on it — the sole read as a plate the boot was standing on
 * rather than a sole under it.
 *
 * So the rungs invert against intuition: the HIGHEST albedo goes on the boot's
 * vertical leather, which the key under-serves, and up-facing faces step DOWN
 * one or two rungs to land beside it instead of blowing past it. The sole is
 * pushed to near-black so that even with the full key on its thin welt lip it
 * stays the darkest thing on the boot — a shadow line under the leather, which
 * is what a sole looks like from above. Nothing exceeds 0.58, well under the
 * knee the gold's 0.78 rung is calibrated against, and nothing is emissive: a
 * boot that blooms is a boot that looks expensive.
 */

import type { EngineQuat, MaterialSpec, SceneInstance } from "@axiom/web-engine";
import { QUAT_IDENTITY, quatMul, quatPitch, quatRoll, quatYaw, rotateByQuat } from "../../../presentation/stage/vectors.ts";
import type { Prize, PrizeFrame, PrizePlace } from "./prize.ts";
import { solid, v3 } from "./prize.ts";

/**
 * Worn brown leather in three rungs, plus the sole and the holes.
 *
 * `PrizeLeather` is the STANDING rung — the shaft and the foot's flanks, the
 * faces a key coming from above and the side barely reaches, so they carry the
 * most albedo. `PrizeLeatherDark` is one rung down and does double duty: a
 * vertical face turned away from the key (the shaft's shadow flank) and an
 * up-facing face the key over-serves (the cuff crown, the toe's upper steps)
 * want the same correction. `PrizeLeatherTop` is the instep — the one broad
 * up-facing plane on the boot, dropped two rungs so the full ~1.25 lands it
 * level with the standing leather rather than turning it into a pale slab.
 */
const BOOT_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  PrizeLeather: solid([0.58, 0.39, 0.23, 1]),
  PrizeLeatherDark: solid([0.44, 0.29, 0.17, 1]),
  PrizeLeatherTop: solid([0.3, 0.2, 0.12, 1]),
  PrizeBootSole: solid([0.13, 0.11, 0.1, 1]),
  PrizeBootShadow: solid([0.07, 0.06, 0.055, 1]),
};

/*
 * ── the stack, in prize-local units ────────────────────────────────────────
 * The boot is authored bottom-up as courses that must MEET, so a seam is one
 * named height shared by the two parts either side of it rather than two
 * numbers that can drift apart. Raising the sole raises the foot with it.
 */
const HEEL_BOTTOM = -0.8;
const SOLE_BOTTOM = -0.7;
/** The welt line: the sole's top face is the upper's bottom face. */
const SOLE_TOP = -0.59;
const FOOT_TOP = -0.17;
const SHAFT_TOP = 0.66;
const CUFF_TOP = 0.8;

/*
 * Depths along the boot's length. The shaft sits over the BACK of the foot, the
 * way a leg sits over a heel — that placement is what buys the toe overhang,
 * and it also re-centres the assembly on the origin, which is what keeps the
 * declared `extent` down.
 */
const SHAFT_BACK = -0.54;
const SHAFT_FRONT = -0.08;
const SOLE_BACK = -0.6;
const SOLE_FRONT = 0.72;

/** Half-widths, widest last. The sole is the widest thing on the boot by
 * design — that is what "stands proud on every side" means, and the ordering
 * states it rather than leaving a reader to spot it. The shaft is deliberately
 * the slimmest: a thick shaft and a long foot read as a mallet, not a boot. */
const SHAFT_HALF = 0.19;
const FOOT_HALF = 0.22;
const CUFF_HALF = 0.23;
const SOLE_HALF = 0.26;

/**
 * How far the boot is turned off head-on, so the camera gets its length.
 *
 * The magnitude (~69°) is a three-quarter view: nearly the full profile, with
 * enough of the boot's width left to say it is a solid object rather than a
 * cardboard cutout. The SIGN is not arbitrary — it is read off the rig. The key
 * travels along (-0.6, -0.58, -0.5), so it arrives from the upper right and
 * from the camera's side; swinging the toe toward -X puts the boot's long flank
 * on a normal of about (+0.36, 0, +0.93), which faces the lens AND the key at
 * once. The mirrored yaw presents the same profile with that flank in shadow,
 * which is how you get a boot-shaped hole instead of a boot.
 */
const BOOT_TOE_YAW = -1.2;

/**
 * The boot's resting attitude. It has been shut in a chest for years; it is not
 * squared to the world axes. `pitch` is about the across-boot axis, so it tips
 * the toe up and settles the weight onto the heel; `roll` is about the
 * along-boot axis, so it slumps the whole thing a few degrees to one side.
 *
 * This is a RIGID rotation of the whole assembly — every part's offset AND its
 * orientation go through it — so it cannot change how far the boot reaches from
 * the origin, and `BOOT_EXTENT` is computed on the un-posed stack.
 */
const BOOT_POSE: EngineQuat = quatMul(quatYaw(BOOT_TOE_YAW), quatMul(quatRoll(0.05), quatPitch(-0.05)));

/**
 * How far the assembly reaches. The binding point is the sole's front-outer-
 * bottom corner — the far end of the longest, lowest, widest slab on the boot:
 * hypot(0.26, 0.70, 0.72) ≈ 1.037. The cuff's back-top corner (≈ 1.02) and the
 * heel's back-bottom corner (≈ 1.02) sit just inside it, which is the sign that
 * the L is centred on the origin rather than hanging off one side of it: a boot
 * pushed forward in Z would declare a much bigger sphere for the same object.
 */
const BOOT_EXTENT = 1.04;

const build = (place: PrizePlace, frame: PrizeFrame): readonly SceneInstance[] => {
  // A whisper of sway, gated on `settle` so it cannot start while the boot is
  // still climbing. It is ~1.7° at the extreme: the boot should look like it is
  // hanging there doing nothing, because the comedy is in the presentation
  // straining and the object refusing to perform. Pure in the tick.
  const sway = Math.sin(frame.tick * 0.05) * 0.03 * frame.settle;
  const pose = quatMul(BOOT_POSE, quatRoll(sway));

  // Every part goes through here rather than through `place` directly: the boot
  // is ONE rigid object, so the pose rotates each part's offset into the posed
  // frame and composes into each part's own rotation — the same "pose the
  // assembly, not the pieces" pattern the beach props and the chest lid use.
  const part: PrizePlace = (suffix, mesh, material, offset, scale, rotation = QUAT_IDENTITY) =>
    place(suffix, mesh, material, rotateByQuat(offset, pose), scale, quatMul(pose, rotation));

  // A course of the stack, given by the SPANS it occupies (y0→y1, z0→z1) instead
  // of a centre and a size. Every part of this boot is symmetric about x, and
  // every seam is a shared span endpoint, so spans are the form the numbers
  // above are actually in — converting them by hand at each call site is where a
  // gap between the sole and the upper would come from.
  const slab = (suffix: string, material: string, half: number, y0: number, y1: number, z0: number, z1: number): SceneInstance =>
    part(suffix, "box", material, v3(0, (y0 + y1) / 2, (z0 + z1) / 2), v3(half * 2, y1 - y0, z1 - z0));

  // The sole: a long dark slab standing proud of the upper at both ends and on
  // both sides. Its top face is covered by the foot, so the only up-facing part
  // of it is the thin welt lip around the edge — a shadow line under the
  // leather. That lip, and not any brighter detail, is what stops the L reading
  // as a bent tube. The heel is only under the BACK of it, so the underside has
  // a step and the toe can tip up.
  const sole = slab("sole", "PrizeBootSole", SOLE_HALF, SOLE_BOTTOM, SOLE_TOP, SOLE_BACK, SOLE_FRONT);
  const heel = slab("heel", "PrizeBootSole", 0.24, HEEL_BOTTOM, SOLE_BOTTOM, -0.58, -0.18);

  // The foot: the L's long limb, on the standing rung because what the camera
  // sees of it is its two flanks.
  const foot = slab("foot", "PrizeLeather", FOOT_HALF, SOLE_TOP, FOOT_TOP, -0.59, 0.36);
  // The instep. Two rungs down, and NOT because it is unimportant — it is the
  // one broad up-facing plane on the boot, so it is the face the rig hits
  // hardest (see the header). It stands a hair proud of the foot so it draws
  // clear of it.
  const vamp = slab("vamp", "PrizeLeatherTop", 0.19, FOOT_TOP - 0.04, FOOT_TOP + 0.02, -0.12, 0.36);
  // Two stepped boxes plus a squashed sphere round the toe off. Each step is
  // shorter AND narrower than the one behind it, so the taper reads in plan and
  // in silhouette; the cap fills the corner the steps leave square. This is the
  // engine's primitive vocabulary being honest — there is no rounded-box mesh,
  // and a single blunt end would read as a brick. They wear the middle rung
  // because their exposed faces tilt up toward the key.
  const toeStep = slab("toe0", "PrizeLeatherDark", 0.2, SOLE_TOP, -0.3, 0.3, 0.53);
  const toeTip = slab("toe1", "PrizeLeatherDark", 0.17, SOLE_TOP, -0.4, 0.47, 0.64);
  const toeCap = part("toecap", "sphere", "PrizeLeatherDark", v3(0, -0.44, 0.53), v3(0.38, 0.28, 0.3));

  // The shaft: the L's rising limb, over the heel end of the foot. Slim, so the
  // toe overhang has something to be long against.
  const shaft = slab("shaft", "PrizeLeather", SHAFT_HALF, -0.26, SHAFT_TOP, SHAFT_BACK, SHAFT_FRONT);
  // Panels standing just proud of the shaft's two flanks, one rung down. With
  // one flat material per primitive a box's four sides shade alike, so this is
  // how the shaft gets a light-side / shadow-side break at all — the same trick
  // the chest body's end caps use. It matters more here than on the chest,
  // because the boot revolves: the flank facing the key and the flank facing
  // away swap every half turn, and a shaft with no side value at all would
  // flatten into a plank twice a revolution.
  const shaftMid = (SHAFT_BACK + SHAFT_FRONT) / 2;
  const shaftSides = [-1, 1].map((side): SceneInstance =>
    part(`shaftside${side < 0 ? "L" : "R"}`, "box", "PrizeLeatherDark", v3(side * (SHAFT_HALF + 0.005), 0.2, shaftMid), v3(0.04, 0.8, 0.42)),
  );

  // The rolled cuff: wider and deeper than the shaft it caps, so the top of the
  // boot flares instead of ending on a cut. Middle rung — its crown is up-facing
  // and would otherwise become the brightest thing in frame. The mouth is a
  // near-black inset a hair proud of that crown: proud so it draws clear of the
  // cuff, near-black so the opening reads as a hole down a leg the player is
  // looking almost straight into.
  const cuff = slab("cuff", "PrizeLeatherDark", CUFF_HALF, 0.62, CUFF_TOP, -0.59, -0.03);
  const mouth = slab("mouth", "PrizeBootShadow", 0.17, CUFF_TOP - 0.06, CUFF_TOP + 0.01, -0.51, -0.07);

  // Two laces crossed over the instep. At the size this thing is ever seen they
  // are texture, not hardware: they break the one large up-facing plane on the
  // boot so it does not read as a slab. Deliberately NOT threaded through
  // eyelets — eyelets would be detail, and detail is what this prize must not
  // have.
  const laces = [-1, 1].map((side, i): SceneInstance =>
    part(`lace${i}`, "box", "PrizeBootShadow", v3(0, FOOT_TOP + 0.03, 0.02 + i * 0.18), v3(0.38, 0.03, 0.06), quatYaw(side * 0.42)),
  );

  return [sole, heel, foot, vamp, toeStep, toeTip, toeCap, shaft, ...shaftSides, cuff, mouth, ...laces];
};

// The boot turns, and stands almost upright: its silhouette is the L of shaft
// against foot, and that L only exists while the boot is standing. Tipped into
// the lens it would read as a boot lying on its back, which is a different and
// much worse joke.
export const LEATHER_BOOT: Prize = { build, extent: BOOT_EXTENT, lean: 0.16, materials: BOOT_MATERIALS, presentation: "turntable" };
