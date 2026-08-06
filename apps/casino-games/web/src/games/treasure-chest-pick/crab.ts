/*
 * crab.ts — the stubby cartoon crab, as one assembly two places share.
 *
 * There are two crabs in this game and they must be the SAME creature: the one
 * scuttling about on the beach, and his girlfriend, who turns up as a prize in
 * the chest wearing a pink bowtie. Building her separately would guarantee they
 * drift apart the first time either is touched, so the body lives here once and
 * both call sites pose it.
 *
 * The split is at the FRAME, not at the geometry. `crabParts` never computes a
 * world position: it hands every part to a caller-supplied `place`, in crab-local
 * space (origin between the feet, +Y up, +Z toward the camera, roughly 0.6 wide
 * and 0.65 tall). The beach maps that onto the sand through the crab's idle
 * pose; the prize maps it into prize-local space and hangs it in mid-air. Same
 * crab, two frames.
 */

import type { EngineQuat, EngineVec3, MaterialSpec, SceneInstance } from "@axiom/web-engine";
import { addV3, quatMul, quatRoll, quatYaw, rotateByQuat, scaleV3, v3 } from "../../presentation/stage/vectors.ts";
import type { CrabPose } from "./game.ts";

/** Place one crab part, given in crab-local space. Both call sites supply this. */
export type CrabPlace = (
  key: string,
  material: string,
  mesh: "box" | "sphere",
  local: EngineVec3,
  scale: EngineVec3,
  localRot?: EngineQuat,
) => SceneInstance;

/** The crab palette. Owned here so both crabs are literally the same color. */
export const CRAB_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  // The crab reads as a coral beach creature, not a second brand accent: pulled
  // off the saturated brand red toward warm coral so the only true reds in frame
  // are the intentional branding surfaces.
  CrabShell: { baseColor: [0.85, 0.34, 0.24, 1] },
  CrabShellDark: { baseColor: [0.66, 0.24, 0.16, 1] },
  CrabEye: { baseColor: [0.06, 0.05, 0.05, 1] },
  // Her bowtie. A soft candy pink that stays clear of the coral shell it sits on
  // — a pink too close to the body would read as a lump rather than a ribbon —
  // and a deeper rose for the knot, so the tie carries the same lit/shadow value
  // step every other prop here uses in place of a texture.
  CrabBow: { baseColor: [0.95, 0.52, 0.68, 1] },
  CrabBowKnot: { baseColor: [0.78, 0.34, 0.5, 1] },
};

/** What this crab is wearing / carrying. */
export interface CrabDress {
  /** Ramp in [0, 1] for the pink bowtie on the top of the shell — hers only. */
  readonly bowtie: number;
  /** Whether the little brand pennant is raised in the right claw — his only. */
  readonly pennant: boolean;
}

// ── the legs ────────────────────────────────────────────────────────────────

/*
 * A crab leg is JOINTED — it goes out from the shell, bends at a knee, and comes
 * back down to the sand. It used to be one flat tab: a single 0.24 × 0.06 box
 * poking sideways out of the body, which read as a paddle rather than a leg and
 * had no silhouette at all once the prize crab was turned to face the camera
 * (from the front, a horizontal tab is a horizontal line).
 *
 * So each leg is now two segments about a knee — the same "a form the primitive
 * vocabulary can't express becomes an honest run of facets" move the chest lid's
 * dome and the clam's ribs make. The bend is what does the work: the thigh goes
 * out from the shoulder almost level, the shin turns at the knuckle and drops
 * steeply to the sand, so the leg reads as a leg from the side (the beach crab)
 * AND head-on (the prize), where the two segments cross at an angle instead of
 * vanishing into one line.
 *
 * ── THE ARCH, and why the leg had to leave the belly ─────────────────────────
 *
 * The joint CHAIN was right and the STANCE was wrong. Hip (0.22, 0.19), knee
 * (0.42, 0.13), toe (0.49, 0) put the whole limb BELOW the shell's equator: the
 * shell is a 0.62 x 0.4 x 0.5 dome centred at y 0.2, so y 0.19 is its widest
 * line and everything the leg did after that went down and INSIDE its own
 * silhouette. Under this game's camera (54.5 deg down) the dome then hides its
 * own legs — the frame showed a bare red blob with two pale claw balls beneath it
 * and one leg peeking out at the bottom left, while the reference crab's whole
 * read is eight limbs radiating CLEAR of a round shell (three walking legs a
 * side, plus the claws in front). No retune of a sub-equator leg recovers that:
 * the leg has to come out of the SHOULDER and arch over the dome's outline.
 *
 * So the hip climbs to y 0.30 — the shell's upper flank, where the dome is still
 * 0.268 half-wide, so the joint stays socketed inside it — the thigh runs out
 * almost LEVEL to 0.52, clearing the dome's projected outline by a real margin at
 * this pitch, and the shin drops steeply from that knuckle to the sand at 0.58.
 * Reach goes 0.49 -> 0.58: from 0.18 past the shell's edge to 0.27, putting the
 * tips at ~1.9x shell width overall against the reference's measured 1.98x.
 *
 * The segments THICKEN with the extra length (0.075/0.06 -> 0.09/0.075). A
 * 0.32-long shin at 0.06 is a 5:1 wire at this scale and the reference's legs are
 * chunky ~3:1 tapers; longer AND thinner would have traded a hidden leg for a
 * hair. Node count is untouched — this is the same twelve boxes, posed.
 */
const HIP = { x: 0.24, y: 0.3 };
const KNEE = { x: 0.52, y: 0.31 };
const TOE = { x: 0.58, y: 0.0 };
const THIGH_THICK = 0.09;
const SHIN_THICK = 0.075;

/**
 * The three walking legs on a side: where each hip sits along the body (`z`, +Z
 * toward the camera) and how far that whole leg is swung fore/aft off
 * straight-out (`fan`, radians, positive = swept toward the crab's REAR).
 *
 * The fan used to be a single constant 0.5 shared by all six, which is the second
 * half of the same defect: three legs at one hip x with one identical heading are
 * three COINCIDENT legs from above, so each side contributed one thick limb to the
 * silhouette instead of three. The reference fans them plainly — rear leg swept
 * back over the shoulder, middle straight out, front leg reaching forward past the
 * shell's cheek — so that is what the table says now, per row.
 */
const LEG_STATIONS: readonly { readonly fan: number; readonly z: number }[] = [
  { fan: 0.72, z: -0.16 },
  { fan: 0.16, z: 0.02 },
  { fan: -0.34, z: 0.2 },
];

/**
 * One jointed leg: `s` is the side, `station` its hip's place and sweep along the
 * body, and the row index gives it its own wiggle phase so the six never paddle
 * in unison.
 *
 * Each segment is a box whose LENGTH runs along its local +Y, rolled onto the
 * segment's own direction in the body's X/Y plane — so the joint angles fall out
 * of the hip/knee/toe points rather than being tuned. The fan yaw is composed
 * OUTSIDE that roll, which swings the whole finished leg toward the front or
 * back of the crab; composing it the other way round would twist each segment
 * about its own length and leave the knee behind.
 *
 * And the fan swings the leg's POSITIONS about the hip, not just its heading. It
 * used to rotate the heading alone, which is why a fan bought no silhouette at
 * all: knee and toe stayed stacked at the same x/z whatever the yaw said, so a
 * "swept" leg was a box pointing one way while sitting exactly where an unswept
 * leg sits. A limb swings from its joint or it does not swing.
 */
const crabLeg = (
  place: CrabPlace,
  s: number,
  row: number,
  station: { readonly fan: number; readonly z: number },
  pose: CrabPose,
  tick: number,
): readonly SceneInstance[] => {
  const fan = quatYaw(s * (station.fan + pose.legWiggle * Math.sin(tick * 0.7 + row * 1.2)));
  const hipAt = v3(HIP.x * s, HIP.y, station.z);
  /** A point on the leg's own plane, swung about the hip into the body frame. */
  const swung = (p: { x: number; y: number }): EngineVec3 =>
    addV3(hipAt, rotateByQuat(v3((p.x - HIP.x) * s, p.y - HIP.y, 0), fan));
  const segment = (suffix: string, from: { x: number; y: number }, to: { x: number; y: number }, thick: number): SceneInstance => {
    const dx = (to.x - from.x) * s;
    const dy = to.y - from.y;
    // quatRoll(t) sends +Y to (−sin t, cos t, 0); this is the t that puts it on
    // (dx, dy), so the box's length lies exactly along the segment.
    const roll = quatRoll(Math.atan2(-dx, dy));
    return place(
      `${suffix}${s}_${row}`,
      "CrabShellDark",
      "box",
      scaleV3(addV3(swung(from), swung(to)), 0.5),
      v3(thick, Math.hypot(dx, dy), thick),
      quatMul(fan, roll),
    );
  };
  return [segment("thigh", HIP, KNEE, THIGH_THICK), segment("shin", KNEE, TOE, SHIN_THICK)];
};

/**
 * The crab's body, eyes, claws and legs, posed by `pose` and placed through
 * `place`. `tick` drives only the per-limb phases (claws flap, legs paddle);
 * everything gross comes from the pose the caller resolved.
 */
export const crabParts = (place: CrabPlace, pose: CrabPose, tick: number, dress: CrabDress): readonly SceneInstance[] => {
  const body = place("body", "CrabShell", "sphere", v3(0, 0.2, 0), v3(0.62, 0.4 * (1 + pose.breath), 0.5));
  const eyes = [-1, 1]
    .map((s): readonly SceneInstance[] => [
      place(`stalk${s}`, "CrabShell", "box", v3(s * 0.14, 0.44, 0.16), v3(0.06, 0.18, 0.06), quatRoll(-s * pose.eye)),
      place(`eye${s}`, "CrabEye", "sphere", v3(s * 0.14 + s * pose.eye * 0.12, 0.55, 0.16), v3(0.1, 0.1, 0.1)),
    ])
    .flat();
  const claws = [-1, 1]
    .map((s): readonly SceneInstance[] => {
      // Each claw lifts and snaps on its own phase, so a wave alternates sides —
      // but only as far as `pose.clawShake` asks for. At 1 this is the full ±30%
      // flap a wave wants; at 0 the claw is simply HELD where the lift puts it,
      // which is what a crab gripping a chest lid needs. The flap used to be
      // unconditional, and on the lid it read as violent shaking.
      const flap = pose.clawShake * 0.3 * (Math.sin(tick * 0.5 + (s > 0 ? 0 : Math.PI)) - 1);
      const lift = pose.clawLift * (1 + flap);
      return [
        place(`arm${s}`, "CrabShellDark", "box", v3(s * 0.42, 0.18 + lift * 0.12, 0.24), v3(0.1, 0.09, 0.28), quatRoll(s * lift)),
        place(`claw${s}`, "CrabShell", "sphere", v3(s * 0.5, 0.18 + lift * 0.3, 0.42), v3(0.22, 0.18, 0.2), quatRoll(s * lift)),
      ];
    })
    .flat();
  const legs = [-1, 1]
    .map((s): readonly SceneInstance[] => LEG_STATIONS.map((station, i) => crabLeg(place, s, i, station, pose, tick)).flat())
    .flat();
  // A little brand pennant on a pole, raised in the right claw — welded to the
  // body frame, so it scoots and turns with the crab.
  const pennant: readonly SceneInstance[] = dress.pennant
    ? [
        place("flagpole", "BrandPost", "box", v3(0.58, 0.5, 0.34), v3(0.04, 0.7, 0.04)),
        place("flag", "BrandPrimary", "box", v3(0.74, 0.66, 0.34), v3(0.3, 0.2, 0.03)),
      ]
    : [];
  return [body, ...eyes, ...claws, ...legs, ...pennant, ...crabBowtie(place, pose, dress.bowtie)];
};

/**
 * The pink bowtie, perched on top of the domed shell and deliberately OFF
 * CENTRE — pushed to her left and cocked a few degrees, because a ribbon
 * squared up on the midline reads as a machine part and one knocked askew reads
 * as something she put on. The whole thing rides the shell's breathe (it is
 * placed in the same crab-local frame as the body it sits on) and grows in on
 * `amount`, so a crab that is not wearing one costs nothing.
 *
 * Three boxes: two wings splayed out from a knot, each rolled up-and-out so the
 * tie catches the key light on a different face than the shell beneath it.
 */
const BOW_OFFSET = v3(0.16, 0.4, 0.02);

/**
 * How far the whole bow is cocked, how far each wing tips away from that, and
 * how far out from the knot each wing's centre sits.
 *
 * The wings are a MIRRORED PAIR, and the mirror is the bow's own axis — the one
 * the cock defines — not crab-local vertical. That distinction is the whole
 * reason this used to look broken: `place` takes a part's offset in the
 * unrotated crab-local frame and applies `localRot` to the part about itself, so
 * the previous bow stepped its wings out along ±X (a mirror about crab-local
 * vertical) while rolling them about the cock (a mirror about the cocked axis).
 * Two different mirror planes cannot describe one mirrored pair: the wing whose
 * tip leaned along the step read as a ribbon flowing out of the knot, and the
 * other read as a ribbon kinked back into it. Retuning the two angles could
 * never fix that, because the angles were never the defect.
 *
 * So the step-out (`BOW_REACH`) is rotated by the SAME cock as the wing.
 * In the bow's own frame each wing then sits at (±reach, 0, 0) with roll
 * ∓splay — mirror-exact by construction, at any cock — and the cocked pair
 * carries a wing slightly above the knot and the other slightly below, which is
 * what a ribbon tied askew actually does. The bow still sits off-centre and
 * cocked; that was never the problem, and it is the whole charm.
 *
 * The wings stay ELONGATED (a 2.5:1 ribbon, not a near-square block), which is
 * what makes a wing read as a wing at any angle at all.
 */
const BOW_COCK = 0.3;
const BOW_SPLAY = 0.24;
const BOW_REACH = 0.125;

const crabBowtie = (place: CrabPlace, pose: CrabPose, amount: number): readonly SceneInstance[] => {
  const lift = BOW_OFFSET.y + 0.4 * pose.breath;
  const knotAt = v3(BOW_OFFSET.x, lift, BOW_OFFSET.z);
  const wings = [-1, 1].map((s): SceneInstance =>
    place(
      `bow${s}`,
      "CrabBow",
      "box",
      // Out from the knot along the bow's own axis, so the offset mirrors in the
      // same frame the roll does.
      addV3(knotAt, rotateByQuat(v3(s * BOW_REACH * amount, 0, 0), quatRoll(BOW_COCK))),
      v3(0.215 * amount, 0.088 * amount, 0.07 * amount),
      quatRoll(BOW_COCK + s * BOW_SPLAY),
    ),
  );
  const knot = place(
    "bowknot",
    "CrabBowKnot",
    "box",
    knotAt,
    v3(0.07 * amount, 0.075 * amount, 0.085 * amount),
    quatRoll(BOW_COCK),
  );
  return amount <= 0.001 ? [] : [...wings, knot];
};

/** A crab standing still and simply breathing — the pose a prize crab holds. */
export const CRAB_AT_REST: CrabPose = {
  bob: 0,
  breath: 0,
  clawLift: 0,
  clawShake: 0,
  eye: 0,
  kind: "rest",
  legWiggle: 0,
  yaw: 0,
};
