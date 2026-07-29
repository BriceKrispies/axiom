/*
 * crab-bride.ts — the crab's girlfriend, hoisted out of a chest as the treasure.
 *
 * The joke only lands if she is LITERALLY the creature scuttling about on the
 * sand, so this file models nothing. `crab.ts` owns the one body and both call
 * sites pose it; the beach maps it onto the sand, and this maps it into the
 * prize's unit box. What is left here is a frame adapter, a pose, and a bowtie —
 * which means the next agent who changes the crab changes his girlfriend too,
 * and the two can never quietly drift into being different animals.
 *
 * The only thing she wears that he does not is the pink bowtie (already built,
 * already off-centre, inside `crab.ts`); the brand pennant is his, and she
 * carries none — see the `dress` argument at the bottom of this file.
 */

import type { SceneInstance } from "@axiom/web-engine";
import type { CrabPlace } from "../crab.ts";
import { CRAB_AT_REST, crabParts } from "../crab.ts";
import type { CrabPose } from "../game.ts";
import type { Prize, PrizeFrame, PrizePlace } from "./prize.ts";
import { v3 } from "./prize.ts";

/*
 * ── crab-local → prize-local ───────────────────────────────────────────────
 * The two spaces agree on their axes (+Y up, +Z toward the camera) and disagree
 * about everything else: the crab stands ON her origin, between her feet, and is
 * a small creature (~0.62 tall, shell ~0.62 across, claw tip to claw tip ~1.22);
 * a prize is authored AROUND its origin. So the adapter does exactly two things:
 * drop her onto the origin, then scale her up to the size a treasure reads at.
 *
 * `CRAB_SCALE` is derived from her SHELL, and deliberately not from her claws.
 * Scaling so the claws span the ±1 box (1.6) is exactly how the first pass
 * failed: a claw tip is a thin horizontal spike, so it swallowed the entire size
 * budget while the part of her anyone can actually read — shell, eyes, bowtie —
 * stayed a smudge roughly half the width of the gold coin's disc in the same
 * pool of light. Her shell is 0.62 across, so 2.25 puts it at 1.4: precisely the
 * coin's diameter, the one treasure in this catalog already proven to sit right
 * in this framing. Her claws are then simply what pokes past the box, which is
 * what a claw is for.
 *
 * `CRAB_MID_Y` is her waistline — feet at 0, eyestalk tips at ~0.62, middle at
 * ~0.31 — pulled slightly above that because the presentation leans her forward
 * into the camera (see `lean` at the bottom of this file), and that lean swings
 * her forward half, the claws she reaches with and the feet she stands on, DOWN
 * the screen. Her geometric middle is therefore not her screen middle, and
 * centring on the geometric one hangs her low in the glow pooled beneath her.
 */
const CRAB_MID_Y = 0.27;
const CRAB_SCALE = 2.25;

/**
 * The crab's `place` in terms of the prize's. Note the argument order flips —
 * `CrabPlace` names the material before the mesh and `PrizePlace` the other way
 * round — so this is a real adapter, not a rename.
 *
 * It adds no rotation of its own, and that is a decision rather than an
 * omission. Her +Z is already her front, and the presentation already turns her:
 * `prizeSpin` leans her into the camera's downward look and then revolves her on
 * a turntable, so a fixed yaw here could only phase-shift a rotation that visits
 * every angle anyway — it would buy a three-quarter view for one instant and
 * cost one somewhere else. What actually separates her claws from her body
 * outline is the POSE below, which holds them up and out at every angle the
 * turntable brings round.
 */
const bridePlace = (place: PrizePlace): CrabPlace =>
  (key, material, mesh, local, scale, localRot): SceneInstance =>
    place(
      key,
      mesh,
      material,
      v3(local.x * CRAB_SCALE, (local.y - CRAB_MID_Y) * CRAB_SCALE, local.z * CRAB_SCALE),
      v3(scale.x * CRAB_SCALE, scale.y * CRAB_SCALE, scale.z * CRAB_SCALE),
      localRot,
    );

/**
 * How she holds herself while she is being presented. She is HOVERING in a
 * light, not scuttling: the beach's business (side scoots, turns, hops) would
 * read as a crab trying to walk away in mid-air, and the reveal already owns her
 * bob and her turn. So she starts from `CRAB_AT_REST` and gets back only the
 * life a held creature still has — a breathe, an eyestalk drift, legs paddling
 * at nothing, and a wave.
 *
 * Her claws are her SILHOUETTE. She is one coral tone at one depth all over, so
 * claws tucked down against the shell dissolve into it and she reads as a blob
 * with two eyes on top; held up and out they break her outline and she reads as
 * a crab. That is why the lift has a standing baseline that never returns to
 * zero — it is a stance, not a flourish, and it therefore also applies while she
 * is still on her way up out of the chest, which is the half of the shot where
 * she most needs to be recognisable.
 *
 * The WAVE — the moving part on top of that stance — is what is gated on
 * `settle`: a crab waving before anyone can see her is a wave spent on nothing.
 * Every term is pure in `tick`, and `crabParts` alternates the two sides on its
 * own phase, so this only says HOW MUCH.
 */
const bridePose = (tick: number, settle: number): CrabPose => ({
  ...CRAB_AT_REST,
  breath: Math.sin(tick * 0.08) * 0.025,
  clawLift: 0.75 + 0.35 * settle * (0.5 + 0.5 * Math.sin(tick * 0.05)),
  eye: Math.sin(tick * 0.05) * 0.06,
  // Nothing under her feet to push against, so the legs paddle faintly — the
  // difference between floating and being a stiff prop hung in the air.
  legWiggle: 0.07,
});

/**
 * The bowtie is the entire joke — it is how a player knows this is the beach
 * crab and that she is somebody's girlfriend — so she wears it OVERSIZED. The
 * `bowtie` amount multiplies both the wing dimensions and the wings' offset from
 * the knot (see `crabBowtie`), so 1.5 gives a bow ~0.6 crab-local across: as
 * wide as her shell, and still off-centre and cocked, because that is authored
 * into the geometry rather than into the amount. At 1.0 it was a pink smudge a
 * couple of pixels tall on a creature this size. Past ~1.6 it grows wider than
 * the animal wearing it and stops reading as a ribbon at all.
 *
 * She wears it at full size from the first frame rather than ramping it in on
 * `settle`: it is who she is, not a flourish, and a bow inflating on her head
 * after she arrives would read as an accessory being equipped.
 */
const build = (place: PrizePlace, frame: PrizeFrame): readonly SceneInstance[] =>
  crabParts(bridePlace(place), bridePose(frame.tick, frame.settle), frame.tick, { bowtie: 1.5, pennant: false });

/**
 * `materials` is empty on purpose. Her palette is `CRAB_MATERIALS`, which lives
 * beside the body in `crab.ts` and is already registered by the chest scene —
 * a prize-local copy of the shell color is exactly how the two crabs would end
 * up different shades of coral.
 *
 * `extent` is measured, not budgeted: her furthest point is a CLAW TIP at the
 * top of a wave — 0.5 out, ~0.51 up and 0.42 forward of her centred origin, plus
 * the claw's own radius, so ~0.79 in crab-local units, which `CRAB_SCALE`
 * carries to ~1.78. Declared a hair above that.
 *
 * That is knowingly past the ±1 the unit box suggests, and it is the right
 * answer for this animal rather than a budget overrun: the box is where her
 * READABLE mass sits (shell 1.4 across, the coin's diameter), and her claws are
 * a thin pair of spikes reaching out of it. Sizing her so the spikes fit instead
 * shrinks everything a player actually looks at by a third.
 */
// She FACES the camera, and does not revolve.
//
// A turntable was the obvious call for a creature and it was wrong, for the same
// reason it is wrong for the coin: a crab is only a crab from the front. Her
// whole read — two eyestalks, two raised claws, and the pink bow — lives on one
// face, and a slow revolution spent two thirds of its cycle showing the player a
// smooth orange shell from behind, which is a blob with legs.
//
// Facing camera leans her fully into the lens (a crab is a WIDE, LOW animal, so
// a camera 50° above one otherwise sees shell and no face at all) and swaps the
// revolution for a gentle rock — which on a crab reads as her shifting her
// weight from side to side, and is far more alive than a turntable ever was.
export const CRAB_BRIDE: Prize = { build, extent: 1.8, lean: 1, materials: {}, presentation: "faces-camera" };
