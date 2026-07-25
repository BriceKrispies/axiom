/*
 * scene.ts — Treasure Chest Pick presentation: nine carved-wood, gold-gilded
 * chests staged as a small arcade ritual. Idle chests breathe out of unison;
 * a chosen chest lifts, tilts toward the camera,
 * and pools warm light beneath it while the other eight dim and go still; the
 * reveal is a readable sequence — anticipation shake, latch drop with a recoil
 * snap, warm light through the seam, a weighty overshooting lid, a compact light
 * burst, and a prize that rises fully clear of the chest to own the frame (or a
 * playful dust puff on an empty chest). Pure view: `chestScene(runtime, state)`
 * returns a Scene value; every animated quantity is a pure function of the tick.
 *
 * Nothing here reads the population or the winning slot for cosmetics: the idle
 * dance draws only from the ambient stream, and the breathe is pure in
 * (index, tick) — so no wobble can hint at which chest holds a prize.
 */

import type { Camera3D, MaterialSpec, Scene, SceneInstance, SceneLight, ViewContext } from "@axiom/web-engine";
import type { EngineQuat, EngineVec3, GameResources, Rgba } from "@axiom/web-engine";
import { drawStylizedWaterSurface } from "@axiom/web-engine";
import { worldToCanvas } from "../../presentation/cameras/picking.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import type { BrandSpec } from "../../presentation/branding/brand.ts";
import { brandMaterials } from "../../presentation/branding/brand.ts";
import { stampText } from "../../presentation/branding/label.ts";
import { lowDetail } from "../../presentation/detail.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { REWARD_MATERIALS, rewardMaterialOf } from "../../presentation/rewards/tiers.ts";
import { celebrationFor, outcomeRarity, speedTicks } from "../round-state.ts";
import { clamp01, easeOutBack, easeOutCubic, lerp, pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import {
  addV3,
  hingedTransform,
  QUAT_IDENTITY,
  quatMul,
  quatPitch,
  quatRoll,
  quatYaw,
  rotateByQuat,
  scaleV3,
  v3,
} from "../../presentation/stage/vectors.ts";
import type { ChestSpec, ChestState, DecorDrag, HeroFraming } from "./game.ts";
import {
  CHEST_BODY as BODY,
  CHEST_BODY_TOP as BODY_TOP,
  CHEST_HEIGHT,
  CHEST_LATCH as LATCH,
  CHEST_LID as LID,
  CHEST_LID_ARCH,
  CHEST_TIMING,
  chestCamera,
  chestPosition,
  crabIdle,
  dancePose,
  flightProgress,
  heroFraming,
  idlePhase,
  palmSway,
  revealTimeline,
  spiralFlight,
} from "./game.ts";

// ── declared resources ──────────────────────────────────────────────────────────

/**
 * The background veil, as a graded ladder of fixed-opacity materials.
 * A material's opacity is registered once at bind time and cannot be animated
 * per-instance, so the ramp is quantized into `dimSteps` rungs and the veil
 * instance simply picks the rung matching its current darkness. With enough
 * rungs the ramp is smooth at the speed it plays (a step lands every few
 * ticks), which is why the count is generous rather than minimal.
 *
 * Dimming the LIGHTS instead — or as well — would be the obvious alternative
 * and is wrong here: the hero chest is lit by the same rig as the stage it is
 * leaving, so darkening the rig darkens the very thing the shot exists to show.
 * The veil dims strictly what sits behind the chest, and leaves it untouched.
 */
const VEIL_MATERIALS: Readonly<Record<string, MaterialSpec>> = Object.fromEntries(
  Array.from({ length: CHEST_TIMING.dimSteps }, (_, i): readonly [string, MaterialSpec] => [
    `Veil${i}`,
    { baseColor: [0.02, 0.03, 0.06, 1], opacity: ((i + 1) / CHEST_TIMING.dimSteps) * CHEST_TIMING.dimVeil },
  ]),
);

/** The veil rung for a darkness level in [0, 1], or null when fully clear. */
const veilMaterialOf = (level: number): string | null => {
  const rung = Math.ceil(clamp01(level) * CHEST_TIMING.dimSteps);
  return rung <= 0 ? null : `Veil${Math.min(CHEST_TIMING.dimSteps, rung) - 1}`;
};

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...REWARD_MATERIALS,
  ...CONFETTI_MATERIALS,
  // The beach margin around the inset lagoon. The shared StageFloor is a pale,
  // near-white cream ([0.94, 0.9, 0.82]) that under the bright warm key lifts to
  // milky bone — the reference sand is a rich, saturated golden tan. Override it
  // for THIS game only (the shared material stays neutral for the other casino
  // stages): pull the blue channel well down and widen the red→blue spread so the
  // warm rig lands the beach at golden sand rather than bleached cream. This is a
  // pure palette warm/saturation move — no grade/tonemap stage exists here.
  StageFloor: { baseColor: [0.9, 0.75, 0.47, 1] },
  // The lagoon surface itself. The shared pavilion turquoise
  // ([0.32, 0.78, 0.76]) is authored GREEN-leaning (G above B), and the warm key
  // ([1, 0.96, 0.88]) then multiplies blue down another ~12% relative to red —
  // so the champion pool lands on a flat sea-GREEN, while the reference lagoon
  // is a vivid CYAN-blue. With no grade/white-balance stage to correct hue after
  // the fact, the compensation has to be pre-baked into the base color: push
  // blue decisively ABOVE green and pull red down, so that after the warm rig
  // eats the blue the lit surface settles on the reference's caribbean cyan
  // rather than on pond green. Overridden for THIS game only — the other casino
  // stages keep the neutral pavilion turquoise.
  StageFloorAccent: { baseColor: [0.24, 0.7, 0.88, 1] },
  // Wood, value-stepped so the chest reads solid without a texture: the lid
  // catches the key light (lightest), the front boards sit mid, side boards go
  // darker, and the gaps between planks are the darkest brown. The ladder is
  // deliberately WIDE and pulled toward warm tan (rather than a uniform
  // saturated orange), because with no albedo texture the only thing carving
  // the chest into stacked planks is this value spread: a distinctly lighter
  // lid catching the key light, and near-black seams reading as the gaps
  // between boards — the carved-wood look of the reference lives in the step,
  // not the hue.
  // The champion ladder rendered as bleached pale pine under the bright warm
  // rig; the reference chests are saturated saddle-brown oak. The whole ramp is
  // pulled DARKER and WARMER (higher red-to-blue ratio) so that after the key
  // light lifts it, the lit faces land at rich caramel rather than milky tan —
  // the value spread that carves the planks is preserved, just seated on a
  // deeper, more saturated wood.
  WoodLid: { baseColor: [0.64, 0.44, 0.25, 1] },
  WoodBrown: { baseColor: [0.52, 0.34, 0.18, 1] },
  WoodSide: { baseColor: [0.38, 0.25, 0.13, 1] },
  WoodGap: { baseColor: [0.18, 0.1, 0.045, 1] },
  WoodDim: { baseColor: [0.28, 0.19, 0.11, 1] },
  WoodDimSide: { baseColor: [0.22, 0.15, 0.09, 1] },
  ChestInterior: { baseColor: [0.11, 0.07, 0.035, 1] },
  // Gold, likewise stepped: a bright highlight on upward edges/latch, the main
  // amber on front trim, a darker ochre on side-facing straps — so it reads as
  // metal catching light rather than glowing.
  //
  // The champion's gilding rendered as blown LEMON-WHITE, not gold: the widest
  // band on every chest measured (251, 248, 133) — green sitting at 0.99 of red,
  // i.e. essentially no chroma left — where the reference's brightest gold is a
  // saturated amber (246, 191, 54), green at 0.78 of red and blue at 0.22. Two
  // things caused it, and both are fixed here.
  //
  // First, the ladder was authored at the TOP of the range (red 0.98–1.0). With
  // no tonemap or grade stage in this engine, the bright warm key multiplies
  // straight into the clamp, so red pinned at 255 while green — starting at 0.9,
  // barely below red — pinned there too. Once two channels clamp together the
  // hue is gone by construction: the surface can only be white-ish. The ladder is
  // therefore seated LOWER (red ~0.8 at the top rung), leaving the key light room
  // to lift the gold to near-clip without flattening it.
  //
  // Second, the additive emissive on the top two rungs was doing the clamping,
  // not the lighting. It is removed: gold is not a light source, and a Lambert
  // amber under a warm key already reads as metal. (`GildBright` — the hover /
  // selected accent — keeps a small emissive: that rung is meant to read hotter
  // than lit gold, and it is now amber-biased so it goes hot-gold, not white.)
  //
  // Every rung's HUE is also corrected to the reference's amber ratio
  // (green ≈ 0.78 × red, blue ≈ 0.27 × red) instead of the old drift toward
  // 0.90/0.50, which was pale brass even before the clamp. The value STEP between
  // rungs (top > front > side > dim) is preserved — that step is what carves the
  // gilding into lit and shadowed metal without a texture.
  GildTop: { baseColor: [0.8, 0.62, 0.21, 1] },
  GildFront: { baseColor: [0.68, 0.53, 0.18, 1] },
  GildSide: { baseColor: [0.52, 0.4, 0.135, 1] },
  GildDim: { baseColor: [0.36, 0.28, 0.095, 1] },
  GildBright: { baseColor: [0.92, 0.71, 0.25, 1], emissive: [0.1, 0.07, 0, 1] },
  // Warm reveal light: a layered pool under the chosen chest, seam leak, inner
  // glow, and the burst — all additive-emissive translucent discs/slabs.
  PoolCore: { baseColor: [1, 0.86, 0.5, 1], emissive: [1, 0.78, 0.4, 1], opacity: 0.5 },
  PoolMid: { baseColor: [1, 0.84, 0.48, 1], emissive: [0.9, 0.66, 0.3, 1], opacity: 0.28 },
  PoolOuter: { baseColor: [1, 0.82, 0.46, 1], emissive: [0.8, 0.58, 0.24, 1], opacity: 0.14 },
  SeamGlow: { baseColor: [1, 0.9, 0.55, 1], emissive: [1, 0.82, 0.4, 1], opacity: 0.7 },
  InnerGlow: { baseColor: [1, 0.85, 0.5, 1], emissive: [0.72, 0.54, 0.26, 1], opacity: 0.7 },
  BurstGlow: { baseColor: [1, 0.92, 0.62, 1], emissive: [1, 0.85, 0.5, 1], opacity: 0.42 },
  BurstRay: { baseColor: [1, 0.9, 0.58, 1], emissive: [1, 0.82, 0.44, 1], opacity: 0.22 },
  Mote: { baseColor: [1, 0.95, 0.72, 1], emissive: [1, 0.9, 0.6, 1] },
  // The arcade stage: a turquoise platform with a rim, a warm central glow, and
  // a darker edge falloff — an intentional board, not a flat marker.
  // The pool's depth wall, carrying the same blue-over-green bias as the lagoon
  // surface above it: it is the SAME body of water seen edge-on, so if the
  // surface reads cyan and the wall reads sea-green the pool splits into two
  // different liquids at the rim. Darker and lower-red than the surface (it is
  // the shaded depth under the waterline), but the hue matches.
  PlatformSide: { baseColor: [0.07, 0.42, 0.58, 1] },
  EdgeVignette: { baseColor: [0.03, 0.2, 0.26, 1], opacity: 0.34 },
  // A gold accent, so it obeys the same amber ratio and the same
  // seated-below-the-clamp rule as the chest gilding above — a lemon-white rivet
  // ring around an amber-gilded chest grid would break the one metal the frame has.
  BoardRivet: { baseColor: [0.78, 0.6, 0.2, 1], emissive: [0.08, 0.055, 0, 1] },
  // Like every other translucent overlay here, the puff carries a little
  // emissive: a purely Lambert translucent grey reads as a dark blob against
  // the warm, brightly-lit chest mouth it coughs out of, which is the opposite
  // of the light, playful "nothing here this time" it is meant to be.
  DustPuff: { baseColor: [0.8, 0.75, 0.68, 1], emissive: [0.34, 0.31, 0.27, 1], opacity: 0.5 },
  // Beach set-dressing (palm, sandcastle, crab, shells) — value-stepped so each
  // prop reads as a chunky faceted assembly under the raking key, matching the
  // reference's toy-diorama shore. No emissive on the solid props (they are lit
  // by the same rig as the chests); the sand tones are pulled a touch lighter and
  // warmer than the floor slab so the castle and shore reads as dry sculpted sand.
  PalmBark: { baseColor: [0.48, 0.34, 0.2, 1] },
  PalmBarkDark: { baseColor: [0.37, 0.26, 0.15, 1] },
  PalmLeaf: { baseColor: [0.29, 0.53, 0.22, 1] },
  PalmLeafDark: { baseColor: [0.19, 0.4, 0.16, 1] },
  Coconut: { baseColor: [0.36, 0.26, 0.15, 1] },
  // Sand pulled a shade lighter and warmer than before: the raking key light
  // rakes the tower cylinders' side walls into deep shadow, and the old darker
  // ladder let those shadow faces sink to a muddy charcoal that read as a heavy
  // dark mass competing with the chests. A lighter, warmer sand keeps the
  // shadow side reading as dry sculpted sand — subordinate, not dominant.
  // The tower cylinders' shadow-side walls no longer carry a fake warm emissive
  // to stop them crushing to charcoal under the raking key: that was paint
  // compensating for light, and it glowed the castle uniformly (emissive is added
  // AFTER the albedo multiply, so it ignored the sand's own color and lifted the
  // door and dark base by the same absolute amount). The scene now authors a real
  // WARM AMBIENT (`ambient`, below), which lifts those faces through the albedo —
  // so a sand wall settles on dim sand and a dark base stays dark.
  CastleSand: { baseColor: [0.95, 0.87, 0.66, 1] },
  CastleSandDark: { baseColor: [0.87, 0.78, 0.57, 1] },
  CastleDoor: { baseColor: [0.42, 0.34, 0.22, 1] },
  CastlePole: { baseColor: [0.34, 0.24, 0.14, 1] },
  // A warm-gold trim stripe on the decorative castle pennant, tying it to the
  // chests' gilding — so it carries the same amber ratio and the same headroom
  // below the clamp, or the "tie" is to a gold the chests no longer wear.
  CastleFlagTrim: { baseColor: [0.78, 0.6, 0.2, 1], emissive: [0.06, 0.045, 0, 1] },
  // The crab reads as a coral beach creature, not a second brand accent: pulled
  // off the saturated brand red toward warm coral so the only true reds in frame
  // are the intentional branding surfaces.
  CrabShell: { baseColor: [0.85, 0.34, 0.24, 1] },
  CrabShellDark: { baseColor: [0.66, 0.24, 0.16, 1] },
  CrabEye: { baseColor: [0.06, 0.05, 0.05, 1] },
  // Shells/starfish shed the same emissive fakery for the same reason: the warm
  // ambient keeps these little shore pieces reading as pale shells catching the
  // sky rather than dark pebbles, without making them self-luminous.
  Shell: { baseColor: [0.96, 0.86, 0.8, 1] },
  Starfish: { baseColor: [0.92, 0.5, 0.29, 1] },
  // ── one consistent contact-shadow family ──────────────────────────────────
  // Every prop anchors to the ground with the same two translucent discs: a
  // wide SOFT rim and a smaller, darker CORE where the object actually meets the
  // ground. Warm-neutral and low-opacity so they read as soft grounding, never
  // as separate black cut-outs. A whisper of nothing else — no emissive — so
  // they only ever darken what is beneath them.
  ContactShadowSoft: { baseColor: [0.12, 0.1, 0.07, 1], opacity: 0.14 },
  ContactShadowCore: { baseColor: [0.1, 0.08, 0.06, 1], opacity: 0.26 },
  ...VEIL_MATERIALS,
};

/** The scene's resources for a given brand: the fixed chest/beach palette plus
 * the brand-derived banner/letter materials (whose colors follow the configured
 * brand). Built once at mount from the game's brand config — a brand color
 * change takes effect on the next mount, exactly like any other material. */
/**
 * The lagoon's radial facet budget, at full detail.
 *
 * The shared `cylinder` primitive is tessellated for a rivet, a coconut palm
 * trunk, a castle turret — the small round props this scene is mostly made of.
 * The lagoon is not one of those: it is a five-world-unit disc spanning a third
 * of the frame, and at the shared budget its shoreline reads as a hard polygon
 * where the reference has a smooth circle. So the pool declares its OWN mesh at
 * its own budget (see `MeshRef.segments`) — the big disc gets a round silhouette
 * without dragging every rivet and trunk in the scene up with it.
 *
 * It is twice `WATER_RIM_POINTS` deliberately: the software backend halves the
 * budget, so on Canvas2D the 3D shoreline lands on exactly the same facet count
 * as the 2D water overlay's projected clip path. The two boundaries are the same
 * circle, so the stylized net can never spill past the shore it is clipped to.
 */
const LAGOON_SEGMENTS = 96;

/** The mesh name the lagoon-scale discs draw with (see `LAGOON_SEGMENTS`). */
const LAGOON_MESH = "lagoon";

export const chestResources = (brand: BrandSpec): GameResources => ({
  materials: { ...MATERIALS, ...brandMaterials(brand) },
  meshes: {
    box: { kind: "box" },
    cylinder: { kind: "cylinder" },
    [LAGOON_MESH]: { kind: "cylinder", segments: LAGOON_SEGMENTS },
    sphere: { kind: "sphere" },
  },
});

// ── small builders ──────────────────────────────────────────────────────────────

/** A flat disc (thin cylinder) — pools, glows, platform layers. `mesh` selects
 * the tessellation: the default shared `cylinder` for the small light pools and
 * contact shadows, `LAGOON_MESH` for the frame-spanning water discs. */
const disc = (key: string, material: string, at: EngineVec3, radius: number, height = 0.02, mesh = "cylinder"): SceneInstance => ({
  key,
  material,
  mesh,
  transform: { position: at, rotation: QUAT_IDENTITY, scale: v3(radius * 2, height, radius * 2) },
});

// ── one directional light, one contact-shadow rule ──────────────────────────────

/**
 * The whole scene is lit by a single directional key (the `light:key` in
 * `stageLights`). Its ground-plane throw is the ONE direction every contact
 * shadow falls, so nothing looks lit from conflicting suns. Kept in lock-step
 * with the key light's `direction` below — change one, change the other.
 */
const KEY_LIGHT_DIR = v3(-0.6, -0.58, -0.5);
const SHADOW_DIR = ((): { readonly x: number; readonly z: number } => {
  const len = Math.hypot(KEY_LIGHT_DIR.x, KEY_LIGHT_DIR.z);
  return { x: KEY_LIGHT_DIR.x / len, z: KEY_LIGHT_DIR.z / len };
})();
/** How far the shadow slides down-light, as a fraction of its radius. */
const SHADOW_SLIDE = 0.26;
/** Just above the ground so the discs never z-fight the water/sand slab. */
const SHADOW_Y = 0.01;

/**
 * A soft directional contact shadow: a wide translucent rim slid a little
 * down-light, plus a smaller, darker CORE held at the object's actual footprint
 * so the point where it meets the ground reads darker than the outer falloff.
 * `radius` is the object's ground footprint; `spread` scales the whole shadow
 * (a ground-fade for a chest leaving the board, or a clarity boost for the hero
 * slot). Returns nothing once the object has lifted clear.
 */
const contactShadow = (keyPrefix: string, at: EngineVec3, radius: number, spread = 1, coreScale = 1): readonly SceneInstance[] => {
  const r = radius * spread;
  return r < 0.04
    ? []
    : [
        disc(`${keyPrefix}:soft`, "ContactShadowSoft", v3(at.x + SHADOW_DIR.x * r * SHADOW_SLIDE, SHADOW_Y, at.z + SHADOW_DIR.z * r * SHADOW_SLIDE), r, 0.008),
        disc(`${keyPrefix}:core`, "ContactShadowCore", v3(at.x + SHADOW_DIR.x * r * SHADOW_SLIDE * 0.5, SHADOW_Y + 0.002, at.z + SHADOW_DIR.z * r * SHADOW_SLIDE * 0.5), r * 0.6 * coreScale, 0.008),
      ];
};

/**
 * The barrel top is faceted into this many box slats. The engine's mesh
 * vocabulary is box / sphere / cylinder — there is no half-cylinder, and a full
 * cylinder sunk to show only its top half would expose its round underside the
 * moment the lid swings open. So the dome is an honest arc of flat slats, which
 * also sits right with the chunky faceted look of everything else here: each
 * slat catches the key light at its own angle, so the curve reads from shading
 * as well as silhouette. Eight slats span the 180° arc (~22.5° a facet) so the
 * crown reads as a smooth rounded barrel top rather than a peaked five-slat
 * ridge — the reference chests carry a full, round hump, not a tent.
 */
const LID_ARC_SLATS = 8;
/** Slats per lid arc on the software backend: half the facets still read as a
 * smooth barrel from the tabletop camera but cost half the geometry, across the
 * dome and both gold bands of every chest. */
const LID_ARC_SLATS_LOW = 4;
const LID_ARC_THICKNESS = 0.11;
/**
 * The reference chest's lid is NOT a plain barrel crossed by two thin gold
 * straps. It is a barrel whose two ENDS are raised WOOD ribs — chunky carved end
 * caps standing proud of a recessed centre panel, in a LIGHTER tan than the
 * panel they flank. That rib/panel step is the single strongest thing carving
 * the lid, and it is what the champion's inboard gold straps were standing in
 * for. So the arc pair moves to the lid's outer ends, widens ~3x, and swaps to
 * wood: the ribs get the light `woodLid`, the centre dome steps down to the
 * mid-tone body wood, and the value break lands exactly where the reference
 * puts it.
 */
const LID_RIB_WIDTH = 0.24;
/** How far the raised end ribs stand proud of the dome panel they flank. */
const LID_RIB_SWELL = 0.03;

/**
 * One arc of slats sweeping the lid's full depth, from its back edge up over
 * the crown and down to its front edge.
 *
 * Every slat is placed by its OWN chord: the two arc points bounding it give
 * the slat's center, its length, and its tilt directly, so the facets meet edge
 * to edge at any slat count and any `swell` without a seam to tune.
 *
 * The offset is rotated by the LID's quaternion while the slat carries the lid
 * rotation composed with its own tilt — so the whole arc is welded to the lid
 * board beneath it. It swings on the hinge when the lid opens, and it rides the
 * chest's yaw/pitch/spiral when the chest moves, exactly like every other part.
 */
const lidArc = (
  keyPrefix: string,
  hinge: EngineVec3,
  lidQ: EngineQuat,
  grow: number,
  material: string,
  width: number,
  swell: number,
  atX = 0,
): readonly SceneInstance[] => {
  const depthRadius = LID.z / 2 + swell;
  const riseRadius = CHEST_LID_ARCH - LID_ARC_THICKNESS / 2 + swell;
  const slats = lowDetail() ? LID_ARC_SLATS_LOW : LID_ARC_SLATS;
  // The arc's mid-thickness surface, swept as a half-ellipse over the lid board.
  const arcAt = (t: number): { readonly y: number; readonly z: number } => {
    const angle = -Math.PI / 2 + t * Math.PI;
    return { y: LID.y + riseRadius * Math.cos(angle), z: LID.z / 2 + depthRadius * Math.sin(angle) };
  };
  return Array.from({ length: slats }, (_, i): SceneInstance => {
    const from = arcAt(i / slats);
    const to = arcAt((i + 1) / slats);
    const dy = to.y - from.y;
    const dz = to.z - from.z;
    // A slat's local +Z runs along its chord. quatPitch(a) sends +Z to
    // (0, −sin a, cos a), so the tilt that aligns it with (dz, dy) is this.
    const tilt = Math.atan2(-dy, dz);
    return {
      key: `${keyPrefix}${i}`,
      material,
      mesh: "box",
      transform: {
        position: addV3(hinge, rotateByQuat(scaleV3(v3(atX, (from.y + to.y) / 2, (from.z + to.z) / 2), grow), lidQ)),
        rotation: quatMul(lidQ, quatPitch(tilt)),
        scale: scaleV3(v3(width, LID_ARC_THICKNESS, Math.hypot(dy, dz)), grow),
      },
    };
  });
};

interface ChestPose {
  /** The chest's GRID slot on the board — where its ground decor (warm pool,
   * focus ring) stays. Fixed for the whole round. */
  readonly origin: EngineVec3;
  /** Where the chest BODY actually is. Equal to `origin` (plus lift) while the
   * chest sits on the board, and somewhere along the spiral once it flies. */
  readonly at: EngineVec3;
  /** Flight progress in [0, 1]. Ground decor belongs to a chest that is ON the
   * board, so it fades out as this rises — an airborne chest pools no light on
   * a board it has left. */
  readonly flight: number;
  readonly yaw: number;
  readonly pitch: number;
  readonly squash: number;
  readonly scale: number;
  readonly lidAngle: number;
  readonly latchAngle: number;
  readonly dim: boolean;
  readonly selected: boolean;
  readonly focusRing: boolean;
  readonly hoverRing: boolean;
  readonly seam: number;
  readonly glow: number;
  /** The brand name stamped across the chest front, welded to this pose. */
  readonly brandName: string;
  /** Whether this chest wears the raised brand NAMEPLATE (gold frame + colored
   * plate + lettering). Only the center featured chest does; the rest stay bare
   * so the one plaque reads as the hero marker instead of nine competing labels. */
  readonly nameplate: boolean;
}

/** All instances of one posed chest (body, planks, gilding, latch, lid,
 * selection pool, seam light). Materials are chosen by facing (front/side/top)
 * and by pose state (dim / selected) rather than by texture. */
const chestInstances = (key: string, pose: ChestPose): readonly SceneInstance[] => {
  // Tilt toward the camera (a small back-pitch) when chosen; yaw carries idle sway.
  const q = quatMul(quatYaw(pose.yaw), quatPitch(-pose.pitch));
  const squashY = 1 - pose.squash;
  const squashXZ = 1 + pose.squash * 0.55;
  const grow = pose.scale;
  const origin = pose.at;
  // On the software backend, shed the finest chest detail — the three groove
  // lines and half the lid-arc slats (see `lidArc`). The nameplate is governed by
  // `pose.nameplate`, not by the backend.
  const low = lowDetail();
  // How much of the chest is still "on the board" — gates every ground-anchored
  // decoration below.
  const grounded = 1 - clamp01(pose.flight);

  const wood = pose.dim ? "WoodDim" : "WoodBrown";
  const woodSide = pose.dim ? "WoodDimSide" : "WoodSide";
  const woodLid = pose.dim ? "WoodDim" : "WoodLid";
  // Front trim brightens on hover/selection.
  const trimFront = pose.dim ? "GildDim" : pose.selected || pose.hoverRing ? "GildBright" : "GildFront";
  const trimSide = pose.dim ? "GildDim" : "GildSide";
  const trimTop = pose.dim ? "GildDim" : "GildTop";

  const part = (suffix: string, local: EngineVec3, scale: EngineVec3, material: string, extraQ = QUAT_IDENTITY): SceneInstance => ({
    key: `${key}:${suffix}`,
    material,
    mesh: "box",
    transform: {
      position: addV3(origin, rotateByQuat(v3(local.x * squashXZ * grow, local.y * squashY * grow, local.z * squashXZ * grow), q)),
      rotation: quatMul(q, extraQ),
      scale: v3(scale.x * squashXZ * grow, scale.y * squashY * grow, scale.z * squashXZ * grow),
    },
  });

  // Lid on its back hinge; latch hangs from the lid's front lip.
  const lidQ = quatMul(q, quatPitch(pose.lidAngle));
  const lidHingeLocal = v3(0, BODY.y, -BODY.z / 2);
  const lidHinge = addV3(origin, rotateByQuat(v3(lidHingeLocal.x * grow, lidHingeLocal.y * squashY * grow, lidHingeLocal.z * squashXZ * grow), q));
  const lid: SceneInstance = {
    key: `${key}:lid`,
    material: woodLid,
    mesh: "box",
    transform: hingedTransform(lidHinge, scaleV3(v3(0, LID.y / 2, LID.z / 2), grow), lidQ, scaleV3(LID, grow)),
  };
  // The chest's ONE piece of heavy gilding: a thick gold RAIL along the lid's
  // front lip. In the reference this is the chest's defining metal — a deep
  // plinth-like band that straddles the lid/body seam and stands well proud of
  // the body front, with the hasp ring hanging off it. The champion's rim was a
  // 0.13-tall, 0.05-deep hairline that read as a thin yellow sliver; this is
  // ~1.6x taller and 2x deeper, and it is dropped so its lower half covers the
  // top of the body front, which is exactly where the reference's gold sits.
  const lidRim: SceneInstance = {
    key: `${key}:lidrim`,
    material: trimTop,
    mesh: "box",
    transform: hingedTransform(lidHinge, scaleV3(v3(0, LID.y / 2 - 0.03, LID.z - 0.03), grow), lidQ, scaleV3(v3(LID.x + 0.05, 0.21, 0.1), grow)),
  };
  // The barrel top: a mid-tone centre panel flanked by two raised, lighter WOOD
  // end ribs at the lid's outer ends (see `LID_RIB_WIDTH`). The ribs are placed
  // so their outer face is flush with the lid end, and they swell above the
  // panel so the step reads in silhouette as well as in value.
  const dome = lidArc(`${key}:dome`, lidHinge, lidQ, grow, wood, LID.x, 0);
  const ribs = [-1, 1]
    .map((side) =>
      lidArc(`${key}:rib${side < 0 ? "L" : "R"}`, lidHinge, lidQ, grow, woodLid, LID_RIB_WIDTH, LID_RIB_SWELL, (side * (LID.x - LID_RIB_WIDTH)) / 2),
    )
    .flat();

  // The hasp: in the reference this is a big HOLLOW square ring hanging off the
  // gold rail onto the dark body front — a buckle you can see daylight through,
  // and after the rail it is the chest's most recognisable piece of hardware.
  // The champion had it as one small solid tab, which read as a nub. Four bars
  // make the ring (the engine has no torus and no CSG, so a square ring IS the
  // primitive-honest form — and it is the shape the reference actually draws).
  // It wears `trimFront` rather than `trimTop`: it faces the camera, and it is
  // now the front-metal that brightens on hover, which is the job the deleted
  // `plate` used to do — with far more surface to read it on.
  const latchQ = quatMul(lidQ, quatPitch(pose.latchAngle));
  const latchHinge = addV3(lidHinge, rotateByQuat(scaleV3(v3(0, 0.02, LID.z - 0.01), grow), lidQ));
  // Measured off the reference: the ring is 0.27 of the chest's width across and
  // ~0.82 as tall as it is wide, with bars ~0.21 of its width. `haspDrop` hangs
  // it below the hinge so its top bar tucks BEHIND the rail's lower edge and the
  // rest of the ring reads clear against the dark body front, and the bars sit a
  // full `LATCH.z` forward so they bite into the rail's front face and stand
  // proud of it instead of landing coplanar with it.
  const haspW = LID.x * 0.27;
  const haspH = haspW * 0.82;
  const haspBar = haspW * 0.21;
  const haspDrop = 0.06;
  const haspPart = (suffix: string, x: number, y: number, w: number, h: number): SceneInstance => ({
    key: `${key}:${suffix}`,
    material: trimFront,
    mesh: "box",
    transform: hingedTransform(latchHinge, scaleV3(v3(x, y - haspDrop, LATCH.z), grow), latchQ, scaleV3(v3(w, h, LATCH.z), grow)),
  });
  const hasp: readonly SceneInstance[] = [
    haspPart("latch", 0, -haspBar / 2, haspW, haspBar),
    haspPart("latchL", -(haspW - haspBar) / 2, -haspH / 2, haspBar, haspH),
    haspPart("latchR", (haspW - haspBar) / 2, -haspH / 2, haspBar, haspH),
    haspPart("latchB", 0, -haspH + haspBar / 2, haspW, haspBar),
  ];

  const interior: SceneInstance = part("interior", v3(0, BODY.y - 0.03, 0), v3(BODY.x - 0.1, 0.05, BODY.z - 0.1), "ChestInterior");
  const glow: SceneInstance[] =
    pose.glow > 0
      ? [part("glow", v3(0, BODY.y - 0.02, 0), scaleV3(v3(BODY.x - 0.2, 0.22, BODY.z - 0.2), pose.glow), "InnerGlow")]
      : [];

  // Warm light pool under a chosen chest: three layered translucent discs read
  // as a soft radial gradient rather than a flat marker.
  const pool: SceneInstance[] =
    (pose.glow > 0 || pose.selected) && grounded > 0.02
      ? [
          disc(`${key}:pool2`, "PoolOuter", v3(pose.origin.x, 0.02, pose.origin.z), BODY.x * (1.05 + pose.glow * 0.3) * grounded, 0.012),
          disc(`${key}:pool1`, "PoolMid", v3(pose.origin.x, 0.028, pose.origin.z), BODY.x * (0.78 + pose.glow * 0.22) * grounded, 0.012),
          disc(`${key}:pool0`, "PoolCore", v3(pose.origin.x, 0.036, pose.origin.z), BODY.x * (0.5 + pose.glow * 0.18) * grounded, 0.012),
        ]
      : [];

  // Directional contact shadow anchoring the chest to the board — the same
  // down-light rule every prop obeys. It shrinks with the chest as it lifts off
  // on the hero flight (`grounded`).
  const shadow: readonly SceneInstance[] = grounded > 0.02 ? contactShadow(`${key}:shadow`, pose.origin, BODY.x * 0.52, grounded, 1) : [];

  // Warm seam light leaking from the lid/body join before it fully opens. It
  // hangs just BELOW and IN FRONT OF the gold rail: the rail is now a deep band
  // straddling the seam, so a glow sitting on the old seam line would be buried
  // behind it for the whole "seam" beat (which plays with the lid still shut).
  // Leaking out from under the rail's lower lip is both visible and the honest
  // reading of where light escapes a lipped chest.
  const seam: SceneInstance[] =
    pose.seam > 0
      ? [
          {
            key: `${key}:seam`,
            material: "SeamGlow",
            mesh: "box",
            transform: {
              position: addV3(origin, rotateByQuat(v3(0, (BODY.y - 0.1) * grow, (BODY.z / 2 + 0.075) * grow), q)),
              rotation: q,
              scale: v3((LID.x - 0.06) * grow, (0.02 + pose.seam * 0.14) * grow, 0.05 * grow),
            },
          },
        ]
      : [];

  // Hover/focus feedback is a soft warm pool, not a flat gold marker: an active
  // pointer/tap hover reads a touch brighter than a resting keyboard cursor.
  const ringBase = v3(pose.origin.x, 0.024, pose.origin.z);
  const rings: SceneInstance[] = pose.hoverRing
    ? [disc(`${key}:ring1`, "PoolMid", ringBase, BODY.x * 1.05, 0.012), disc(`${key}:ring0`, "PoolCore", ringBase, BODY.x * 0.62, 0.012)]
    : pose.focusRing
      ? [disc(`${key}:ring`, "PoolOuter", ringBase, BODY.x * 1.0, 0.012)]
      : [];

  // A brand NAMEPLATE mounted on the chest LID crown, facing up so it reads
  // clearly from the tabletop camera (which looks down on the lid). It is a raised
  // plaque — a gold frame under a brand-colored plate — carrying the brand name in
  // the on-primary color, NOT letters laid straight on the wood.
  //
  // CRUCIAL: it is welded to the LID frame (lidHinge + lidQ), exactly like the
  // dome, gold bands, rim and latch above — NOT to the body (origin, q). The
  // plaque sits ON the lid, so it must ride the lid: when the lid swings open on
  // its hinge the whole nameplate lifts and tilts back WITH it, instead of hanging
  // in place over the opening. (Parenting it to the body was the bug — the plaque
  // is lid furniture, so it belongs in the lid's frame.) It scales by `grow`, the
  // lid's own convention. `plateOrient` lays it flat on the crown: its normal is
  // the lid's local +Y, its reading direction the lid's +X, its "up" toward the
  // lid's −Z so it reads top-away from the camera. Long names shrink (label.ts).
  const plateOrient = quatMul(lidQ, quatPitch(-Math.PI / 2));
  const plateBasis = v3(grow, grow, grow);
  // The dome's outer crown in LID-LOCAL space (relative to the hinge): the highest
  // point of the arch (y = lid board + arch height) at mid-depth (z = LID.z/2),
  // lifted a hair so the plaque rests ON the crown rather than sinking into it.
  const crownLid = v3(0, LID.y + CHEST_LID_ARCH + 0.015, LID.z / 2);
  const crownAnchor = addV3(lidHinge, rotateByQuat(scaleV3(crownLid, grow), lidQ));
  // A flat box on the plaque frame: `size`/`offset` are in oriented-local units
  // (x across, y along the lid depth, z up off the lid), scaled by `plateBasis`.
  const platePart = (suffix: string, size: EngineVec3, offset: EngineVec3, material: string): SceneInstance => ({
    key: `${key}:${suffix}`,
    material,
    mesh: "box",
    transform: {
      position: addV3(crownAnchor, rotateByQuat(v3(offset.x * plateBasis.x, offset.y * plateBasis.y, offset.z * plateBasis.z), plateOrient)),
      rotation: plateOrient,
      scale: v3(size.x * plateBasis.x, size.y * plateBasis.y, size.z * plateBasis.z),
    },
  });
  const plateW = BODY.x * 0.82;
  const plateH = 0.5;
  // Only the center featured chest wears the nameplate — frame, plate, and
  // lettering; the eight around it stay bare carved-wood chests, so the one
  // plaque reads as the hero marker rather than nine competing labels. It uses the
  // same front gild as every other chest — the plate IS the distinction, not a
  // brighter highlight, so the center never reads as "always selected". The label
  // is stamped on BOTH backends here (no longer shed by the Canvas2D `low` LOD): a
  // single plaque is cheap, and it is the one piece of lettering the player reads.
  const plaque = pose.nameplate
    ? [
        platePart("plaqueframe", v3(plateW + 0.1, plateH + 0.1, 0.05), v3(0, 0, 0.0), pose.dim ? "GildDim" : "GildFront"),
        platePart("plaque", v3(plateW, plateH, 0.06), v3(0, 0, 0.035), pose.dim ? "BrandPrimaryDim" : "BrandPrimary"),
      ]
    : [];
  const label = pose.nameplate
    ? stampText(
        `${key}:brand`,
        pose.brandName,
        { basis: plateBasis, center: v3(0, 0, 0), orient: plateOrient, origin: crownAnchor },
        { depth: 0.02, height: 0.3, lift: 0.08, material: pose.dim ? "BrandLetterDim" : "BrandLetterOnPrimary", maxWidth: BODY.x * 0.72 },
      )
    : [];

  return [
    ...shadow,
    ...pool,
    part("body", v3(0, BODY.y / 2, 0), BODY, wood),
    // Board gap lines (darkest) read as separate planks without a texture. The
    // reference chest bodies are carved into a stack of four distinct boards, so
    // three evenly-spaced grooves divide the face rather than two — and each
    // groove stands a touch prouder and thicker than a hairline so the near-black
    // seam actually reads as the gap between planks under the bright rig, which is
    // the only thing carving the untextured wood into stacked boards.
    ...(low
      ? []
      : [
          part("gap1", v3(0, BODY.y * 0.26, 0), v3(BODY.x + 0.02, 0.034, BODY.z + 0.02), "WoodGap"),
          part("gap2", v3(0, BODY.y * 0.5, 0), v3(BODY.x + 0.02, 0.034, BODY.z + 0.02), "WoodGap"),
          part("gap3", v3(0, BODY.y * 0.74, 0), v3(BODY.x + 0.02, 0.034, BODY.z + 0.02), "WoodGap"),
        ]),
    // Side-facing wood on the end caps for a value step.
    part("endL", v3(-BODY.x / 2 + 0.02, BODY.y / 2, 0), v3(0.04, BODY.y - 0.04, BODY.z - 0.04), woodSide),
    part("endR", v3(BODY.x / 2 - 0.02, BODY.y / 2, 0), v3(0.04, BODY.y - 0.04, BODY.z - 0.04), woodSide),
    // Gilding: corner brackets only. The reference body carries NO inboard
    // vertical straps and NO small separate lock plate — its front is a plain
    // dark panel, framed by two corner brackets, capped by the gold rail above,
    // with the big hasp ring hanging over it. The champion's two thin ochre
    // straps and its 0.26-wide plate were both inventions that broke that read
    // (the straps in particular sliced the front into strips the reference does
    // not have), so both are gone and the corner brackets are thickened to the
    // chunky posts the reference draws. Net instance cost of this whole
    // re-model is zero: three parts removed here pay for the three extra hasp
    // bars above.
    part("edgeL", v3(-BODY.x / 2 + 0.01, BODY.y / 2, BODY.z / 2), v3(0.09, BODY.y + 0.02, 0.09), trimSide),
    part("edgeR", v3(BODY.x / 2 - 0.01, BODY.y / 2, BODY.z / 2), v3(0.09, BODY.y + 0.02, 0.09), trimSide),
    ...plaque,
    ...label,
    interior,
    ...glow,
    ...seam,
    lid,
    ...dome,
    ...ribs,
    lidRim,
    ...hasp,
    ...rings,
  ];
};

// ── the light burst (bounded: soft glow + a few rays + a few motes) ─────────────

const lightBurst = (at: EngineVec3, tick: number, t: number, s: number): readonly SceneInstance[] => {
  // t is burst progress 0→1 over the burst window; intensity peaks early, fades.
  // `s` scales the whole figure with the hero chest it erupts from.
  const strength = pulse(t);
  if (strength <= 0.001) {
    return [];
  }
  const rise = (0.2 + t * 0.9) * s;
  const glow: SceneInstance = disc("burst:glow", "BurstGlow", v3(at.x, at.y + 0.05 * s, at.z), (0.35 + strength * 0.9) * s, 0.02);
  const rays = [0, 1, 2, 3, 4].map((i) => {
    const a = (i / 5) * Math.PI * 2 + tick * 0.02;
    const spread = 0.28 * strength * s;
    return {
      key: `burst:ray${i}`,
      material: "BurstRay",
      mesh: "box",
      transform: {
        position: v3(at.x + Math.cos(a) * spread, at.y + (0.3 * s + rise * 0.5), at.z + Math.sin(a) * spread),
        rotation: quatYaw(a),
        scale: scaleV3(v3(0.05 + strength * 0.05, 0.5 + strength * 1.3, 0.05 + strength * 0.05), s),
      },
    } satisfies SceneInstance;
  });
  const motes = Array.from({ length: CHEST_TIMING.burstParticles }, (_, i) => {
    const a = (i / CHEST_TIMING.burstParticles) * Math.PI * 2 + i * 1.3;
    const r = (0.15 + (i % 3) * 0.12) * (0.4 + t) * s;
    const climb = rise * (0.6 + (i % 4) * 0.18);
    const size = (0.05 + (i % 2) * 0.02) * strength * s;
    return {
      key: `burst:mote${i}`,
      material: "Mote",
      mesh: "sphere",
      transform: { position: v3(at.x + Math.cos(a) * r, at.y + 0.25 * s + climb, at.z + Math.sin(a) * r), rotation: QUAT_IDENTITY, scale: v3(size, size, size) },
    } satisfies SceneInstance;
  });
  return [glow, ...rays, ...motes];
};

// ── the hero prize (rises fully clear of the chest, large, spinning, pulsing) ──

/**
 * The prize the winning chest yields — a big spinning rarity gem that climbs
 * fully out to hover as the frame's focal point, with a settle bob, a size
 * pulse, and a pulsing halo behind it. `at` is the chest's open mouth; `riseT`
 * the climb progress; `settle` ramps in the idle bob/pulse once it has arrived.
 */
const heroPrize = (rarity: Parameters<typeof rewardMaterialOf>[0], at: EngineVec3, riseT: number, tick: number, settle: number, s: number): readonly SceneInstance[] => {
  const material = rewardMaterialOf(rarity);
  // The climb and the gem both scale with the hero chest, but DAMPED: at full
  // hero scale an undamped rise would carry the prize straight out of frame.
  const rise = s * CHEST_TIMING.riseDamp;
  const gem = s * CHEST_TIMING.prizeDamp;
  const climb = CHEST_TIMING.riseHeight * easeOutBack(riseT) * rise;
  const bob = Math.sin(tick * 0.12) * 0.035 * settle * gem;
  const center = v3(at.x, at.y + climb + bob, at.z);
  const rarityBonus = rarity === "jackpot" ? 0.18 : rarity === "rare" ? 0.1 : 0;
  const size = (0.54 + rarityBonus) * (0.5 + 0.5 * riseT) * (1 + Math.sin(tick * 0.16) * 0.04 * settle) * gem;
  const halo = 0.82 * (0.5 + 0.5 * riseT) * (0.9 + Math.sin(tick * 0.14) * 0.12 * settle) * gem;
  const spin = quatYaw(tick * 0.04);
  return [
    disc("reward:halo", "BurstGlow", v3(center.x, center.y, center.z + 0.001), halo, 0.02),
    { key: "reward:core", material, mesh: "sphere", transform: { position: center, rotation: spin, scale: v3(size, size, size) } },
    { key: "reward:facet", material, mesh: "box", transform: { position: center, rotation: quatYaw(tick * 0.04 + 0.7), scale: v3(size * 0.72, size * 0.72, size * 0.72) } },
  ];
};

// ── the arcade platform (rim, central glow, edge falloff, corner rivets) ────────

/**
 * Radius of the turquoise lagoon the chests sit on. In the reference the water
 * is a small rounded pool with a wide golden-sand beach all around it, not a
 * full-frame flood — so the disc is sized only a little larger than the nine
 * chests it holds (whose grid reaches ~3.6 world-units from center), leaving a
 * broad ring of the sandy `stageRoom` floor showing around the pool. The other
 * discs (edge vignette, center glow) and the rim rivets keep their original
 * proportions relative to this radius.
 */
export const WATER_RADIUS = 5.0;

const platform = (): readonly SceneInstance[] => [
  // Both water discs are lagoon-scale, so both draw the high-tessellation mesh:
  // the vignette is the OUTER of the two, and a faceted vignette under a round
  // pool would just move the polygon out to the sand line.
  disc("plat:vignette", "EdgeVignette", v3(0, -0.048, 0), WATER_RADIUS * (9 / 8.4), 0.006, LAGOON_MESH),
  disc("plat:side", "PlatformSide", v3(0, -0.062, 0), WATER_RADIUS, 0.06, LAGOON_MESH),
  // No central warm glow: pooling warmth at the middle brightened the center chest
  // and made it read as permanently highlighted. The lagoon stays evenly lit.
  ...[
    [-1, -1],
    [1, -1],
    [-1, 1],
    [1, 1],
  ].map(([sx, sz], i) => ({
    key: `plat:rivet${i}`,
    material: "BoardRivet",
    mesh: "cylinder" as const,
    transform: { position: v3((sx ?? 0) * WATER_RADIUS * (6.7 / 8.4), -0.02, (sz ?? 0) * WATER_RADIUS * (6.7 / 8.4)), rotation: QUAT_IDENTITY, scale: v3(0.34, 0.05, 0.34) },
  })),
];

// ── the beach set-dressing (palm, sandcastle, crab, shells) ─────────────────────

/*
 * The reference stages the lagoon inside a lived-in cartoon beach: a leaning palm
 * at the far left, a turreted sandcastle flying a red flag at the far right, a
 * little red crab on the near sand, and shells/starfish dotted around the shore.
 * The champion left that sand bare, so the frame read as a lone pool of chests.
 *
 * None of it needs a new primitive — every prop is an assembly of the same box /
 * cylinder / sphere vocabulary the chests are built from, placed ONCE on the sand
 * ring OUTSIDE the water disc (radius > the vignette so nothing floats on the
 * lagoon) so the decor frames the pool the way the reference does. It is purely
 * static cosmetic dressing: it reads neither the outcome nor the tick, and sits
 * behind the veil so a hero reveal still dims it away with the rest of the stage.
 */

/** A single decor box/cylinder/sphere at a world position. */
const decorPart = (
  key: string,
  material: string,
  mesh: "box" | "cylinder" | "sphere",
  position: EngineVec3,
  scale: EngineVec3,
  rotation: EngineQuat = QUAT_IDENTITY,
): SceneInstance => ({ key, material, mesh, transform: { position, rotation, scale } });

/** A leaning palm swaying in the wind: a curved stack of tapering bark cylinders,
 * a coconut cluster, and a fan of drooping frond boards radiating from the crown.
 * `tick` drives a gentle whole-crown sway (bend grows with height, so the trunk
 * arcs and the crown leads) plus a faster per-frond flutter — a pure function of
 * the tick via `palmSway`, so it can never correlate with the outcome. */
const PALM_CROWN_Y = 2.66;
const palmTree = (origin: EngineVec3, tick: number): readonly SceneInstance[] => {
  const sway = palmSway(tick);
  const segs = [
    { y: 0.4, x: 0.0, r: 0.34, tilt: 0.04, mat: "PalmBarkDark" },
    { y: 1.08, x: 0.12, r: 0.3, tilt: 0.12, mat: "PalmBark" },
    { y: 1.74, x: 0.3, r: 0.26, tilt: 0.22, mat: "PalmBarkDark" },
    { y: 2.34, x: 0.56, r: 0.22, tilt: 0.34, mat: "PalmBark" },
  ];
  // Bend scales with height so the base stays planted and the crown travels most.
  const bendAt = (y: number): number => sway.bend * (y / PALM_CROWN_Y) ** 1.6;
  const trunk = segs.map((s, i) =>
    decorPart(
      `palm:trunk${i}`,
      s.mat,
      "cylinder",
      addV3(origin, v3(s.x + bendAt(s.y) * 1.4, s.y, 0)),
      v3(s.r * 2, 0.72, s.r * 2),
      quatRoll(-s.tilt - bendAt(s.y)),
    ),
  );
  const crown = addV3(origin, v3(0.74 + sway.bend * 1.4, PALM_CROWN_Y, 0));
  // The whole crown rolls with the wind, carrying the coconuts and frond bases.
  const crownRoll = quatRoll(-sway.bend);
  const coconuts = [v3(-0.14, -0.04, 0.12), v3(0.12, -0.02, -0.14), v3(-0.02, -0.16, -0.02)].map((d, i) =>
    decorPart(`palm:coco${i}`, "Coconut", "sphere", addV3(crown, rotateByQuat(d, crownRoll)), v3(0.2, 0.2, 0.2)),
  );
  const fronds = Array.from({ length: 7 }, (_, i): SceneInstance => {
    const a = (i / 7) * Math.PI * 2;
    const droop = 0.55 + (i % 2) * 0.12 + sway.flutter(i);
    const q = quatMul(crownRoll, quatMul(quatYaw(a), quatPitch(droop)));
    const len = 1.5 + (i % 3) * 0.14;
    return decorPart(
      `palm:frond${i}`,
      i % 2 === 0 ? "PalmLeaf" : "PalmLeafDark",
      "box",
      addV3(crown, rotateByQuat(v3(0, 0.05, len / 2), q)),
      v3(0.34, 0.09, len),
      q,
    );
  });
  return [...contactShadow("palm:shadow", origin, 0.62), ...trunk, ...coconuts, ...fronds];
};

/** Yaw of the whole sandcastle so its square base runs parallel to the diagonal
 * shore line it sits behind (rather than square to the world axes). Clockwise
 * from the top-down view. */
const CASTLE_YAW = -1.05;

/** The castle is scaled down from its authored size so it reads as a secondary
 * beach prop framing the pool rather than a heavy mass competing with the chest
 * grid — the same "peripheral props stay subordinate" rule the whole pass obeys.
 * Applied uniformly to every local offset AND scale, so the assembly shrinks
 * about its origin without changing its proportions. */
const CASTLE_SCALE = 0.82;

/** A turreted sandcastle: a broad base, a central keep with two flanking turrets,
 * crenellations, an arched door, and a simple decorative pennant (brand colors,
 * no logo). The whole assembly is yawed by `CASTLE_YAW` and scaled by
 * `CASTLE_SCALE` about its origin so it lines up with the shore and stays
 * secondary to the chests. */
const sandcastle = (origin: EngineVec3): readonly SceneInstance[] => {
  const q = quatYaw(CASTLE_YAW);
  // Place a part given in castle-local space: scale it down about the origin,
  // rotate its offset into the yawed frame, and compose the yaw into its own
  // rotation, so the castle turns and shrinks as one.
  const place = (key: string, material: string, mesh: "box" | "cylinder", local: EngineVec3, scale: EngineVec3): SceneInstance =>
    decorPart(key, material, mesh, addV3(origin, rotateByQuat(scaleV3(local, CASTLE_SCALE), q)), scaleV3(scale, CASTLE_SCALE), q);
  const base = place("castle:base", "CastleSandDark", "box", v3(0, 0.28, 0), v3(2.4, 0.56, 2.0));
  const towers = [
    { key: "keep", x: 0, r: 0.52, h: 1.7, mat: "CastleSand" },
    { key: "turnL", x: -0.92, r: 0.34, h: 1.15, mat: "CastleSandDark" },
    { key: "turnR", x: 0.92, r: 0.34, h: 1.15, mat: "CastleSandDark" },
  ];
  const towerParts = towers
    .map((t): readonly SceneInstance[] => {
      const top = 0.56 + t.h;
      const shaft = place(`castle:${t.key}`, t.mat, "cylinder", v3(t.x, 0.56 + t.h / 2, 0), v3(t.r * 2, t.h, t.r * 2));
      const crenels = Array.from({ length: 6 }, (_, i): SceneInstance => {
        const a = (i / 6) * Math.PI * 2;
        return place(
          `castle:${t.key}cren${i}`,
          "CastleSand",
          "box",
          v3(t.x + Math.cos(a) * t.r * 0.82, top + 0.11, Math.sin(a) * t.r * 0.82),
          v3(0.16, 0.22, 0.16),
        );
      });
      return [shaft, ...crenels];
    })
    .flat();
  const door = place("castle:door", "CastleDoor", "box", v3(0, 0.5, 1.0), v3(0.42, 0.62, 0.08));
  const poleTop = 0.56 + 1.7;
  const pole = place("castle:pole", "CastlePole", "cylinder", v3(0, poleTop + 0.42, 0), v3(0.05, 0.84, 0.05));
  // A simple decorative pennant flying the brand colors — a warm-red flag with a
  // gold trim stripe along the pole, and NO logo or lettering. It reads as
  // festive beach dressing that shares the branding palette, not a second sign.
  const flag = place("castle:flag", "BrandPrimary", "box", v3(0.24, poleTop + 0.72, 0), v3(0.5, 0.24, 0.03));
  const flagTrim = place("castle:flagtrim", "CastleFlagTrim", "box", v3(0.24, poleTop + 0.56, 0), v3(0.5, 0.08, 0.035));
  return [...contactShadow("castle:shadow", origin, 1.28 * CASTLE_SCALE), base, ...towerParts, door, pole, flag, flagTrim];
};

/** A stubby cartoon crab with a small set of idle animations: a domed shell, two
 * eyestalks, two front claws, and a row of little legs down each side. `crabIdle`
 * elects one bit of business (scuttle / claw wave / bob / turn) or a rest on a
 * random interval from the ambient stream; here every part is placed through the
 * resulting body frame so the crab scoots, bobs, turns, waves, and breathes as
 * one creature. Pure in (tick, seed) — outcome-independent. */
const crab = (origin: EngineVec3, tick: number, seed: number): readonly SceneInstance[] => {
  const pose = crabIdle(tick, seed);
  const bodyQ = quatYaw(pose.yaw);
  const bodyShift = v3(pose.scootX, pose.bob, 0);
  // Place a part given in body-local space: rotate its offset into the (turned)
  // body frame, add the whole-body scoot/bob, and compose the body yaw into its
  // own rotation, so one pose moves the crab as a single creature.
  const place = (key: string, material: string, mesh: "box" | "sphere", local: EngineVec3, scale: EngineVec3, localRot: EngineQuat = QUAT_IDENTITY): SceneInstance =>
    decorPart(key, material, mesh, addV3(origin, addV3(bodyShift, rotateByQuat(local, bodyQ))), scale, quatMul(bodyQ, localRot));
  const body = place("crab:body", "CrabShell", "sphere", v3(0, 0.2, 0), v3(0.62, 0.4 * (1 + pose.breath), 0.5));
  const eyes = [-1, 1]
    .map((s): readonly SceneInstance[] => [
      place(`crab:stalk${s}`, "CrabShell", "box", v3(s * 0.14, 0.44, 0.16), v3(0.06, 0.18, 0.06), quatRoll(-s * pose.eye)),
      place(`crab:eye${s}`, "CrabEye", "sphere", v3(s * 0.14 + s * pose.eye * 0.12, 0.55, 0.16), v3(0.1, 0.1, 0.1)),
    ])
    .flat();
  const claws = [-1, 1]
    .map((s): readonly SceneInstance[] => {
      // Each claw lifts and snaps on its own phase, so a wave alternates sides.
      const lift = pose.clawLift * (0.7 + 0.3 * Math.sin(tick * 0.5 + (s > 0 ? 0 : Math.PI)));
      return [
        place(`crab:arm${s}`, "CrabShellDark", "box", v3(s * 0.42, 0.18 + lift * 0.12, 0.24), v3(0.1, 0.09, 0.28), quatRoll(s * lift)),
        place(`crab:claw${s}`, "CrabShell", "sphere", v3(s * 0.5, 0.18 + lift * 0.3, 0.42), v3(0.22, 0.18, 0.2), quatRoll(s * lift)),
      ];
    })
    .flat();
  const legs = [-1, 1]
    .map((s): readonly SceneInstance[] =>
      [-0.16, 0.02, 0.2].map((z, i) => {
        const wiggle = pose.legWiggle * Math.sin(tick * 0.7 + i * 1.2);
        return place(`crab:leg${s}_${i}`, "CrabShellDark", "box", v3(s * 0.38, 0.08, z), v3(0.24, 0.06, 0.07), quatYaw(s * 0.5 + s * wiggle));
      }),
    )
    .flat();
  // A little brand pennant on a pole, raised in the crab's right claw — welded to
  // the body frame, so it scoots and turns with the crab.
  const flagPole = place("crab:flagpole", "BrandPost", "box", v3(0.58, 0.5, 0.34), v3(0.04, 0.7, 0.04));
  const flag = place("crab:flag", "BrandPrimary", "box", v3(0.74, 0.66, 0.34), v3(0.3, 0.2, 0.03));
  // The shadow follows the crab's side-scuttle (the horizontal scoot) but not its
  // vertical bob, so it stays planted on the sand as the little creature hops.
  const shadow = contactShadow("crab:shadow", addV3(origin, v3(pose.scootX, 0, 0)), 0.5);
  return [...shadow, body, ...eyes, ...claws, ...legs, flagPole, flag];
};

/*
 * The shore litter. The reference does NOT leave the sand ring bare: it is the
 * single most heavily-dressed surface in the frame, speckled all the way round
 * the lagoon with small pale pebbles, a handful of ridged clam shells, and a few
 * starfish — roughly twenty pieces, densest along the wide right-hand band and
 * in the two near corners. The champion carried five squashed spheres and two
 * flat starfish, so ~40% of the frame (everything outside the pool) read as an
 * empty tan field and the whole diorama looked under-dressed next to the
 * reference's lived-in beach.
 *
 * This is pure detail density, and it needs no primitive the scene does not
 * already use. Three authored forms cover everything the reference draws:
 *
 *   * `pebble` — a small faceted chip. A yawed, rolled BOX rather than a squashed
 *     sphere: the reference's pebbles are angular low-poly stones that catch the
 *     key light on one face, and a sphere at this size reads as a soft dot.
 *   * `clam`  — a scallop: a short fan of thin ribs splayed about a common hinge
 *     and tipped up, so the shell shows its ridges in silhouette. This is the one
 *     form the champion had no geometry for at all (it drew clams as plain
 *     spheres), and the engine has no fan/sector primitive, so a rib fan IS the
 *     primitive-honest reading — the same argument the lid dome makes.
 *   * `star`  — the existing five-arm cross, kept as-is, at more positions.
 *
 * Positions are authored, not scattered by hash: they are placed to match where
 * the reference actually puts its litter (a dense right band, two near corners, a
 * thin scatter across the far shore), all at radius > the water vignette so
 * nothing floats on the lagoon, and all clear of the three draggable props' home
 * footprints. Fixed and outcome-independent, exactly as before.
 */

/** Litter radii live in [5.7, 8.0] — outside the vignette (WATER_RADIUS · 9/8.4
 * ≈ 5.36) and inside the frame edge on every band. */
type LitterKind = "pebble" | "clam" | "star";
interface LitterPiece {
  readonly x: number;
  readonly z: number;
  readonly kind: LitterKind;
  /** Size multiplier — the reference's pieces are not uniform. */
  readonly s: number;
  /** Ground yaw, so no two neighbours present the same face. */
  readonly a: number;
}

/** Ribs per clam fan; halved on the software backend with the rest of the LOD. */
const CLAM_RIBS = 4;
const CLAM_RIBS_LOW = 2;
/** How far the fan splays, end rib to end rib. */
const CLAM_SPREAD = 1.15;

/** One ridged clam shell: a fan of thin ribs hinged at a common point and tipped
 * up out of the sand, so the shell reads as a scalloped fan from the tabletop
 * camera instead of a blob. */
const clamShell = (key: string, at: EngineVec3, s: number, yaw: number, ribs: number): readonly SceneInstance[] =>
  Array.from({ length: ribs }, (_, i): SceneInstance => {
    // Ribs share a hinge behind the shell and splay forward, each rolled a little
    // so the fan domes rather than lying flat.
    const spread = ribs === 1 ? 0 : (i / (ribs - 1) - 0.5) * CLAM_SPREAD;
    const q = quatMul(quatYaw(yaw + spread), quatMul(quatPitch(-0.28), quatRoll(spread * 0.5)));
    const len = 0.5 * s * (1 - Math.abs(spread) * 0.18);
    return decorPart(key + i, i % 2 === 0 ? "Shell" : "CastleSand", "box", addV3(at, rotateByQuat(v3(0, 0.05 * s, len / 2), q)), v3(0.17 * s, 0.07 * s, len), q);
  });

/** The authored shore scatter, read off the reference's own distribution. */
const LITTER: readonly LitterPiece[] = [
  // Far shore, above the lagoon rim: a thin sprinkle, the reference's sparsest band.
  { a: 0.4, kind: "pebble", s: 0.9, x: -1.7, z: -6.1 },
  { a: 1.9, kind: "clam", s: 1.0, x: -0.4, z: -6.5 },
  { a: 2.7, kind: "pebble", s: 0.75, x: 2.3, z: -6.2 },
  { a: 0.9, kind: "pebble", s: 1.05, x: -3.5, z: -5.3 },
  // Left band, behind and around the crab.
  { a: 2.2, kind: "pebble", s: 0.85, x: -6.7, z: -0.7 },
  { a: 0.6, kind: "pebble", s: 1.15, x: -7.0, z: 1.9 },
  { a: 3.3, kind: "clam", s: 1.35, x: -6.2, z: 3.5 },
  { a: 0.35, kind: "star", s: 1.0, x: -4.6, z: 5.2 },
  // Right band — the widest stretch of sand in frame, and the reference's densest.
  { a: 1.4, kind: "star", s: 1.1, x: 6.7, z: -2.6 },
  { a: 4.1, kind: "clam", s: 1.2, x: 6.9, z: -1.0 },
  { a: 0.2, kind: "pebble", s: 0.8, x: 7.3, z: 0.4 },
  { a: 2.5, kind: "pebble", s: 1.0, x: 6.9, z: 1.7 },
  { a: 1.1, kind: "pebble", s: 0.7, x: 7.4, z: 3.0 },
  { a: 5.0, kind: "pebble", s: 1.1, x: 6.4, z: 4.3 },
  { a: 0.8, kind: "star", s: 0.9, x: 5.9, z: 5.5 },
  // Near shore, between the two corners.
  { a: 1.7, kind: "pebble", s: 0.95, x: 2.4, z: 5.6 },
  { a: 3.9, kind: "pebble", s: 0.8, x: -1.9, z: 6.0 },
];

const beachLitter = (): readonly SceneInstance[] => {
  const low = lowDetail();
  const ribs = low ? CLAM_RIBS_LOW : CLAM_RIBS;
  return LITTER
    // The software backend keeps every clam and starfish (they are the pieces
    // that read as objects) but sheds half the pebbles, which are the cheapest
    // detail to lose and the least missed at that resolution.
    .filter((p, i) => !low || p.kind !== "pebble" || i % 2 === 0)
    .map((p, i): readonly SceneInstance[] => {
      const at = v3(p.x, 0, p.z);
      if (p.kind === "clam") {
        return clamShell(`clam${i}:`, v3(p.x, 0.04, p.z), p.s, p.a, ribs);
      }
      if (p.kind === "star") {
        return Array.from({ length: 5 }, (_, k): SceneInstance =>
          decorPart(`star${i}:arm${k}`, "Starfish", "box", v3(at.x, 0.05, at.z), v3(0.12 * p.s, 0.05, 0.44 * p.s), quatYaw(p.a + (k / 5) * Math.PI * 2)),
        );
      }
      // A pebble is a single faceted chip: yawed on the ground and rolled a
      // little off flat so one face catches the key light.
      return [
        decorPart(
          `pebble${i}`,
          "Shell",
          "box",
          v3(at.x, 0.07 * p.s, at.z),
          v3(0.3 * p.s, 0.17 * p.s, 0.24 * p.s),
          quatMul(quatYaw(p.a), quatRoll(0.22 + (i % 3) * 0.13)),
        ),
      ];
    })
    .flat();
};

/** The whole shore of set-dressing. The palm/castle/crab are placed at the
 * player-controlled positions in `decor` (they can be picked up and moved); the
 * one currently held is lifted so it reads as "in hand". The palm and crab are
 * alive (wind sway / idle animations); `tick`/`seed` drive only those poses, via
 * pure ambient-keyed values — nothing here reads the outcome. The litter is
 * fixed. */
const HELD_LIFT = v3(0, 0.5, 0);
const beachDecor = (tick: number, seed: number, decor: DecorDrag): readonly SceneInstance[] => {
  const at = (key: keyof DecorDrag["props"]): EngineVec3 => addV3(decor.props[key], decor.held === key ? HELD_LIFT : v3(0, 0, 0));
  return [...palmTree(at("palm"), tick), ...sandcastle(at("castle")), ...crab(at("crab"), tick, seed), ...beachLitter()];
};

// The freestanding brand billboard that used to stand across the back of the beach
// has been removed: the CENTER branded chest (its ACME nameplate) is the only
// logo placement now, so the frame reads clean instead of signed.

// ── the background veil ─────────────────────────────────────────────────────────

/**
 * A dark sheet hung across the frustum BETWEEN the hero chest and everything
 * else — the board, the eight other chests, the platform, the backdrop. As the
 * chosen chest spirals forward the veil rises behind it, so the stage falls
 * away into near-darkness and the chest is left owning a quiet frame.
 *
 * A veil is the honest tool here rather than dimming the lights: the pavilion
 * backdrop is EMISSIVE, so it ignores lighting entirely and would stay bright
 * while everything around it fell dark. Occluding it works on every material.
 * The renderer draws translucent geometry after opaque, depth-tested but
 * without depth writes, so the hero chest — nearer than the veil — punches
 * through it for free, with no sorting work here.
 */
const backgroundVeil = (camera: Camera3D, framing: HeroFraming, level: number): readonly SceneInstance[] => {
  const material = veilMaterialOf(level);
  if (material === null) {
    return [];
  }
  const depth = framing.distance + CHEST_TIMING.veilGap;
  const half = depth * Math.tan(camera.fovY / 2);
  // Turn the sheet's +Z face back down the view axis so it squarely faces the
  // camera, and oversize it well past the frustum so no aspect ratio can
  // uncover an edge.
  return [
    {
      key: "veil",
      material,
      mesh: "box",
      transform: {
        position: addV3(camera.position, scaleV3(framing.forward, depth)),
        rotation: quatPitch(Math.atan2(framing.forward.y, -framing.forward.z)),
        scale: v3(half * 9, half * 5, 0.02),
      },
    },
  ];
};

// ── the scene ───────────────────────────────────────────────────────────────────

export const chestScene = (runtime: GameRuntime<ChestSpec>, state: ChestState): Scene => {
  const session = state.session;
  const count = session.config.choiceCount ?? 9;
  const seed = session.seed;
  const tick = session.tick;
  const speed = session.config.presentationSpeed;
  const spec = runtime.config.gameSpecific;
  const choice = state.extra.choice;
  const selected = choice.selected;
  const plan = session.committed;
  const timeline = revealTimeline(speed, runtime.settings.reducedMotion);
  const revealAge =
    session.phase === "revealing" ? phaseAge(session) : session.phase === "celebrating" || session.phase === "complete" ? timeline.total : -1;
  const idleActive = session.phase === "ready" || session.phase === "intro";
  const liveliness = idleActive ? spec.danceLiveliness : 0;
  // A winning reveal earns the warm treasure glow + light burst; an empty chest
  // opens through the same ritual but stays dim inside (honest, not broken).
  const winReveal = plan !== null && plan.win && outcomeRarity(session) !== "loss";

  // Master selection ramp: 0 before a pick, eases in over the commit pause, then
  // holds at 1 through the whole reveal and result — the whole scene reorganizes.
  const selectT =
    selected === null
      ? 0
      : session.phase === "committing"
        ? clamp01(phaseAge(session) / speedTicks(CHEST_TIMING.liftInTicks, speed))
        : session.phase === "ready"
          ? 0
          : 1;
  const selectEase = easeOutCubic(selectT);

  // Burst progress (0→1 over the burst window, right after the lid opens).
  const burstT = revealAge >= timeline.burstAt ? clamp01((revealAge - timeline.burstAt) / Math.max(1, timeline.lidEnd - timeline.lidStart)) : 0;

  // ── the hero flight ───────────────────────────────────────────────────────────
  // The camera does NOT move in this game. Instead the chosen chest leaves the
  // board and spirals into a close hero framing derived from that fixed camera
  // — which is why the other eight stay exactly where the player left them.
  //
  // The flown transform is computed ONCE here and is the single anchor every
  // downstream element hangs off (prize, burst, reveal lights, celebration).
  // Before this, six call sites each independently re-derived "where the chosen
  // chest is" from its grid slot; now the chest moves and they all follow it.
  const camera = chestCamera(count);
  const framing = heroFraming(camera);
  const flight = selected === null ? 0 : flightProgress(session, speed);
  const liftAmount = CHEST_TIMING.lift * selectEase;
  // `framing.anchor` frames the chest's CENTER; a chest is posed from its base.
  const heroBase = addV3(framing.anchor, v3(0, (-CHEST_HEIGHT / 2) * framing.scale, 0));
  const flown = spiralFlight(
    addV3(selected === null ? v3(0, 0, 0) : chestPosition(selected, count), v3(0, liftAmount, 0)),
    heroBase,
    flight,
    framing,
  );
  const heroScale = lerp(lerp(1, CHEST_TIMING.selectScale, selectEase), framing.scale, flown.grow);
  /** The chosen chest's open mouth, wherever the flight has carried it. */
  const heroTop = addV3(flown.position, v3(0, BODY_TOP * heroScale, 0));

  // The center featured chest: the slot nearest the board origin (index 4 on the
  // standard 3×3). It wears the brand nameplate — the plaque IS its only mark, so
  // the center never reads as permanently highlighted; it looks like every other
  // chest apart from carrying the ACME plate.
  const centerIndex = Array.from({ length: count }, (_, i) => i).reduce((best, i) => {
    const p = chestPosition(i, count);
    const b = chestPosition(best, count);
    return p.x * p.x + p.z * p.z < b.x * b.x + b.z * b.z ? i : best;
  }, 0);

  const chests = Array.from({ length: count }, (_, index) => {
    const origin = chestPosition(index, count);
    const dance = dancePose(index, count, tick, seed, liveliness);
    const isSelected = selected === index;

    // Continuous, per-chest-desynced idle breathe (stilled once a pick is made).
    const idleGate = liveliness * (1 - selectT);
    const ph = idlePhase(index);
    const clock = (tick / CHEST_TIMING.idleBobPeriod) * 2 * Math.PI;
    const idleBob = Math.sin(clock + ph) * CHEST_TIMING.idleBobAmp * idleGate;
    const idleTwist = Math.sin(clock * 0.5 + ph) * CHEST_TIMING.idleTwistAmp * idleGate;

    // Anticipation brace: a tiny shiver before the latch moves (selected only).
    const bracing = isSelected && revealAge >= 0 && revealAge < timeline.braceEnd;
    const braceT = bracing ? revealAge / timeline.braceEnd : 0;
    const shiver = bracing ? Math.sin(revealAge * 1.5) * CHEST_TIMING.shakeMag * pulse(braceT) : 0;

    // Latch: swings open over [latchStart, latchEnd] with a recoil snap at the end.
    const latchT = isSelected ? clamp01((revealAge - timeline.latchStart) / Math.max(1, timeline.latchEnd - timeline.latchStart)) : 0;
    const latchRecoil = isSelected && revealAge >= timeline.latchEnd && revealAge < timeline.latchEnd + 4 ? Math.sin((revealAge - timeline.latchEnd) * 1.3) * CHEST_TIMING.latchRecoil * (1 - (revealAge - timeline.latchEnd) / 4) : 0;
    // Lid: opens with an overshoot-and-settle after the pause.
    const lidT = isSelected ? clamp01((revealAge - timeline.lidStart) / Math.max(1, timeline.lidEnd - timeline.lidStart)) : 0;
    // Seam light grows from latch-land through the lid opening.
    const seam = isSelected ? clamp01((revealAge - timeline.seamStart) / Math.max(1, timeline.lidEnd - timeline.seamStart)) * (1 - lidT * 0.6) : 0;

    const dimmed = selected !== null && !isSelected;

    // A chosen chest rides the spiral; every other chest stays in its slot,
    // breathing on the idle bob.
    const lift = isSelected ? liftAmount : idleBob;
    const at = isSelected ? flown.position : v3(origin.x, origin.y + lift, origin.z);

    return chestInstances(`chest${index}`, {
      at,
      brandName: spec.brand.name,
      dim: dimmed,
      flight: isSelected ? flight : 0,
      focusRing: session.phase === "ready" && choice.focused === index && choice.hovered !== index && choice.armed !== index,
      glow: isSelected ? easeOutCubic(lidT) * (winReveal ? 1 : 0.32) : 0,
      hoverRing: session.phase === "ready" && (choice.hovered === index || choice.armed === index),
      latchAngle: easeOutCubic(latchT) * CHEST_TIMING.latchDrop + latchRecoil,
      lidAngle: -easeOutBack(lidT) * CHEST_TIMING.lidOpen,
      nameplate: index === centerIndex,
      origin,
      pitch: isSelected ? CHEST_TIMING.tilt * selectEase + flown.tumble : 0,
      scale: isSelected ? heroScale : 1,
      seam,
      selected: isSelected,
      squash: dance.squash + (bracing ? pulse(braceT) * 0.05 : 0),
      yaw: dance.twist + idleTwist + shiver + (isSelected ? flown.spin : 0),
    });
  }).flat();

  // Reward / empty reveal rising fully clear of the selected, open chest.
  const rewardInstances: SceneInstance[] = [];
  const burst: SceneInstance[] = [];
  if (selected !== null && plan !== null && revealAge >= timeline.lidEnd) {
    const chestTop = heroTop;
    const riseT = clamp01((revealAge - timeline.lidEnd) / Math.max(1, timeline.riseEnd - timeline.lidEnd));
    const settle = clamp01((revealAge - timeline.riseEnd) / 20);
    const rarity = outcomeRarity(session);

    if (rarity !== "loss") {
      // A win: the warm light burst fires and the prize climbs fully clear of
      // the chest to hover as the frame's focal point.
      burst.push(...lightBurst(chestTop, tick, burstT, heroScale));
      rewardInstances.push(...heroPrize(rarity, chestTop, riseT, tick, settle, heroScale));
    } else {
      // An empty chest: a playful grey dust puff coughs up and out (no burst,
      // no prize) — a clear, warm "nothing here this time".
      const puffs = Array.from({ length: 6 }, (_, i) => {
        const local = revealAge - timeline.lidEnd - i * 2.5;
        const life = 46;
        if (local < 0 || local > life) {
          return null;
        }
        const pt = local / life;
        const a = (i / 6) * Math.PI * 2 + i;
        // The puff belongs to the chest, so it grows and climbs with it — damped
        // on the climb for the same reason the prize is: to stay in frame.
        const puff = heroScale * CHEST_TIMING.prizeDamp;
        const spread = (0.16 + pt * 0.5) * puff;
        const size = (0.16 + pt * 0.3) * (1 - pt * 0.35) * puff;
        return {
          key: `dust:${i}`,
          material: "DustPuff",
          mesh: "sphere",
          transform: {
            position: v3(
              chestTop.x + Math.cos(a) * spread,
              chestTop.y + (0.05 + pt * 0.75) * heroScale * CHEST_TIMING.riseDamp,
              chestTop.z + Math.sin(a) * spread,
            ),
            rotation: QUAT_IDENTITY,
            scale: v3(size, size * 0.82, size),
          },
        } satisfies SceneInstance;
      }).filter((d): d is SceneInstance => d !== null);
      rewardInstances.push(...puffs);
    }
  }

  // Celebration.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null) {
    const profile = celebrationFor(runtime.settings, session);
    const at = addV3(heroTop, v3(0, 0.4 * heroScale, 0));
    celebration.push(
      ...(plan.win
        ? confettiBurst("confetti", at, profile.particles, plan.presentationSeed, phaseAge(session))
        : sparkleRing("cheer", at, profile.particles, plan.presentationSeed, phaseAge(session))),
    );
  }

  // Lights: standard rig, a warm escape light once the lid parts, and a brief
  // burst light that flashes the chest faces at the pop. All three follow the
  // FLOWN chest, so the reveal stays lit as it travels off the board.
  const focus = selected === null ? v3(0, 0, 0) : flown.position;
  // Beach sun under a big bright sky — NOT a bare raked sun. The reference is a
  // soft, high-key diorama: the palm's cast shadow is barely darker than the
  // sand, the castle's away-facing towers stay pale sand, and a chest's shadow
  // side sits maybe 3:1 under its lid.
  //
  // What was actually missing is ENVIRONMENT, not another lamp. A beach at midday
  // is lit twice: once by the sun, and once by a whole hemisphere of sky and hot
  // sand bouncing warm light back onto every surface from every direction. The
  // engine's ambient was a fixed monochrome 0.12, so this scene could only fake
  // that hemisphere two illegal ways — a near-white DIRECTIONAL "fill" (still
  // directional: it lights the faces it happens to point at and leaves the faces
  // pointing away from BOTH lamps crushed at 0.12) and fake material `emissive`
  // on the sand props. Both are now gone; the scene authors the hemisphere
  // directly as `ambient` (see the returned Scene below), which is the engine
  // field that actually models it.
  //
  // With a real ambient the rig collapses back to what a beach rig should be: a
  // warm sun key plus the shared cool sky fill at its normal weight. The key and
  // fill are trimmed by exactly the exposure the ambient now supplies, so the LIT
  // faces and the overall frame brightness are held where the champion had them
  // (top-face ≈ 1.06 R, unchanged) while the darkest faces rise from 0.12·albedo
  // to 0.28·albedo — a ~4:1 lit-to-shadow spread instead of ~9:1, landing on warm
  // sand rather than charcoal. The key DIRECTION is untouched, so every contact
  // shadow stays in lock-step.
  const lights: SceneLight[] = stageLights(focus, 0.5 + 0.4 * selectEase).map((entry) => {
    const fill = { key: entry.key, light: { ...entry.light, color: [0.9, 0.94, 1, 1] as Rgba, intensity: 0.3 } };
    const key = { key: entry.key, light: { ...entry.light, intensity: 1.15 } };
    return entry.key === "light:fill" ? fill : entry.key === "light:key" ? key : entry;
  });
  if (selected !== null && revealAge >= timeline.pauseEnd) {
    const warm = clamp01((revealAge - timeline.pauseEnd) / 12);
    lights.push({
      key: "light:chest",
      light: { color: [1, 0.82, 0.45, 1], intensity: 1.3 * warm * (winReveal ? 1 : 0.4), kind: "point", position: addV3(flown.position, scaleV3(v3(0, 1.1, 0.3), heroScale)) },
    });
  }
  if (winReveal && selected !== null && burstT > 0 && burstT < 1) {
    lights.push({
      key: "light:burst",
      light: { color: [1, 0.9, 0.6, 1], intensity: 1.8 * pulse(burstT), kind: "point", position: addV3(flown.position, scaleV3(v3(0, 1.5, 0.2), heroScale)) },
    });
  }

  return {
    // The hemisphere of warm bounce a beach sits in — sky above, hot sand all
    // around — as one honest engine value instead of a fake fill lamp plus fake
    // emissive. Warm and red-leading (R > G > B) because the dominant bounce
    // source in frame IS the sand: an away-facing wooden chest board or a
    // shadow-side castle tower now settles onto a dim version of its OWN color
    // (ambient multiplies the albedo) rather than a grey or a self-lit glow.
    // Weighted to keep the chests' shadow boards clearly readable while staying
    // well under the key, so the sun still models the forms.
    ambient: [0.28, 0.25, 0.21, 1],
    camera,
    clearColor: SKY_CLEAR,
    // The veil sits between the board and the hero chest: everything before it
    // in this list is what gets dimmed, everything after it stays clear.
    instances: [
      // The floor-ring is pulled in to the water radius so the sandy floor slab
      // reads as a wide beach around the inset lagoon rather than one more
      // turquoise disc flooding the frame out to the old ring radius.
      //
      // The slab itself is sized MUCH larger than the water so the sandy beach
      // fills the frame all the way to the top edge. At the tabletop pitch the
      // top-of-frame frustum ray strikes the ground well past the old radius-8
      // slab, so its far edge fell short and the emissive pastel backdrop/sky
      // leaked in as a light-blue horizon band across the top — a horizon the
      // reference does not have (there the sandy beach, with palm and sandcastle,
      // runs unbroken to the top edge with no sky showing). Extending the slab
      // past the furthest in-frame ground point drops that whole band onto beach,
      // cropping the horizon out and matching the reference's full-bleed sand.
      // The turquoise ring (accentRadius = WATER_RADIUS) is unchanged, so the
      // inset lagoon and its beach margin keep exactly the held framing.
      // The turquoise floor-ring is concentric with the pool at the same radius,
      // so it takes the same high-tessellation mesh — otherwise the ring's
      // polygon corners would stick out past the round pool onto the sand.
      ...stageRoom(48, WATER_RADIUS, LAGOON_MESH),
      ...platform(),
      ...beachDecor(tick, seed, state.extra.decor),
      ...chests,
      ...backgroundVeil(camera, framing, flight),
      ...burst,
      ...rewardInstances,
      ...celebration,
    ],
    lights,
  };
};

// ── the Canvas2D water overlay ────────────────────────────────────────────────────

/*
 * The lagoon's water treatment is a flat 2D touch the 3D scene graph cannot
 * express well (soft blur, a shoreline fade), so it is drawn by the engine's
 * reusable Canvas2D primitive `drawStylizedWaterSurface` onto the overlay layer
 * the casino harness mounts over the render canvas. This app owns only the
 * BOUNDARY: it projects the pool rim (the clip path) and the chest footprints
 * (holes, so the net is not painted over the chests) into the shared 960×600
 * canvas space, then hands the rendering to the engine. Deterministic — the
 * pattern is a coordinate hash and the drift comes from the explicit `nowMs`.
 */

/** Points traced around the pool rim to approximate its screen silhouette. Held
 * in lock-step with `LAGOON_SEGMENTS` (which is twice this, because the software
 * backend halves a mesh's facet budget): the 2D clip path and the 3D shoreline it
 * is clipped to are then literally the same polygon on Canvas2D. */
const WATER_RIM_POINTS = 48;
/** The height up each chest the punched hole is centered on, the world half-width
 * whose projection sets each hole's radius (so far/smaller chests get smaller
 * holes and are not ringed by raw pool), and a small screen margin. */
const CHEST_HOLE_LIFT = 0.3;
const CHEST_HALF_WIDTH = 0.82;
const CHEST_HOLE_MARGIN = 6;
/** The lagoon's water palette. The EDGE color matches the rendered pool so the
 * shoreline cover is invisible except that it hides the net; the LINE/TROUGH pair
 * reads as a ripple crest and trough; SPARKLE catches the light on some peaks;
 * SHALLOW is the lighter band where the water meets the sand. (No sun glint: in a
 * pool this packed with chests a sheen has nowhere to sit without ringing the
 * holed chests in bright water.)
 *
 * Every tint here is CYAN-biased (blue above green), for the same reason the
 * `StageFloorAccent` base color is: this overlay is blended over the rendered
 * pool at ~32%, so a green-neutral (G == B) edge tint does not merely sit on
 * the water, it actively pulls the whole lit surface back toward sea-green and
 * undoes the warm-rig compensation baked into the 3D material. The 2D and 3D
 * authorities have to agree on the hue or the pool averages out between them. */
const POOL_EDGE_COLOR = "rgb(34, 142, 168)";
const WATER_LINE_COLOR = "rgba(210, 244, 252, 0.95)";
const WATER_TROUGH_COLOR = "rgba(10, 84, 116, 0.6)";
const WATER_SPARKLE_COLOR = "rgba(234, 251, 255, 0.9)";
const WATER_SHALLOW_COLOR = "rgba(148, 224, 240, 0.44)";

/** Draw the stylized water into the overlay layer for one frame. Fades out as the
 * chosen chest flies off and the veil dims the board (the pool is no longer the
 * subject then). */
export const chestWaterOverlay = (state: ChestState, ctx: CanvasRenderingContext2D, view: ViewContext): void => {
  const session = state.session;
  const count = session.config.choiceCount ?? 9;
  const camera = chestCamera(count);
  // Fade the whole overlay OUT fast the moment a chest is picked. The overlay is
  // a 2D layer on TOP of the render, so it is NOT darkened by the 3D reveal veil;
  // if it lingered, its lit water (with chest holes punched) would float over the
  // darkening scene and ring each chest in an "orb". Fading it to nothing well
  // before the veil is noticeable (gone by ~20% of the flight) avoids that.
  const flight = flightProgress(session, session.config.presentationSpeed);
  const strength = Math.max(0, 1 - flight * 5);
  if (strength <= 0.01) {
    return;
  }

  // Project the pool rim (the boundary) and the chest footprints (the holes).
  const rim = Array.from({ length: WATER_RIM_POINTS }, (_, i): { readonly x: number; readonly y: number } | null => {
    const a = (i / WATER_RIM_POINTS) * Math.PI * 2;
    return worldToCanvas(camera, v3(Math.cos(a) * WATER_RADIUS, -0.04, Math.sin(a) * WATER_RADIUS));
  }).filter((p): p is { readonly x: number; readonly y: number } => p !== null);
  if (rim.length < 3) {
    return;
  }
  // Each hole is sized to its OWN chest: project the chest center and a point one
  // half-width to the side, and use the on-screen distance as the radius — so a
  // far, smaller chest gets a smaller hole and is not haloed by an oversized one.
  const holes = Array.from({ length: count }, (_, i): { readonly x: number; readonly y: number; readonly r: number } | null => {
    const base = chestPosition(i, count);
    const center = worldToCanvas(camera, addV3(base, v3(0, CHEST_HOLE_LIFT, 0)));
    const side = worldToCanvas(camera, addV3(base, v3(CHEST_HALF_WIDTH, CHEST_HOLE_LIFT, 0)));
    return center === null || side === null ? null : { r: Math.hypot(side.x - center.x, side.y - center.y) + CHEST_HOLE_MARGIN, x: center.x, y: center.y };
  }).filter((p): p is { readonly x: number; readonly y: number; readonly r: number } => p !== null);
  const xs = rim.map((p) => p.x);
  const ys = rim.map((p) => p.y);
  const minX = Math.min(...xs);
  const minY = Math.min(...ys);

  drawStylizedWaterSurface(ctx, {
    // No `depthColor` and no `glint` here: over a pool packed with chests, any
    // broad tint or sheen brightens/darkens the water AROUND the punched chest
    // holes, ringing the chests in "orbs". The water read comes from the lighter
    // SHALLOW rim, the ripple net, and sparkles, which leave no hole seams.
    bounds: { height: Math.max(...ys) - minY, width: Math.max(...xs) - minX, x: minX, y: minY },
    cellSize: 58,
    driftAmount: 2.4,
    edgeColor: POOL_EDGE_COLOR,
    edgeFadePx: 36,
    lineColor: WATER_LINE_COLOR,
    lineWidth: 2.2,
    opacity: 0.32 * strength,
    shallowColor: WATER_SHALLOW_COLOR,
    softnessPx: 1.4,
    sparkleColor: WATER_SPARKLE_COLOR,
    timeSeconds: view.nowMs / 1000,
    troughColor: WATER_TROUGH_COLOR,
    traceHoles: (c) => {
      for (const p of holes) {
        c.moveTo(p.x + p.r, p.y);
        c.arc(p.x, p.y, p.r, 0, Math.PI * 2);
      }
    },
    tracePool: (c) => {
      rim.forEach((p, i) => (i === 0 ? c.moveTo(p.x, p.y) : c.lineTo(p.x, p.y)));
      c.closePath();
    },
  });
};
