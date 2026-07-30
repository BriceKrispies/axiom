/*
 * pearl-clam.ts — an open clam shell holding a pearl.
 *
 * The gape is the whole read. A closed clam is a rock, and a clam whose opening
 * points anywhere but at the lens is a rock seen from behind, so everything in
 * this file exists to hold one mouth open toward the camera with something worth
 * looking at inside it.
 *
 * ── how the two valves meet the camera ─────────────────────────────────────
 * This prize is `faces-camera`, which means the staging leans it until its local
 * +Z lies along the vector back to the camera and its local +Y is screen-up (see
 * `prizeSpin`), then rocks it gently instead of revolving it. A turntable would
 * carry the smooth back of the shell round for most of its cycle, which is the
 * one view of a clam that says nothing.
 *
 * That lean has a consequence worth stating plainly, because it is the trap: at
 * full lean the camera looks straight down the prize's local −Z, so any surface
 * lying in the local XZ plane — anything "horizontal" in prize-local space — is
 * seen EDGE-ON and disappears. A first pass built the clam the way one sits on a
 * seabed: a level lower valve with an upper valve raised off it. The upper valve
 * read fine and the lower one collapsed into a sliver of rib-ends under the
 * pearl.
 *
 * So the hinge is authored at the BACK on the mouth's axis and the two valves
 * straddle that axis symmetrically — each tipped `GAPE / 2` (43°) off local +Z,
 * one up and one down. The mouth then opens directly at the lens, and both
 * valves are seen at the same 43° off face-on: foreshortened, but each showing a
 * real fan rather than an edge. It is also the pose that makes the LIGHT
 * symmetric — see the palette note below.
 *
 * ── why each valve is a fan of ribs ────────────────────────────────────────
 * A clam's radial ribs are its signature, and the engine's mesh vocabulary is
 * box / sphere / cylinder — there is no fan, sector, or shell primitive. The
 * beach already answers this: `clamShell` in scene.ts draws its shore litter as
 * a short splay of thin box ribs about a common hinge. This is the same creature
 * at ten times the size, so it is the same construction with more ribs (nine a
 * valve rather than four), which is what turns a splay into a scallop.
 *
 * Each rib is also ROLLED about its own long axis in proportion to how far out
 * the fan it sits, so the fan bows into a dish instead of lying flat. That roll
 * is the difference between a shell and a paper fan, and it is the same
 * argument `lidArc` makes for the chest dome: a curve the primitives cannot
 * express becomes an honest arc of facets, each meeting the key at its own
 * angle, so the form reads from shading as well as from silhouette.
 */

import type { EngineQuat, EngineVec3, MaterialSpec, SceneInstance } from "@axiom/web-engine";
import { addV3, normalizeV3, quatMul, quatPitch, quatRoll, quatYaw, rotateByQuat, scaleV3 } from "../../../presentation/stage/vectors.ts";
import type { Prize, PrizeFrame, PrizePlace } from "./prize.ts";
import { solid, sparkleAt, v3 } from "./prize.ts";

// ── the palette ────────────────────────────────────────────────────────────
/*
 * Authored against the reveal rig, which is the only light this object is ever
 * seen under: ambient 0.19, a directional key of 1.15 arriving from
 * (0.62, 0.60, 0.51), and a warm point lamp of 0.62 sitting just off that same
 * direction — roughly 1.4 summed onto a surface that faces it squarely, and
 * more than that onto the one rib whose roll happens to aim straight at it. The
 * backend composites `tonemap(diffuse · albedo + specular + emissive)` with the
 * curve's knee at 0.9, so an albedo anywhere near 1.0 multiplies past the knee,
 * two channels clip together, and the hue is gone by construction. The prize
 * this catalog replaced measured (250, 253, 252) on screen: a formless white
 * blob with no shape in it at all. Nothing here goes above 0.63.
 *
 * With no textures in this engine, the value STEP between rungs is the only
 * thing carving the form, and it is spent on two jobs at once:
 *
 *   * ALTERNATING rungs around each fan, so two neighbouring ribs never share a
 *     value and the fan reads as separate ribs even in the frames where the key
 *     falls on them all alike. The alternation is pinned to the rib index — to
 *     the geometry — never to an angle, and the ribs it lands on are genuinely
 *     corrugated (see `RIB_RELIEF`), so it reinforces a real surface instead of
 *     painting stripes onto a smooth one.
 *   * A step BETWEEN the valves. The cradle takes the top two rungs and the
 *     raised valve the bottom two, so the raised valve recedes into being the
 *     backdrop the pearl is read against, and the cradle stays the lit stage the
 *     pearl sits on. The symmetric gape lights the two valves within 10% of each
 *     other, so nothing in the rig separates them for us; this does.
 */
const CLAM_MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  /** The lit rung: a proud rib on the cradle. Warm pale shell, seated low
   * enough that even the rib whose roll aims at the key only just reaches the
   * knee — that rib is the brightest flat plate in the assembly. */
  PrizeClamRidge: solid([0.63, 0.575, 0.5, 1]),
  /** The mid rung, shared by the cradle's recessed ribs and the raised valve's
   * proud ones — the hinge of the whole ladder, which is why it is one material
   * and not two that would drift apart. */
  PrizeClamShell: solid([0.44, 0.4, 0.345, 1]),
  /** The deep rung: a recessed rib on the raised valve, and the shell's own
   * shaded reading of a surface turned away from the mouth. */
  PrizeClamShellDeep: solid([0.24, 0.215, 0.185, 1]),
  /** Not a rung at all — a hole. The gloom filling the throat behind and under
   * the pearl, so the pearl sits in a dark mouth instead of floating against a
   * pale shell with the background showing through the gape. */
  PrizeClamGloom: solid([0.1, 0.09, 0.085, 1]),
  /**
   * The pearl: WHITE, and SMOOTH. Smooth is the whole distinction from the
   * diamond in `wedding-ring.ts` — that stone is faceted and value-stepped
   * across its facets, because a cut stone's read IS its facets. A pearl has
   * none, so its only modelling is the falloff across one sphere.
   *
   * ── why the albedo is BLUE ──────────────────────────────────────────────
   * Because a pearl that is white ON SCREEN cannot be authored white. This
   * albedo is solved backwards from the render, the same way `LagoonWater` in
   * `scene.ts` is, and the arithmetic is worth keeping because it is not
   * guessable by eye.
   *
   * The reveal rig is WARM: a warm key (1, 0.96, 0.88) at 1.15 plus a warm lamp
   * (1, 0.82, 0.45) at 0.62, over a warm ambient. Summed on the pearl's crown
   * that light carries the ratio G = 0.909·R, B = 0.738·R. Multiply ANY neutral
   * albedo by that and you get a tan object — a plain 0.72 grey measures
   * (206, 187, 153) at mid-tone. The first pass of this pearl was authored warm
   * on top of that (0.58, 0.555, 0.505) and measured (245, 235, 176) at the
   * crown, against the shell's lit rung at (246, 231, 163): the same colour, to
   * within a couple of levels, which is why the pearl and the oyster read as one
   * tan mass.
   *
   * So the albedo is the INVERSE of the light's ratio — cool in exactly the
   * proportion the rig is warm — which lands all three channels together and the
   * pearl renders neutral: (245, 245, 245) at the crown, (178, 179, 183) at
   * mid-tone, (44, 44, 49) at the terminator. White, with a full sphere of
   * falloff still in it.
   *
   * The top channel sits at 0.78 and cannot go higher: blue is the largest
   * component here, so pushing for more brightness clips BLUE first and swings
   * the pearl from white to icy — which would then read as the ring's diamond.
   * A whiter pearl than this needs a cooler rig, not a brighter material.
   *
   * The emissive is small, cool-biased to match, and there for one thing: it
   * lifts the shadowed underside sunk into the cradle, which no light in this
   * rig reaches, so the pearl reads as softly luminous rather than as a ball
   * with a bite out of it. It is ADDED after the albedo multiply, so it lifts the
   * sphere uniformly and cannot do the shading's job — the falloff still does.
   */
  PrizePearl: { baseColor: [0.575, 0.633, 0.78, 1], emissive: [0.06, 0.066, 0.08, 1] },
  /** A gleam on the pearl. A light, not a surface — a black albedo carrying the
   * emissive — so it is the same brightness wherever it lands and cannot be
   * modulated by the sphere under it. Cooler and paler than the gold catalog's
   * `PrizeSparkle`, because a pearl's gleam is a sheen and not a spark. */
  // Cool-biased like the pearl it sits on. An emissive is NOT multiplied by the
  // rig, so this one does not need the inverse-warm solve above — but a gleam
  // authored warmer than the surface under it would pull the highlight back
  // toward the tan the pearl just escaped.
  PrizePearlGlint: { baseColor: [0, 0, 0, 1], emissive: [0.88, 0.91, 0.95, 1] },
};

// ── the shell ──────────────────────────────────────────────────────────────

/** The hinge: on the mouth's axis, behind the origin, so the two valves splay
 * forward around local +Z and the assembly balances about the origin rather
 * than hanging off the back of the unit box. */
const HINGE: EngineVec3 = v3(0, 0, -0.52);

/** How far the shell reaches from the hinge along the centre of a fan. */
const SHELL_REACH = 1.28;

/**
 * The gape: the full angle between the two valves, 86°. Wide enough that no
 * frame of the breathe below can read as a shell merely ajar, and — since the
 * valves straddle the view axis — wide enough that both are seen well off
 * edge-on. It is also what has to clear the pearl: the valves' planes both pass
 * through the hinge, so the gape and the pearl's radius together decide how far
 * out along the mouth the pearl can possibly sit (see `PEARL_OUT`).
 */
const GAPE = 1.5;
/** The breathe: the raised valve opening a touch wider and back. Only the
 * RAISED valve moves, which is both how a clam resting on its lower valve
 * actually opens and what keeps the pearl's seat still — a cradle that rocked
 * would carry the pearl with it, and the subject of the shot should not drift. */
const GAPE_BREATHE = 0.075;
/** ~146 ticks a cycle: slow enough to read as breathing rather than chewing. */
const GAPE_RATE = 0.043;

/** Ribs per valve. Below ~7 the fan reads as the beach's shore-litter splay
 * rather than a scallop; above ~11 the ribs are narrower than the grooves
 * between them and the fan smooths back into a lozenge. Odd, so one rib runs
 * down the shell's midline and the fan's outline bulges forward at the centre,
 * the way a clam's does. */
const RIBS = 9;
/** How far a fan splays, end rib to end rib — 95°, a little wider than the
 * beach clam's, which is what the extra ribs buy. */
const RIB_SPREAD = 1.66;
/** How much shorter an END rib is than the centre one, as a fraction. This is
 * the only taper available: a box's scale is constant along its length, so the
 * fan's rounded outline has to come from the ribs' relative LENGTHS. */
const RIB_TAPER = 0.17;
/**
 * A rib in section, wide enough that adjacent ribs still OVERLAP out at the rim.
 *
 * This was 0.185 — narrower than the 0.22–0.27 gap between rib centres at that
 * radius — on the reasoning that the gaps would read as scalloping. They did
 * not: at the size a prize occupies, the gaps let the brightly lit orange chest
 * show straight through the shell, and the valve read as a set of slats rather
 * than as one surface. A shell is solid; its ribs are CORRUGATIONS on that
 * surface, not holes in it.
 *
 * So the fan is closed and the scalloped read is carried entirely by
 * `RIB_RELIEF`, which steps alternate ribs proud of their neighbours — a real
 * value break on a continuous surface, which is what a rib actually is. The
 * overlap is the same trick the coin's smile arc used before it was cut and the
 * chest's lid slats still use: a box has square ends, so consecutive facets have
 * to bite into each other or the wedge between them shows as a nick.
 */
const RIB_WIDTH = 0.29;
const RIB_THICK = 0.1;
/** Roll per radian of splay. The end ribs come out rolled ~30°, so the fan
 * turns through ~59° across its width: a domed dish, not a flat fan. */
const RIB_DOME = 0.62;
/** How far each valve's ribs stand off the hinge plane toward its own convex
 * side. Both fans emanate from the same hinge, so without this they interleave
 * where they meet; with it they open a gap the umbo fills. */
const SHELL_LIFT = 0.045;
/** The corrugation: every other rib sits this much further toward the convex
 * side, so the surface the camera sees genuinely steps in and out. The value
 * alternation rides the same parity, which is what stops the ribbing from being
 * paint. */
const RIB_RELIEF = 0.055;

/**
 * One valve: a fan of ribs hinged at `HINGE` and rotated open about it.
 *
 * `axisPitch` tips the whole fan about the hinge axis — that single rotation IS
 * the valve opening, so the gape can move per frame without any of the fan's
 * own geometry knowing about it. The splay is applied INSIDE that pitch (yaw
 * first, then the valve's tilt), so the ribs always splay across the valve's
 * own plane; splaying about the prize's vertical instead would wrap the fan
 * around a cone as soon as the valve was steep, which is the bug a fan hinged
 * flat on the sand never has to notice.
 *
 * `domeSign` is which way the dish bows: +1 for a valve concave toward local
 * +Y, −1 for its mirror. A bivalve's two halves are reflections of each other
 * across the hinge plane, not copies of each other rotated — so this sign, not
 * a second `axisPitch`, is what makes both concavities face into the mouth.
 */
const valve = (
  place: PrizePlace,
  prefix: string,
  axisPitch: number,
  domeSign: number,
  ladder: readonly string[],
): readonly SceneInstance[] =>
  Array.from({ length: RIBS }, (_, i): SceneInstance => {
    const spread = (i / (RIBS - 1) - 0.5) * RIB_SPREAD;
    const q: EngineQuat = quatMul(quatPitch(axisPitch), quatMul(quatYaw(spread), quatRoll(spread * RIB_DOME * domeSign)));
    const length = SHELL_REACH * (1 - (RIB_TAPER * Math.abs(spread)) / (RIB_SPREAD / 2));
    // A rib runs from the hinge along its own +Z, so its centre is half its
    // length out — and `seat` slides it onto the valve's convex side by the
    // valve separation plus, on every other rib, the corrugation step.
    const seat = -(SHELL_LIFT + RIB_RELIEF * (i % 2)) * domeSign;
    return place(
      `${prefix}${i}`,
      "box",
      ladder[i % ladder.length],
      addV3(HINGE, rotateByQuat(v3(0, seat, length / 2), q)),
      v3(RIB_WIDTH, RIB_THICK, length),
      q,
    );
  });

// ── the pearl ──────────────────────────────────────────────────────────────
/*
 * A plain sphere, and that is not a shortcut: a pearl is smooth, so the sphere
 * is the honest primitive here in a way the diamond's tipped box and kite
 * facets never could be. Faceting it would make it a bead.
 */

/** Big — over half the shell's width. It is the subject, and a pearl a shade
 * too large for the shell holding it is the whole charm of the object. It
 * genuinely noses a little past the valves' lips, which reads as a mouth full
 * rather than as a clipped sphere. */
const PEARL_RADIUS = 0.38;
/** How far the pearl settles below the mouth's axis. Small, and load-bearing:
 * the two valves are symmetric about that axis, so a pearl centred ON it would
 * be equally close to both, and this is what makes it rest in the cradle and
 * keep clear of the valve arching over it. */
const PEARL_SINK = 0.1;

/** The cradle's inner surface, as a plane through the hinge: the direction it
 * faces, and how far the proud ribs' faces stand off along it. */
const CRADLE_NORMAL: EngineVec3 = rotateByQuat(v3(0, 1, 0), quatPitch(GAPE / 2));
const CRADLE_FACE = RIB_THICK / 2 - SHELL_LIFT;

/**
 * How far out along the mouth the pearl has to sit to rest exactly ON that
 * surface — SOLVED from the cradle's own geometry rather than eyeballed, so a
 * wider gape, a thicker rib, or a bigger pearl re-seats it instead of leaving
 * it hanging in the air or sunk through the shell.
 */
const PEARL_OUT = (CRADLE_FACE + PEARL_RADIUS + PEARL_SINK * CRADLE_NORMAL.y) / CRADLE_NORMAL.z;
const PEARL_AT: EngineVec3 = addV3(HINGE, v3(0, -PEARL_SINK, PEARL_OUT));

/** A gleam's diameter at full envelope. Each one is scaled by its OWN envelope,
 * so one at rest scales to nothing and vanishes rather than sitting on the
 * pearl as a permanent dot. */
const GLEAM_SIZE = 0.12;

/** A point just clear of the pearl's surface in a given direction. */
const gleamAt = (direction: EngineVec3): EngineVec3 => addV3(PEARL_AT, scaleV3(normalizeV3(direction), PEARL_RADIUS + 0.01));

/**
 * Two gleams, and two is the count. The first sits where a pearl's highlight
 * actually is — up and to the front-right, on the bisector of the view and the
 * reveal key — and the second is the soft counter-sheen low on the other side.
 * A third would overlap the others often enough to read as a steady glow, which
 * is the opposite of a sheen catching a curve.
 */
const GLEAMS: readonly EngineVec3[] = [gleamAt(v3(0.34, 0.24, 0.91)), gleamAt(v3(-0.3, -0.14, 0.94))];

// ── the assembly ───────────────────────────────────────────────────────────

/** How wide the shell stands open this tick. Gated on `settle`, so the shell
 * holds its gape while it is still climbing out of the chest and only starts
 * breathing once anyone can see it. Pure in the tick, like every cosmetic here. */
const gapeAt = (tick: number, settle: number): number => GAPE + GAPE_BREATHE * settle * Math.sin(tick * GAPE_RATE);

const build = (place: PrizePlace, frame: PrizeFrame): readonly SceneInstance[] => {
  // The cradle is fixed at half the gape below the mouth's axis; the raised
  // valve takes up whatever the breathe has opened above it.
  const cradle = valve(place, "cradle", GAPE / 2, 1, ["PrizeClamRidge", "PrizeClamShell"]);
  const raised = valve(place, "raised", GAPE / 2 - gapeAt(frame.tick, frame.settle), -1, ["PrizeClamShell", "PrizeClamShellDeep"]);

  // The umbo: the knob of shell at the hinge. Nine ribs a side converge here
  // and overlap, and a solid block is what turns that convergence into a hinge
  // a viewer can name rather than a tangle where the two fans meet.
  const umbo = place("umbo", "box", "PrizeClamShell", v3(0, 0, HINGE.z + 0.08), v3(0.52, 0.34, 0.26));

  // The throat. Without it the gape is a hole in the middle of the frame with
  // the veiled stage showing through it, and the pearl reads as pasted on. A
  // squashed ellipsoid parked behind and just under the pearl closes the mouth
  // and gives the pearl the dark it needs to sit IN something.
  const gloom = place("gloom", "sphere", "PrizeClamGloom", v3(0, -0.03, HINGE.z + 0.3), v3(0.78, 0.46, 0.44));

  const pearl = place("pearl", "sphere", "PrizePearl", PEARL_AT, v3(PEARL_RADIUS * 2, PEARL_RADIUS * 2, PEARL_RADIUS * 2));

  // `sparkleAt` is pure in (index, tick) and already folds in `settle`, so
  // nothing twinkles during the climb, and a gleam whose envelope is at rest
  // scales to zero rather than lingering.
  const gleams = GLEAMS.map((at, i): SceneInstance => {
    const flash = GLEAM_SIZE * sparkleAt(i, frame.tick, frame.settle);
    return place(`gleam${i}`, "sphere", "PrizePearlGlint", at, v3(flash, flash, flash));
  });

  return [umbo, ...raised, gloom, pearl, ...cradle, ...gleams];
};

/**
 * The treasure.
 *
 * `extent` is measured, not budgeted. The furthest point is the tip corner of
 * the RAISED valve's centre rib at the top of the breathe — 1.005 up and 0.279
 * forward of the origin, ~1.05 out — with the cradle's matching corner next at
 * ~1.01. Declared a hair above that.
 *
 * Modestly past the ±1 unit box, and the right answer here: what reaches out
 * there is a thin rib at the rim of a fan, while the shell's readable mass (a
 * mouth ~1.7 across and ~1.9 tall, with an 0.76 pearl in it) sits inside the
 * box. Sizing the assembly so the rib tips fit instead would shrink everything
 * a player actually looks at.
 *
 * `presentation` and `lean` are the two declarations that matter most: the
 * subject is what is INSIDE the mouth, so the clam holds that opening at the
 * lens and rocks rather than revolving. Nothing in this file adds a lean of its
 * own — the staging owns the pose, and a second one here would fight it.
 */
export const PEARL_CLAM: Prize = {
  build,
  extent: 1.06,
  lean: 1,
  materials: CLAM_MATERIALS,
  presentation: "faces-camera",
};
