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

// A tighter span than a card-table default: the reference frames the chest grid
// large — it claims ~55% of the frame width, and the sandy lagoon fills the top
// of the frame with no horizon showing. At the looser span the grid projected at
// ~36% and the camera's top edge cleared the floor rim to expose the pastel
// backdrop sheet as an intruding sky band. Pulling the span in (~0.66x) both
// scales the grid up to reference size and drops the top frame-edge ray onto the
// lagoon floor, cropping that backdrop band out of frame. The pitch angle is
// preset-fixed and unchanged; only the zoom tightens. The hero-flight close-up is
// derived from a fixed heroDistance + fovY, so its on-screen scale is untouched.
export const chestCamera = (count: number): ReturnType<typeof tabletopCamera> =>
  tabletopCamera(v3(0, 0.42, -0.1), 5.0 + Math.ceil(count / CHEST_COLUMNS) * 0.78);

export const chestTargets = (count: number): readonly PickTarget[] =>
  Array.from({ length: count }, (_, index) => ({
    at: chestPosition(index, count),
    index,
    radiusPx: 78,
  }));

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
  tilt: 0.15, // radians tilted toward the camera
  selectScale: 1.07, // slight enlarge of the chosen chest
  // The hero flight — the chosen chest spirals off the board and up into a
  // close, screen-filling framing before the lid is ever touched. The CAMERA
  // does not move for this: the chest comes to the camera, which keeps the
  // eight others (and the board) exactly where the player left them.
  spiralTicks: 66, // commit-phase length; the whole spiral plays inside it
  spiralTurns: 2, // whole turns, so the chest lands facing front again
  spiralConverge: 3, // how sharply the orbit radius collapses (see spiralFlight)
  spiralApproach: 2, // how sharply the remaining DEPTH closes (see spiralFlight)
  spiralArc: 0.35, // extra mid-flight lift, so it arcs rather than slides
  spiralTumble: 0.2, // radians of pitch wobble, returning to 0 on arrival
  spiralGrowDelay: 0.3, // fraction of the flight before it starts enlarging
  spiralSpinFinish: 0.72, // fraction of the flight by which the turning is DONE
  spiralTurnEaseIn: 2, // how gently the turn starts (see spiralFlight)
  heroDistance: 4, // world units in front of the camera the chest settles at
  // Sized so the chest is big AND its OPEN lid still fits: the lid swings up
  // and back well above the closed silhouette, so the closed chest cannot be
  // allowed to claim the whole frame height on its own.
  heroFill: 0.45, // fraction of frame HEIGHT the closed hero chest occupies
  heroDrop: 0.36, // how far below frame center it sits (fraction of half-height)
  heroWidthMargin: 0.86, // width guard: fraction of the frame it may ever span
  // The background veil that drops behind the hero chest.
  dimVeil: 0.82, // peak darkness of the veil (0 = none, 1 = black)
  dimSteps: 16, // quantization of the veil ramp (materials carry fixed opacity)
  veilGap: 1.3, // world units the veil sits BEHIND the hero chest — clear of the
  // hero chest's own depth, still nearer than the closest chest on the board
  // The reveal happens at hero scale, so its offsets are damped to stay framed.
  riseDamp: 0.38, // prize climb, relative to the hero scale
  prizeDamp: 0.55, // prize size, relative to the hero scale
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
  lidOpen: 1.9, // radians the lid swings open
  burstParticles: 12, // bounded upward light-burst motes
  riseHeight: 1.2, // world-units the prize climbs to hover clear above the chest
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
export const flightProgress = (session: SessionState, presentationSpeed: number): number => {
  const phase = session.phase;
  const age = phaseAge(session);
  if (phase === "committing") {
    return clamp01(age / speedTicks(CHEST_TIMING.spiralTicks, presentationSpeed));
  }
  if (phase === "revealing" || phase === "celebrating" || phase === "complete" || phase === "interacting") {
    return 1;
  }
  if (phase === "resetting") {
    return 1 - clamp01(age / speedTicks(10, presentationSpeed));
  }
  return 0;
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
  const resting: CrabPose = { bob: 0, breath, clawLift: 0, eye: eyeDrift, kind: "rest", legWiggle: 0, scootX: 0, yaw: 0 };
  const poses: Record<CrabIdleKind, CrabPose> = {
    // A little side scuttle with the legs paddling and the body leaning into it.
    scuttle: {
      bob: Math.abs(Math.sin(local * Math.PI * 4)) * 0.03 * env,
      breath,
      clawLift: 0,
      eye: eyeDrift,
      kind: "scuttle",
      legWiggle: Math.sin(tick * 0.6) * 0.4 * env,
      scootX: Math.sin(local * Math.PI * 3 + jitter * 6) * 0.45 * env,
      yaw: Math.sin(local * Math.PI * 3 + jitter * 6) * 0.12 * env,
    },
    // Raising and snapping the claws.
    wave: { bob: 0, breath, clawLift: (0.5 + jitter * 0.35) * env, eye: eyeDrift, kind: "wave", legWiggle: 0, scootX: 0, yaw: 0 },
    // Bobbing up and down with the eyestalks wagging.
    bob: {
      bob: Math.abs(Math.sin(local * Math.PI * 4)) * 0.16 * env,
      breath,
      clawLift: 0,
      eye: eyeDrift + Math.sin(tick * 0.22) * 0.12 * env,
      kind: "bob",
      legWiggle: 0,
      scootX: 0,
      yaw: 0,
    },
    // Turning to look around.
    turn: { bob: 0, breath, clawLift: 0, eye: eyeDrift, kind: "turn", legWiggle: Math.sin(tick * 0.5) * 0.12 * env, scootX: 0, yaw: Math.sin(local * Math.PI * 2 + jitter * 3) * 0.5 * env },
    rest: resting,
  };
  return active ? poses[kind] : resting;
};

export const initialChestExtra = (_session: SessionState, previous: ChestExtra | null): ChestExtra => ({
  choice: initialChoice(4),
  // The player's placed props persist across rounds (a New Round / Replay keeps
  // them where they were left); only a page reload starts a session from null
  // and returns them home. The transient drag fields always start clean.
  decor: previous === null ? DEFAULT_DECOR : { ...DEFAULT_DECOR, props: previous.decor.props },
  revealStartTick: null,
});

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

  // The player can pick up and move the beach props. The drag runs every tick
  // and takes the pointer first; when it owns the pointer, a chest pick is
  // suppressed for that tick (input is stripped while the session is locked, so
  // dragging only happens in ready/celebrating/complete).
  const drag = stepDecorDrag(state.extra.decor, input, camera);
  const withDecor: ChestState = { ...state, extra: { ...state.extra, decor: drag.decor } };

  if (session.phase === "ready") {
    if (drag.active) {
      return withDecor;
    }
    // Tap-to-confirm: on touch the first tap highlights a chest and the second
    // opens it; a desktop click still opens in one action (hover pre-arms it).
    const result = stepChoice(withDecor.extra.choice, input, camera, chestTargets(count), CHEST_COLUMNS, true);
    if (result.selectedNow !== null) {
      return {
        ...withDecor,
        extra: { ...withDecor.extra, choice: result.core },
        pendingContext: { selectedIndex: result.selectedNow },
        session: transition(session, "committing"),
      };
    }
    return { ...withDecor, extra: { ...withDecor.extra, choice: result.core } };
  }

  if (session.phase === "revealing") {
    const start = withDecor.extra.revealStartTick ?? session.phaseStartTick;
    const timeline = revealTimeline(session.config.presentationSpeed, runtime.settings.reducedMotion);
    const withStart: ChestState =
      withDecor.extra.revealStartTick === null ? { ...withDecor, extra: { ...withDecor.extra, revealStartTick: start } } : withDecor;
    if (phaseAge(session) >= timeline.total) {
      return { ...withStart, session: transition(session, "celebrating") };
    }
    return withStart;
  }

  return withDecor;
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
