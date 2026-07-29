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
import { quatMul, quatRoll, quatYaw, v3 } from "../../presentation/stage/vectors.ts";
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

/**
 * The crab's body, eyes, claws and legs, posed by `pose` and placed through
 * `place`. `tick` drives only the per-limb phases (claws alternate, legs
 * paddle); everything gross comes from the pose the caller resolved.
 */
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
 * out and slightly down, the shin turns and drops steeply, so the leg reads as a
 * leg from the side (the beach crab) AND head-on (the prize), where the two
 * segments cross at an angle instead of vanishing into one line.
 */
const HIP = { x: 0.22, y: 0.19 };
const KNEE = { x: 0.42, y: 0.13 };
const TOE = { x: 0.49, y: 0.0 };
const LEG_ROW = [-0.16, 0.02, 0.2];
const THIGH_THICK = 0.075;
const SHIN_THICK = 0.06;

/**
 * One jointed leg: `s` is the side, `z` its place along the body, and the row
 * index gives it its own wiggle phase so the six never paddle in unison.
 *
 * Each segment is a box whose LENGTH runs along its local +Y, rolled onto the
 * segment's own direction in the body's X/Y plane — so the joint angles fall out
 * of the hip/knee/toe points rather than being tuned. The fan yaw is composed
 * OUTSIDE that roll, which swings the whole finished leg toward the front or
 * back of the crab; composing it the other way round would twist each segment
 * about its own length and leave the knee behind.
 */
const crabLeg = (place: CrabPlace, s: number, row: number, z: number, pose: CrabPose, tick: number): readonly SceneInstance[] => {
  const fan = quatYaw(s * 0.5 + s * pose.legWiggle * Math.sin(tick * 0.7 + row * 1.2));
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
      v3(((from.x + to.x) / 2) * s, (from.y + to.y) / 2, z),
      v3(thick, Math.hypot(dx, dy), thick),
      quatMul(fan, roll),
    );
  };
  return [segment("thigh", HIP, KNEE, THIGH_THICK), segment("shin", KNEE, TOE, SHIN_THICK)];
};

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
      // Each claw lifts and snaps on its own phase, so a wave alternates sides.
      const lift = pose.clawLift * (0.7 + 0.3 * Math.sin(tick * 0.5 + (s > 0 ? 0 : Math.PI)));
      return [
        place(`arm${s}`, "CrabShellDark", "box", v3(s * 0.42, 0.18 + lift * 0.12, 0.24), v3(0.1, 0.09, 0.28), quatRoll(s * lift)),
        place(`claw${s}`, "CrabShell", "sphere", v3(s * 0.5, 0.18 + lift * 0.3, 0.42), v3(0.22, 0.18, 0.2), quatRoll(s * lift)),
      ];
    })
    .flat();
  const legs = [-1, 1].map((s): readonly SceneInstance[] => LEG_ROW.map((z, i) => crabLeg(place, s, i, z, pose, tick)).flat()).flat();
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
 * How far the whole bow is cocked, and how far each wing tips away from that.
 *
 * These two used to be 0.34 and 0.42 — nearly equal, which is what made one wing
 * look wrong. The wings ARE mirrored about the cock, but at those magnitudes the
 * pair landed on −0.08 and +0.76 radians: one wing sat essentially square to the
 * world while the other stood at 43°. A near-square box reads as a rectangle and
 * a box at 43° reads as a diamond, so the two halves of one bow read as two
 * different shapes and the tie looked broken rather than jaunty.
 *
 * The splay is now clearly smaller than the cock, so both wings sit on the same
 * side of square and read as a matched pair tipped together — and the wings are
 * ELONGATED (a 2.5:1 ribbon rather than the old near-square block), which is what
 * makes a wing read as a wing at any angle at all. The bow still sits off-centre
 * and askew; that was never the problem, and it is the whole charm.
 */
const BOW_COCK = 0.3;
const BOW_SPLAY = 0.24;

const crabBowtie = (place: CrabPlace, pose: CrabPose, amount: number): readonly SceneInstance[] => {
  const lift = BOW_OFFSET.y + 0.4 * pose.breath;
  const wings = [-1, 1].map((s): SceneInstance =>
    place(
      `bow${s}`,
      "CrabBow",
      "box",
      v3(BOW_OFFSET.x + s * 0.125 * amount, lift, BOW_OFFSET.z),
      v3(0.215 * amount, 0.088 * amount, 0.07 * amount),
      quatRoll(BOW_COCK + s * BOW_SPLAY),
    ),
  );
  const knot = place(
    "bowknot",
    "CrabBowKnot",
    "box",
    v3(BOW_OFFSET.x, lift, BOW_OFFSET.z),
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
  eye: 0,
  kind: "rest",
  legWiggle: 0,
  scootX: 0,
  yaw: 0,
};
