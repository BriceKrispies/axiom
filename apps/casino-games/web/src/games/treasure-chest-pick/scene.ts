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

import type { Camera3D, MaterialSpec, Scene, SceneInstance, SceneLabel, SceneLight, ViewContext } from "@axiom/web-engine";
import type { EngineQuat, EngineVec3, GameResources, MeshData, Rgba } from "@axiom/web-engine";
import { drawStylizedWaterSurface } from "@axiom/web-engine";
import { worldToCanvas } from "../../presentation/cameras/picking.ts";
import type { GameRuntime } from "../../chance-engine/registry/definition.ts";
import { phaseAge } from "../../chance-engine/sessions/session.ts";
import type { BrandSpec } from "../../presentation/branding/brand.ts";
import { brandMaterials } from "../../presentation/branding/brand.ts";
import { stampText } from "../../presentation/branding/label.ts";
import { gpuDetail, lowDetail, sparseDetail, weldedLetteringReads } from "../../presentation/detail.ts";
import { confettiBurst, CONFETTI_MATERIALS, sparkleRing } from "../../presentation/celebrations/confetti.ts";
import { celebrationFor, outcomeRarity, speedTicks } from "../round-state.ts";
import { clamp01, easeOutBack, easeOutCubic, lerp, pulse } from "../../presentation/stage/easing.ts";
import { SKY_CLEAR, STAGE_MATERIALS, stageLights, stageRoom } from "../../presentation/stage/props.ts";
import {
  addV3,
  hingedTransform,
  lerpV3,
  QUAT_IDENTITY,
  quatMul,
  quatPitch,
  quatRoll,
  quatYaw,
  rotateByQuat,
  scaleV3,
  v3,
} from "../../presentation/stage/vectors.ts";
import type { CrabDress, CrabPlace } from "./crab.ts";
import { CRAB_MATERIALS, crabParts } from "./crab.ts";
import type { PrizeKind } from "./prizes/index.ts";
import { PRIZE_MATERIALS, PRIZE_SIZE, prizeExtentOf, prizeInstances, prizeKindOf, prizeSpin } from "./prizes/index.ts";
import type { ChestSpec, ChestState, CrabJourney, CrabPose, DecorDrag, HeroFraming } from "./game.ts";
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
  crabJourney,
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

/**
 * A GLOW overlay: a translucent piece whose rendered color is its `emissive`
 * and nothing else, because its albedo is BLACK.
 *
 * This is the rule the whole warm-light family below obeys, and it is the fix
 * for the reveal's "flashlight pointed out of the box". The backend composites
 * `tonemap(diffuse · albedo + specular + emissive)`: an overlay authored with a
 * near-WHITE albedo (the champion's `[1, 0.85, 0.5]`) is therefore a fully LIT
 * Lambert card that happens to also emit. On the board that was survivable; at
 * the hero framing the light sum reaches ~2.2, so the albedo term ALONE was
 * ~2.2 before the emissive was added and every overlay clipped to flat white —
 * measured (254, 253, 245) on the interior glow and (254, 253, 251) on the
 * prize halo. Dimming the emissive could never fix that, because the emissive
 * was never what was bright.
 *
 * With a black albedo the diffuse term vanishes and the piece renders as
 * exactly the warm color authored here, at exactly this opacity, under ANY
 * rig — which is what "a glow" means. A light source does not take light.
 */
const glowOverlay = (emissive: Rgba, opacity: number): MaterialSpec => ({ baseColor: [0, 0, 0, 1], emissive, opacity });

const MATERIALS: Readonly<Record<string, MaterialSpec>> = {
  ...STAGE_MATERIALS,
  ...CONFETTI_MATERIALS,
  // The beach margin around the inset lagoon. The shared StageFloor is a pale,
  // near-white cream ([0.94, 0.9, 0.82]) that under the bright warm key lifts to
  // milky bone — the reference sand is a rich, saturated golden tan. Override it
  // for THIS game only (the shared material stays neutral for the other casino
  // stages): pull the blue channel well down and widen the red→blue spread so the
  // warm rig lands the beach at golden sand rather than bleached cream. This is a
  // pure palette warm/saturation move — no grade/tonemap stage exists here.
  StageFloor: { baseColor: [0.9, 0.75, 0.47, 1] },
  // The lagoon's SHALLOW SHELF — the full-radius water disc (the shared
  // `stage:floor-ring`), which the inset open-water body sits inside, so a
  // lighter band of water rings the whole shore. The reference lagoon is not one
  // flat blue: it is a vivid cyan body with a distinctly PALER ring where the
  // pool shallows out onto the sand, and that two-tone step is most of what
  // reads as "water" rather than "blue disc" in a frame with no textures and no
  // specular. Same cyan hue and same pre-baked warm-rig compensation as the
  // deeper `LagoonWater` below (blue decisively ABOVE green, red pulled down, so
  // the warm key cannot drag it to pond green) — just seated much higher up the
  // value ladder. Overridden for THIS game only; the other casino stages keep
  // the neutral pavilion turquoise.
  StageFloorAccent: { baseColor: [0.44, 0.86, 0.94, 1] },
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
  // glow, and the burst. Every one is a `glowOverlay` (see above) — a black
  // albedo carrying a warm emissive — so each renders as precisely the color
  // written here and cannot be inflated by the rig it happens to sit in. That
  // one property is what stops the open chest reading as a flashlight: the
  // interior warmth is now AUTHORED at amber rather than being a white card the
  // three reveal lights drove to clip.
  //
  // The values are chosen as rendered colors, not as fudge factors: `PoolCore`
  // is the hot centre right under the chest, stepping down through `PoolMid` /
  // `PoolOuter` to a wide, faint edge; `InnerGlow` is the amber wash that fills
  // the open mouth; the burst pieces are the flash that fires as the lid lands.
  // They stay saturated (green ≈ 0.7·red, blue ≈ 0.35·red) because a warm light
  // that has lost its chroma is just a white light.
  PoolCore: glowOverlay([1, 0.76, 0.4, 1], 0.5),
  PoolMid: glowOverlay([0.62, 0.44, 0.2, 1], 0.3),
  PoolOuter: glowOverlay([0.4, 0.27, 0.11, 1], 0.22),
  SeamGlow: glowOverlay([1, 0.79, 0.42, 1], 0.72),
  InnerGlow: glowOverlay([0.85, 0.55, 0.22, 1], 0.5),
  BurstGlow: glowOverlay([0.95, 0.7, 0.33, 1], 0.4),
  BurstRay: glowOverlay([0.85, 0.63, 0.3, 1], 0.22),
  Mote: { baseColor: [0, 0, 0, 1], emissive: [1, 0.88, 0.55, 1] },
  // The arcade stage: a turquoise platform with a rim, a warm central glow, and
  // a darker edge falloff — an intentional board, not a flat marker.
  // The lagoon's OPEN WATER: the deeper cyan body inside the shallow shelf, and
  // the single largest surface in the frame.
  //
  // This material used to be `PlatformSide`, the pool's shaded *depth wall*
  // ([0.07, 0.42, 0.58]) — and the champion rendered the ENTIRE lagoon with it.
  // The wall disc sat at the same radius as the cyan surface ring but 0.007
  // HIGHER, so it occluded the water it was supposed to sit under and the camera
  // only ever saw the wall tone: a flat, dark navy where the reference has vivid
  // caribbean cyan. The disc is now the water body proper (see `platform`), so
  // the color is authored for a LIT top face rather than a shaded edge — the
  // cyan the surface ring was always meant to be, one step below the shallow
  // shelf that now rings it.
  //
  // The TRIPLE is solved BACKWARDS from the reference rather than picked: divide
  // the reference's measured open-water median by this rig's Lambert multiplier on
  // a water-facing normal and the base color falls out.
  //
  // It had gone STALE. The triple was solved against the PREVIOUS reference, whose
  // open water measured (51, 160, 159); the reference installed on 2026-08-06
  // measures (53, 194, 197) — a full 35 levels brighter in green and 44 in blue.
  // So the champion's lagoon was not "a shade off": the single largest surface in
  // the frame rendered at 82% of the reference's green and 78% of its blue, which
  // is why the pool reads as a dull aquarium teal where the reference is lit
  // caribbean turquoise. Measured on the judged webgl2 champion, the water body
  // came out (52, 159, 153) from the old base — i.e. an effective multiplier of
  // (1.199, 1.134, 1.017), matching the rig's independently-computed one and
  // confirming the sample is the bare 3D surface, not the 2D overlay over it.
  //
  // Re-solved against the CURRENT reference: (53, 194, 197) / (1.199, 1.134,
  // 1.017) / 255 = this triple. Nothing else about the water's construction moves.
  //
  // The distinguishing property is no longer GREEN ~= BLUE in the BASE: the new
  // reference's rendered water sits at blue a hair ABOVE green (197 vs 194), and
  // this rig's blue multiplier (1.017) is the weakest of the three, so the base
  // must carry blue clearly above green for the warm key to land them level. That
  // is the same pre-baked warm-rig compensation `StageFloorAccent` already does
  // one step up the value ladder — the two water tones now agree on the recipe as
  // well as on the hue. Both channels still land well under the clamp (194, 197),
  // so the turquoise keeps its chroma instead of blowing to cyan-white.
  //
  // The shelf ring above is deliberately NOT touched: it already measures
  // (119, 209, 220) against the reference's (103-142, 215-228) shore band. Lifting
  // the body to meet it is what turns the champion's harsh dark-core-inside-a-pale-
  // ring step into the reference's gentle one — the reference lagoon is very nearly
  // ONE bright turquoise, brightening only in a narrow band at the sand line.
  LagoonWater: { baseColor: [0.173, 0.671, 0.76, 1] },
  EdgeVignette: { baseColor: [0.03, 0.2, 0.26, 1], opacity: 0.34 },
  // A gold accent, so it obeys the same amber ratio and the same
  // seated-below-the-clamp rule as the chest gilding above — a lemon-white rivet
  // ring around an amber-gilded chest grid would break the one metal the frame has.
  BoardRivet: { baseColor: [0.78, 0.6, 0.2, 1], emissive: [0.08, 0.055, 0, 1] },
  // The empty-chest puff is the one translucent piece here that is NOT a glow —
  // it is real dust, and it should be modelled by the key like everything else.
  // So it keeps a lit albedo, seated LOW: at the reveal's light sum a pale-grey
  // albedo multiplies straight past 1 and the puff clips to the same white every
  // overlay used to. A dark warm-grey albedo lands it on lit dust instead, and a
  // whisper of emissive keeps it from reading as a soot blob against the warm
  // mouth it coughs out of.
  DustPuff: { baseColor: [0.34, 0.31, 0.28, 1], emissive: [0.06, 0.055, 0.05, 1], opacity: 0.5 },
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
  // The crab's own palette lives in `crab.ts` beside the assembly, so the beach
  // crab and the prize crab cannot drift apart (see `CRAB_MATERIALS` below).
  // Shells/starfish shed the same emissive fakery for the same reason: the warm
  // ambient keeps these little shore pieces reading as pale shells catching the
  // sky rather than dark pebbles, without making them self-luminous.
  Shell: { baseColor: [0.96, 0.86, 0.8, 1] },
  Starfish: { baseColor: [0.92, 0.5, 0.29, 1] },
  // ── one consistent cast-shadow family ─────────────────────────────────────
  // Every prop anchors to the ground with the same two translucent pieces: a
  // SOFT tail raking down-light (length set by how tall the caster is) and a
  // smaller, darker CORE where the object actually meets the ground. A whisper
  // of nothing else — no emissive — so they only ever darken what is beneath
  // them.
  //
  // The DENSITY is not a taste dial, it is the reference's, measured. There is no
  // shadow mapping on either backend (see campaign.toml), so these discs ARE the
  // scene's shadow term and their opacity is the only lever on how deep a shadow
  // reads. Sampled off reference.png, a cast shadow multiplies the ground it
  // falls on by a hair under 0.6 — open sand (250,197,95) against the palm's
  // shadow band (148,113,62) is 0.59/0.57/0.65 per channel, and the water under
  // a chest darkens the same way (158G -> 118G, 0.62). The champion's tail was
  // landing at 0.72 of the sand: present, but a smudge you have to look for, so
  // no key direction read across the frame at all — the frame's props were lit
  // from the upper right and grounded by nothing.
  //
  // 0.26 is solved for, not nudged. `CULL_FACE` is off and translucent nodes draw
  // with `depthMask(false)`, so a disc blends TWICE (its lit top face at ~1.06x
  // albedo, its unlit underside at ambient only): the ground survives at
  // (1-a)^2, i.e. 0.55 at a=0.26, and the two dark faces add back ~0.035 — which
  // puts the tail at 0.58/0.59/0.63 of the sand and 0.60 of the water, inside two
  // levels of the reference on every channel. The CORE, stacking over the tail,
  // then lands at ~0.33 against the reference's darkest contact (0.35).
  //
  // The tint goes cool-neutral rather than brown for the same measured reason: a
  // shadow here is not "less sun", it is what the SKY and the warm ambient still
  // deliver, which is relatively bluer than the direct key. That is why the
  // reference's shadow keeps more of its blue (0.65) than its red (0.59); a warm
  // brown overlay pulled blue down hardest and flattened the shadow into a mud
  // wash of the sand's own hue.
  ContactShadowSoft: { baseColor: [0.1, 0.1, 0.12, 1], opacity: 0.26 },
  ContactShadowCore: { baseColor: [0.08, 0.08, 0.1, 1], opacity: 0.3 },
  ...CRAB_MATERIALS,
  ...PRIZE_MATERIALS,
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

/** The mesh name the edge vignette draws with (see `LAGOON_RING_INNER`). */
const LAGOON_RING_MESH = "lagoonRing";

/**
 * Where the vignette RING starts, as a fraction of its own outer radius.
 *
 * The vignette is the pool's outermost layer and it sat at the BOTTOM of the
 * stack: the opaque shallow-shelf ring (`stage:floor-ring`, radius
 * `WATER_RADIUS`) covers everything inside 5.0/5.357 = 0.933 of it, so only a
 * thin band at the sand line was ever visible. Drawn as a solid disc, the other
 * 87% of it was shaded, blended and thrown away — and because it is TRANSLUCENT
 * it was doing that in the renderer's most expensive inner loop. Measured on the
 * software backend it was the single largest consumer of fill in the whole frame:
 * 137k covered pixels on a 137k-pixel framebuffer, of which 5% survived the depth
 * test.
 *
 * So the geometry stops where the visibility does. `0.9` rather than `0.933` so
 * the ring's inner edge tucks a clear margin UNDER the shelf that covers it,
 * leaving no hairline seam where two same-radius polygons would have met.
 */
const LAGOON_RING_INNER = 0.9;

/**
 * A flat RING (annulus) in the XZ plane, unit outer diameter and unit height, so
 * it drops into the same `disc()` scale convention as `cylinder`.
 *
 * Two annular caps and no side wall: the walls of a 0.006-tall decal project to
 * four hundredths of a pixel and cannot be seen, while BOTH caps must stay — a
 * translucent surface writes no depth, so the underside blends too, and dropping
 * it would visibly lighten the rim it is there to darken.
 */
const ringMeshData = (segments: number, innerRatio: number): MeshData => {
  const outer = 0.5;
  const inner = outer * innerRatio;
  const rim = Array.from({ length: segments + 1 }, (_, seg) => (seg / segments) * Math.PI * 2);
  // Vertex order per rim step, per cap: [outer, inner].
  const positions = [1, -1].flatMap((side) =>
    rim.flatMap((theta) =>
      [outer, inner].map((radius) => v3(Math.cos(theta) * radius, side * 0.5, Math.sin(theta) * radius)),
    ),
  );
  const normals = positions.map((p) => v3(0, Math.sign(p.y), 0));
  const capVerts = (segments + 1) * 2;
  const indices = [0, 1].flatMap((cap) =>
    Array.from({ length: segments }, (_, seg) => {
      const base = cap * capVerts + seg * 2;
      const [outerA, innerA, outerB, innerB] = [base, base + 1, base + 2, base + 3];
      // The bottom cap faces -Y, so its two triangles wind the other way round.
      return cap === 0
        ? [outerA, outerB, innerA, innerA, outerB, innerB]
        : [outerA, innerA, outerB, innerA, innerB, outerB];
    }).flat(),
  );
  return { indices, normals, positions };
};

export const chestResources = (brand: BrandSpec): GameResources => ({
  materials: { ...MATERIALS, ...brandMaterials(brand) },
  meshes: {
    box: { kind: "box" },
    cylinder: { kind: "cylinder" },
    [LAGOON_MESH]: { kind: "cylinder", segments: LAGOON_SEGMENTS },
    [LAGOON_RING_MESH]: { data: ringMeshData(LAGOON_SEGMENTS, LAGOON_RING_INNER) },
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

// ── one directional light, one cast-shadow rule ─────────────────────────────────

/**
 * The whole scene is lit by a single directional key (the `light:key` in
 * `stageLights`). Its ground-plane throw is the ONE direction every cast
 * shadow falls, so nothing looks lit from conflicting suns. Kept in lock-step
 * with the key light's `direction` below — change one, change the other.
 */
const KEY_LIGHT_DIR = v3(-0.6, -0.58, -0.5);
const SHADOW_DIR = ((): { readonly x: number; readonly z: number } => {
  const len = Math.hypot(KEY_LIGHT_DIR.x, KEY_LIGHT_DIR.z);
  return { x: KEY_LIGHT_DIR.x / len, z: KEY_LIGHT_DIR.z / len };
})();

/** The yaw that turns local +Z onto the key's ground throw, so a shadow can be
 * stretched along the light in local space and then swung to point down-light. */
const SHADOW_YAW = Math.atan2(SHADOW_DIR.x, SHADOW_DIR.z);

/**
 * How far a shadow reaches down-light, per world unit of the CASTER'S HEIGHT.
 *
 * This is the whole grounding rule in one number, and it is derived from the key
 * rather than hand-picked so a tall prop and a squat one can never disagree
 * about where the sun is: the palm rakes ~1.8 units across the sand, a chest
 * only ~0.6, from the same constant.
 *
 * Geometrically this key throws `hypot(x, z) / |y|` ≈ 1.35 lengths per unit of
 * height. A SOFT shadow does not read that far — the penumbra widens with
 * distance from the contact point and washes into the ambient well before the
 * hard-shadow tip — so half of the geometric length is the part a viewer
 * actually sees, and the part worth drawing. There is no second sun here, only
 * one sun with a soft edge.
 */
const SHADOW_THROW = (0.5 * Math.hypot(KEY_LIGHT_DIR.x, KEY_LIGHT_DIR.z)) / Math.abs(KEY_LIGHT_DIR.y);

/** Just above the ground so the discs never z-fight the water/sand slab. */
const SHADOW_Y = 0.01;

/** A shadow ellipse: a thin disc `width` across and `length` along the key's
 * ground throw, yawed onto it. (A cylinder under T·R·S — the non-uniform scale
 * is applied in local space, so the ellipse elongates along the light.) */
const shadowEllipse = (key: string, material: string, at: EngineVec3, width: number, length: number): SceneInstance => ({
  key,
  material,
  mesh: "cylinder",
  transform: { position: at, rotation: quatYaw(SHADOW_YAW), scale: v3(width, 0.008, length) },
});

/**
 * A soft directional CAST shadow: an ellipse stretched down-light by the
 * caster's height and swung onto the key's throw, plus a smaller, darker CORE
 * held at the object's actual footprint so the point where it meets the ground
 * reads darker than the tail raking away from it. `radius` is the object's
 * ground footprint and `height` how tall it stands — together they set the
 * shadow's shape. `spread` scales the whole shadow (a ground-fade for a chest
 * leaving the board, or a clarity boost for the hero slot). Returns nothing once
 * the object has lifted clear.
 */
const contactShadow = (keyPrefix: string, at: EngineVec3, radius: number, height: number, spread = 1, coreScale = 1): readonly SceneInstance[] => {
  const r = radius * spread;
  const reach = height * spread * SHADOW_THROW;
  return r < 0.04
    ? []
    : [
        shadowEllipse(
          `${keyPrefix}:soft`,
          "ContactShadowSoft",
          v3(at.x + SHADOW_DIR.x * reach * 0.5, SHADOW_Y, at.z + SHADOW_DIR.z * reach * 0.5),
          r * 2.2,
          r * 2 + reach,
        ),
        disc(`${keyPrefix}:core`, "ContactShadowCore", v3(at.x + SHADOW_DIR.x * r * 0.22, SHADOW_Y + 0.002, at.z + SHADOW_DIR.z * r * 0.22), r * 0.6 * coreScale, 0.008),
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
/**
 * `labelSink` collects scene TEXT this chest wants drawn. It is an out-parameter
 * rather than part of the return because labels are a different list in the
 * `Scene` than instances are, and the plaque's placement is only derivable here —
 * it hangs off the lid's live pose (`crownAnchor`/`plateOrient`), which nothing
 * outside this builder knows.
 */
const chestInstances = (key: string, labelSink: SceneLabel[], pose: ChestPose): readonly SceneInstance[] => {
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
  // The DOM renderer needs a deeper cut than `low`: it pays per element, not per
  // pixel, so a few-pixel trim costs as much as the chest. See `sparseDetail`.
  const sparse = sparseDetail();
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
  const shadow: readonly SceneInstance[] = grounded > 0.02 ? contactShadow(`${key}:shadow`, pose.origin, BODY.x * 0.52, CHEST_HEIGHT, grounded, 1) : [];

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
  // The plaque itself always stamps; only the welded LETTERING is conditional.
  // On a backend that cannot draw hairline strokes (the DOM renderer — see
  // `weldedLetteringReads`) the board-scale plaque reads as a blank brand plate
  // rather than a half-drawn word. The SELECTED chest still gets its lettering:
  // it flies to hero framing at `heroScale`, where the same strokes are an order
  // of magnitude larger and render exactly as they should. The word appears
  // precisely when the shot is about it.
  // On the DOM renderer the brand is real TEXT — one element, a real font — rather
  // than welded stroke boxes. That is the thing that backend does better than a
  // rasterizer, and it is the only way the word survives there: as strokes, the
  // small ones are culled and the plaque reads "A ME" (measured), which is why
  // `weldedLetteringReads` had it suppressed entirely.
  const wantsLabel = pose.nameplate && sparseDetail();
  if (wantsLabel) {
    labelSink.push({
      color: pose.dim ? [0.55, 0.5, 0.48, 1] : [1, 0.98, 0.94, 1],
      key: `${key}:brand`,
      // Lifted off the plate by the same clearance the welded lettering used, so
      // the text sits ON the plaque rather than inside it.
      size: 0.3 * plateBasis.y,
      text: pose.brandName,
      transform: {
        position: addV3(crownAnchor, rotateByQuat(v3(0, 0, 0.08 * plateBasis.z), plateOrient)),
        rotation: plateOrient,
        scale: v3(1, 1, 1),
      },
    });
  }
  const label = pose.nameplate && !wantsLabel && (weldedLetteringReads() || pose.selected)
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
    // Side-facing wood on the end caps for a value step. On the DOM renderer
    // these four trim parts (and the lid ribs below) are dropped: they are a
    // shading nuance a few pixels wide, they cost a whole element each, and none
    // of them carries the chest's silhouette. See `sparseDetail`.
    ...(sparse
      ? []
      : [
          part("endL", v3(-BODY.x / 2 + 0.02, BODY.y / 2, 0), v3(0.04, BODY.y - 0.04, BODY.z - 0.04), woodSide),
          part("endR", v3(BODY.x / 2 - 0.02, BODY.y / 2, 0), v3(0.04, BODY.y - 0.04, BODY.z - 0.04), woodSide),
        ]),
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
    ...(sparse
      ? []
      : [
          part("edgeL", v3(-BODY.x / 2 + 0.01, BODY.y / 2, BODY.z / 2), v3(0.09, BODY.y + 0.02, 0.09), trimSide),
          part("edgeR", v3(BODY.x / 2 - 0.01, BODY.y / 2, BODY.z / 2), v3(0.09, BODY.y + 0.02, 0.09), trimSide),
        ]),
    ...plaque,
    ...label,
    interior,
    ...glow,
    ...seam,
    lid,
    ...dome,
    ...(sparse ? [] : ribs),
    lidRim,
    ...hasp,
    ...rings,
  ];
};

// ── the light burst (bounded: soft glow + a few rays + a few motes) ─────────────

const BURST_SPIKES = 6;

/**
 * The flare that fires as the lid lands.
 *
 * It is a STARBURST, not a beam. The champion's version was five tall vertical
 * boxes climbing out of the chest's mouth under a frame-wide horizontal glow
 * disc — which is, precisely and literally, a torch shining up out of the box,
 * and it is the single frame that most made this reveal look like one. Two
 * things were wrong with it and both are fixed here.
 *
 * The RAYS were vertical and long (up to 1.8 units before scaling, taller than
 * the chest itself), so they read as god-rays escaping a lid rather than as a
 * pop of light around a find. They are now short spikes lying in the SCREEN
 * PLANE — the classic sparkle star — radiating out from the mouth and gone in
 * the same beat. Building them in the screen plane rather than in world XZ is
 * what makes them read as a flash from any camera: `quatPitch(-elevation)`
 * carries the local XY plane onto the plane the player is actually looking at,
 * so a spike always points somewhere on screen instead of foreshortening to a
 * dot when it happens to aim at the lens.
 *
 * The GLOW was a horizontal pancake wider than the chest, so at a 50° camera it
 * spread across the whole floor of the frame. It is now a modest bloom squared
 * up to the lens, sized to the mouth it comes out of.
 */
const lightBurst = (at: EngineVec3, tick: number, t: number, s: number, elevation: number): readonly SceneInstance[] => {
  // t is burst progress 0→1 over the burst window; intensity peaks early, fades.
  // `s` scales the whole figure with the hero chest it erupts from.
  const strength = pulse(t);
  if (strength <= 0.001) {
    return [];
  }
  const rise = (0.2 + t * 0.9) * s;
  // The screen plane at this camera: local +X/+Y span it, local +Z faces the lens.
  const screenQ = quatPitch(-elevation);
  const glow: SceneInstance = {
    key: "burst:glow",
    material: "BurstGlow",
    mesh: "cylinder",
    transform: {
      position: v3(at.x, at.y + 0.12 * s, at.z),
      rotation: quatPitch(Math.PI / 2 - elevation),
      scale: v3((0.34 + strength * 0.3) * 2 * s, 0.02, (0.34 + strength * 0.3) * 2 * s),
    },
  };
  const rays = Array.from({ length: BURST_SPIKES }, (_, i): SceneInstance => {
    const a = (i / BURST_SPIKES) * Math.PI * 2 + tick * 0.02;
    // A box's length runs along its local +Y, so this roll turns that axis onto
    // the spoke's own direction; the spoke is then pushed out to sit clear of
    // the centre rather than crossing through it.
    const spike = (0.22 + strength * 0.34) * s;
    const inner = 0.16 * s;
    const along = rotateByQuat(v3(Math.cos(a) * (inner + spike / 2), Math.sin(a) * (inner + spike / 2), 0), screenQ);
    return {
      key: `burst:ray${i}`,
      material: "BurstRay",
      mesh: "box",
      transform: {
        position: v3(at.x + along.x, at.y + 0.12 * s + along.y, at.z + along.z),
        rotation: quatMul(screenQ, quatRoll(a - Math.PI / 2)),
        scale: v3(0.045 * s, spike, 0.045 * s),
      },
    };
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

// ── the hero prize (rises fully clear of the chest, large, turning, pulsing) ───

/** Clearance between a treasure's lowest point and the warm pool beneath it, in
 * prize-local units — enough air that the object reads as floating above the
 * glow rather than resting on it. */
const HALO_DROP = 0.22;

/**
 * Where the treasure hovers, given the chest's open mouth (`at`) and how far
 * through its climb it is.
 *
 * Named and shared rather than inlined because the reveal LIGHT has to follow
 * it: the lamp that lights the prize and the prize itself must agree about
 * where the prize is, or the shot ends up lighting the box the treasure has
 * already left. The climb is damped against the hero scale — at full scale an
 * undamped rise would carry the prize straight out of frame.
 */
const prizeCentre = (at: EngineVec3, riseT: number, s: number): EngineVec3 =>
  v3(at.x, at.y + CHEST_TIMING.riseHeight * easeOutBack(riseT) * s * CHEST_TIMING.riseDamp, at.z);

const heroPrize = (kind: PrizeKind, at: EngineVec3, riseT: number, tick: number, settle: number, s: number, elevation: number): readonly SceneInstance[] => {
  const bob = Math.sin(tick * 0.12) * 0.035 * settle * s * CHEST_TIMING.prizeDamp;
  const risen = prizeCentre(at, riseT, s);
  const center = v3(risen.x, risen.y + bob, risen.z);
  // One size for every treasure, so the catalog is interchangeable: a prize
  // grows in over its climb and breathes gently once settled. `PRIZE_SIZE` is in
  // world units per prize-local unit at hero scale — the budget every prize is
  // authored inside (see `prize.ts`).
  const size = PRIZE_SIZE * (0.5 + 0.5 * riseT) * (1 + Math.sin(tick * 0.16) * 0.04 * settle) * s * CHEST_TIMING.prizeDamp;
  const halo = 0.62 * (0.5 + 0.5 * riseT) * (0.9 + Math.sin(tick * 0.14) * 0.12 * settle) * s * CHEST_TIMING.prizeDamp;
  const spin = prizeSpin(kind, elevation, tick);
  return [
    // A soft warm disc the treasure hovers OVER — a pool of light it is standing
    // in, not a card it is standing against.
    //
    // Behind is the obvious place for a halo and it is the wrong one here. The
    // overlay is translucent and depth-tested, so a disc sharing the prize's own
    // plane intersects it: the ring, the coin and the crab all had a bright band
    // cutting straight through them. Dropping the disc clear of the object's
    // lowest point removes the intersection by construction rather than by
    // tuning a radius — and it reads better besides, since the prize's own cast
    // warmth on the chest floor is a thing a player already understands.
    // `prizeExtentOf` is what makes "clear of it" honest: each treasure declares
    // its own reach, so a wide open clam pushes the pool further down than a coin.
    disc("reward:halo", "BurstGlow", v3(center.x, center.y - size * prizeExtentOf(kind) - HALO_DROP * size, center.z), halo, 0.02),
    ...prizeInstances(kind, "reward", { center, settle, size, spin, tick }),
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

/**
 * How far in from the shore the deeper open water starts, as a fraction of
 * `WATER_RADIUS`. The remainder is the paler shallow shelf (the shared
 * `stage:floor-ring`, `StageFloorAccent`) showing as a ring all the way round —
 * measured off the reference, where the light band at the shore is roughly a
 * tenth of the pool's radius.
 */
const LAGOON_SHELF_INSET = 0.9;

const platform = (): readonly SceneInstance[] => [
  // Every water disc is lagoon-scale, so all of them draw the high-tessellation
  // mesh: the vignette is the OUTERMOST, and a faceted vignette under a round
  // pool would just move the polygon out to the sand line. It draws the RING
  // variant of that mesh, because the shelf above it hides all but its outer band
  // — see `LAGOON_RING_INNER`.
  disc("plat:vignette", "EdgeVignette", v3(0, -0.048, 0), WATER_RADIUS * (9 / 8.4), 0.006, LAGOON_RING_MESH),
  // The open water, inset so the shallow shelf rings it. It is deliberately
  // stacked ABOVE the shelf ring (whose top face is at -0.039) with clear air
  // between the two faces: co-planar water discs would z-fight across a third of
  // the frame, and the previous full-radius disc — 0.007 above the ring at the
  // SAME radius — hid the shelf entirely and painted the whole lagoon in the
  // depth-wall tone (see `LagoonWater`).
  disc("plat:water", "LagoonWater", v3(0, -0.032, 0), WATER_RADIUS * LAGOON_SHELF_INSET, 0.02, LAGOON_MESH),
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

/** Leaflet stations PER SIDE of a frond's midrib on the hardware arm. Five spaced
 * a fifth of the frond apart, with leaflets `FROND_LEAFLET_WIDTH` across, leaves a
 * notch of ~0.06 between neighbours — a serrated blade edge, which is exactly how
 * the reference's leaflets read at this size. */
const FROND_LEAFLETS = 5;
/** How far a leaflet leans back toward the frond's tip, off straight-out. */
const FROND_LEAFLET_SWEEP = 0.72;
/** The widest leaflet's reach, as a fraction of the frond's length. Twice this is
 * the blade's width/length ratio (~0.42), measured off the reference. */
const FROND_LEAFLET_SPAN = 0.21;
const FROND_LEAFLET_WIDTH = 0.24;
/** The midrib's width once leaflets carry the blade — a stem, not a board. */
const FROND_RIB_WIDTH = 0.11;
/** The whole frond as ONE board, which is what a frond IS on the frugal arms. */
const FROND_BOARD_WIDTH = 0.34;

/**
 * One palm frond.
 *
 * The reference's fronds are FEATHERS: a slim midrib carrying a row of leaflets
 * that splay back toward the tip, the blade widest around mid-length and closing
 * to a point. The champion drew each frond as one flat 0.34-wide board — a green
 * popsicle stick — and seven of those radiating from the crown read as a paper
 * pinwheel rather than a palm. The palm is the second-largest subject in the shot
 * and it was the crudest proxy left in the frame.
 *
 * The engine's mesh vocabulary is box / sphere / cylinder — there is no sheet, no
 * alpha-cutout card, and no way to cut a leaf silhouette out of a quad. So a rib
 * plus a splayed row of leaflet boxes IS the primitive-honest frond, the same
 * argument the lid dome (`lidArc`) and the clam fan (`clamShell`) already make in
 * this file. It reads for the same reason too: every leaflet meets the key light
 * at its own angle, so the blade carries interior shading as well as a feathered
 * silhouette.
 *
 * It costs 2·`FROND_LEAFLETS` extra nodes a frond, so it is gated on `gpuDetail()`
 * (webgl2-or-better — see `detail.ts`). On the frugal arms the frond stays the
 * single full-width board it already was, unchanged and unmoved, so their node
 * count moves by exactly zero.
 */
const palmFrond = (key: string, material: string, crown: EngineVec3, q: EngineQuat, len: number): readonly SceneInstance[] => {
  const feathered = gpuDetail();
  const rib = decorPart(
    key,
    material,
    "box",
    addV3(crown, rotateByQuat(v3(0, 0.05, len / 2), q)),
    v3(feathered ? FROND_RIB_WIDTH : FROND_BOARD_WIDTH, 0.09, len),
    q,
  );
  const leaflets = feathered
    ? Array.from({ length: FROND_LEAFLETS * 2 }, (_, i): SceneInstance => {
        const side = i % 2 === 0 ? -1 : 1;
        // Stations march up the rib from just off the crown to just short of the tip.
        const t = (Math.floor(i / 2) + 0.5) / FROND_LEAFLETS;
        // The blade profile: short leaflets at the base, longest around mid-frond,
        // closing to a point at the tip.
        const span = len * FROND_LEAFLET_SPAN * Math.sin(Math.PI * t) ** 0.6;
        // Out to the side, leaning back toward the tip, and tipped down a little so
        // the blade domes over its rib instead of lying flat in one plane.
        const leafQ = quatMul(q, quatMul(quatYaw(side * (Math.PI / 2 - FROND_LEAFLET_SWEEP)), quatPitch(0.26)));
        return decorPart(
          `${key}f${i}`,
          material,
          "box",
          addV3(crown, addV3(rotateByQuat(v3(0, 0.05, t * len), q), rotateByQuat(v3(0, 0, span / 2), leafQ))),
          v3(FROND_LEAFLET_WIDTH, 0.055, span),
          leafQ,
        );
      })
    : [];
  return [rib, ...leaflets];
};

/** A leaning palm swaying in the wind: a curved stack of tapering bark cylinders,
 * a coconut cluster, and a fan of drooping feathered fronds radiating from the crown.
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
  const fronds = Array.from({ length: 7 }, (_, i): readonly SceneInstance[] => {
    const a = (i / 7) * Math.PI * 2;
    const droop = 0.55 + (i % 2) * 0.12 + sway.flutter(i);
    const q = quatMul(crownRoll, quatMul(quatYaw(a), quatPitch(droop)));
    const len = 1.5 + (i % 3) * 0.14;
    return palmFrond(`palm:frond${i}`, i % 2 === 0 ? "PalmLeaf" : "PalmLeafDark", crown, q, len);
  }).flat();
  return [...contactShadow("palm:shadow", origin, 0.62, PALM_CROWN_Y), ...trunk, ...coconuts, ...fronds];
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
  return [...contactShadow("castle:shadow", origin, 1.28 * CASTLE_SCALE, poleTop * CASTLE_SCALE), base, ...towerParts, door, pole, flag, flagTrim];
};

/** The crab on the beach: the shared `crabParts` assembly (see `crab.ts` — his
 * girlfriend, the chest prize, is the same creature) posed by `crabIdle`, which
 * elects one bit of business (scuttle / claw wave / bob / turn) or a rest on a
 * random interval from the ambient stream. Every part is placed through the
 * resulting body frame, so the crab scoots, bobs, turns, waves, and breathes as
 * one creature. Pure in (tick, seed) — outcome-independent. */
/**
 * Where the crab is standing, how big he is there, and what he is doing.
 *
 * He has two homes now — his patch of sand, and the front of a chest he has
 * climbed onto and is carrying off to the close-up — so the geometry cannot bake
 * in either. `scale` is what makes the second one work: it is 1 on the beach and
 * the CHEST'S OWN scale while he rides, so the two stay in proportion through a
 * flight that shrinks the chest in world units while growing it on screen.
 */
interface CrabStance {
  /** World point his feet sit at. */
  readonly at: EngineVec3;
  /** Body yaw, composed with whatever turn his pose carries. */
  readonly yaw: number;
  /** World units per crab-local unit. */
  readonly scale: number;
  readonly pose: CrabPose;
  /** Whether to plant a contact shadow beneath him — true on the sand, false in
   * mid-air on a flying chest, which has a shadow of its own or none at all. */
  readonly grounded: boolean;
}

const crabAt = (stance: CrabStance, tick: number, dress: CrabDress): readonly SceneInstance[] => {
  const pose = stance.pose;
  const bodyQ = quatYaw(stance.yaw + pose.yaw);
  const bodyShift = v3(pose.scootX, pose.bob, 0);
  // Place a part given in body-local space: rotate its offset into the (turned)
  // body frame, add the whole-body scoot/bob, scale the lot to wherever he is
  // standing, and compose the body yaw into its own rotation — so one stance
  // moves the crab as a single creature at whatever size he is.
  const place: CrabPlace = (key, material, mesh, local, scale, localRot = QUAT_IDENTITY): SceneInstance =>
    decorPart(
      `crab:${key}`,
      material,
      mesh,
      addV3(stance.at, scaleV3(addV3(bodyShift, rotateByQuat(local, bodyQ)), stance.scale)),
      scaleV3(scale, stance.scale),
      quatMul(bodyQ, localRot),
    );
  // The shadow follows the crab's side-scuttle (the horizontal scoot) but not its
  // vertical bob, so it stays planted on the sand as the little creature hops.
  const shadow = stance.grounded
    ? contactShadow("crab:shadow", addV3(stance.at, v3(pose.scootX * stance.scale, 0, 0)), 0.5 * stance.scale, 0.6 * stance.scale)
    : [];
  // He carries the brand pennant and wears no bowtie; she is the other way round.
  return [...shadow, ...crabParts(place, pose, tick, dress)];
};

/** The crab at home on his patch of sand, running his ambient idle repertoire. */
const beachCrab = (origin: EngineVec3, tick: number, seed: number): readonly SceneInstance[] =>
  crabAt({ at: origin, grounded: true, pose: crabIdle(tick, seed), scale: 1, yaw: 0 }, tick, { bowtie: 0, pennant: true });

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

/** The beach, minus the crab when he has left it. He is emitted separately while
 * he is running his errand, because the veil sits between the two: a crab drawn
 * with the rest of the shore would be dimmed to near-black exactly when the shot
 * is about him (see the instance order in `chestScene`). */
/** How far into the chosen chest's flight the veil is dense enough that the
 * chests left behind can stop being drawn on the DOM renderer (see the drop in
 * `chestScene`). Matches the beat by which the water overlay has already faded. */
const VEILED_CHEST_DROP = 0.25;

const beachDecor = (tick: number, seed: number, decor: DecorDrag, crabAtHome: boolean): readonly SceneInstance[] => {
  const at = (key: keyof DecorDrag["props"]): EngineVec3 => addV3(decor.props[key], decor.held === key ? HELD_LIFT : v3(0, 0, 0));
  // The DOM renderer skips the two most expensive pieces of set-dressing outright.
  // The sandcastle is a base, a keep, two turrets and their crenels — the single
  // biggest prop on the board — and the resident crab is a whole little figure of
  // shell, claws, legs and eyes. Between them they are a large slice of a ~300
  // element budget, spent on two things standing at the edge of frame that the
  // GAME never refers to. Dropping them buys back the budget the chests and the
  // reveal actually need. See `sparseDetail`.
  //
  // Only the crab AT HOME goes: the one that scuttles out to fetch the chosen
  // chest is a beat of the reveal, not set-dressing, and it still runs.
  const sparse = sparseDetail();
  return [
    ...palmTree(at("palm"), tick),
    ...(sparse ? [] : sandcastle(at("castle"))),
    ...(crabAtHome && !sparse ? beachCrab(at("crab"), tick, seed) : []),
    ...beachLitter(),
  ];
};

// ── the crab's errand (scuttle → grip → ride → hop) ────────────────────────────

/*
 * Where the crab rides, in CHEST-LOCAL units: CLINGING to the chest's front-left
 * corner, part-way up it, rather than standing on the ground beside it.
 *
 * On the ground beside it was the first attempt and it fails on this framing: the
 * reveal deliberately sits the chest LOW (see `heroDrop`), with its base at ~0.94
 * of frame height, so anything level with the chest's feet is already at the
 * bottom edge — the crab hung half out of frame for the whole close-up. Lifting
 * him onto the chest's front face solves it at the root instead of nudging the
 * camera: he is now on the object the shot is framed around, so he is in frame by
 * construction however that framing is retuned later.
 *
 * It is also the better read. A crab CLINGING to the front of a chest with his
 * claws hooked over the lid rail looks like he is prising it open; a crab standing
 * politely beside one looks like he is waiting for someone else to. Front-left
 * rather than centred, so he does not cover the hasp — or, once it is open, the
 * treasure rising out of the middle.
 *
 * `CRAB_FACE_YAW` is derived from the offset rather than picked: it is the yaw that
 * turns him to look at the chest's centre from wherever the offset puts him, so
 * moving him around the chest can never leave him facing into space. It works out
 * near 135°, which is the useful angle — claws onto the lid while the camera still
 * catches his eyestalks and one claw in profile rather than a flat view of his back.
 */
const CRAB_ON_CHEST = v3(-0.66, 0.5, 0.76);
const CRAB_FACE_YAW = Math.atan2(-CRAB_ON_CHEST.x, -CRAB_ON_CHEST.z);
/** How high the hop carries him, in crab-local units (so it scales with him). */
const CRAB_HOP_HEIGHT = 0.5;
/** Fraction of the walk over which he turns from facing where he is GOING to
 * facing the chest he is arriving at. */
const CRAB_TURN_IN = 0.35;

/** Shortest-arc blend between two yaws, so a crab crossing the ±π seam turns the
 * short way round instead of spinning most of a circle. */
const blendYaw = (from: number, to: number, t: number): number => {
  const delta = (((to - from + Math.PI) % (Math.PI * 2)) + Math.PI * 2) % (Math.PI * 2) - Math.PI;
  return from + delta * t;
};

/**
 * The crab's pose while he is on the errand — walking, gripping, or hopping.
 *
 * `journey.grip` raises the claws onto the rail, and the lid's own opening angle
 * pushes them further: as the lid swings up his arms extend with it, so it reads
 * as HIM opening it rather than as him standing beside a lid that opens itself.
 * The legs paddle hard while he is crossing the sand and settle once he is
 * aboard, and the hop rides `pose.bob`, which `crabAt` scales with him.
 */
const errandPose = (journey: CrabJourney, lidOpen: number, tick: number): CrabPose => ({
  bob: journey.hop * CRAB_HOP_HEIGHT,
  breath: Math.sin(tick * 0.09) * 0.03,
  clawLift: journey.grip * (0.55 + 0.45 * lidOpen),
  // A faint tremor of effort, not a wave. He is holding a lid, and at anything
  // like the wave's full flap his claws looked like they were shaking violently
  // against it (see `clawShake`).
  clawShake: 0.1,
  eye: Math.sin(tick * 0.06) * 0.06,
  kind: "wave",
  // Paddling while he crosses; a fidget once he has hold of something.
  legWiggle: journey.riding ? 0.12 : 0.55,
  scootX: 0,
  yaw: 0,
});

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
  // Where the chests actually ARE. The grid is only their starting layout: the
  // player can pick a chest up and put it anywhere, so every position the view
  // needs comes from state (see `stepChestDrag`). `slotAt` falls back to the home
  // grid, which matters when the operator changes `choiceCount` from the Set Up
  // panel and the carried layout is briefly the wrong length.
  const drag = state.extra.chests;
  const slotAt = (index: number): EngineVec3 => drag.slots[index] ?? chestPosition(index, count);
  const camera = chestCamera(count);
  const framing = heroFraming(camera);
  // How far the camera looks DOWN, in radians above the horizontal. The reveal
  // stages the prize against this rather than against the world axes — see
  // `PrizePresentation` — so a treasure meets the lens the way it was authored
  // to, whatever the camera preset does next.
  const cameraElevation = Math.asin(-framing.forward.y);
  const flight = selected === null ? 0 : flightProgress(session, speed);
  const sparse = sparseDetail();
  /** Scene TEXT the chests want drawn (the brand plaque). See `chestInstances`. */
  const chestLabels: SceneLabel[] = [];
  const liftAmount = CHEST_TIMING.lift * selectEase;
  // `framing.anchor` frames the chest's CENTER; a chest is posed from its base.
  const heroBase = addV3(framing.anchor, v3(0, (-CHEST_HEIGHT / 2) * framing.scale, 0));
  const flown = spiralFlight(
    addV3(selected === null ? v3(0, 0, 0) : slotAt(selected), v3(0, liftAmount, 0)),
    heroBase,
    flight,
    framing,
  );
  const heroScale = lerp(lerp(1, CHEST_TIMING.selectScale, selectEase), framing.scale, flown.grow);
  /** The chosen chest's open mouth, wherever the flight has carried it. */
  const heroTop = addV3(flown.position, v3(0, BODY_TOP * heroScale, 0));

  // The chosen chest's own animated quantities, hoisted out of the per-chest loop
  // below because the CRAB is welded to this chest and has to read exactly the
  // same numbers: he shakes with its anticipation brace, and his claws extend
  // with its lid. Two places computing "how far is the lid open" would drift.
  const bracing = selected !== null && revealAge >= 0 && revealAge < timeline.braceEnd;
  const braceT = bracing ? revealAge / timeline.braceEnd : 0;
  const heroShiver = bracing ? Math.sin(revealAge * 1.5) * CHEST_TIMING.shakeMag * pulse(braceT) : 0;
  const heroLidT = selected === null ? 0 : clamp01((revealAge - timeline.lidStart) / Math.max(1, timeline.lidEnd - timeline.lidStart));
  /** The chest's world yaw — its flight spin plus the brace shake. */
  const heroYaw = heroShiver + flown.spin;

  // The center featured chest: the slot nearest the board origin (index 4 on the
  // standard 3×3). It wears the brand nameplate — the plaque IS its only mark, so
  // the center never reads as permanently highlighted; it looks like every other
  // chest apart from carrying the ACME plate.
  // Resolved on the HOME grid, not on the live layout: the plaque belongs to a
  // chest, not to a location, so a player dragging the board around must not
  // pass the nameplate between chests as they shuffle past the middle.
  const centerIndex = Array.from({ length: count }, (_, i) => i).reduce((best, i) => {
    const p = chestPosition(i, count);
    const b = chestPosition(best, count);
    return p.x * p.x + p.z * p.z < b.x * b.x + b.z * b.z ? i : best;
  }, 0);

  const chests = Array.from({ length: count }, (_, index) => {
    const origin = slotAt(index);
    // A chest the player is holding rides up out of the board, so it reads as
    // being IN HAND rather than sliding across the water.
    const held = drag.grab?.dragging === true && drag.grab.index === index;
    const dance = dancePose(index, count, tick, seed, liveliness);
    const isSelected = selected === index;

    // Continuous, per-chest-desynced idle breathe (stilled once a pick is made).
    const idleGate = liveliness * (1 - selectT);
    const ph = idlePhase(index);
    const clock = (tick / CHEST_TIMING.idleBobPeriod) * 2 * Math.PI;
    const idleBob = Math.sin(clock + ph) * CHEST_TIMING.idleBobAmp * idleGate;
    const idleTwist = Math.sin(clock * 0.5 + ph) * CHEST_TIMING.idleTwistAmp * idleGate;

    // Anticipation brace: a tiny shiver before the latch moves (selected only).
    // Read off the hoisted hero values so the crab riding this chest cannot
    // disagree with it about how hard it is shaking.
    const shiver = isSelected ? heroShiver : 0;

    // Latch: swings open over [latchStart, latchEnd] with a recoil snap at the end.
    const latchT = isSelected ? clamp01((revealAge - timeline.latchStart) / Math.max(1, timeline.latchEnd - timeline.latchStart)) : 0;
    const latchRecoil = isSelected && revealAge >= timeline.latchEnd && revealAge < timeline.latchEnd + 4 ? Math.sin((revealAge - timeline.latchEnd) * 1.3) * CHEST_TIMING.latchRecoil * (1 - (revealAge - timeline.latchEnd) / 4) : 0;
    // Lid: opens with an overshoot-and-settle after the pause.
    const lidT = isSelected ? heroLidT : 0;
    // Seam light grows from latch-land through the lid opening.
    const seam = isSelected ? clamp01((revealAge - timeline.seamStart) / Math.max(1, timeline.lidEnd - timeline.seamStart)) * (1 - lidT * 0.6) : 0;

    const dimmed = selected !== null && !isSelected;
    // On the DOM renderer, a chest the reveal has left behind stops being drawn
    // once the veil is actually over it. This is the single biggest saving in the
    // whole phase: the reveal keeps all eight of them IN FRAME as near-black
    // silhouettes, and the camera is moving, so each one is re-transformed,
    // re-sorted and re-composited every frame to contribute a dark smudge on a
    // dark floor. The threshold waits for the veil rather than popping them out
    // at the moment of the pick. See `sparseDetail`.
    if (sparse && dimmed && flight > VEILED_CHEST_DROP) {
      return [];
    }

    // A chosen chest rides the spiral; every other chest stays in its slot,
    // breathing on the idle bob.
    const lift = isSelected ? liftAmount : idleBob + (held ? CHEST_TIMING.heldLift : 0);
    const at = isSelected ? flown.position : v3(origin.x, origin.y + lift, origin.z);

    return chestInstances(`chest${index}`, chestLabels, {
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
      squash: dance.squash + (isSelected && bracing ? pulse(braceT) * 0.05 : 0),
      yaw: dance.twist + idleTwist + shiver + (isSelected ? flown.spin : 0),
    });
  }).flat();

  // How far the treasure is through its climb out of the chest. Hoisted out of
  // the reward block below because the reveal LIGHT needs it too — the lamp
  // follows the prize up, and it can only do that if both read the same clock.
  const riseT = revealAge >= timeline.lidEnd ? clamp01((revealAge - timeline.lidEnd) / Math.max(1, timeline.riseEnd - timeline.lidEnd)) : 0;

  // Reward / empty reveal rising fully clear of the selected, open chest.
  const rewardInstances: SceneInstance[] = [];
  const burst: SceneInstance[] = [];
  if (selected !== null && plan !== null && revealAge >= timeline.lidEnd) {
    const chestTop = heroTop;
    const settle = clamp01((revealAge - timeline.riseEnd) / 20);
    const rarity = outcomeRarity(session);

    if (rarity !== "loss") {
      // A win: the warm light burst fires and the treasure this chest was
      // assigned at commit time climbs fully clear to hover as the frame's
      // focal point. The prize is a pure READ of the committed tier — the
      // presentation never picks it (see `prizes/index.ts`).
      // The burst's rays and motes are the same one-frame element storm as the
      // confetti (see the celebration below); the glow disc under them carries the
      // beat on its own. Dropped on the DOM renderer.
      burst.push(...(sparse ? [] : lightBurst(chestTop, tick, burstT, heroScale, cameraElevation)));
      rewardInstances.push(...heroPrize(prizeKindOf(plan.tierId, rarity), chestTop, riseT, tick, settle, heroScale, cameraElevation));
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

  // ── the crab's errand ─────────────────────────────────────────────────────────
  // He crosses the sand to the chosen chest, takes hold of its lid, and rides it
  // into the close-up to open it himself. Two stances, one creature: while he is
  // WALKING he is a beach prop at beach scale, and once he is RIDING he is welded
  // into the chest's own frame at the chest's own scale, so the flight carries him
  // with it and the pair stay in proportion the whole way.
  const journey = crabJourney(session, speed, timeline);
  const onErrand = journey.riding || journey.approach > 0.001;
  const crabHome = addV3(state.extra.decor.props.crab, state.extra.decor.held === "crab" ? HELD_LIFT : v3(0, 0, 0));
  // Where he is heading, in world space. He walks to the chest's front-left corner
  // at GROUND level — he cannot walk to the spot he ends up clinging to, which is
  // part-way up the chest's face — and then climbs the last bit as his grip takes
  // hold. On the board the chest is unrotated, so the walk can aim at the plain
  // offset with its height dropped.
  const chestSide = selected === null ? crabHome : addV3(slotAt(selected), v3(CRAB_ON_CHEST.x, 0, CRAB_ON_CHEST.z));
  // The DOM renderer drops the courier crab too. He is a whole articulated figure
  // — shell, claws, legs, eyes — riding the one object the camera is closest to,
  // which is precisely when elements are dearest, and he is pure ceremony: the
  // chest's flight and the reveal read the same without him. See `sparseDetail`.
  const errandCrab: readonly SceneInstance[] = !onErrand || sparse
    ? []
    : crabAt(
        journey.riding
          ? {
              at: addV3(flown.position, rotateByQuat(scaleV3(CRAB_ON_CHEST, heroScale), quatYaw(heroYaw))),
              grounded: false,
              pose: errandPose(journey, easeOutCubic(heroLidT), tick),
              scale: heroScale,
              yaw: heroYaw + CRAB_FACE_YAW,
            }
          : {
              // …and climbs the chest's face over the same stretch the grip ramps
              // on, so taking hold and getting up there are one movement.
              at: addV3(lerpV3(crabHome, chestSide, journey.approach), v3(0, CRAB_ON_CHEST.y * journey.grip, 0)),
              grounded: journey.grip < 0.5,
              pose: errandPose(journey, 0, tick),
              scale: 1,
              // He walks facing where he is GOING and turns to face the chest over
              // the last stretch, so he arrives square to it instead of pivoting
              // on the spot the instant he lands.
              yaw: blendYaw(
                Math.atan2(chestSide.x - crabHome.x, chestSide.z - crabHome.z),
                CRAB_FACE_YAW,
                clamp01((journey.approach - (1 - CRAB_TURN_IN)) / CRAB_TURN_IN),
              ),
            },
        tick,
        // No pennant on the errand. Both claws are on the lid — he cannot be
        // prising a chest open and waving a flag at the same time, and the pole
        // read as a stray red stick across the close-up. He picks it back up when
        // he gets home.
        { bowtie: 0, pennant: false },
      );

  // Celebration.
  //
  // Particles are the reveal's HITCH on the DOM renderer, and the hitch is what a
  // player actually feels — the steady phases all trace at ~60fps, but one frame
  // measured 133ms while it created 689 elements. A confetti burst is dozens of
  // nodes appearing on a single frame, and each one has to have its DOM built
  // right then; there is no way to spread that over the frames before it, because
  // the frame it appears on is the first frame it exists.
  //
  // So the DOM renderer celebrates without confetti. Everything that carries the
  // outcome — the open chest, the prize, the warm light — is untouched.
  const celebration: SceneInstance[] = [];
  if (session.phase === "celebrating" && plan !== null && selected !== null && !sparse) {
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
  //
  // ONE further rule governs the close-up, and it is what stopped the reveal
  // reading as a flashlight: the BOARD'S OWN RIG LEAVES WITH THE BOARD. The
  // shared `light:focus` is the stage's focal pool — it belongs to the nine
  // chests sitting on the lagoon, and the veil is already dragging that whole
  // stage down to near-black behind the hero chest. But `focus` is re-posed onto
  // the FLYING chest and its intensity used to be ramped UP on selection
  // (0.5 → 0.9), so the moment the chest arrived in close-up it was lit by two
  // rigs at once: the full board rig AND the dedicated reveal kiss below. The
  // measured light sum on the open lid was ~2.2 against the board's ~1.5, which
  // is a full stop of over-exposure on surfaces that were already at the top of
  // the tone curve — so every warm face clipped and the wood's hue went with it.
  //
  // Fading the focus light out along the FLIGHT (not the selection) is the
  // honest correction: a chest that has left the board is no longer standing in
  // the board's pool of light. The sun (`light:key`) and the sky (`light:fill`)
  // stay untouched — they are the world, and they are what keeps modelling the
  // chest's forms — and the close-up is then lit by exactly one purpose-built
  // lamp, the warm `light:chest` kiss.
  const lights: SceneLight[] = stageLights(focus, (0.5 + 0.4 * selectEase) * (1 - flight)).map((entry) => {
    const fill = { key: entry.key, light: { ...entry.light, color: [0.9, 0.94, 1, 1] as Rgba, intensity: 0.3 } };
    const key = { key: entry.key, light: { ...entry.light, intensity: 1.15 } };
    return entry.key === "light:fill" ? fill : entry.key === "light:key" ? key : entry;
  });
  if (selected !== null && revealAge >= timeline.pauseEnd) {
    const warm = clamp01((revealAge - timeline.pauseEnd) / 12);
    // The reveal's one lamp FOLLOWS THE SUBJECT, because the subject moves.
    //
    // For the seam and lid beats the subject is the chest's mouth, and the lamp
    // hangs just above it. Then the treasure climbs a full chest-height clear
    // and becomes the subject — and a lamp still parked at the mouth lights the
    // empty box while the thing the shot is about hangs in the dark above it.
    // That is precisely what happened when the reveal was recomposed as a
    // poster: the gold bar rendered brown against a brightly lit chest, the
    // contrast exactly inverted. So the lamp rides the climb, ending up beside
    // the risen prize — offset onto the KEY'S OWN SIDE (up, right and forward,
    // the direction `light:key` arrives from) rather than straight down the view
    // axis, so it reinforces the sun's modelling instead of flattening it with a
    // second frontal source.
    const kissAt = lerpV3(
      addV3(flown.position, scaleV3(v3(0, 1.1, 0.3), heroScale)),
      addV3(prizeCentre(heroTop, riseT, heroScale), scaleV3(v3(0.62, 0.6, 0.51), 0.9 * heroScale)),
      riseT,
    );
    lights.push({
      key: "light:chest",
      // The close-up's ONE purpose-built lamp: a warm kiss that lifts the seam,
      // the interior, and the near face of whatever rose out of the chest. It is
      // now the only thing added on top of the sun and sky (the board's focal
      // pool having left with the board), so it can afford to be a real light
      // rather than the apologetic remnant it had to be when it was the fourth
      // lamp stacked on the same square metre.
      light: { color: [1, 0.82, 0.45, 1], intensity: 0.62 * warm * (winReveal ? 1 : 0.4), kind: "point", position: kissAt },
    });
  }
  if (winReveal && selected !== null && burstT > 0 && burstT < 1) {
    lights.push({
      key: "light:burst",
      // The flash as the lid lands. Halved against the champion's 1.8: that value
      // was set when the reveal already sat at the clip point, so the flash could
      // only be read as "everything goes white for a moment". Against the darker,
      // hue-intact close-up the same beat now reads as a genuine flare of light
      // across the wood — a smaller number doing more work.
      light: { color: [1, 0.9, 0.6, 1], intensity: 0.9 * pulse(burstT), kind: "point", position: addV3(flown.position, scaleV3(v3(0, 1.5, 0.2), heroScale)) },
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
    //
    // It eases DOWN along the flight for the same reason the board's focal light
    // does: a hemisphere of hot sand bouncing warm light back is a fact about
    // standing on the beach, and the hero chest has left it — the veil has pulled
    // the whole shore to near-black behind it. Holding the full beach ambient
    // through the close-up meant every face of the chest carried a bright floor
    // it was no longer standing in, which is a third of the over-exposure that
    // made the open chest read as a lit box. It only eases (not to zero): the
    // chest is still lit by the sky, just no longer by the sand.
    ambient: [lerp(0.28, 0.19, flight), lerp(0.25, 0.165, flight), lerp(0.21, 0.14, flight), 1],
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
      // The crab is skipped here while he is on his errand — he is emitted AFTER
      // the veil instead, so the shot that is about him does not dim him away
      // with the rest of the shore.
      ...beachDecor(tick, seed, state.extra.decor, !onErrand),
      ...chests,
      ...backgroundVeil(camera, framing, flight),
      ...errandCrab,
      ...burst,
      ...rewardInstances,
      ...celebration,
    ],
    labels: chestLabels,
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
/**
 * The chest's world ENVELOPE, as offsets from its base origin — the volume a
 * punched hole has to cover, and nothing more.
 *
 * A hole exists so the ripple net is not painted across a chest. It used to be a
 * CIRCLE, of a world radius (0.82) larger than the chest's own half-width, taken
 * at a fixed height up the chest — so it overshot the chest on every side, and
 * because the overlay carries a broad edge TINT and not only the net, the
 * overshoot was not "no ripples here", it was a disc of visibly untinted pool.
 * That is the "orb": the back row wore it most plainly, but every chest had one.
 *
 * A circle cannot be the answer, at any radius: a chest is a BOX, so a circle that
 * covers its corners must overshoot its edges, and one that hugs its edges must
 * leave its corners netted. The hole is now the chest's actual screen silhouette —
 * the convex hull of this envelope's eight corners, projected — which is the one
 * shape that both covers the chest and stops there.
 *
 * The numbers are the chest's real extents: the lid (plus its gold rail's 0.05
 * overhang) is the widest part, the rail the frontmost, the dome the tallest.
 *
 * The envelope is NOT a plain box, and that matters. The lid is barrel-topped, so
 * a box drawn to `CHEST_HEIGHT` has two top corners the chest never occupies —
 * enough, at this camera pitch, to leave a bright tab of pool above each lid. So
 * the top is the dome's CREST (a ridge along x at the lid's mid-depth) sitting over
 * the flat-lidded box below it, and the hull wraps that instead.
 */
const CHEST_HOLE_HALF_WIDTH = (LID.x + 0.05) / 2;
const CHEST_HOLE_BACK = -BODY.z / 2;
const CHEST_HOLE_FRONT = LID.z / 2 + 0.05;
/** Top of the lid's flat BOARD — where the box part of the silhouette ends. */
const CHEST_HOLE_BOARD_TOP = BODY.y + LID.y;
/**
 * The barrel top, as the half-ellipse `lidArc` actually sweeps it: centred on the
 * lid board at the lid's mid-depth, rising `CHEST_LID_ARCH` and reaching
 * `LID.z / 2` fore and aft, both widened by the end ribs' swell so the hole clears
 * the raised ribs too.
 *
 * Sampled rather than reduced to its crest. A single crest point leaves the hull
 * cutting a CHORD under the dome, and a hull that falls short is the mirror of the
 * orb: instead of pool showing beside the chest, the pool's tint paints a hazy band
 * across the back of the lid. Five samples put the outline on the curve.
 */
const CHEST_HOLE_ARCH_Y = BODY.y + LID.y;
const CHEST_HOLE_ARCH_Z = -BODY.z / 2 + LID.z / 2;
const CHEST_HOLE_ARCH_RISE = CHEST_LID_ARCH + LID_RIB_SWELL;
const CHEST_HOLE_ARCH_DEPTH = LID.z / 2 + LID_RIB_SWELL + LID_ARC_THICKNESS / 2;
const CHEST_HOLE_ARCH_SAMPLES = 5;
/**
 * Screen-space slack on the silhouette, as a FRACTION of its own size rather than
 * a pixel count — the old fixed 6px was a fifth of a far chest's width and a tenth
 * of a near one's, which is precisely the asymmetry that made the back row worst.
 *
 * It is here because the overlay knows each chest's SLOT but not its live pose. Two
 * of the three displacements are handled exactly instead of being absorbed here —
 * a HELD chest rides up by `CHEST_TIMING.heldLift`, which `chestHole` takes as its
 * `lift`, and the idle dance only twists and squashes a chest (it does not scoot
 * it; see `dancePose`'s use in `chestScene`). What is left for slack is the idle
 * bob (0.014 world units, under a pixel here), the dance's ±2.8° twist, and its
 * 1.7% swell — a few percent of the silhouette, not a doubling of it.
 */
const CHEST_HOLE_SLACK = 1.02;
/** The lagoon's water palette. The EDGE color matches the rendered pool AT THE
 * SHORE — which is now the paler shallow shelf, not the deeper open water — so
 * the shoreline cover is invisible except that it hides the net. (Left at the
 * deep-water tint it would re-darken the shelf band at 32% and quietly undo the
 * two-tone step the 3D discs draw.) The LINE/TROUGH pair
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
const POOL_EDGE_COLOR = "rgb(102, 196, 206)";
const WATER_LINE_COLOR = "rgba(210, 244, 252, 0.95)";
// The ripple TROUGH is a fraction of the water it is drawn over, so it moved with
// the re-solved `LagoonWater` above. At (16, 92, 92) x 0.6 x the layer's 0.32 it
// took 19 levels of green out of the surface — calibrated against the old, darker
// pool. On the lifted turquoise that same tint would have dragged the troughs to
// ~174 green where the reference's darkest ripple sits at ~191: the reference's
// net is mostly BRIGHTER caustic lines on an even turquoise, not dark gouges in it.
// Seated higher and at a lower alpha so the net keeps exactly its current legibility
// as a ripple pattern without re-darkening the body the lift just corrected.
const WATER_TROUGH_COLOR = "rgba(40, 150, 152, 0.5)";
const WATER_SPARKLE_COLOR = "rgba(234, 251, 255, 0.9)";
const WATER_SHALLOW_COLOR = "rgba(148, 224, 240, 0.44)";

/** A point in the shared 960×600 overlay space. */
interface OverlayPoint {
  readonly x: number;
  readonly y: number;
}

/**
 * Convex hull of a handful of screen points (Andrew's monotone chain).
 *
 * Eight projected box corners come in; the outline that encloses them comes out.
 * A hull rather than a fixed vertex order because WHICH corners are on the
 * silhouette depends on where the chest sits relative to the camera — the near
 * row shows its front face, the far row shows more of its top — and a hard-coded
 * winding would cross itself for some of them. The clip is applied with the
 * `evenodd` rule, so a self-crossing hole would un-punch its own overlap.
 */
const screenHull = (points: readonly OverlayPoint[]): readonly OverlayPoint[] => {
  const sorted = [...points].sort((p, q) => p.x - q.x || p.y - q.y);
  const cross = (o: OverlayPoint, a: OverlayPoint, b: OverlayPoint): number =>
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const half = (source: readonly OverlayPoint[]): OverlayPoint[] => {
    const chain: OverlayPoint[] = [];
    for (const p of source) {
      while (chain.length >= 2 && cross(chain[chain.length - 2]!, chain[chain.length - 1]!, p) <= 0) {
        chain.pop();
      }
      chain.push(p);
    }
    chain.pop();
    return chain;
  };
  return [...half(sorted), ...half([...sorted].reverse())];
};

/**
 * One chest's punched hole: the projected silhouette of its envelope, grown by
 * `CHEST_HOLE_SLACK` about its own centroid.
 *
 * Growing about the centroid rather than adding a pixel margin keeps the slack
 * proportional to how big the chest actually is on screen, which is the whole
 * point — a far chest is smaller, so its slack is smaller too.
 *
 * `lift` is how far the chest currently rides above its slot. It is a parameter
 * rather than an assumption because a chest the player is DRAGGING rises a clear
 * `CHEST_TIMING.heldLift` out of the board: a hole cut at the slot would leave the
 * net painted across the top of the very chest in the player's hand.
 */
const chestHole = (camera: Camera3D, base: EngineVec3, lift: number): readonly OverlayPoint[] => {
  const corners = [-1, 1].flatMap((sx) => [
    // The flat-lidded box: four corners a side.
    ...[0, CHEST_HOLE_BOARD_TOP].flatMap((y) =>
      [CHEST_HOLE_BACK, CHEST_HOLE_FRONT].map((z) => v3(sx * CHEST_HOLE_HALF_WIDTH, y + lift, z)),
    ),
    // The barrel top, sampled along its sweep.
    ...Array.from({ length: CHEST_HOLE_ARCH_SAMPLES }, (_, i) => {
      const angle = -Math.PI / 2 + (i / (CHEST_HOLE_ARCH_SAMPLES - 1)) * Math.PI;
      return v3(
        sx * CHEST_HOLE_HALF_WIDTH,
        CHEST_HOLE_ARCH_Y + CHEST_HOLE_ARCH_RISE * Math.cos(angle) + lift,
        CHEST_HOLE_ARCH_Z + CHEST_HOLE_ARCH_DEPTH * Math.sin(angle),
      );
    }),
  ]);
  const onScreen = corners
    .map((local) => worldToCanvas(camera, addV3(base, local)))
    .filter((p): p is OverlayPoint => p !== null);
  if (onScreen.length < 3) {
    return [];
  }
  const hull = screenHull(onScreen);
  const cx = hull.reduce((sum, p) => sum + p.x, 0) / hull.length;
  const cy = hull.reduce((sum, p) => sum + p.y, 0) / hull.length;
  return hull.map((p) => ({ x: cx + (p.x - cx) * CHEST_HOLE_SLACK, y: cy + (p.y - cy) * CHEST_HOLE_SLACK }));
};

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
  // Each hole is the chest's own projected SILHOUETTE (see `chestHole`), so it
  // covers the chest and no pool around it.
  // Read off the LIVE layout, not the grid: the holes exist so the ripple net is
  // not painted over the chests, so they have to follow a chest the player has
  // dragged — otherwise a moved chest wears a net and leaves a hole behind it.
  const drag = state.extra.chests;
  const heldIndex = drag.grab?.dragging === true ? drag.grab.index : null;
  const holes = Array.from({ length: count }, (_, i) =>
    chestHole(camera, drag.slots[i] ?? chestPosition(i, count), i === heldIndex ? CHEST_TIMING.heldLift : 0),
  ).filter((hole) => hole.length >= 3);
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
    //  CAUSTIC FREQUENCY. `cellSize` is the hexagon's centre-to-vertex radius, so
    //  the net's pitch on screen is 1.5x it. At 58 the pitch was ~87px across a
    //  pool only ~560px wide in this overlay space — seven cells edge to edge, big
    //  enough that the eye stops reading "ripples" and starts reading the LATTICE:
    //  whole hexagons are countable in the judged frame, which is the exact
    //  board-game-tile failure `EDGE_KEEP_PERCENT` exists to break up (it cannot,
    //  at that size — dropping 42% of the edges of a seven-cell grid just makes a
    //  gappy seven-cell grid). The reference's caustics are fine CRAZING: on the
    //  order of seventeen cells across the pool, thin bright filaments, no readable
    //  repeat unit. 22 puts the pitch at ~33px, which is that frequency.
    cellSize: 22,
    //  Both layers of the net drift, and their offset shows as a doubled line. At a
    //  33px pitch, 2.4px of separation is a visible ghost on a hairline stroke, so
    //  the drift comes down with the cell.
    driftAmount: 1.6,
    edgeColor: POOL_EDGE_COLOR,
    edgeFadePx: 36,
    lineColor: WATER_LINE_COLOR,
    //  A filament, not a wire: 2.2px was a fifteenth of the old cell and would be a
    //  twenty-second of the new pitch's worth of ink, thickening the finer net into
    //  a mesh. The reference's caustic lines are ~1.5px at this scale.
    lineWidth: 1.5,
    //  Thinner, finer strokes lay down less ink over the same water, so the net
    //  would read WEAKER than before at the old alpha even though there is more of
    //  it. 0.42 holds the caustics as legible as the reference's without touching
    //  the broad tints (`depthColor`/`glint` stay off, so no hole can be ringed).
    opacity: 0.42 * strength,
    shallowColor: WATER_SHALLOW_COLOR,
    //  Blur is scaled to the stroke, not to the pool: at 1.4px it was equal to the
    //  new line width, which dissolves a 1.5px filament into haze instead of
    //  softening its edge.
    softnessPx: 1,
    sparkleColor: WATER_SPARKLE_COLOR,
    timeSeconds: view.nowMs / 1000,
    troughColor: WATER_TROUGH_COLOR,
    // Bow every filament to the effect's full authored curve. The reference's
    // lagoon is a net of WANDERING caustic ribbons; drawn straight, the same net
    // reads as a hex TILING laid over the pool — the single most obviously
    // synthetic thing in the frame, and the one the eye finds first because the
    // water is the second-largest area in it. Curvature is not something this
    // caller can reach by choosing `cellSize` (a smaller cell is a denser hex
    // grid, not a curvier one), so it is an engine option; 1 is "as authored".
    waviness: 1,
    traceHoles: (c) => {
      for (const hole of holes) {
        hole.forEach((p, i) => (i === 0 ? c.moveTo(p.x, p.y) : c.lineTo(p.x, p.y)));
        c.closePath();
      }
    },
    tracePool: (c) => {
      rim.forEach((p, i) => (i === 0 ? c.moveTo(p.x, p.y) : c.lineTo(p.x, p.y)));
      c.closePath();
    },
  });
};
