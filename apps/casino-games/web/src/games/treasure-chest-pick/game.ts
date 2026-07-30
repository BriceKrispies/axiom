/*
 * game.ts — the Treasure Chest Pick controller: the mount spec's mechanic,
 * per-tick step, and reveal timeline. Nine carved-wood chests in a 3×3 grid;
 * the choice-population adapter preassigns which chests hold prizes before
 * the player can possibly choose; the reveal follows the classic cadence —
 * focus, anticipation brace, LATCH FALLS FIRST, pause, lid pops with
 * overshoot, warm light, reward (or honest empty interior).
 *
 * Idle "chest dances" draw exclusively from the AMBIENT stream keyed by tick
 * window and grid slot — never from the population — so no wobble can hint
 * at contents. The dance test pins this.
 */

import type { InputFrame, TickContext, ToneSpec } from "@axiom/web-engine";
import type { Camera3D, EngineVec3 } from "@axiom/web-engine";
import type { BrandSpec } from "../../presentation/branding/brand.ts";
import { sample01, sampleInt } from "../../chance-engine/randomness/streams.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge, transition } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { shimmerCue, thumpCue, tickCue } from "../../presentation/audio/cues.ts";
import { tabletopCamera } from "../../presentation/cameras/presets.ts";
import type { PickTarget } from "../../presentation/cameras/picking.ts";
import { canvasToGround, pickAt } from "../../presentation/cameras/picking.ts";
import { clamp01, smoothstep } from "../../presentation/stage/easing.ts";
import { addV3, crossV3, dotV3, normalizeV3, scaleV3, subV3, v3 } from "../../presentation/stage/vectors.ts";
import type { CasinoState } from "../round-state.ts";
import { speedTicks } from "../round-state.ts";
import type { ChoiceCore } from "../choice-input.ts";
import { initialChoice, stepChoice } from "../choice-input.ts";

export interface ChestSpec {
  /** Idle dance liveliness in [0, 1]. */
  readonly danceLiveliness: number;
  /** The white-label brand stamped across the scene (chest fronts, banners,
   * flags, signs, mat). Configurable name + color scheme; see `brand.ts`. */
  readonly brand: BrandSpec;
}

export interface ChestExtra {
  readonly choice: ChoiceCore;
  /** Tick at which the reveal began (session tick space), for cue edges. */
  readonly revealStartTick: number | null;
  /** Where the nine chests sit, and any chest currently being dragged. */
  readonly chests: ChestDrag;
  /** Where the three draggable beach props sit, and any current drag. */
  readonly decor: DecorDrag;
}

// ── draggable beach props ──────────────────────────────────────────────────────
// The palm, castle, and crab are pieces the player can pick up and move. Their
// positions therefore live in game STATE (not hardcoded in the view), driven by
// pointer input through the pure fold below — so a drag is as deterministic and
// replayable as any other input, and the view is a pure function of where the
// props ended up.

/** The three movable beach props and their world origins (base on the sand). */
export interface DecorProps {
  readonly palm: EngineVec3;
  readonly castle: EngineVec3;
  readonly crab: EngineVec3;
}

/** Drag state: where each prop sits, which one (if any) is currently held, the
 * grab offset (so a grabbed prop doesn't snap its centre to the cursor), and the
 * previous pointer-down state for press-edge detection. */
export interface DecorDrag {
  readonly props: DecorProps;
  readonly held: keyof DecorProps | null;
  readonly grabOffset: EngineVec3;
  readonly pointerDown: boolean;
}

export const DECOR_KEYS: readonly (keyof DecorProps)[] = ["palm", "castle", "crab"];

/** The props' home positions — where the beach was authored. */
export const DEFAULT_DECOR: DecorDrag = {
  grabOffset: v3(0, 0, 0),
  held: null,
  pointerDown: false,
  props: { castle: v3(5.0, 0, -3.3), crab: v3(-5.4, 0, 1.0), palm: v3(-5.3, 0, -2.8) },
};

/** Per-prop grab anchor height (up its visible mass) and screen pick radius —
 * these are big friendly objects, so the radii are generous. */
const DECOR_PICK: Readonly<Record<keyof DecorProps, { readonly h: number; readonly r: number }>> = {
  castle: { h: 1.0, r: 95 },
  crab: { h: 0.3, r: 70 },
  palm: { h: 1.7, r: 90 },
};

/** Screen hit-targets for the three props, anchored a little up their mass so a
 * click on the visible prop grabs it. */
export const decorTargets = (props: DecorProps): readonly PickTarget[] =>
  DECOR_KEYS.map((key, index) => ({ at: addV3(props[key], v3(0, DECOR_PICK[key].h, 0)), index, radiusPx: DECOR_PICK[key].r }));

/** The result of one drag tick: the new drag state, and whether the drag OWNS
 * the pointer this tick (so chest-picking is suppressed while placing a prop). */
export interface DecorStep {
  readonly decor: DecorDrag;
  readonly active: boolean;
}

/**
 * One tick of the pick-up-and-move interaction, pure in (decor, input, camera):
 * - holding a prop → it follows the cursor's ground point (offset preserved),
 *   and releasing (or losing the cursor) drops it;
 * - otherwise, a fresh press whose cursor is over a prop grabs the nearest one.
 * Props sit on the sand away from the chests, so this only competes with a chest
 * pick when the cursor is actually over a prop.
 */
export const stepDecorDrag = (decor: DecorDrag, input: InputFrame, camera: Camera3D): DecorStep => {
  const pointer = input.pointer;
  const down = pointer?.down ?? false;
  const ground = canvasToGround(camera, pointer);
  const wasDown = decor.pointerDown;

  if (decor.held !== null) {
    if (!down || ground === null) {
      return { active: true, decor: { ...decor, held: null, pointerDown: down } };
    }
    const to = addV3(ground, decor.grabOffset);
    return { active: true, decor: { ...decor, pointerDown: down, props: { ...decor.props, [decor.held]: v3(to.x, 0, to.z) } } };
  }

  const freshPress = down && !wasDown && ground !== null;
  const hit = freshPress ? pickAt(camera, decorTargets(decor.props), pointer) : null;
  if (hit !== null && ground !== null) {
    const key = DECOR_KEYS[hit] as keyof DecorProps;
    return { active: true, decor: { ...decor, grabOffset: subV3(decor.props[key], ground), held: key, pointerDown: down } };
  }
  return { active: false, decor: { ...decor, pointerDown: down } };
};

export type ChestState = CasinoState<ChestExtra>;

export const CHEST_COLUMNS = 3;
export const CHEST_SPACING = 2.05;

// ── chest proportions ─────────────────────────────────────────────────────────
// The chest's physical facts live beside its layout and timing, so the framing
// math below can size the hero shot from the real object rather than a copy of
// its numbers. `scene.ts` builds its geometry from exactly these.

/*
 * The chest is a shorter body under a barrel-topped lid — the proportion that
 * actually reads as a treasure chest, rather than a tall box with a flat plate.
 *
 * The three vertical parts are chosen to sum to the SAME closed height the
 * flat-lidded chest had (0.86). That is deliberate, not coincidence: the hero
 * camera, its fill/drop/width budgets, the veil depth, and the prize climb are
 * all tuned against this envelope, and the fit runs snug — the hero shot has
 * only a few percent of headroom over its "is it really a close-up" floor. So
 * the dome is carved OUT of the existing silhouette instead of stacked on top
 * of it, and every framing invariant holds untouched.
 */
export const CHEST_BODY = v3(1.3, 0.44, 0.92);
/** The lid's flat BOARD — the plate the barrel top is built on, and the face a
 * player sees on the underside once the lid swings open. Deliberately thin: the
 * arch above it, not this board, is the lid's visual bulk. */
export const CHEST_LID = v3(1.34, 0.1, 0.96);
/** How far the barrel-topped lid rises above that board. */
export const CHEST_LID_ARCH = 0.32;
export const CHEST_LATCH = v3(0.2, 0.18, 0.05);
/** Y of the chest's closed mouth (where a lid-open prize emerges). */
export const CHEST_BODY_TOP = 0.46;
/** Overall closed height and width — what a framing must actually fit. */
export const CHEST_HEIGHT = CHEST_BODY.y + CHEST_LID.y + CHEST_LID_ARCH;
export const CHEST_WIDTH = CHEST_LID.x;

/** Grid slot world position (3 columns, rows recede in −Z). */
export const chestPosition = (index: number, count: number): EngineVec3 => {
  const columns = CHEST_COLUMNS;
  const rows = Math.ceil(count / columns);
  const col = index % columns;
  const row = Math.floor(index / columns);
  return v3((col - (columns - 1) / 2) * CHEST_SPACING, 0, (row - (rows - 1) / 2) * CHEST_SPACING * 0.92);
};

/*
 * How much LONGER the lens is than the tabletop preset's. The reference is shot
 * on a long lens; the tabletop preset is a wide one, and that mismatch — not the
 * seat — is the largest remaining framing error. Two scale-invariant quantities
 * pin it. Both are ratios of on-screen sizes between the grid's BACK row and its
 * FRONT row, so neither depends on image size, zoom, or where the rows sit in
 * frame, and both were measured the same way on reference.png and champion.png:
 *
 *                                          reference   champion
 *   column spacing, front row / back row      1.130       1.276
 *   row bounding width,      back / front     0.882       0.775
 *
 * They agree: the champion's near row is roughly twice as much bigger than its
 * far row as the reference's is — 13% divergence against 28%. That is a lens
 * fact, not a pose fact. Everything else is already at parity (lagoon width
 * 0.687 vs 0.700 of frame width, its far rim at 0.220 vs 0.226 of frame height,
 * mid-row grid width 0.143 vs 0.148), so nothing about the seat is wrong except
 * how close the camera stands. Solving the perspective model
 * `divergence = rowSpacing·cos(pitch)/distance` against each measurement gives
 * 1.98x and 2.02x; 2 is the shared answer.
 *
 * So the camera DOLLIES BACK to twice the distance along the same view ray and
 * takes half the lens angle. That holds `tan(fovY/2)·distance` — the projected
 * size at the board's center plane — exactly constant, so the pitch, the target,
 * the board's on-screen size, and the lagoon framing are all untouched. What
 * changes is only how the three rows stack: the far row grows ~6%, the near row
 * shrinks ~6%, and the predicted row widths then land on the reference's at BOTH
 * depths (back 0.365 vs 0.367, front 0.412 vs 0.416 of frame width). The lagoon
 * disc stops bulging toward the bottom of frame for the same reason.
 *
 * `heroFraming` sizes the hero close-up as a FRACTION of the frustum at a fixed
 * heroDistance, so it re-solves against the new lens and the close-up's
 * on-screen size is unchanged; only its world scale (and so the length of the
 * spiral flight) follows the lens.
 */
const LENS_PULL = 2;

// A tighter span than a card-table default: the reference frames the chest grid
// large — it claims ~55% of the frame width, and the sandy lagoon fills the top
// of the frame with no horizon showing. At the looser span the grid projected at
// ~36% and the camera's top edge cleared the floor rim to expose the pastel
// backdrop sheet as an intruding sky band. Pulling the span in (~0.66x) both
// scales the grid up to reference size and drops the top frame-edge ray onto the
// lagoon floor, cropping that backdrop band out of frame. The pitch angle is
// preset-fixed and unchanged; only the zoom tightens. The hero-flight close-up is
// derived from a fixed heroDistance + fovY, so its on-screen scale is untouched.
export const chestCamera = (count: number): ReturnType<typeof tabletopCamera> => {
  const span = 5.0 + Math.ceil(count / CHEST_COLUMNS) * 0.78;
  const center = v3(0, 0.42, -0.1);
  // Start from the shared tabletop framing (which the other card-table games
  // keep), then reseat the PITCH for THIS game only, measured off the reference
  // rather than judged by eye. Two scale-invariant quantities pin the reference
  // camera's elevation, and both are read straight off reference.png:
  //
  //   * the 3x3 chest grid's screen bbox is TALLER than it is wide — depth/width
  //     = 0.467/0.433 = 1.08. A ground-plane grid's screen depth/width is
  //     essentially sin(elevation), so 1.08 wants a high, plan-leaning camera.
  //   * the lagoon disc's far rim sits at 0.225 of frame height. A shallower
  //     camera pushes that rim DOWN the frame and opens a dead band of empty
  //     sand above the pool.
  //
  // The previous seat (span·0.95 / span·1.02, ~43° down) failed both: it
  // projected the grid at depth/width 0.93 — a full 14% too shallow, so the
  // three rows crowded together and the nine chests stopped reading as a grid —
  // and it put the lagoon rim at 0.264, with the sand band above it. This
  // reseats the camera at span·1.076 / span·0.887 (~50.5° down), which is the
  // joint least-squares fit over grid depth/width, grid top/bottom, grid width,
  // row-to-row perspective divergence, and lagoon rim + width: it cuts the
  // weighted framing error against the reference by ~4x. Only the angle moves
  // there; the on-screen size of the board is held by `LENS_PULL` above, which
  // scales distance and lens angle together. Pitching UP also strictly helps the
  // old backdrop worry below: a steeper look drops the top frame-edge ray onto
  // the lagoon floor even closer in, so the pastel sheet stays out of frame. The
  // hero close-up is derived from this camera via `heroFraming` off a fixed
  // heroDistance + fovY, so its on-screen scale is untouched and it re-centers.
  const base = tabletopCamera(center, span);
  return {
    ...base,
    fovY: 2 * Math.atan(Math.tan(base.fovY / 2) / LENS_PULL),
    position: v3(center.x, center.y + span * 1.076 * LENS_PULL, center.z + span * 0.887 * LENS_PULL),
  };
};

/** Screen hit-targets for chests sitting at `slots` — the LIVE layout, which a
 * drag can have moved away from the grid. */
export const chestTargetsAt = (slots: readonly EngineVec3[]): readonly PickTarget[] =>
  slots.map((at, index) => ({ at, index, radiusPx: 78 }));

/** Hit-targets for an untouched board. Kept as the `count`-shaped convenience
 * the resilient shell and the shared choice-input tests are written against;
 * the live game resolves picks through `chestTargetsAt` instead. */
export const chestTargets = (count: number): readonly PickTarget[] => chestTargetsAt(defaultChestSlots(count));

// ── draggable chests ──────────────────────────────────────────────────────────
/*
 * The nine chests can be picked up and rearranged, exactly as the beach props
 * can. So — like the props — their positions live in game STATE rather than
 * being a pure function of the grid slot, which is what makes a drag as
 * deterministic and replayable as any other input.
 *
 * A chest differs from a beach prop in one way that matters, and it is the whole
 * reason this is not simply another `stepDecorDrag`: a chest is ALSO the thing
 * you click to open. The props grab on the press EDGE, which is fine because
 * they sit out on the sand where a press can only mean one thing. Under that
 * rule a press on a chest would immediately start a drag and suppress the pick,
 * and the game would be left with no way to open a chest at all.
 *
 * So a press on a chest commits to nothing. It is remembered as a PENDING grab —
 * which chest, where on screen the press began, and the offset from the cursor's
 * ground point to the chest's base — and it only becomes a drag once the cursor
 * has travelled `DRAG_THRESHOLD_PX` from that point. Release before then and it
 * was a click, and the pick runs exactly as it always has. That is how every
 * desktop UI separates a click from a drag, and it leaves the existing
 * interaction untouched: a click still opens, a touch still arms-then-confirms,
 * and dragging is something you have to deliberately do.
 *
 * Positions PERSIST across a New Round / Replay (see `initialChestExtra`), the
 * same as the props: a board the player rearranged stays rearranged, and only a
 * page reload deals the grid again.
 */

/** How far the cursor must travel from the press point, in logical canvas
 * pixels, before a press on a chest becomes a drag instead of a click. Small
 * enough that a deliberate drag feels immediate, comfortably larger than the
 * jitter of a mouse click or the roll of a thumb on a tap. */
export const DRAG_THRESHOLD_PX = 9;

/** A press that has landed on a chest and may or may not become a drag. */
export interface ChestGrab {
  readonly index: number;
  /** Canvas-space point the press began at — what the threshold measures from. */
  readonly from: { readonly x: number; readonly y: number };
  /** Cursor-ground-point → chest-base offset, held so a grabbed chest does not
   * snap its centre to the cursor. */
  readonly offset: EngineVec3;
  /** True once the press has travelled past the threshold and committed to being
   * a drag. While false this is still only a candidate click. */
  readonly dragging: boolean;
}

export interface ChestDrag {
  /** Where each chest currently sits, indexed by its slot. */
  readonly slots: readonly EngineVec3[];
  readonly grab: ChestGrab | null;
  readonly pointerDown: boolean;
}

/** The untouched 3×N grid — where a fresh board deals its chests. */
export const defaultChestSlots = (count: number): readonly EngineVec3[] =>
  Array.from({ length: count }, (_, index) => chestPosition(index, count));

export const initialChestDrag = (count: number): ChestDrag => ({
  grab: null,
  pointerDown: false,
  slots: defaultChestSlots(count),
});

export interface ChestDragStep {
  readonly drag: ChestDrag;
  /** Whether the drag OWNS the pointer this tick. True only once a grab has
   * committed to dragging — a pending press deliberately does NOT own the
   * pointer, which is what lets it still land as a pick. */
  readonly active: boolean;
}

/**
 * One tick of chest dragging, pure in (drag, input, camera).
 *
 * Three states, in the order they are tested: the pointer is up (any grab ends),
 * a grab is live (measure the travel, and move the chest once it has committed),
 * or a fresh press just landed on a chest (remember it as pending).
 */
export const stepChestDrag = (drag: ChestDrag, input: InputFrame, camera: Camera3D): ChestDragStep => {
  const pointer = input.pointer;
  const down = pointer?.down ?? false;
  const ground = canvasToGround(camera, pointer);

  if (!down || pointer === undefined || ground === null) {
    // The grab ends. It still OWNS this tick if it had committed to dragging —
    // otherwise the release would be read as the click it stopped being.
    return { active: drag.grab?.dragging ?? false, drag: { ...drag, grab: null, pointerDown: down } };
  }

  const grab = drag.grab;
  if (grab !== null) {
    const travelled = Math.hypot(pointer.pos.x - grab.from.x, pointer.pos.y - grab.from.y);
    // Once committed it stays committed for the rest of the press, so a drag that
    // returns to where it started does not turn back into a click.
    const dragging = grab.dragging || travelled >= DRAG_THRESHOLD_PX;
    const to = addV3(ground, grab.offset);
    const slots = dragging ? drag.slots.map((at, i) => (i === grab.index ? v3(to.x, 0, to.z) : at)) : drag.slots;
    return { active: dragging, drag: { ...drag, grab: { ...grab, dragging }, pointerDown: down, slots } };
  }

  const hit = down && !drag.pointerDown ? pickAt(camera, chestTargetsAt(drag.slots), pointer) : null;
  const base = hit === null ? null : drag.slots[hit];
  return base === undefined || base === null || hit === null
    ? { active: false, drag: { ...drag, pointerDown: down } }
    : { active: false, drag: { ...drag, grab: { dragging: false, from: pointer.pos, index: hit, offset: subV3(base, ground) }, pointerDown: down } };
};

// ── presentation timing (ONE central config — no scattered magic numbers) ──────

/**
 * Every duration, easing magnitude, and staging constant of the chest's
 * presentation ritual, gathered here so the sequence is tuned in one place
 * rather than sprinkled through the view. Durations are in ticks (speed-scaled
 * where used); magnitudes are world-space unless noted. All of it is purely
 * cosmetic — nothing here can reach the outcome.
 */
export const CHEST_TIMING = {
  // Idle — a gentle, per-chest-desynced breathing.
  idleBobPeriod: 150, // ticks per idle bob cycle
  idleBobAmp: 0.014, // world-units of vertical idle bob
  idleTwistAmp: 0.035, // radians of idle sway
  // Selection staging — the chosen chest lifts, tilts, and the others recede.
  liftInTicks: 12, // ease-up time when a chest is committed
  lift: 0.17, // world-units the chosen chest rises (~10 px at this camera)
  heldLift: 0.34, // world-units a chest the player is DRAGGING rides up by, so it
  // reads as picked up and in hand rather than sliding across the water
  tilt: 0.15, // radians tilted toward the camera
  selectScale: 1.07, // slight enlarge of the chosen chest
  // The hero flight — the chosen chest spirals off the board and up into a
  // close, screen-filling framing before the lid is ever touched. The CAMERA
  // does not move for this: the chest comes to the camera, which keeps the
  // eight others (and the board) exactly where the player left them.
  // The crab fetches the chest. Before the spiral runs, the beach crab scuttles
  // from wherever he is standing to the front of the chosen chest and gets his
  // claws onto the lid rail; then he rides it into the close-up and opens it
  // himself. So the commit beat is now TWO beats — approach, then flight — and
  // `commitBeatTicks` is what the mount declares as its `commitPauseTicks`.
  //
  // The approach is deliberately the shorter of the two: it is a piece of
  // anticipation, and the player has already made their choice, so it must read
  // as "something is happening about my pick" rather than as a wait.
  approachTicks: 42,
  crabHopTicks: 20, // the little jump when the lid pops
  spiralTicks: 66, // the flight's own length; the whole spiral plays inside it
  spiralTurns: 2, // whole turns, so the chest lands facing front again
  spiralConverge: 3, // how sharply the orbit radius collapses (see spiralFlight)
  spiralApproach: 2, // how sharply the remaining DEPTH closes (see spiralFlight)
  spiralArc: 0.35, // extra mid-flight lift, so it arcs rather than slides
  spiralTumble: 0.2, // radians of pitch wobble, returning to 0 on arrival
  spiralGrowDelay: 0.3, // fraction of the flight before it starts enlarging
  spiralSpinFinish: 0.72, // fraction of the flight by which the turning is DONE
  spiralTurnEaseIn: 2, // how gently the turn starts (see spiralFlight)
  heroDistance: 4, // world units in front of the camera the chest settles at
  /*
   * The reveal is composed as a POSTER, and these three numbers are the whole
   * composition: the chest sits LOW and reads as the plinth, the treasure it
   * yields owns the air above it, and the result banner — DOM chrome pinned
   * across the lower third — lands on the chest's body rather than on the prize.
   *
   * That ordering is the correction. The champion sized the chest to claim 45%
   * of the frame height CLOSED, which meant its open lid ran from the top edge
   * of frame to the bottom, and the prize had nowhere to go but inside the
   * mouth — where the banner then covered it. Shrinking the chest and dropping
   * it is what buys the upper third back for the treasure; the prize's own
   * climb and size (`riseHeight`, `riseDamp`, `prizeDamp`) are tuned against
   * these, so the four move together.
   *
   * Solved rather than eyeballed, in fractions of frame height from the top:
   * chest bottom 0.94, open-lid apex 0.37, prize spanning 0.18–0.47 around a
   * centre at 0.32, and the widest treasure's apex (including the overshoot of
   * its ease) at 0.94 of the half-frame — inside the edge with margin to spare.
   */
  heroFill: 0.31, // fraction of frame HEIGHT the closed hero chest occupies
  heroDrop: 0.54, // how far below frame center it sits (fraction of half-height)
  heroWidthMargin: 0.86, // width guard: fraction of the frame it may ever span
  // The background veil that drops behind the hero chest.
  // Peak darkness of the veil (0 = none, 1 = black). Deliberately well short of
  // opaque: the veil exists to push the stage back, not to delete it, and the
  // beach should still be READABLE behind the hero chest — a palm, a sandcastle,
  // a crab, all sunk to near-black but still there. Two changes since this was
  // authored had both pushed it toward flat black without anyone lowering it:
  // the scene's ambient now eases down through the flight, and the board's focal
  // lamp fades out with it, so the very thing the veil is dimming got dimmer
  // underneath it too. This is the compensation.
  dimVeil: 0.76,
  dimSteps: 16, // quantization of the veil ramp (materials carry fixed opacity)
  veilGap: 1.3, // world units the veil sits BEHIND the hero chest — clear of the
  // hero chest's own depth, still nearer than the closest chest on the board
  // The reveal happens at hero scale, so its offsets are damped to stay framed.
  // Both are up sharply on the champion (0.38 / 0.55), and for one reason: the
  // treasure now has to climb FULLY CLEAR of the chest and own the top of the
  // frame, instead of hovering in its mouth where the result banner covered it.
  // See the composition note on `heroFill` above — these are solved against it,
  // not tuned independently.
  riseDamp: 0.58, // prize climb, relative to the hero scale
  prizeDamp: 0.9, // prize size, relative to the hero scale
  // Reveal ritual durations (ticks, speed-scaled at build time).
  brace: 22,
  latch: 16,
  pause: 12,
  lid: 14,
  rise: 34,
  hold: 12,
  burst: 10, // the light-burst flash window, right after the lid opens
  // Reveal magnitudes.
  shakeMag: 0.05, // anticipation shake amplitude
  latchDrop: 1.55, // radians the latch swings open
  latchRecoil: 0.22, // extra kick on the latch's release snap
  lidOpen: 1.7, // radians the lid swings open — stands the open lid nearer upright
  // (~97° vs the old ~109°), matching the reference silhouette: the lid presents
  // its inner face rather than reclining so far back that the interior floor
  // reads as a large flat top-down plane. Kept short of vertical so the tall open
  // lid still clears the top of the hero frame.
  burstParticles: 12, // bounded upward light-burst motes
  riseHeight: 2.4, // world-units the prize climbs to hover clear above the chest
} as const;

// ── the hero framing (where the chosen chest flies to, and how big) ───────────

/**
 * The close-up framing the chosen chest occupies for its reveal, derived from
 * the live camera rather than hand-placed. The chest travels to a point on the
 * camera's own view axis, so it lands dead-center horizontally no matter how
 * the table camera is posed, and it is sized from the frustum's real extent at
 * that distance — which is what keeps it ON SCREEN.
 *
 * The size is the smaller of two budgets: a share of the frame's HEIGHT
 * (`heroFill`), and a width guard evaluated against a SQUARE frame
 * (`heroWidthMargin`). The width guard is the conservative one — the view
 * context carries no aspect ratio, so the framing assumes the narrowest
 * viewport the rest of this scene already assumes and never spends more
 * horizontal room than that. A wider window simply leaves more margin.
 */
export interface HeroFraming {
  /** World point the chest's body settles at (screen center, dropped a little). */
  readonly anchor: EngineVec3;
  /** Chest scale multiplier at the hero framing. */
  readonly scale: number;
  /** Camera basis at the hero plane — used to place the veil and to test framing. */
  readonly forward: EngineVec3;
  readonly up: EngineVec3;
  readonly right: EngineVec3;
  /** Visible half-height (world units) at the hero plane. */
  readonly halfHeight: number;
  /** Distance from the camera to the hero plane. */
  readonly distance: number;
}

export const heroFraming = (camera: Camera3D): HeroFraming => {
  const forward = normalizeV3(subV3(camera.target, camera.position));
  const right = normalizeV3(crossV3(forward, v3(0, 1, 0)));
  const up = crossV3(right, forward);
  const distance = CHEST_TIMING.heroDistance;
  const halfHeight = distance * Math.tan(camera.fovY / 2);
  // Center of the frame at the hero plane, then dropped so the chest sits low
  // and leaves headroom for the prize that rises out of it.
  const center = addV3(camera.position, scaleV3(forward, distance));
  const anchor = addV3(center, scaleV3(up, -CHEST_TIMING.heroDrop * halfHeight));
  // Size the chest against its NEAR face, not its center plane. The chest is
  // most of a unit deep, and once it is enlarged and this close to the camera
  // its front face is meaningfully nearer than its middle — so it projects
  // bigger than a center-plane fit predicts, and a naive fit overflows the
  // frame by a few percent. Solving where the object is actually widest:
  //
  //   (extent/2)·scale  ≤  frac · (distance − halfDepth·scale) · tan(fov/2)
  //
  // rearranged for scale. `frac` is the share of the half-frame the extent may
  // claim: `heroFill` against height, `heroWidthMargin` against the width of a
  // SQUARE window — the narrowest this scene's camera is built for.
  const tan = Math.tan(camera.fovY / 2);
  const halfDepth = CHEST_LID.z / 2;
  const fit = (extent: number, frac: number): number => (frac * distance * tan) / (extent / 2 + frac * halfDepth * tan);
  const scale = Math.min(fit(CHEST_HEIGHT, CHEST_TIMING.heroFill), fit(CHEST_WIDTH, CHEST_TIMING.heroWidthMargin));
  return { anchor, distance, forward, halfHeight, right, scale, up };
};

// ── the spiral flight (grid slot → hero framing) ──────────────────────────────

/** A camera's screen-plane basis. `HeroFraming` satisfies this directly. */
export interface ScreenBasis {
  readonly right: EngineVec3;
  readonly up: EngineVec3;
  readonly forward: EngineVec3;
}

/**
 * The chosen chest's pose partway through its spiral to the hero framing.
 * `t` is flight progress in [0, 1].
 *
 * The spiral is described in the camera's SCREEN PLANE, not in world XZ. The
 * chest's offset from the hero anchor is split into a screen part (right/up)
 * and a depth part (forward); the screen part rotates while its radius
 * collapses, and the depth part simply closes. So the chest traces a spiral
 * *as seen by the player*, winding inward to the middle of the frame while it
 * comes forward — which is the motion this is meant to be.
 *
 * Doing it in world XZ instead looks similar from some angles but is subtly
 * wrong, and provably so: rotating a world-space offset turns the chest's large
 * DEPTH offset into an equally large LATERAL one, flinging the outer chests off
 * the side of the frame mid-flight. In the screen plane the excursion can never
 * exceed the chest's own starting screen radius — and every chest starts on
 * screen — so the flight is bounded in frame by construction rather than by
 * tuning. The framing test pins this for all nine slots.
 *
 * The turn count is whole, which lands the chest facing FRONT again: the latch,
 * lock plate, and lid all read correctly the moment the reveal begins. Every
 * quantity returns to a clean resting value at `t = 1` — the tumble unwinds to
 * level and the radius to zero.
 *
 * Pure in (from, to, t, basis) — no seed, no clock, no outcome — so the flight
 * is identical on every replay and can never hint at what the chest holds.
 */
export interface FlightPose {
  readonly position: EngineVec3;
  /** Yaw of the chest itself (ends at a whole number of turns → front-facing). */
  readonly spin: number;
  /** Pitch wobble, peaking mid-flight and unwinding to 0 on arrival. */
  readonly tumble: number;
  /** Growth ramp in [0, 1] toward the hero scale — delayed, so the chest is
   * still small while it is swinging widest and only fills out once centered. */
  readonly grow: number;
}

export const spiralFlight = (from: EngineVec3, to: EngineVec3, t: number, basis: ScreenBasis): FlightPose => {
  const path = smoothstep(clamp01(t));
  // The turning FINISHES before the flight does, leaving a final stretch that is
  // pure settle-and-fill. That ordering is deliberate: a box carries a wider
  // footprint on the diagonal than square-on, so a chest still turning while it
  // reaches full size briefly overflows the frame at its corners. Spinning down
  // first means it only ever fills out square-on — and it reads better too,
  // arriving and settling rather than growing mid-tumble.
  //
  // The turn also eases in HARD (a squared smoothstep), so there is almost no
  // rotation while the chest is still far out. Rotating early is what pushed
  // the front-row chests — which already sit low in frame — down past the
  // bottom edge: their offset from the anchor points downward, and turning it
  // before the radius has collapsed swings it further down still. Easing in
  // means the turning happens once the radius is small, where it costs no
  // screen room. It still decelerates smoothly to a stop, so nothing snaps.
  const turn = smoothstep(clamp01(path / CHEST_TIMING.spiralSpinFinish)) ** CHEST_TIMING.spiralTurnEaseIn;
  const spin = CHEST_TIMING.spiralTurns * Math.PI * 2 * turn;
  // The screen radius collapses a little ahead of the turn, so the orbit
  // tightens as it winds rather than circling at a constant distance.
  const shrink = (1 - path) ** CHEST_TIMING.spiralConverge;
  const offset = subV3(from, to);
  const a = dotV3(offset, basis.right);
  const b = dotV3(offset, basis.up);
  const cos = Math.cos(spin);
  const sin = Math.sin(spin);
  // Screen-plane offset, wound and pulled in; plus a small lift along the
  // camera's up so the chest rises clear of the board rather than sliding
  // through its neighbours on the way out.
  const screenX = (a * cos - b * sin) * shrink;
  const screenY = (a * sin + b * cos) * shrink + Math.sin(Math.PI * path) * CHEST_TIMING.spiralArc;
  // Depth closes ahead of the growth ramp, and the screen radius ahead of that,
  // so the flight sequences cleanly: wind in to the middle, come forward, THEN
  // fill out. Letting the chest reach full size while it still had depth to
  // cover made it dip low in frame on the approach — the camera looks steeply
  // down, so "still further away" also means "lower on screen", and a chest
  // that is already near full size has no margin left to spend on that dip.
  const depth = dotV3(offset, basis.forward) * (1 - path) ** CHEST_TIMING.spiralApproach;
  const delay = CHEST_TIMING.spiralGrowDelay;
  return {
    grow: smoothstep(clamp01((clamp01(t) - delay) / (1 - delay))),
    position: addV3(
      to,
      addV3(scaleV3(basis.right, screenX), addV3(scaleV3(basis.up, screenY), scaleV3(basis.forward, depth))),
    ),
    spin,
    tumble: Math.sin(Math.PI * turn) * CHEST_TIMING.spiralTumble,
  };
};

/** Flight progress for a state: 0 on the board, 1 at the hero framing. It ramps
 * over the commit beat, HOLDS at 1 for the whole reveal and result (the chest
 * stays in close-up while it opens), and eases back out as the round resets. */
/** The whole commit beat: the crab's approach, then the chest's flight. What the
 * mount declares as `commitPauseTicks`, and what every progress read below
 * divides up. */
export const commitBeatTicks = CHEST_TIMING.approachTicks + CHEST_TIMING.spiralTicks;

export const flightProgress = (session: SessionState, presentationSpeed: number): number => {
  const phase = session.phase;
  const age = phaseAge(session);
  if (phase === "committing") {
    // The flight does not start until the crab has arrived — he is the one who
    // takes the chest, so it cannot leave before he reaches it.
    const approach = speedTicks(CHEST_TIMING.approachTicks, presentationSpeed);
    return clamp01((age - approach) / speedTicks(CHEST_TIMING.spiralTicks, presentationSpeed));
  }
  if (phase === "revealing" || phase === "celebrating" || phase === "complete" || phase === "interacting") {
    return 1;
  }
  if (phase === "resetting") {
    return 1 - clamp01(age / speedTicks(10, presentationSpeed));
  }
  return 0;
};

// ── the crab's errand ─────────────────────────────────────────────────────────

/*
 * The beach crab fetches the chosen chest and opens it.
 *
 * He scuttles from wherever he is standing — his position is player-movable, so
 * "wherever" is real — to the front of the chosen chest, gets his claws onto the
 * lid rail, and then RIDES the chest as it spirals into its close-up, so the
 * player watches him push the lid open at full size and hop when it goes.
 *
 * Everything here is a pure read of the session phase and its age. That matters
 * for the same reason the idle dance's independence matters: the crab reacts to
 * WHICH CHEST THE PLAYER CHOSE, which is the player's own input, and to nothing
 * else. He never reads the population, the committed plan, or the tier — so no
 * part of his errand can hint at what is inside the chest he is opening. The
 * chest test pins this.
 */
export interface CrabJourney {
  /** 0 = standing on his own patch of sand, 1 = arrived at the chest. Drives the
   * scuttle across the beach, and runs BACKWARDS as the round resets. */
  readonly approach: number;
  /** True once he is welded to the chest and travelling with it. */
  readonly riding: boolean;
  /** How far his claws are up on the lid rail, in [0, 1]. */
  readonly grip: number;
  /** The hop when the lid pops, in [0, 1] — one spike that settles. */
  readonly hop: number;
}

const AT_HOME: CrabJourney = { approach: 0, grip: 0, hop: 0, riding: false };

export const crabJourney = (session: SessionState, presentationSpeed: number, timeline: RevealTimeline): CrabJourney => {
  const phase = session.phase;
  const age = phaseAge(session);
  const approachTicks = speedTicks(CHEST_TIMING.approachTicks, presentationSpeed);

  if (phase === "committing") {
    // Two beats in one phase: he crosses the sand, then the chest lifts with him
    // aboard. `grip` ramps over the tail of the approach so the claws are already
    // on the rail by the time it leaves the ground.
    const walk = clamp01(age / approachTicks);
    return { approach: smoothstep(walk), grip: clamp01((walk - 0.6) / 0.4), hop: 0, riding: age >= approachTicks };
  }
  if (phase === "revealing") {
    // The claws follow the lid: they are fully committed by the time it starts to
    // swing, and the hop fires as it lands open.
    const hopSpan = speedTicks(CHEST_TIMING.crabHopTicks, presentationSpeed);
    const since = age - timeline.lidEnd;
    return { approach: 1, grip: 1, hop: since < 0 || since > hopSpan ? 0 : Math.sin((since / hopSpan) * Math.PI), riding: true };
  }
  if (phase === "celebrating" || phase === "complete" || phase === "interacting") {
    return { approach: 1, grip: 1, hop: 0, riding: true };
  }
  if (phase === "resetting") {
    // He lets go and walks home, the same easing in reverse.
    const back = 1 - clamp01(age / speedTicks(CHEST_TIMING.approachTicks, presentationSpeed));
    return { approach: smoothstep(back), grip: 0, hop: 0, riding: false };
  }
  return AT_HOME;
};

// ── the reveal timeline (ticks from entering "revealing", speed-scaled) ────────

export interface RevealTimeline {
  readonly braceEnd: number;
  readonly latchStart: number;
  readonly latchEnd: number;
  /** Warm seam light begins leaking here (as the latch lands). */
  readonly seamStart: number;
  readonly pauseEnd: number;
  /** The lid begins to swing (= pauseEnd). */
  readonly lidStart: number;
  readonly lidEnd: number;
  /** The upward light burst peaks here (= lidEnd). */
  readonly burstAt: number;
  readonly riseEnd: number;
  readonly total: number;
}

export const revealTimeline = (presentationSpeed: number, reducedMotion: boolean): RevealTimeline => {
  const scale = reducedMotion ? 0.6 : 1;
  const t = (n: number): number => speedTicks(Math.round(n * scale), presentationSpeed);
  const braceEnd = t(CHEST_TIMING.brace);
  const latchStart = braceEnd;
  const latchEnd = latchStart + t(CHEST_TIMING.latch);
  const seamStart = latchEnd;
  const pauseEnd = latchEnd + t(CHEST_TIMING.pause);
  const lidStart = pauseEnd;
  const lidEnd = pauseEnd + t(CHEST_TIMING.lid);
  const burstAt = lidEnd;
  const riseEnd = lidEnd + t(CHEST_TIMING.rise);
  return {
    braceEnd,
    burstAt,
    latchEnd,
    latchStart,
    lidEnd,
    lidStart,
    pauseEnd,
    riseEnd,
    seamStart,
    total: riseEnd + t(CHEST_TIMING.hold),
  };
};

// ── formalized presentation phases (readable names for the reveal ritual) ──────

/** The named visual phases the chest presentation moves through. The legal
 * ordering is guaranteed upstream by the session phase machine (which also
 * hard-locks input during the protected phases), so this is a pure read of
 * where the ritual is — never a place a stray click can jump. */
export type ChestPresentation =
  | "idle"
  | "committed"
  | "anticipation"
  | "latch"
  | "seam"
  | "lid"
  | "burst"
  | "prize"
  | "result"
  | "reset";

export const presentationPhase = (session: SessionState, timeline: RevealTimeline): ChestPresentation => {
  const phase = session.phase;
  if (phase === "intro" || phase === "ready") {
    return "idle";
  }
  if (phase === "committing") {
    return "committed";
  }
  if (phase === "resetting") {
    return "reset";
  }
  if (phase === "celebrating" || phase === "complete") {
    return "result";
  }
  const age = phaseAge(session);
  if (age < timeline.braceEnd) {
    return "anticipation";
  }
  if (age < timeline.latchEnd) {
    return "latch";
  }
  if (age < timeline.pauseEnd) {
    return "seam";
  }
  if (age < timeline.lidEnd) {
    return "lid";
  }
  if (age < timeline.burstAt + timeline.lidEnd - timeline.lidStart) {
    return "burst";
  }
  return "prize";
};

// ── idle cosmetics (deterministic, per-chest, outcome-independent) ─────────────

/** A per-chest idle phase (radians), spaced by the golden angle so the nine
 * chests never bob in unison. Pure in the slot index — no seed — so it cannot
 * correlate with which chest wins. */
export const idlePhase = (index: number): number => (index * 2.399963 + 0.4) % (Math.PI * 2);

/**
 * The idle dance pose for chest `index` at `tick` — AMBIENT stream only.
 * Time is cut into windows; each window elects one dancer (and, rarely, a
 * second) and gives it a small scoot + twist + squash figure.
 */
export interface DancePose {
  readonly scootX: number;
  readonly twist: number;
  readonly squash: number;
}

export const dancePose = (index: number, count: number, tick: number, seed: number, liveliness: number): DancePose => {
  const window = Math.floor(tick / 96);
  const dancer = sampleInt(count, seed, "ambient", window, 0);
  const second = sampleInt(count, seed, "ambient", window, 1);
  const duet = sample01(seed, "ambient", window, 2) < 0.2;
  const isDancing = index === dancer || (duet && index === second);
  if (!isDancing || liveliness <= 0) {
    return { scootX: 0, squash: 0, twist: 0 };
  }
  const local = (tick % 96) / 96;
  const envelope = Math.sin(Math.PI * local);
  const figure = sample01(seed, "ambient", window, 3 + index);
  return {
    scootX: Math.sin(local * Math.PI * 4 + figure * 6) * 0.05 * liveliness * envelope,
    squash: Math.abs(Math.sin(local * Math.PI * 6)) * 0.045 * liveliness * envelope,
    twist: Math.sin(local * Math.PI * 2 + figure * 4) * 0.07 * liveliness * envelope,
  };
};

// ── beach set-dressing life (deterministic, outcome-independent) ───────────────
// The shore props breathe so the frame is not a still life. Like the chest dance,
// every animated quantity here is a PURE function of the tick (and, for the crab's
// randomly-timed idles, the AMBIENT stream only) — never the population, the
// committed plan, or the wall clock — so no wobble on the beach can hint at which
// chest holds a prize. scene.ts applies these poses to the palm and crab parts.

/** The palm's wind sway at `tick`. A gentle compound lean (two slow frequencies
 * so it never reads as a clean metronome) plus a faster flutter phase the fronds
 * ride. No seed: wind is the same every session and cannot correlate with any
 * outcome. `bend` is the crown's downwind lean in radians; `flutter(i)` is one
 * frond's extra droop. */
export interface PalmSway {
  readonly bend: number;
  readonly flutter: (frond: number) => number;
}

export const palmSway = (tick: number): PalmSway => ({
  bend: Math.sin(tick * 0.017) * 0.016 + Math.sin(tick * 0.006 + 1.3) * 0.009,
  flutter: (frond: number): number => Math.sin(tick * 0.045 + frond * 1.7) * 0.016,
});

/** The crab's idle repertoire. `rest` is the between-animation default (just a
 * faint breathe + eyestalk drift); the other four are the little bits of
 * business it performs. */
export type CrabIdleKind = "rest" | "scuttle" | "wave" | "bob" | "turn";

/** One tick of the crab's idle pose. Whole-body `scootX`/`bob`/`yaw`, plus the
 * per-limb `clawLift`/`legWiggle`/`eye` amounts and an always-on `breath`. */
export interface CrabPose {
  readonly kind: CrabIdleKind;
  readonly scootX: number;
  readonly bob: number;
  readonly yaw: number;
  readonly clawLift: number;
  /**
   * How much the raised claws FLAP, in [0, 1] — separate from how far they are
   * raised.
   *
   * These used to be the same thing: the assembly always oscillated a raised claw
   * by ±30% at ~5 Hz, which is right for a crab WAVING and wrong for a crab
   * GRIPPING something. Once the crab started prising chests open, that baked-in
   * flap read as his claws shaking violently on the lid. Holding still and waving
   * are different actions, so they are now different numbers.
   */
  readonly clawShake: number;
  readonly legWiggle: number;
  readonly eye: number;
  readonly breath: number;
}

/** Ticks per idle slot (~2.5 s at 60 Hz). Each slot the crab either performs one
 * elected idle or rests, decided from the ambient stream — so the animations
 * fire on a random interval rather than every window. */
export const CRAB_WINDOW = 150;
const CRAB_KINDS: readonly CrabIdleKind[] = ["scuttle", "wave", "bob", "turn"];

/**
 * The crab's idle pose at `tick`, drawn ONLY from the AMBIENT stream (the same
 * independence invariant the chest dance obeys). Each `CRAB_WINDOW` slot elects
 * one idle and whether it plays at all; the chosen figure eases in and out over
 * the slot on a `sin(pi·local)` envelope. Pure in (tick, seed).
 */
export const crabIdle = (tick: number, seed: number): CrabPose => {
  const window = Math.floor(tick / CRAB_WINDOW);
  const local = (tick % CRAB_WINDOW) / CRAB_WINDOW;
  const env = Math.sin(Math.PI * local);
  const breath = Math.sin(tick * 0.08) * 0.02;
  const eyeDrift = Math.sin(tick * 0.05) * 0.05;
  const active = sample01(seed, "ambient", window, 40) < 0.55;
  const kind = CRAB_KINDS[sampleInt(CRAB_KINDS.length, seed, "ambient", window, 41)] as CrabIdleKind;
  const jitter = sample01(seed, "ambient", window, 42);
  const resting: CrabPose = { bob: 0, breath, clawLift: 0, clawShake: 0, eye: eyeDrift, kind: "rest", legWiggle: 0, scootX: 0, yaw: 0 };
  const poses: Record<CrabIdleKind, CrabPose> = {
    // A little side scuttle with the legs paddling and the body leaning into it.
    scuttle: {
      bob: Math.abs(Math.sin(local * Math.PI * 4)) * 0.03 * env,
      breath,
      clawLift: 0,
      clawShake: 0,
      eye: eyeDrift,
      kind: "scuttle",
      legWiggle: Math.sin(tick * 0.6) * 0.4 * env,
      scootX: Math.sin(local * Math.PI * 3 + jitter * 6) * 0.45 * env,
      yaw: Math.sin(local * Math.PI * 3 + jitter * 6) * 0.12 * env,
    },
    // Raising and snapping the claws.
    wave: { bob: 0, breath, clawLift: (0.5 + jitter * 0.35) * env, clawShake: 1, eye: eyeDrift, kind: "wave", legWiggle: 0, scootX: 0, yaw: 0 },
    // Bobbing up and down with the eyestalks wagging.
    bob: {
      bob: Math.abs(Math.sin(local * Math.PI * 4)) * 0.16 * env,
      breath,
      clawLift: 0,
      clawShake: 0,
      eye: eyeDrift + Math.sin(tick * 0.22) * 0.12 * env,
      kind: "bob",
      legWiggle: 0,
      scootX: 0,
      yaw: 0,
    },
    // Turning to look around.
    turn: { bob: 0, breath, clawLift: 0, clawShake: 0, eye: eyeDrift, kind: "turn", legWiggle: Math.sin(tick * 0.5) * 0.12 * env, scootX: 0, yaw: Math.sin(local * Math.PI * 2 + jitter * 3) * 0.5 * env },
    rest: resting,
  };
  return active ? poses[kind] : resting;
};

export const initialChestExtra = (session: SessionState, previous: ChestExtra | null): ChestExtra => {
  const count = session.config.choiceCount ?? 9;
  const fresh = initialChestDrag(count);
  return {
    // Rearranged chests persist across rounds for the same reason the props do —
    // it is the player's board. The one exception is a CHANGED chest count: the
    // reward ladder and `choiceCount` are editable from the Set Up panel, so a
    // carried-over layout can be the wrong length, and a nine-slot arrangement
    // dealt onto a six-chest board would leave chests stacked or missing. On any
    // mismatch the grid is dealt fresh.
    chests: previous === null || previous.chests.slots.length !== count ? fresh : { ...fresh, slots: previous.chests.slots },
    choice: initialChoice(4),
    // The player's placed props persist across rounds (a New Round / Replay keeps
    // them where they were left); only a page reload starts a session from null
    // and returns them home. The transient drag fields always start clean.
    decor: previous === null ? DEFAULT_DECOR : { ...DEFAULT_DECOR, props: previous.decor.props },
    revealStartTick: null,
  };
};

/** Per-tick controller. Selection commits; the reveal advances on the shared
 * timeline and hands off to "celebrating" when it completes. */
export const stepChest = (
  runtime: GameRuntime<ChestSpec>,
  state: ChestState,
  input: InputFrame,
  _ctx: TickContext,
): ChestState => {
  const session = state.session;
  const count = session.config.choiceCount ?? 9;
  const camera = chestCamera(count);

  // The player can pick up and move the beach props AND the chests. The props
  // take the pointer first — they sit out on the sand, so a press over one is
  // unambiguous — and the chests only consider a press the props did not claim.
  const props = stepDecorDrag(state.extra.decor, input, camera);
  // Chests are only rearrangeable while the board IS the subject. Once a pick
  // commits, the chosen chest is flying to its close-up and its position belongs
  // to that flight, not to a drag; any live grab is dropped at the transition.
  const chests =
    session.phase === "ready" && !props.active
      ? stepChestDrag(state.extra.chests, input, camera)
      : { active: false, drag: { ...state.extra.chests, grab: null, pointerDown: input.pointer?.down ?? false } };
  const dragged: ChestState = { ...state, extra: { ...state.extra, chests: chests.drag, decor: props.decor } };

  if (session.phase === "ready") {
    // When either drag owns the pointer, the choice step sees NO POINTER at all —
    // the same trick the harness uses to lock input (`lockedFrame`). That does two
    // jobs with one move: it suppresses the selection, and it clears the press
    // state, so a press that turned into a drag can never land as a pick when it
    // is finally released. Keyboard navigation keeps working throughout.
    const frame = props.active || chests.active ? { ...input, pointer: undefined } : input;
    // Tap-to-confirm: on touch the first tap highlights a chest and the second
    // opens it; a desktop click still opens in one action (hover pre-arms it).
    const result = stepChoice(dragged.extra.choice, frame, camera, chestTargetsAt(chests.drag.slots), CHEST_COLUMNS, true);
    if (result.selectedNow !== null) {
      return {
        ...dragged,
        extra: { ...dragged.extra, choice: result.core },
        pendingContext: { selectedIndex: result.selectedNow },
        session: transition(session, "committing"),
      };
    }
    return { ...dragged, extra: { ...dragged.extra, choice: result.core } };
  }

  if (session.phase === "revealing") {
    const start = dragged.extra.revealStartTick ?? session.phaseStartTick;
    const timeline = revealTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    const withStart: ChestState =
      dragged.extra.revealStartTick === null ? { ...dragged, extra: { ...dragged.extra, revealStartTick: start } } : dragged;
    if (phaseAge(session) >= timeline.total) {
      return { ...withStart, session: transition(session, "celebrating") };
    }
    return withStart;
  }

  return dragged;
};

/**
 * The chest's own reveal-ritual cues, phrased as marks crossed on the reveal
 * timeline: a light latch click, the weighty latch-land thump, the rising seam
 * shimmer, the heavy lid-open thump, and the burst shimmer as the lid settles —
 * plus soft count-up ticks over the first stretch of a winning celebration. The
 * win/loss fanfare itself is played centrally by the mount harness.
 */
export const chestCues = (prev: ChestState, next: ChestState): readonly ToneSpec[] => {
  const session = next.session;
  const seed = session.committed?.presentationSeed ?? session.seed;
  const before = phaseAge(prev.session);
  const after = phaseAge(session);
  const crossed = (mark: number): boolean => before < mark && after >= mark;

  if (session.phase === "revealing" && prev.session.phase === "revealing") {
    const tl = revealTimeline(session.config.presentationSpeed, false);
    return [
      ...(crossed(tl.latchStart) ? tickCue(seed, 1) : []), // latch click as it releases
      ...(crossed(tl.latchEnd) ? thumpCue(seed, 2) : []), // latch lands / recoil snap
      ...(crossed(tl.seamStart) ? shimmerCue(seed, 3) : []), // warm seam light rising
      ...(crossed(tl.lidStart) ? thumpCue(seed, 4) : []), // weighty lid heave
      ...(crossed(tl.lidEnd) ? shimmerCue(seed, 5) : []), // light burst as the lid settles
    ];
  }

  // Count-up ticks accompanying the number climbing during a winning result.
  if (session.phase === "celebrating" && prev.session.phase === "celebrating" && (session.committed?.win ?? false)) {
    return [4, 8, 12, 16, 20, 24].filter((mark) => crossed(mark)).flatMap((_, i) => tickCue(seed, 30 + i));
  }
  return [];
};
