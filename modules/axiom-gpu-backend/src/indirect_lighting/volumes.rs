//! **Interior indirect probe volumes** — the gate that stops skylight reaching
//! the middle of a closed room, and the two-band bounce fill it gates.
//!
//! This is the thing the reference's boot log names:
//!
//! ```text
//! [render] indirect gate: 2 interior volumes
//! ```
//!
//! # Read this before looking for `render/probe.js`
//!
//! **`src/render/probe.js` is not a light probe and contains no indirect
//! lighting.** It is `RenderProbeScene` — a procedural *blockout validation
//! scene* (fBm-generated albedo/normal/ORM textures, fourteen buildings,
//! twenty-two crates, four metal spheres, three emissive lamps) that the
//! renderer adds on frame 0 if the world subsystem has not yet put geometry in
//! the scene, and deletes itself the moment six foreign meshes appear
//! (`index.js:1218-1245`, `_ensureProbe`). Its own header says so: "Nothing here
//! is shipped content." It is content, it is driven end to end by the app's
//! `Rng` (xoshiro128\*\*, `src/core/rng.js`), and a `modules/` crate may not
//! depend on an app — so it could not live here even if it belonged here. Its
//! correct Axiom home is `apps/shmup/src/render/probe.rs`, and it is **not
//! ported by this slice**.
//!
//! The indirect lighting the boot log describes lives in two other files:
//!
//! | source | what it owns | ported here |
//! |---|---|---|
//! | `materialpatch.js:206-238` (`owInteriorGate`) | the volume test + blend | yes |
//! | `materialpatch.js:249-256` (`owSunBounce`) | the warm anti-sun wrap | yes |
//! | `materialpatch.js:148-186` (the `lights_fragment_maps` injection) | the two-band fill and the IBL budget | yes |
//! | `index.js:1091-1152` (`_updateBounceFill`) | the CPU side that produces the band colours | yes, [`bounce_fill_bands`] |
//! | `index.js:1156-1216` (`_updateRooms`) | building footprints -> volumes | yes, [`interior_volumes`] |
//! | `materialpatch.js` `owSampleAO` / `owMultiBounce` / `owSpecularOcclusion` / `owContactShadow` / `MaterialPatcher` | AO and contact-shadow plumbing | **no** — those belong to the `materialpatch.js` / `gtao.js` slices, and duplicating them here is how six copies of `Math.hypot` happened |
//!
//! # Why two volumes, from fifteen light anchors
//!
//! The brief guessed the two volumes were built from the level's fifteen
//! interior light anchors. They are not, and the arithmetic that produces the
//! `2` is worth writing down because it is a load-bearing filter:
//! `_updateRooms` walks `world.buildings`, keeps those whose spec has
//! `enterable === true`, and then **drops any that are `collapse` or `ruin`** —
//! "a collapsed or ruined shell is open to the sky: it must keep its skylight,
//! or the one room in the level with a hole in its roof is the one that reads as
//! a cave." Axiom's ported level has exactly three enterable buildings — `W2`,
//! `E1`, `E3` (`apps/shmup/src/world/layout.rs:317, :462, :542`) — and `E3`
//! carries `ruin: true`. Three minus one is the two the log prints. The fifteen
//! anchors (`apps/shmup/src/world/system.rs:265`, `interior_anchors`) are
//! point lights inside those shells; they are a different quantity and they do
//! not define a volume. A port that built two volumes out of fifteen anchors
//! would agree with the log and be wrong.
//!
//! # How this relates to `axiom_host::FrameAmbient` — replace, do not add
//!
//! [`axiom_host::FrameAmbient`] is a strength-folded `sky`/`ground` linear-RGB
//! pair that a backend applies as `mix(ground, sky, up)`. That is *the same
//! quantity* as the reference's `owSkyFill` / `owGroundFill` — the two-band
//! hemispheric bounce fill — and it is **not** the reference's IBL. So the
//! answer to "replace, modulate, or a new lane?" is: **replace the blend, keep
//! the carrier, and add one lane.** Concretely, three differences, in the order
//! they matter:
//!
//! 1. **The blend is wrong, not just ungated.** `FrameAmbient` *lerps* between
//!    ground and sky by the normal's up-component. The reference deliberately
//!    does not: the two bands are **independently gated** by two smoothsteps
//!    (`owFillDir`), and the source's comment says why — "Lerping them put a
//!    warm street bounce on every wall and made shadows come out warmer than the
//!    sun that cast them." With the shipped `owFillDir = (-0.95, 0.85, -0.05,
//!    0.7)`, a vertical wall gets `smoothstep(-0.95, 0.85, 0) = 0.5416` of the
//!    sky band *and* `smoothstep(-0.05, 0.7, 0) = 0.0127` of the ground band — a
//!    sum of `0.554`, not a partition of `1.0`. This is the single change that makes
//!    a shaded facade read as skylit, and it costs nothing but the two gates.
//! 2. **Both bands are multiplied by the interior gate and by `sqrt(ao)`** —
//!    `sqrt`, never `ao`, because "a fill term that AO can drive to zero is not a
//!    fill, it is just another way to make a black hole." The gate is what makes
//!    an interior darker than the street framed in its own doorway. `FrameAmbient`
//!    has no gate, which is exactly why Axiom's interiors read flat.
//! 3. **There is a third term `FrameAmbient` has no room for**: the warm anti-sun
//!    wrap ([`sun_bounce`]), scaled off the *ground* band. It is a directional
//!    term, so it cannot be folded into a hemisphere pair.
//!
//! The minimal honest frame contract is therefore: keep `FrameAmbient`'s two
//! colours (they are [`bounce_fill_bands`]'s two outputs), and add **one** new
//! neutral frame lane — a `FrameIndirect` peer of `frame_ambient.rs` carrying
//! `fill_dir: [f32; 4]`, `fill_gain: [f32; 2]`, `indirect: [f32; 4]`, the level
//! transform and up to [`MAX_ROOMS`] volumes. That is the only new data; every
//! other input this module needs (world position, world normal, AO, the sun
//! direction) already exists inside the fragment stage.
//!
//! # What the frame-graph sibling must supply
//!
//! This module is pure arithmetic over data it does not own. The `render/index.js`
//! slice owns all three inputs:
//!
//! - the **volume list and level transform**, once, when the world first appears
//!   (`_updateRooms`; [`interior_volumes`] and [`LevelTransform`] here);
//! - the **band colours**, per frame, from the sky's published ambient
//!   (`_updateBounceFill`; [`bounce_fill_bands`] here);
//! - the **viewmodel exception**: `index.js:1410-1429` sets `owIndirect.z = 0`
//!   for the view-scene pass, because "the interior gate is a WORLD-space volume
//!   test and the viewmodel's world position is the camera's, so standing in a
//!   shop would drop the weapon's whole indirect term at once." A frame graph
//!   that renders a viewmodel pass and forgets this will dim the gun indoors.
//!   The same block also scales both bands by `settings.viewFillOcclusion`.
//!
//! And it owns the one number this module deliberately does not compute:
//! [`LevelTransform::from_world_axis`] takes the axis length rather than calling
//! `hypot`, because the source calls `Math.hypot`, which in V8 is a max-scaled
//! Kahan sum and not `f64::hypot`. That primitive lives in `apps/shmup/src/jsmath.rs`
//! (measured to disagree with the plain root on 37.5% of metre-scale triples), and
//! a module may not reach into an app for it.

// `MAX_ROOMS` and `sun_bounce` are the parent's, and so was this file's copy of
// `FILL_DIR`, which nothing here read. This file and
// `indirect_lighting.rs` were briefed as separate slices, converged on the same
// source, and produced two independent transcriptions that agreed everywhere —
// same epsilon placement, same written-out `normalize`, same trailing division
// left as a division. One survives; the agreement is recorded in the notes.
//
// The constants this file still owns are the ones only it names (`ROOM_FEATHER`,
// `AO_GATE`, the band budgets). The parent writes those as the source's literals
// inside the shader composition, where the literal is the thing being checked
// against the GLSL.
use super::{sun_bounce, MAX_ROOMS};


/// The interior-volume feather, in metres of depth inside the box
/// (`materialpatch.js:224`, `smoothstep( 0.06, 0.30, d )`).
///
/// The gate keys off **depth inside the footprint**, not containment, and that
/// is the whole trick: a facade's outer skin sits exactly on the boundary at
/// depth 0 and its inner skin one wall thickness in, so a 6..30 cm feather
/// separates the two faces of the same wall with no per-room geometry.
pub(crate) const ROOM_FEATHER: (f32, f32) = (0.06, 0.30);

/// The AO-driven half of the gate (`materialpatch.js:231-233`): even outside a
/// tagged volume, a pocket the sky cannot see should not get full skylight.
/// `smoothstep( 0.45, 0.98, ao )`, mixed in at [`AO_GATE_MIX`] "so it shapes
/// rather than doubling up as a second AO multiply".
pub(crate) const AO_GATE: (f32, f32) = (0.45, 0.98);

/// How much of the AO gate is mixed in (`materialpatch.js:233`).
pub(crate) const AO_GATE_MIX: f32 = 0.6;


/// The wrap constant in `owSunBounce` (`materialpatch.js:255`). Tight on purpose
/// (0.12, not 0.35): "a face turned away from the sunlit side of the street
/// receives none of its bounce."
pub(crate) const SUN_BOUNCE_WRAP: f32 = 0.12;

/// The anti-sun direction's fixed up-component (`materialpatch.js:254`).
pub(crate) const ANTI_SUN_UP: f32 = 0.28;

/// The `vec3( 1e-4 )` added to the anti-sun vector before normalising
/// (`materialpatch.js:254`) — a degeneracy guard for a sun at the zenith, added
/// to **all three** components, not just the horizontal ones.
pub(crate) const ANTI_SUN_EPSILON: f32 = 1e-4;

/// `settings.skyFill` (`index.js:412`) — the cool skylight band's budget, as a
/// fraction of the whole-sky reference level. "The frame's ONLY strongly
/// chromatic indirect term."
pub(crate) const SKY_FILL: f64 = 0.32;

/// `settings.groundFill` (`index.js:414`) — the warm street bounce, as a
/// fraction of the key's intensity.
pub(crate) const GROUND_FILL: f64 = 0.013;

/// `settings.bounceFill` (`index.js:418`) — the anti-sun wrap term's budget.
/// Carried as a *ratio against* [`GROUND_FILL`] in `owFillGain.y`, because the
/// wrap is scaled off the ground band's colour.
pub(crate) const BOUNCE_FILL: f64 = 0.008;

/// `settings.iblDiffuse` (`index.js:423`) — the prefiltered environment's
/// diffuse budget. "Scaling its diffuse here is the only place the total
/// indirect budget can actually be controlled from."
pub(crate) const IBL_DIFFUSE: f64 = 0.030;

/// `settings.interiorIndirect` (`index.js:427`) — the indirect floor deep inside
/// a volume. Not zero: "floored so nothing ever goes black."
pub(crate) const INTERIOR_INDIRECT: f64 = 0.035;

/// The whole-sky ambient's published fraction of the beam (`index.js:1105-1135`,
/// `SKY_AMBIENT_FRACTION` in `apps/shmup/src/sky/system.rs:126`). Used twice, in
/// opposite directions: to synthesise `_ambLevel` when no sky is published, and
/// to recover the beam reference from it when one is.
pub(crate) const SKY_AMBIENT_FRACTION: f64 = 0.15;

/// The hue `_updateBounceFill` falls back to when no sky subsystem has published
/// an ambient colour (`index.js:1107`).
pub(crate) const FALLBACK_SKY_HUE: [f64; 3] = [0.36, 0.56, 1.0];

/// The chroma push applied to the sky band's hue (`index.js:1122`, `const k =
/// 1.18`). `sky.ambientColor` is the *whole*-sky average; this band is only what
/// an up-facing or vertical surface sees of the upper hemisphere, which is the
/// bluest part of it. Pushing the chroma out from its own luminance recovers
/// that "without inventing a hue".
pub(crate) const CHROMA_PUSH: f64 = 1.18;

/// The ground albedo the warm band takes the key's colour through
/// (`index.js:1142`) — the same dry-street albedo the sky dome itself uses.
pub(crate) const GROUND_ALBEDO: [f64; 3] = [0.33, 0.29, 0.225];

/// The bottom of an interior volume (`index.js:1209`, `roomsY[n].set(-0.8, …)`)
/// — below the ground slab, "so the floor plate counts as interior".
pub(crate) const ROOM_FLOOR_Y: f64 = -0.8;

/// How far under the roof deck (or under a setback's terrace) a volume stops
/// (`index.js:1205-1208`, `- 0.06`).
pub(crate) const ROOM_ROOF_INSET: f64 = 0.06;

/// `b.roofY ?? 12` (`index.js:1205`) — the roof height assumed for a building
/// that publishes none.
pub(crate) const DEFAULT_ROOF_Y: f64 = 12.0;

/// One coarse interior volume, in the layout the two uniform arrays carry.
///
/// `rect` is `owRooms[i]` — `(cx, cz, hx, hz)` in **level** space; `y` is
/// `owRoomsY[i]` — `(y0, y1, 0, 0)` in **world** height (the level transform is a
/// yaw plus an XZ translate, so height is shared).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InteriorVolume {
    /// `(cx, cz, hx, hz)` in level space.
    pub(crate) rect: [f32; 4],
    /// `(y0, y1, 0, 0)` in world height.
    pub(crate) y: [f32; 4],
}

/// The world -> level 2D transform, `owRoomXf` = `(cos, sin, tx, tz)`
/// (`materialpatch.js:60`).
///
/// The level is authored on one yaw, so a world position reaches level space
/// through a 2D rotation — cheap enough to do per fragment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LevelTransform {
    /// `(cos, sin, tx, tz)`, exactly the vec4 the fragment stage reads.
    pub(crate) xf: [f32; 4],
}

impl LevelTransform {
    /// `_updateRooms`'s transform recovery (`index.js:1174-1191`).
    ///
    /// The yaw is recovered from two *transformed* level-space points rather
    /// than read from the world subsystem, "so this stays correct if the world
    /// subsystem re-authors its transform": `origin` is `levelToWorld(0,0,0)`
    /// and `x_axis_end` is `levelToWorld(1,0,0)`, both as `(x, z)`.
    ///
    /// `axis_length` is the source's `Math.hypot(c, sn)`, and it is a
    /// **parameter** rather than a call. V8's `Math.hypot` is a max-scaled Kahan
    /// sum; `f64::hypot` is not the same function, and this port has already
    /// measured the two disagreeing on 37.5% of metre-scale inputs. The faithful
    /// primitive is `apps/shmup/src/jsmath.rs::hypot`, which a module may not
    /// reach, so the caller supplies the number it computed.
    pub(crate) fn from_world_axis(
        origin: [f64; 2],
        x_axis_end: [f64; 2],
        axis_length: f64,
    ) -> LevelTransform {
        let (ox, oz) = (origin[0], origin[1]);
        let c = x_axis_end[0] - ox;
        let sn = x_axis_end[1] - oz;
        let inv = 1.0 / f64::max(1e-6, axis_length);
        let cs = c * inv;
        let sni = sn * inv;
        // world -> level: p' = R^T (p - o), expanded exactly as the source writes
        // the four components.
        LevelTransform {
            xf: [
                cs as f32,
                sni as f32,
                -(ox * cs + oz * sni) as f32,
                -(-ox * sni + oz * cs) as f32,
            ],
        }
    }
}

/// One building, as much of it as `_updateRooms` reads.
///
/// A deliberately flat input struct rather than a trait: the world data lives in
/// an app, and the rule that turns a building into a volume is the part that
/// belongs to the renderer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BuildingFootprint {
    /// `spec.enterable === true`.
    pub(crate) enterable: bool,
    /// `spec.collapse === true` — excluded, the shell is open to the sky.
    pub(crate) collapse: bool,
    /// `spec.ruin === true` — excluded, for the same reason.
    pub(crate) ruin: bool,
    /// `spec.x`, `spec.z` — the footprint centre in level space.
    pub(crate) center: [f64; 2],
    /// `spec.w`, `spec.d` — the footprint's full extents (halved here).
    pub(crate) size: [f64; 2],
    /// `b.roofY`, or [`DEFAULT_ROOF_Y`] when the building publishes none.
    pub(crate) roof_y: Option<f64>,
    /// `spec.setback.from` — the floor index a setback starts at, if any.
    pub(crate) setback_from: Option<usize>,
    /// `b.floorY` — per-floor heights, indexed by [`Self::setback_from`].
    pub(crate) floor_y: Vec<f64>,
}

/// `_updateRooms`'s volume list (`index.js:1193-1214`), in source order.
///
/// The order is the building list's and it is preserved: the volumes are a
/// uniform array indexed by the gate's loop, so the list is order-dependent even
/// though the gate reduces with `max` and is therefore order-*insensitive* in
/// value. Truncated at [`MAX_ROOMS`], which is where the source `break`s.
///
/// A volume's top is the roof deck less [`ROOM_ROOF_INSET`] — **or**, when the
/// building has a setback whose start floor publishes a height, that floor's
/// height less the same inset, "whose terrace is outdoors and sits inside the
/// footprint". A setback index the `floorY` array does not cover falls back to
/// the roof, matching the source's `b.floorY?.[sb] !== undefined` guard.
pub(crate) fn interior_volumes(buildings: &[BuildingFootprint]) -> Vec<InteriorVolume> {
    buildings
        .iter()
        .filter(|b| b.enterable & !b.collapse & !b.ruin)
        .take(MAX_ROOMS)
        .map(|b| {
            let roof_top = b.roof_y.unwrap_or(DEFAULT_ROOF_Y) - ROOM_ROOF_INSET;
            let setback_top = b
                .setback_from
                .and_then(|sb| b.floor_y.get(sb))
                .map(|fy| fy - ROOM_ROOF_INSET);
            InteriorVolume {
                rect: [
                    b.center[0] as f32,
                    b.center[1] as f32,
                    (b.size[0] * 0.5) as f32,
                    (b.size[1] * 0.5) as f32,
                ],
                y: [
                    ROOM_FLOOR_Y as f32,
                    setback_top.unwrap_or(roof_top) as f32,
                    0.0,
                    0.0,
                ],
            }
        })
        .collect()
}

/// GLSL `clamp( x, lo, hi )` — `min( max( x, lo ), hi )`, written out because
/// that expansion is the specification and a builtin's is not guaranteed to be.
fn glsl_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    f32::min(f32::max(x, lo), hi)
}

/// GLSL `mix( x, y, a )` — `x * (1 - a) + y * a`, in that term order.
fn glsl_mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// GLSL `smoothstep( e0, e1, x )`, written out.
///
/// `t = clamp( (x - e0) / (e1 - e0), 0, 1 ); t * t * (3 - 2 * t)`. The `/` is a
/// **division**, not a multiply by a precomputed reciprocal — five of the ten
/// defects found in this port's `sky/` slice were exactly that substitution.
fn glsl_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = glsl_clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// World position -> level-space `(lx, lz)` (`materialpatch.js:216-217`).
///
/// Computed once per fragment, outside the volume loop, exactly as the source
/// hoists it.
pub(crate) fn level_xz(world_pos: [f32; 3], xf: [f32; 4]) -> (f32, f32) {
    (
        world_pos[0] * xf[0] + world_pos[2] * xf[1] + xf[2],
        -world_pos[0] * xf[1] + world_pos[2] * xf[0] + xf[3],
    )
}

/// Signed depth inside one volume (`materialpatch.js:221-224`) — positive
/// inside, negative outside, and its magnitude is the distance to the nearest
/// face.
///
/// The `min` nesting is the source's: the two horizontal faces are reduced
/// against each other first, then the two vertical ones, then the pair. Flat
/// four-way reduction would be a different expression tree.
pub(crate) fn room_depth(lx: f32, lz: f32, world_y: f32, room: &InteriorVolume) -> f32 {
    let r = room.rect;
    let ry = room.y;
    f32::min(
        f32::min(r[2] - (lx - r[0]).abs(), r[3] - (lz - r[1]).abs()),
        f32::min(world_y - ry[0], ry[1] - world_y),
    )
}

/// `owInteriorGate`'s inner reduction (`materialpatch.js:213-227`) — how far
/// inside *any* volume this fragment is, in `0..=1`.
///
/// `count` is `owIndirect.z`, the live volume count as a float. Both of the
/// source's guards are reproduced: the `owIndirect.z > 0.5` enable, and the
/// `int( owIndirect.z )` truncation that the loop's `i >= n` break tests. They
/// overlap for every value `_updateRooms` can write (it writes an integer), and
/// they are both kept because "either one alone" is a claim about the caller.
pub(crate) fn indoor(
    world_pos: [f32; 3],
    xf: [f32; 4],
    count: f32,
    rooms: &[InteriorVolume],
) -> f32 {
    let live = f32::from(u8::from(count > 0.5));
    // `int()` in GLSL truncates toward zero; a negative count therefore yields an
    // `n` no iteration can satisfy, which is the zero-room case either way.
    let n = ((count * live) as usize).min(MAX_ROOMS);
    let (lx, lz) = level_xz(world_pos, xf);
    rooms.iter().take(n).fold(0.0_f32, |acc, room| {
        f32::max(
            acc,
            glsl_smoothstep(
                ROOM_FEATHER.0,
                ROOM_FEATHER.1,
                room_depth(lx, lz, world_pos[1], room),
            ),
        )
    })
}

/// `owInteriorGate`'s blend (`materialpatch.js:228-237`): 1 outdoors, falling to
/// `interior_floor` deep inside a volume.
///
/// `indoor` is [`indoor`]'s reduction and `ao` the sampled visibility.
pub(crate) fn interior_gate(indoor: f32, ao: f32, interior_floor: f32) -> f32 {
    let ao_gate = glsl_mix(1.0, glsl_smoothstep(AO_GATE.0, AO_GATE.1, ao), AO_GATE_MIX);
    let g = f32::min(1.0 - indoor, ao_gate);
    glsl_mix(interior_floor, 1.0, glsl_clamp(g, 0.0, 1.0))
}


/// The three indirect terms the injected fragment code produces
/// (`materialpatch.js:148-186`).
///
/// Returned as three separate lanes rather than one sum, because the source
/// applies them as three statements in a fixed order and **float addition is not
/// associative**. Apply them exactly like this:
///
/// ```text
/// irradiance    += hemisphere;      // the two normal-gated bands
/// iblIrradiance *= ibl_scale;       // the prefiltered environment's budget
/// irradiance    += sun_bounce;      // the warm anti-sun wrap
/// ```
///
/// Collapsing the two adds into `irradiance += hemisphere + sun_bounce` changes
/// the result in the last bits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IndirectTerms {
    /// The two-band hemispheric fill, already gated and AO-occluded.
    pub(crate) hemisphere: [f32; 3],
    /// The warm anti-sun wrap, scaled off the ground band.
    pub(crate) sun_bounce: [f32; 3],
    /// What to multiply the prefiltered environment's diffuse irradiance by.
    pub(crate) ibl_scale: f32,
}

/// Everything the fragment stage's indirect block computes, for one fragment.
///
/// `indirect` is `owIndirect` = `(iblDiffuse * indirectScale, interiorIndirect,
/// roomCount, unused)`; `fill_dir` is [`FILL_DIR`]; `fill_gain` is
/// `owFillGain` = `(hemispheric gain, wrap gain)`.
///
/// The groupings below are the source's, parenthesis for parenthesis. In
/// particular the hemisphere term is
/// `( sky * skyG + ground * gndG * indoor ) * ( fillAo * gain.x )` — the sky band
/// carries `indoor` *inside* `skyG` while the ground band takes it as a third
/// factor afterwards, which is a different association even though it is the
/// same set of factors.
pub(crate) fn indirect_terms(
    world_pos: [f32; 3],
    world_normal: [f32; 3],
    ao: f32,
    sky_fill: [f32; 3],
    ground_fill: [f32; 3],
    fill_dir: [f32; 4],
    fill_gain: [f32; 2],
    indirect: [f32; 4],
    xf: [f32; 4],
    rooms: &[InteriorVolume],
    sun_dir_world: [f32; 3],
) -> IndirectTerms {
    let ow_indoor = interior_gate(
        indoor(world_pos, xf, indirect[2], rooms),
        ao,
        indirect[1],
    );
    // sqrt(AO), never AO: "a fill term that AO can drive to zero is not a fill,
    // it is just another way to make a black hole."
    let ow_fill_ao = f32::max(ao, 0.0).sqrt();
    let ow_up = glsl_clamp(world_normal[1], -1.0, 1.0);
    let ow_sky_g = glsl_smoothstep(fill_dir[0], fill_dir[1], ow_up) * ow_indoor;
    let ow_gnd_g = glsl_smoothstep(fill_dir[2], fill_dir[3], -ow_up);
    let hemi_scale = ow_fill_ao * fill_gain[0];
    let wrap_scale = sun_bounce(world_normal, sun_dir_world) * fill_gain[1] * ow_fill_ao * ow_indoor;
    IndirectTerms {
        hemisphere: [0_usize, 1, 2]
            .map(|c| (sky_fill[c] * ow_sky_g + ground_fill[c] * ow_gnd_g * ow_indoor) * hemi_scale),
        sun_bounce: [0_usize, 1, 2].map(|c| ground_fill[c] * wrap_scale),
        ibl_scale: indirect[0] * ow_indoor,
    }
}

/// The per-frame band colours and budgets `_updateBounceFill` uploads
/// (`index.js:1091-1152`), narrowed to the widths the uniforms hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BounceFill {
    /// `owSkyFill` — the cool upper band, irradiance in the same units as a
    /// directional light's `colour * intensity`.
    pub(crate) sky_fill: [f32; 3],
    /// `owGroundFill` — the warm lower band, and the wrap term's colour.
    pub(crate) ground_fill: [f32; 3],
    /// `owFillGain` — `(hemispheric gain, wrap gain)`.
    pub(crate) fill_gain: [f32; 2],
    /// `owIndirect.x` — the prefiltered environment's diffuse budget.
    pub(crate) ibl_diffuse: f32,
    /// `owIndirect.y` — the indirect floor inside a volume.
    pub(crate) interior_indirect: f32,
    /// `this._ambLevel` — the published whole-sky level, republished because
    /// `index.js:1038` and the viewmodel rig both read it.
    pub(crate) ambient_level: f64,
}

/// `_updateBounceFill` (`index.js:1091-1152`).
///
/// `ambient_color` is the sky subsystem's published `sky.ambientColor`
/// (`apps/shmup/src/sky/system.rs:441`); pass `[0.0, 0.0, 0.0]` for the source's
/// "no sky system" case, which takes the same arm as a black one because the
/// guard is `Math.max(r, g, b) > 1e-5`. `sun_intensity` and `sun_color` are the
/// active key's; `indirect_scale` is `sky.indirectScale`
/// (`apps/shmup/src/sky/system.rs:445`, `?? 1` when absent).
///
/// Everything is evaluated in **f64** — JavaScript numbers and `THREE.Vector3`
/// components both are — and narrowed exactly once, where the uniform is written.
///
/// # `divideScalar` is a reciprocal multiply
///
/// All three normalisations here go through `THREE.Vector3.divideScalar`, whose
/// body is `return this.multiplyScalar( 1 / scalar );`
/// (`three/src/math/Vector3.js:559-561`). So the source *is* the reciprocal
/// multiply, and writing `x / m` here would be the transcription defect run
/// backwards. The two genuine divisions in this function — `_ambLevel / 0.15`
/// and `bounceFill / max(groundFill, 1e-6)` — are written as divisions.
///
/// Note also that the first normalisation has **no** `1e-6` floor in its `max`
/// and the second and third do. That asymmetry is the source's.
pub(crate) fn bounce_fill_bands(
    ambient_color: [f64; 3],
    sun_intensity: f64,
    sun_color: [f64; 3],
    indirect_scale: f64,
) -> BounceFill {
    let sun_i = f64::max(0.0, sun_intensity);
    let amb_max = f64::max(f64::max(ambient_color[0], ambient_color[1]), ambient_color[2]);
    let published = usize::from(amb_max > 1e-5);
    let hue0 = [FALLBACK_SKY_HUE, ambient_color][published];
    let ambient_level = [SKY_AMBIENT_FRACTION * sun_i, amb_max][published];

    // hue.divideScalar( Math.max( hue.x, hue.y, hue.z ) ) — no epsilon floor here.
    let inv0 = 1.0 / f64::max(f64::max(hue0[0], hue0[1]), hue0[2]);
    let hue1 = [hue0[0] * inv0, hue0[1] * inv0, hue0[2] * inv0];

    // Push the chroma out from the hue's own luminance, then renormalise.
    let l = 0.2126 * hue1[0] + 0.7152 * hue1[1] + 0.0722 * hue1[2];
    let hue2 = hue1.map(|c| f64::max(0.0, l + (c - l) * CHROMA_PUSH));
    let inv2 = 1.0 / f64::max(f64::max(f64::max(hue2[0], hue2[1]), hue2[2]), 1e-6);
    let hue3 = hue2.map(|c| c * inv2);

    // The cool band rides the sky's own published irradiance, not the key.
    let sky_ref = ambient_level / SKY_AMBIENT_FRACTION;
    let sky_level = SKY_FILL * sky_ref;

    // The lower band is sunlight off the road: the KEY's colour through the
    // ground albedo, warm rather than blue.
    let g0 = [
        sun_color[0] * GROUND_ALBEDO[0],
        sun_color[1] * GROUND_ALBEDO[1],
        sun_color[2] * GROUND_ALBEDO[2],
    ];
    let invg = 1.0 / f64::max(f64::max(f64::max(g0[0], g0[1]), g0[2]), 1e-6);
    let g1 = g0.map(|c| c * invg);
    let ground_level = GROUND_FILL * sun_i;

    BounceFill {
        sky_fill: hue3.map(|c| (c * sky_level) as f32),
        ground_fill: g1.map(|c| (c * ground_level) as f32),
        fill_gain: [1.0, (BOUNCE_FILL / f64::max(GROUND_FILL, 1e-6)) as f32],
        ibl_diffuse: (IBL_DIFFUSE * indirect_scale) as f32,
        interior_indirect: INTERIOR_INDIRECT as f32,
        ambient_level,
    }
}

/// The interior gate and the two-band fill as WGSL — the same functions, in the
/// same order, as `materialpatch.js`'s `EXTRA_PARS` chunk and the code it injects
/// at `#include <lights_fragment_maps>`.
///
/// A `&str` with no bindings and no entry point, so it concatenates in front of
/// whichever pass needs it — the shape [`crate::agx`] and `material_shader`'s
/// twelve layers already use. Nothing in this crate concatenates it yet; see
/// `tests::nothing_in_the_lighting_path_compiles_this_yet`.
///
/// `clamp`, `mix` and `smoothstep` are written out and `dot`/`normalize` are
/// expanded, because WGSL's builtins are permitted to factor differently from
/// GLSL's and this text has to mean exactly what `materialpatch.js` means. The
/// `for` loop is a loop, exactly as the source writes it: shader text is data,
/// and the Branchless Law reads Rust HIR.
pub(crate) const INDIRECT_PROBE_WGSL: &str = r#"
// Interior indirect probe volumes, from Claude-of-Duty
// `src/render/materialpatch.js` (owInteriorGate / owSunBounce and the
// lights_fragment_maps injection). See `probe.rs` for the provenance table.

const AXIOM_PROBE_MAX_ROOMS: u32 = 10u;

fn axiom_probe_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    return min(max(x, lo), hi);
}

fn axiom_probe_mix(x: f32, y: f32, a: f32) -> f32 {
    return x * (1.0 - a) + y * a;
}

fn axiom_probe_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = axiom_probe_clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// Signed depth inside one volume: positive inside, and the `min` nesting is the
// source's (horizontal pair, then vertical pair, then the two).
fn axiom_probe_room_depth(
    lx: f32,
    lz: f32,
    world_y: f32,
    r: vec4<f32>,
    ry: vec4<f32>,
) -> f32 {
    return min(
        min(r.z - abs(lx - r.x), r.w - abs(lz - r.y)),
        min(world_y - ry.x, ry.y - world_y));
}

// How far inside ANY volume this fragment is. `count` is owIndirect.z.
fn axiom_probe_indoor(
    world_pos: vec3<f32>,
    xf: vec4<f32>,
    count: f32,
    rooms_in: array<vec4<f32>, 10>,
    rooms_y_in: array<vec4<f32>, 10>,
) -> f32 {
    // Copied into `var` because WGSL only permits a dynamic index into a
    // reference, and a parameter is a value.
    var rooms = rooms_in;
    var rooms_y = rooms_y_in;
    var indoor = 0.0;
    if (count > 0.5) {
        let lx = world_pos.x * xf.x + world_pos.z * xf.y + xf.z;
        let lz = -world_pos.x * xf.y + world_pos.z * xf.x + xf.w;
        let n = i32(count);
        for (var i = 0; i < i32(AXIOM_PROBE_MAX_ROOMS); i = i + 1) {
            if (i >= n) { break; }
            let d = axiom_probe_room_depth(lx, lz, world_pos.y, rooms[i], rooms_y[i]);
            indoor = max(indoor, axiom_probe_smoothstep(0.06, 0.30, d));
        }
    }
    return indoor;
}

// 1 outdoors, -> interior_floor deep inside a volume. The AO half is mixed in at
// 0.6 so it shapes rather than doubling up as a second AO multiply.
fn axiom_probe_interior_gate(indoor: f32, ao: f32, interior_floor: f32) -> f32 {
    let ao_gate = axiom_probe_mix(1.0, axiom_probe_smoothstep(0.45, 0.98, ao), 0.6);
    let g = min(1.0 - indoor, ao_gate);
    return axiom_probe_mix(interior_floor, 1.0, axiom_probe_clamp(g, 0.0, 1.0));
}

// Wrapped diffuse from the anti-sun hemisphere: the reflected street key. The
// 1e-4 is added to all three components, and the /1.12 stays a division.
fn axiom_probe_sun_bounce(world_normal: vec3<f32>, sun_dir_world: vec3<f32>) -> f32 {
    let v = vec3<f32>(-sun_dir_world.x, 0.28, -sun_dir_world.z) + vec3<f32>(1e-4);
    let len = sqrt(v.x * v.x + v.y * v.y + v.z * v.z);
    let anti = vec3<f32>(v.x / len, v.y / len, v.z / len);
    let d = world_normal.x * anti.x + world_normal.y * anti.y + world_normal.z * anti.z;
    return axiom_probe_clamp((d + 0.12) / 1.12, 0.0, 1.0);
}

struct AxiomProbeIndirect {
    hemisphere: vec3<f32>,
    sun_bounce: vec3<f32>,
    ibl_scale: f32,
};

// The three terms, kept separate: the source applies them as three statements
// and float addition is not associative. Apply as
//   irradiance += hemisphere; iblIrradiance *= ibl_scale; irradiance += sun_bounce;
fn axiom_probe_indirect(
    world_pos: vec3<f32>,
    world_normal: vec3<f32>,
    ao: f32,
    sky_fill: vec3<f32>,
    ground_fill: vec3<f32>,
    fill_dir: vec4<f32>,
    fill_gain: vec2<f32>,
    indirect: vec4<f32>,
    xf: vec4<f32>,
    sun_dir_world: vec3<f32>,
    rooms: array<vec4<f32>, 10>,
    rooms_y: array<vec4<f32>, 10>,
) -> AxiomProbeIndirect {
    let ow_indoor = axiom_probe_interior_gate(
        axiom_probe_indoor(world_pos, xf, indirect.z, rooms, rooms_y), ao, indirect.y);
    let ow_fill_ao = sqrt(max(ao, 0.0));
    let ow_up = axiom_probe_clamp(world_normal.y, -1.0, 1.0);
    let ow_sky_g = axiom_probe_smoothstep(fill_dir.x, fill_dir.y, ow_up) * ow_indoor;
    let ow_gnd_g = axiom_probe_smoothstep(fill_dir.z, fill_dir.w, -ow_up);
    var out: AxiomProbeIndirect;
    out.hemisphere =
        (sky_fill * ow_sky_g + ground_fill * ow_gnd_g * ow_indoor)
        * (ow_fill_ao * fill_gain.x);
    out.sun_bounce = ground_fill
        * (axiom_probe_sun_bounce(world_normal, sun_dir_world)
           * fill_gain.y * ow_fill_ao * ow_indoor);
    out.ibl_scale = indirect.x * ow_indoor;
    return out;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indirect_lighting::FILL_DIR;

    /// The two volumes the reference's boot log names, as Axiom's ported level
    /// publishes them: `W2` and `E1` are enterable and intact, `E3` is enterable
    /// and `ruin: true` (`apps/shmup/src/world/layout.rs:317, :462, :542`).
    fn level_buildings() -> Vec<BuildingFootprint> {
        vec![
            BuildingFootprint {
                enterable: true,
                collapse: false,
                ruin: false,
                center: [-12.0, -1.5],
                size: [15.0, 13.0],
                roof_y: Some(7.0),
                setback_from: Some(1),
                floor_y: vec![0.0, 3.45],
            },
            BuildingFootprint {
                enterable: true,
                collapse: false,
                ruin: false,
                center: [11.0, 15.0],
                size: [12.0, 16.0],
                roof_y: Some(10.05),
                setback_from: None,
                floor_y: vec![0.0, 3.45, 6.5],
            },
            BuildingFootprint {
                enterable: true,
                collapse: false,
                ruin: true,
                center: [13.0, -22.0],
                size: [11.0, 16.0],
                roof_y: Some(7.0),
                setback_from: None,
                floor_y: vec![0.0, 3.45],
            },
            BuildingFootprint {
                enterable: false,
                ..blank()
            },
        ]
    }

    fn blank() -> BuildingFootprint {
        BuildingFootprint {
            enterable: false,
            collapse: false,
            ruin: false,
            center: [0.0, 0.0],
            size: [4.0, 4.0],
            roof_y: None,
            setback_from: None,
            floor_y: Vec::new(),
        }
    }

    #[test]
    fn the_level_yields_the_two_interior_volumes_the_boot_log_names() {
        let volumes = interior_volumes(&level_buildings());
        assert_eq!(
            volumes.len(),
            2,
            "three enterable buildings minus one ruin is the '2 interior volumes' \
             the reference logs; got {}",
            volumes.len()
        );
        // The first has a setback from floor 1, so its top is that floor's height
        // less the inset — NOT the roof.
        assert!(
            (f64::from(volumes[0].y[1]) - (3.45 - ROOM_ROOF_INSET)).abs() < 1e-6,
            "a setback's terrace is outdoors, so the volume stops under it: got {}",
            volumes[0].y[1]
        );
        assert!(
            (f64::from(volumes[1].y[1]) - (10.05 - ROOM_ROOF_INSET)).abs() < 1e-6,
            "without a setback the volume runs to the roof deck: got {}",
            volumes[1].y[1]
        );
        assert_eq!(
            volumes[0].y[0], ROOM_FLOOR_Y as f32,
            "every volume starts below the ground slab so the floor plate counts as interior"
        );
        assert_eq!(
            volumes[0].rect,
            [-12.0, -1.5, 7.5, 6.5],
            "the rect is (cx, cz, w/2, d/2) in level space"
        );
    }

    #[test]
    fn a_collapsed_shell_and_an_absent_roof_take_their_documented_arms() {
        let collapsed = vec![BuildingFootprint {
            enterable: true,
            collapse: true,
            ..blank()
        }];
        assert!(
            interior_volumes(&collapsed).is_empty(),
            "a collapsed shell is open to the sky and must keep its skylight"
        );
        let no_roof = vec![BuildingFootprint {
            enterable: true,
            roof_y: None,
            ..blank()
        }];
        assert_eq!(
            interior_volumes(&no_roof)[0].y[1],
            (DEFAULT_ROOF_Y - ROOM_ROOF_INSET) as f32,
            "b.roofY ?? 12 is the fallback"
        );
        // A setback index the floorY array does not cover falls back to the roof,
        // matching `b.floorY?.[sb] !== undefined`.
        let short_floors = vec![BuildingFootprint {
            enterable: true,
            roof_y: Some(9.0),
            setback_from: Some(4),
            floor_y: vec![0.0, 3.45],
            ..blank()
        }];
        assert_eq!(
            interior_volumes(&short_floors)[0].y[1],
            (9.0 - ROOM_ROOF_INSET) as f32,
            "an out-of-range setback index must fall back to the roof, not index out"
        );
    }

    #[test]
    fn the_volume_list_stops_at_the_uniform_arrays_length() {
        let many: Vec<BuildingFootprint> = (0..MAX_ROOMS + 5)
            .map(|_| BuildingFootprint {
                enterable: true,
                ..blank()
            })
            .collect();
        assert_eq!(
            interior_volumes(&many).len(),
            MAX_ROOMS,
            "_updateRooms breaks at rooms.length"
        );
    }

    #[test]
    fn the_level_transform_inverts_a_yawed_level_placement() {
        // A level authored at yaw 0.5877 and translated by (0.9, 1.34) — the
        // shmup's LEVEL_YAW / LEVEL_TX / LEVEL_TZ (`world/system.rs:135-137`).
        let (yaw, tx, tz) = (0.5877_f64, 0.9_f64, 1.34_f64);
        let level_to_world = |lx: f64, lz: f64| {
            [
                lx * yaw.cos() - lz * yaw.sin() + tx,
                lx * yaw.sin() + lz * yaw.cos() + tz,
            ]
        };
        let origin = level_to_world(0.0, 0.0);
        let x_end = level_to_world(1.0, 0.0);
        let axis = [x_end[0] - origin[0], x_end[1] - origin[1]];
        let length = (axis[0] * axis[0] + axis[1] * axis[1]).sqrt();
        let xf = LevelTransform::from_world_axis(origin, x_end, length).xf;
        // Round-trip four level points through world space and back.
        let worst = [(3.0, -2.0), (-7.5, 11.25), (0.0, 0.0), (18.0, 18.0)]
            .iter()
            .map(|(lx, lz)| {
                let w = level_to_world(*lx, *lz);
                let (bx, bz) = level_xz([w[0] as f32, 0.0, w[1] as f32], xf);
                f64::max(
                    (f64::from(bx) - lx).abs(),
                    (f64::from(bz) - lz).abs(),
                )
            })
            .fold(0.0_f64, f64::max);
        // The transform is stored as f32 and applied to world coordinates of
        // magnitude ~25, where one f32 ULP is ~2e-6, so the round trip is bounded
        // by a few of those and not by the f64 algebra above it.
        assert!(
            worst < 1.0e-5,
            "world -> level must invert level -> world; worst component error {worst:e}"
        );
    }

    #[test]
    fn a_degenerate_axis_is_floored_rather_than_dividing_by_zero() {
        let xf = LevelTransform::from_world_axis([0.0, 0.0], [0.0, 0.0], 0.0).xf;
        assert!(
            xf.iter().all(|c| c.is_finite()),
            "Math.max(1e-6, hypot) is the guard; got {xf:?}"
        );
    }

    #[test]
    fn the_gate_separates_the_two_faces_of_one_wall() {
        // The whole point of keying off depth rather than containment: a facade's
        // outer skin sits at depth 0 and its inner skin one wall thickness in.
        let room = InteriorVolume {
            rect: [0.0, 0.0, 5.0, 5.0],
            y: [-0.8, 6.94, 0.0, 0.0],
        };
        let identity = [1.0, 0.0, 0.0, 0.0];
        let outer = indoor([5.0, 2.0, 0.0], identity, 1.0, &[room]);
        let inner = indoor([5.0 - 0.32, 2.0, 0.0], identity, 1.0, &[room]);
        assert_eq!(
            outer, 0.0,
            "a fragment exactly on the footprint boundary is at depth 0 and fully outdoors"
        );
        assert_eq!(
            inner, 1.0,
            "one 32 cm wall thickness in is past the 0.30 feather and fully indoors"
        );
        // And the feather really is a feather, not a step.
        let midway = indoor([5.0 - 0.18, 2.0, 0.0], identity, 1.0, &[room]);
        assert!(
            midway > 0.0 && midway < 1.0,
            "0.18 m in must be inside the 0.06..0.30 feather; got {midway}"
        );
    }

    #[test]
    fn the_volume_test_uses_the_nearest_face_in_all_three_axes() {
        let room = InteriorVolume {
            rect: [0.0, 0.0, 5.0, 5.0],
            y: [-0.8, 3.0, 0.0, 0.0],
        };
        let identity = [1.0, 0.0, 0.0, 0.0];
        // Well inside horizontally but above the ceiling: outdoors.
        assert_eq!(
            indoor([0.0, 3.5, 0.0], identity, 1.0, &[room]),
            0.0,
            "above the roof deck is outdoors even at the footprint centre"
        );
        // Below the floor slab: also outdoors.
        assert_eq!(
            indoor([0.0, -1.2, 0.0], identity, 1.0, &[room]),
            0.0,
            "below y0 is outdoors"
        );
        // The depth is the *minimum* over the four faces, so the z face bites too.
        assert_eq!(
            indoor([0.0, 1.0, 4.95], identity, 1.0, &[room]),
            0.0,
            "5 cm from the z face is inside the 0.06 feather's lower edge"
        );
    }

    #[test]
    fn the_room_reduction_is_disabled_and_bounded_by_the_count() {
        let room = InteriorVolume {
            rect: [0.0, 0.0, 5.0, 5.0],
            y: [-0.8, 6.0, 0.0, 0.0],
        };
        let identity = [1.0, 0.0, 0.0, 0.0];
        let deep = [0.0_f32, 2.0, 0.0];
        assert_eq!(
            indoor(deep, identity, 1.0, &[room]),
            1.0,
            "with one live volume the fragment is indoors"
        );
        assert_eq!(
            indoor(deep, identity, 0.0, &[room]),
            0.0,
            "owIndirect.z = 0 disables the gate entirely — the viewmodel pass's exception"
        );
        assert_eq!(
            indoor(deep, identity, 0.4, &[room]),
            0.0,
            "the > 0.5 enable rejects a fractional count"
        );
        assert_eq!(
            indoor(deep, identity, -3.0, &[room]),
            0.0,
            "a negative count yields no iterations, as int() truncation does in GLSL"
        );
        // The count, not the slice length, bounds the loop.
        let two = [
            InteriorVolume {
                rect: [100.0, 100.0, 1.0, 1.0],
                y: [-0.8, 6.0, 0.0, 0.0],
            },
            room,
        ];
        assert_eq!(
            indoor(deep, identity, 1.0, &two),
            0.0,
            "a count of 1 must read only the first volume, not every one supplied"
        );
        assert_eq!(
            indoor(deep, identity, 2.0, &two),
            1.0,
            "and a count of 2 must reach the second"
        );
        // The MAX_ROOMS clamp holds even against a count the frame graph never
        // writes, because a uniform is not a contract the shader can check.
        assert_eq!(
            indoor(deep, identity, 400.0, &two),
            1.0,
            "an over-large count must not read past the array"
        );
    }

    #[test]
    fn the_gate_floors_indoors_and_releases_outdoors() {
        let floor = INTERIOR_INDIRECT as f32;
        assert_eq!(
            interior_gate(1.0, 1.0, floor),
            floor,
            "fully indoors with full visibility lands exactly on the floor"
        );
        assert_eq!(
            interior_gate(0.0, 1.0, floor),
            1.0,
            "fully outdoors with full visibility passes skylight untouched"
        );
        // The AO half bites outside a volume too — arcades, stairwells, awnings.
        let occluded = interior_gate(0.0, 0.2, floor);
        assert!(
            occluded > floor && occluded < 1.0,
            "an occluded pocket outside a volume must be shaped, not floored or free: {occluded}"
        );
        // ...but only to `1 - AO_GATE_MIX` of the way, by construction.
        let fully_dark = interior_gate(0.0, 0.0, 0.0);
        assert!(
            (fully_dark - (1.0 - AO_GATE_MIX)).abs() < 1e-6,
            "with a zero floor the AO gate alone can only reach 1 - 0.6 = 0.4; got {fully_dark}"
        );
        // The gate is monotone in both inputs.
        let deeper: Vec<f32> = (0..=10)
            .map(|i| interior_gate(i as f32 * 0.1, 1.0, floor))
            .collect();
        assert!(
            deeper.windows(2).all(|w| w[1] <= w[0]),
            "deeper inside a volume can only ever reduce the gate: {deeper:?}"
        );
    }

    #[test]
    fn the_sun_bounce_is_a_tight_wrap_around_the_anti_sun_direction() {
        // Sun low in +Z, so the anti-sun hemisphere points roughly -Z and up.
        let sun = [0.0_f32, 0.2, 0.979_795_9];
        let facing_away = sun_bounce([0.0, 0.0, -1.0], sun);
        let facing_sun = sun_bounce([0.0, 0.0, 1.0], sun);
        assert!(
            facing_away > 0.8,
            "a wall facing the shaded side receives the street's bounce: {facing_away}"
        );
        assert_eq!(
            facing_sun, 0.0,
            "a face turned toward the sun receives none of it — the wrap is 0.12, not 0.35"
        );
        // The clamp's upper arm is reachable: a normal aligned with `anti` gives
        // (1 + 0.12) / 1.12 = 1 exactly.
        let aligned = sun_bounce([0.0, 1.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(
            aligned > 0.99,
            "a normal near the anti-sun axis saturates the wrap: {aligned}"
        );
        // The 1e-4 guard keeps a zenith sun from producing a zero-length anti
        // vector... which it would NOT, because of the fixed 0.28 up-component.
        // The epsilon is therefore belt-and-braces; assert it is at least benign.
        assert!(
            sun_bounce([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]).is_finite(),
            "a null sun direction must not produce a NaN"
        );
    }

    #[test]
    fn the_two_bands_are_gated_independently_and_do_not_partition_unity() {
        // The single most consequential difference from FrameAmbient's `mix`.
        let up = 0.0_f32; // a vertical wall
        let sky_g = glsl_smoothstep(FILL_DIR[0], FILL_DIR[1], up);
        let gnd_g = glsl_smoothstep(FILL_DIR[2], FILL_DIR[3], -up);
        assert!(
            (sky_g - 0.5416).abs() < 0.01,
            "a vertical wall sees ~54% of the sky band (cosine-hemisphere, not a \
             narrow smoothstep); got {sky_g}"
        );
        assert!(
            gnd_g < 0.05,
            "and almost none of the warm street band; got {gnd_g}"
        );
        assert!(
            (sky_g + gnd_g - 1.0).abs() > 0.4,
            "the two gates are independent smoothsteps, NOT a partition of unity — \
             their sum here is {}, and a `mix` would force 1.0",
            sky_g + gnd_g
        );
        // A soffit is the ground band's case and an up-face the sky band's.
        assert!(
            glsl_smoothstep(FILL_DIR[2], FILL_DIR[3], 1.0) > 0.99,
            "a downward-facing soffit takes the warm band"
        );
        assert_eq!(
            glsl_smoothstep(FILL_DIR[2], FILL_DIR[3], -1.0),
            0.0,
            "an up-facing surface takes none of it"
        );
    }

    #[test]
    fn the_indirect_terms_collapse_toward_the_floor_inside_a_volume() {
        let room = InteriorVolume {
            rect: [0.0, 0.0, 5.0, 5.0],
            y: [-0.8, 6.0, 0.0, 0.0],
        };
        let bands = bounce_fill_bands([0.36, 0.52, 0.86], 4.0, [1.0, 0.975, 0.94], 1.0);
        let call = |pos: [f32; 3]| {
            indirect_terms(
                pos,
                [0.0, 0.0, -1.0],
                1.0,
                bands.sky_fill,
                bands.ground_fill,
                FILL_DIR,
                bands.fill_gain,
                [bands.ibl_diffuse, bands.interior_indirect, 1.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                &[room],
                [0.0, 0.4, 0.916_515_1],
            )
        };
        let inside = call([0.0, 2.0, 0.0]);
        let outside = call([40.0, 2.0, 0.0]);
        // Every one of the three terms must be gated, not just the hemisphere.
        assert!(
            inside.hemisphere[2] < outside.hemisphere[2] * 0.2,
            "the sky band must collapse indoors: {} vs {}",
            inside.hemisphere[2],
            outside.hemisphere[2]
        );
        assert!(
            inside.sun_bounce[0] < outside.sun_bounce[0] * 0.2,
            "the warm wrap must be gated too — it was not, and that is why an \
             interior metered the same as the sunlit exterior in its own doorway: \
             {} vs {}",
            inside.sun_bounce[0],
            outside.sun_bounce[0]
        );
        assert!(
            (inside.ibl_scale / outside.ibl_scale - bands.interior_indirect).abs() < 1e-6,
            "the IBL budget is scaled by exactly the gate: {} vs {}",
            inside.ibl_scale,
            outside.ibl_scale
        );
        // Nothing ever reaches zero — the floor is 0.035, not 0.
        assert!(
            inside.ibl_scale > 0.0 && inside.hemisphere[2] > 0.0,
            "floored so nothing goes black: {inside:?}"
        );
    }

    #[test]
    fn ambient_occlusion_enters_the_fill_as_its_square_root() {
        let bands = bounce_fill_bands([0.36, 0.52, 0.86], 4.0, [1.0, 0.975, 0.94], 1.0);
        let call = |ao: f32, floor: f32| {
            indirect_terms(
                [40.0, 2.0, 0.0],
                [0.0, 1.0, 0.0],
                ao,
                bands.sky_fill,
                bands.ground_fill,
                FILL_DIR,
                bands.fill_gain,
                [bands.ibl_diffuse, floor, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                &[],
                [0.0, 0.4, 0.916_515_1],
            )
            .hemisphere[2]
        };
        // AO reaches the fill by two separate routes — `sqrt(ao)` as an occluder
        // and `smoothstep(0.45, 0.98, ao)` inside the gate — so isolating the
        // first needs the second held flat. An interior floor of 1.0 does exactly
        // that: `mix(1, 1, g)` is identically 1 whatever `g` is.
        let ratio = call(0.25, 1.0) / call(1.0, 1.0);
        assert!(
            (ratio - 0.5).abs() < 1.0e-6,
            "AO must enter the fill as sqrt(ao): a quarter of the visibility must \
             leave half the fill, but the ratio is {ratio} (a plain multiply would give 0.25)"
        );
        // And with the shipped floor the gate bites as well, so the same drop in
        // visibility costs more than the sqrt alone.
        let gated = call(0.25, bands.interior_indirect) / call(1.0, bands.interior_indirect);
        assert!(
            gated < ratio,
            "the AO gate must compound with the sqrt: gated {gated} vs sqrt-only {ratio}"
        );
        assert_eq!(
            call(-1.0, 1.0),
            0.0,
            "a negative AO is floored at zero by max(ao, 0) before the sqrt"
        );
        assert!(call(0.0, 1.0).abs() < 1e-12, "zero visibility gives zero fill");
    }

    #[test]
    fn the_band_colours_follow_the_published_sky_and_fall_back_when_it_is_dark() {
        let lit = bounce_fill_bands([0.36, 0.52, 0.86], 4.0, [1.0, 0.975, 0.94], 1.0);
        assert!(
            lit.sky_fill[2] > lit.sky_fill[0],
            "the cool band must stay blue-dominant: {:?}",
            lit.sky_fill
        );
        assert!(
            lit.ground_fill[0] > lit.ground_fill[2],
            "the warm band takes the key through a dry-street albedo: {:?}",
            lit.ground_fill
        );
        assert!(
            (f64::from(lit.ambient_level) - 0.86).abs() < 1e-6,
            "_ambLevel is the published ambient's max channel: {}",
            lit.ambient_level
        );
        // No published ambient (or a black one) takes the fallback arm.
        let dark = bounce_fill_bands([0.0, 0.0, 0.0], 4.0, [1.0, 0.975, 0.94], 1.0);
        assert!(
            (dark.ambient_level - SKY_AMBIENT_FRACTION * 4.0).abs() < 1e-12,
            "the fallback _ambLevel is 0.15 * sun intensity: {}",
            dark.ambient_level
        );
        assert!(
            dark.sky_fill[2] > dark.sky_fill[0],
            "the fallback hue (0.36, 0.56, 1.0) is also blue-dominant: {:?}",
            dark.sky_fill
        );
        // A negative key intensity is floored, not propagated.
        let negative = bounce_fill_bands([0.0, 0.0, 0.0], -3.0, [1.0, 1.0, 1.0], 1.0);
        assert_eq!(
            negative.ground_fill,
            [0.0, 0.0, 0.0],
            "Math.max(0, intensity) floors the key before it scales the ground band"
        );
    }

    #[test]
    fn the_chroma_push_saturates_a_near_neutral_ambient_without_inventing_a_hue() {
        // The measured motivation: a shaded facade at B-R = 0.0002 (4.5%
        // saturation, "dead neutral") must come out legibly cool.
        let nearly_grey = [0.500, 0.503, 0.512];
        let pushed = bounce_fill_bands(nearly_grey, 4.0, [1.0, 1.0, 1.0], 1.0).sky_fill;
        let before = (nearly_grey[2] - nearly_grey[0]) / nearly_grey[2];
        let after = f64::from(pushed[2] - pushed[0]) / f64::from(pushed[2]);
        assert!(
            after > before * 1.15,
            "k = 1.18 must push the chroma out from the hue's own luminance: \
             relative B-R {before} -> {after}"
        );
        // The hue's *sign* is unchanged — the push scales chroma, it does not
        // rotate. Blue stays the max channel.
        assert!(
            pushed[2] >= pushed[1] && pushed[1] >= pushed[0],
            "the channel ORDER must survive the push: {pushed:?}"
        );
        // The `max(0, ...)` arm is reachable: a strongly saturated ambient pushed
        // by 1.18 drives the darkest channel negative before the clamp.
        let saturated = bounce_fill_bands([1.0, 0.2, 0.05], 4.0, [1.0, 1.0, 1.0], 1.0).sky_fill;
        assert!(
            saturated.iter().all(|c| *c >= 0.0),
            "the push's max(0, ...) must floor a channel it drives negative: {saturated:?}"
        );
        assert_eq!(
            saturated[2], 0.0,
            "and here it drives exactly one channel to the floor"
        );
    }

    #[test]
    fn the_fill_gain_and_indirect_budgets_are_the_settings_ratios() {
        let b = bounce_fill_bands([0.4, 0.5, 0.8], 4.0, [1.0, 1.0, 1.0], 1.0);
        assert_eq!(b.fill_gain[0], 1.0, "the hemispheric gain is fixed at 1");
        assert!(
            (f64::from(b.fill_gain[1]) - BOUNCE_FILL / GROUND_FILL).abs() < 1e-6,
            "the wrap gain is bounceFill / groundFill = {}, got {}",
            BOUNCE_FILL / GROUND_FILL,
            b.fill_gain[1]
        );
        assert_eq!(
            b.interior_indirect,
            INTERIOR_INDIRECT as f32,
            "the interior floor is settings.interiorIndirect verbatim"
        );
        // The sky's elevation-dependent budget multiplies the IBL term, and it
        // goes ABOVE unity at night (2.2) so a night frame is not empty.
        let night = bounce_fill_bands([0.02, 0.03, 0.05], 0.05, [1.0, 1.0, 1.0], 2.2);
        assert!(
            (f64::from(night.ibl_diffuse) - IBL_DIFFUSE * 2.2).abs() < 1e-9,
            "owIndirect.x is iblDiffuse * sky.indirectScale: {}",
            night.ibl_diffuse
        );
    }

    #[test]
    fn divide_scalar_is_a_reciprocal_multiply_and_this_port_uses_it() {
        // three's Vector3.divideScalar(s) is multiplyScalar(1 / s), so the source
        // IS the reciprocal multiply and transcribing it as a division would be
        // the usual defect run backwards. Demonstrated on a divisor where the two
        // differ: 1/49 is not exactly representable, so x * (1/49) != x / 49.
        let m = 49.0_f64;
        let hue = [1.0_f64, 0.7, 0.3];
        let reciprocal = hue.map(|c| c * (1.0 / m));
        let division = hue.map(|c| c / m);
        assert_ne!(
            reciprocal, division,
            "the two forms must actually differ here, or this test proves nothing"
        );
        // And the port takes the reciprocal path: normalising a hue whose max is
        // 49 must reproduce `multiplyScalar(1/49)` for every channel.
        let bands = bounce_fill_bands([49.0, 34.3, 14.7], 0.0, [1.0, 1.0, 1.0], 1.0);
        let l = 0.2126 * reciprocal[0] + 0.7152 * reciprocal[1] + 0.0722 * reciprocal[2];
        let pushed = reciprocal.map(|c| f64::max(0.0, l + (c - l) * CHROMA_PUSH));
        let inv2 = 1.0 / f64::max(f64::max(f64::max(pushed[0], pushed[1]), pushed[2]), 1e-6);
        let level = SKY_FILL * (49.0 / SKY_AMBIENT_FRACTION);
        let want: [f32; 3] = pushed.map(|c| (c * inv2 * level) as f32);
        assert_eq!(
            bands.sky_fill, want,
            "the sky band must be bit-identical to the reciprocal-multiply chain"
        );
    }

    #[test]
    fn the_wgsl_declares_every_function_this_module_mirrors() {
        [
            "fn axiom_probe_clamp(",
            "fn axiom_probe_mix(",
            "fn axiom_probe_smoothstep(",
            "fn axiom_probe_room_depth(",
            "fn axiom_probe_indoor(",
            "fn axiom_probe_interior_gate(",
            "fn axiom_probe_sun_bounce(",
            "fn axiom_probe_indirect(",
        ]
        .iter()
        .for_each(|needle| {
            assert!(
                INDIRECT_PROBE_WGSL.contains(needle),
                "INDIRECT_PROBE_WGSL must declare `{needle}` for the CPU reference to have a peer"
            );
        });
        // The constants have to match on both sides, and a literal in shader text
        // cannot be checked by the compiler.
        [
            ("0.06, 0.30", "the room feather"),
            ("0.45, 0.98", "the AO gate"),
            ("0.12", "the sun-bounce wrap"),
            ("1.12", "the wrap's divisor, written as a division"),
            ("0.28", "the anti-sun up-component"),
            ("1e-4", "the anti-sun epsilon"),
        ]
        .iter()
        .for_each(|(needle, what)| {
            assert!(
                INDIRECT_PROBE_WGSL.contains(needle),
                "the WGSL must carry {what} (`{needle}`) verbatim"
            );
        });
        assert!(
            !INDIRECT_PROBE_WGSL.contains("0.892857"),
            "the /1.12 must stay a division, never a folded reciprocal"
        );
    }

    #[test]
    fn nothing_in_the_lighting_path_compiles_this_yet() {
        // The frame contract has no indirect lane, so no pass concatenates this
        // chunk. Stated as a test rather than a comment so the deferral cannot
        // quietly expire: when a `FrameIndirect` lane lands and `scene_wgsl.rs`
        // splices this in, the assertion fails and the wiring is reviewed
        // deliberately. See the module docs, "What the frame-graph sibling must
        // supply".
        let sources = [
            include_str!("../scene_wgsl.rs"),
            include_str!("../scene_renderer.rs"),
        ];
        let wired = sources
            .iter()
            .filter(|s| s.contains("INDIRECT_PROBE_WGSL"))
            .count();
        assert_eq!(
            wired, 0,
            "{wired} render path(s) now splice the indirect chunk; re-read this \
             module's docs on what the frame graph must supply before relying on it"
        );
    }
}

// The CPU reference above is the semantic definition; this holds it up against a
// real GPU running `INDIRECT_PROBE_WGSL`. Compiled only with
// `--features offscreen`, and it ASSERTS an adapter was acquired rather than
// skipping. The harness shape is `crate::agx::parity`'s, which is in turn
// `crate::surface_program::parity`'s; neither is reusable from here because both
// are private to a module this slice may not edit.
//
// !! THIS TEST HAS NEVER BEEN RUN. !! The final-wave brief forbids building, so
// `EXPECTED_WORST_UNVERIFIED` below is an ESTIMATE, not a measurement, and it is
// labelled as one at its definition and in the assertion message. The
// integration pass owns replacing it with the real number.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::*;
    use crate::indirect_lighting::FILL_DIR;

    /// How many fragments one run compares, and the target's width.
    const SAMPLES: usize = 24;

    /// `copy_texture_to_buffer` requires each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// Sixteen-byte lanes of shared state ahead of the per-sample array:
    /// 10 rooms + 10 room-Y + xf + indirect + fill_dir + fill_gain + sky_fill +
    /// ground_fill + sun_dir.
    const SHARED_LANES: usize = MAX_ROOMS * 2 + 7;

    /// Sixteen-byte lanes per sample: `(world_pos, ao)` and `(normal, 0)`.
    const SAMPLE_LANES: usize = 2;

    /// The agreement budget, **relative above unit magnitude**: a deviation is
    /// scored as `|got - want| / max(|want|, 1)`.
    ///
    /// # Where the error is expected to come from
    ///
    /// Not from the arithmetic's conditioning — nothing here cancels the way
    /// `agx::contrast` does. It comes from **world coordinates**, and the chain
    /// is worth writing out because it is the one thing that could make this
    /// budget look surprisingly loose for eight lines of maths:
    ///
    /// 1. `axiom_probe_indoor` starts with `world_pos.x * xf.x + world_pos.z *
    ///    xf.y + xf.z`, evaluated at street-scale coordinates (this sweep reaches
    ///    ~25 m). One f32 ULP at 25 is `1.9e-6`, and a GPU is entitled to
    ///    contract that expression into two `fma`s where Rust is not — so `lx`
    ///    and `lz` can differ by ~`2e-6` **absolute** before anything else runs.
    /// 2. `room_depth` is a difference of those, so it inherits the same
    ///    absolute error while its own magnitude collapses toward zero at a
    ///    volume boundary — which is exactly where the interesting samples are.
    /// 3. `smoothstep(0.06, 0.30, d)` divides by `0.24`, multiplying that
    ///    absolute error by `~4.2`, and the cubic's slope peaks at `1.5`. So a
    ///    `2e-6` positional disagreement becomes up to `~1.3e-5` in `indoor`.
    /// 4. `interior_gate` and the fill are near-unit-slope in `indoor`, so that
    ///    figure lands essentially undiminished on a result of magnitude `<= 1`,
    ///    where the `max(|want|, 1)` floor makes it an absolute budget.
    ///
    /// The normalise in `axiom_probe_sun_bounce` is the only other candidate: it
    /// is written `v / sqrt(dot(v, v))` on both sides precisely so an adapter's
    /// `inverseSqrt` substitution shows up as a measurement rather than hiding,
    /// and it is worth an ULP or two on a unit-magnitude result.
    ///
    /// The budget is ~4x [`EXPECTED_WORST_UNVERIFIED`] — room for a second
    /// contracted multiply-add on another vendor, and no more. If a run needs
    /// more than this, the answer is not a bigger number: it is to check whether
    /// the level transform should be evaluated in a camera-relative frame, which
    /// is a real design question this measurement would be raising.
    const TOLERANCE: f32 = 3.0e-5;

    /// **UNVERIFIED ESTIMATE — not a measurement.** The final-wave brief forbids
    /// building, so this module's parity proof has never executed. This is the
    /// worst scaled deviation the error account above *predicts* (step 3's
    /// `~1.3e-5` is the ceiling; `8e-6` is the estimate for a sweep that only
    /// grazes the feather's steepest point).
    ///
    /// The integration pass must run this test and replace this constant with the
    /// number the adapter actually reports, keeping the assertion that pins it.
    /// If the real figure is larger, that is information about the hardware, not
    /// a licence to widen [`TOLERANCE`] — re-derive the account first.
    const EXPECTED_WORST_UNVERIFIED: f32 = 8.0e-6;

    /// One fragment's inputs.
    struct Sample {
        world_pos: [f32; 3],
        normal: [f32; 3],
        ao: f32,
    }

    /// The per-frame state every sample is evaluated against — the uniform block,
    /// as the CPU reference names its parts.
    struct Shared {
        rooms: [InteriorVolume; 2],
        xf: [f32; 4],
        indirect: [f32; 4],
        sky_fill: [f32; 3],
        ground_fill: [f32; 3],
        fill_gain: [f32; 2],
        sun_dir: [f32; 3],
    }

    fn shared() -> Shared {
        let bands = bounce_fill_bands([0.36, 0.52, 0.86], 4.0, [1.0, 0.975, 0.94], 1.0);
        Shared {
            rooms: [
                InteriorVolume {
                    rect: [-12.0, -1.5, 7.5, 6.5],
                    y: [-0.8, 3.39, 0.0, 0.0],
                },
                InteriorVolume {
                    rect: [11.0, 15.0, 6.0, 8.0],
                    y: [-0.8, 9.99, 0.0, 0.0],
                },
            ],
            // A yawed level transform, not the identity: the world -> level
            // rotate is where a transposed sign would hide.
            xf: [0.833_211_9, 0.552_952_2, 0.9, -1.34],
            indirect: [bands.ibl_diffuse, bands.interior_indirect, 2.0, 0.0],
            sky_fill: bands.sky_fill,
            ground_fill: bands.ground_fill,
            fill_gain: bands.fill_gain,
            sun_dir: [0.0, 0.4, 0.916_515_1],
        }
    }

    /// The fragments, chosen to cross every regime the gate has: deep inside each
    /// volume, exactly on a boundary, inside the 0.06..0.30 feather on each of
    /// the four horizontal faces and both vertical ones, well outside, and with
    /// normals sweeping the full up-component range so both fill gates and the
    /// wrap term are exercised across their clamps.
    fn samples() -> Vec<Sample> {
        (0..SAMPLES)
            .map(|index| {
                let t = index as f32;
                let angle = t * 0.7853;
                // Walk from deep inside volume 0, out through its wall, across
                // the street, and into volume 1.
                let along = -14.0 + t * 1.6;
                Sample {
                    world_pos: [along, 0.4 + (t * 0.37).sin() * 3.2, -1.5 + (t * 0.21).cos() * 8.0],
                    normal: [angle.cos() * 0.6, (t * 0.41).sin(), angle.sin() * 0.6],
                    // Crosses both AO gate edges (0.45, 0.98) and the sqrt's floor.
                    ao: [1.0, 0.62, 0.28, 0.97][index % 4] - t * 0.01,
                }
            })
            .collect()
    }

    /// The harness: a fullscreen triangle whose fragment stage evaluates the
    /// entry point at the sample its pixel column names.
    const HARNESS_WGSL: &str = r#"
struct ProbeCtx {
    rooms: array<vec4<f32>, 10>,
    rooms_y: array<vec4<f32>, 10>,
    xf: vec4<f32>,
    indirect: vec4<f32>,
    fill_dir: vec4<f32>,
    fill_gain: vec4<f32>,
    sky_fill: vec4<f32>,
    ground_fill: vec4<f32>,
    sun_dir: vec4<f32>,
    items: array<vec4<f32>, 48>,
};
@group(0) @binding(0) var<uniform> ctx: ProbeCtx;

fn lane(index: u32, slot: u32) -> vec4<f32> { return ctx.items[index * 2u + slot]; }

@vertex
fn probe_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

// The gate, isolated: the volume reduction and the blend, plus the wrap term, so
// a miss can be attributed to the room test rather than to the fill arithmetic.
@fragment
fn probe_gate_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = lane(i, 0u);
    let b = lane(i, 1u);
    let ind = axiom_probe_indoor(a.xyz, ctx.xf, ctx.indirect.z, ctx.rooms, ctx.rooms_y);
    return vec4<f32>(
        ind,
        axiom_probe_interior_gate(ind, a.w, ctx.indirect.y),
        axiom_probe_sun_bounce(b.xyz, ctx.sun_dir.xyz),
        axiom_probe_room_depth(
            a.x * ctx.xf.x + a.z * ctx.xf.y + ctx.xf.z,
            -a.x * ctx.xf.y + a.z * ctx.xf.x + ctx.xf.w,
            a.y, ctx.rooms[0], ctx.rooms_y[0]),
    );
}

// The hemispheric fill, and the IBL budget in the alpha lane.
@fragment
fn probe_hemi_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = lane(i, 0u);
    let b = lane(i, 1u);
    let r = axiom_probe_indirect(
        a.xyz, b.xyz, a.w, ctx.sky_fill.xyz, ctx.ground_fill.xyz,
        ctx.fill_dir, ctx.fill_gain.xy, ctx.indirect, ctx.xf, ctx.sun_dir.xyz,
        ctx.rooms, ctx.rooms_y);
    return vec4<f32>(r.hemisphere, r.ibl_scale);
}

// The warm anti-sun wrap term.
@fragment
fn probe_wrap_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = lane(i, 0u);
    let b = lane(i, 1u);
    let r = axiom_probe_indirect(
        a.xyz, b.xyz, a.w, ctx.sky_fill.xyz, ctx.ground_fill.xyz,
        ctx.fill_dir, ctx.fill_gain.xy, ctx.indirect, ctx.xf, ctx.sun_dir.xyz,
        ctx.rooms, ctx.rooms_y);
    return vec4<f32>(r.sun_bounce, 0.0);
}
"#;

    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl Gpu {
        fn acquire() -> Gpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            Gpu {
                device: gpu.device.clone(),
                queue: gpu.queue.clone(),
                backend: gpu.backend,
            }
        }

        fn render(&self, module: &wgpu::ShaderModule, entry: &str, uniform: &[u8]) -> Vec<[f32; 4]> {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-probe-parity-bgl"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                });
            let buffer = wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-probe-parity-uniform"),
                    contents: uniform,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-probe-parity-bg"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-probe-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-probe-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("probe_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba32Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-probe-parity-target"),
                size: wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-probe-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-probe-parity-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            self.device
                .poll(wgpu::PollType::Wait)
                .expect("the readback must complete");
            let mapped = slice.get_mapped_range();
            (0..SAMPLES)
                .map(|sample| {
                    [0_usize, 1, 2, 3].map(|lane| {
                        let at = sample * 16 + lane * 4;
                        f32::from_le_bytes([
                            mapped[at],
                            mapped[at + 1],
                            mapped[at + 2],
                            mapped[at + 3],
                        ])
                    })
                })
                .collect()
        }
    }

    /// The uniform block, in exactly the order `ProbeCtx` declares it.
    fn uniform_bytes() -> Vec<u8> {
        let s = shared();
        let padded: Vec<InteriorVolume> = (0..MAX_ROOMS)
            .map(|i| {
                *s.rooms.get(i).unwrap_or(&InteriorVolume {
                    rect: [0.0; 4],
                    y: [0.0; 4],
                })
            })
            .collect();
        let head: Vec<[f32; 4]> = padded
            .iter()
            .map(|r| r.rect)
            .chain(padded.iter().map(|r| r.y))
            .chain([
                s.xf,
                s.indirect,
                FILL_DIR,
                [s.fill_gain[0], s.fill_gain[1], 0.0, 0.0],
                [s.sky_fill[0], s.sky_fill[1], s.sky_fill[2], 0.0],
                [s.ground_fill[0], s.ground_fill[1], s.ground_fill[2], 0.0],
                [s.sun_dir[0], s.sun_dir[1], s.sun_dir[2], 0.0],
            ])
            .collect();
        let body: Vec<[f32; 4]> = samples()
            .iter()
            .flat_map(|s| {
                [
                    [s.world_pos[0], s.world_pos[1], s.world_pos[2], s.ao],
                    [s.normal[0], s.normal[1], s.normal[2], 0.0],
                ]
            })
            .collect();
        let bytes: Vec<u8> = head
            .iter()
            .chain(body.iter())
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        // An equality, never a `resize`: a `resize` to a smaller length is a
        // silent truncation, and `crate::exposure`'s harness lost a whole day of
        // confidence to exactly that.
        assert_eq!(
            bytes.len(),
            (SHARED_LANES + SAMPLES * SAMPLE_LANES) * 16,
            "the packed block must match what ProbeCtx strides by"
        );
        bytes
    }

    /// Compare one entry point's four lanes against the CPU reference, and return
    /// the worst scaled deviation together with the lane it came from.
    ///
    /// One assertion at the end rather than one per lane, so a run reports the
    /// *worst* disagreement rather than the first — which is what a budget has to
    /// be set from.
    fn compare(
        gpu: &Gpu,
        module: &wgpu::ShaderModule,
        entry: &str,
        expected: &[[f32; 4]],
    ) -> (f32, String) {
        let actual = gpu.render(module, entry, &uniform_bytes());
        actual
            .iter()
            .zip(expected)
            .enumerate()
            .flat_map(|(sample, (got, want))| {
                got.iter()
                    .zip(want)
                    .enumerate()
                    .map(move |(lane, (g, w))| (sample, lane, *g, *w))
            })
            .map(|(sample, lane, got, want)| {
                let scaled = (got - want).abs() / f32::max(want.abs(), 1.0);
                (
                    scaled,
                    format!("{entry} sample {sample} lane {lane}: GPU {got} vs CPU {want}"),
                )
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .expect("the sweep compares at least one lane")
    }

    #[test]
    fn indirect_probe_wgsl_agrees_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let (module, failure) = crate::test_gpu::validating(&gpu.device, || {
            gpu.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-probe-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        format!("{INDIRECT_PROBE_WGSL}\n{HARNESS_WGSL}").into(),
                    ),
                })
        });
        assert!(failure.is_none(), "INDIRECT_PROBE_WGSL must compile");

        let sh = shared();
        let s = samples();

        let gate_expected: Vec<[f32; 4]> = s
            .iter()
            .map(|f| {
                let ind = indoor(f.world_pos, sh.xf, sh.indirect[2], &sh.rooms);
                let (lx, lz) = level_xz(f.world_pos, sh.xf);
                [
                    ind,
                    interior_gate(ind, f.ao, sh.indirect[1]),
                    sun_bounce(f.normal, sh.sun_dir),
                    room_depth(lx, lz, f.world_pos[1], &sh.rooms[0]),
                ]
            })
            .collect();
        let terms: Vec<IndirectTerms> = s
            .iter()
            .map(|f| {
                indirect_terms(
                    f.world_pos,
                    f.normal,
                    f.ao,
                    sh.sky_fill,
                    sh.ground_fill,
                    FILL_DIR,
                    sh.fill_gain,
                    sh.indirect,
                    sh.xf,
                    &sh.rooms,
                    sh.sun_dir,
                )
            })
            .collect();
        let hemi_expected: Vec<[f32; 4]> = terms
            .iter()
            .map(|t| [t.hemisphere[0], t.hemisphere[1], t.hemisphere[2], t.ibl_scale])
            .collect();
        let wrap_expected: Vec<[f32; 4]> = terms
            .iter()
            .map(|t| [t.sun_bounce[0], t.sun_bounce[1], t.sun_bounce[2], 0.0])
            .collect();

        let per_entry = [
            ("probe_gate_fs", gate_expected),
            ("probe_hemi_fs", hemi_expected),
            ("probe_wrap_fs", wrap_expected),
        ]
        .iter()
        .map(|(entry, expected)| compare(&gpu, &module, entry, expected))
        .collect::<Vec<(f32, String)>>();
        // Every entry point's worst, not just the overall one: the budget has to
        // be ATTRIBUTABLE, and "which stage costs what" is only visible if the
        // failure message carries all of them.
        let summary = per_entry
            .iter()
            .map(|(w, at)| format!("{w:e} at {at}"))
            .collect::<Vec<String>>()
            .join(" | ");
        let (worst, at) = per_entry
            .iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .cloned()
            .expect("at least one entry point is compared");

        assert!(
            worst <= TOLERANCE,
            "indirect-probe parity on {:?}: worst scaled delta {worst:e} exceeds the \
             budget {TOLERANCE:e}, at {at}. Per entry point: {summary}",
            gpu.backend
        );
        // The budget must stay a *measurement* plus headroom, never a number
        // fitted to the miss that happened to be observed — so the figure is
        // asserted, not printed. (Not printed at all: console output is banned in
        // a module and the hygiene scan is not `cfg(test)`-aware.)
        assert!(
            worst <= EXPECTED_WORST_UNVERIFIED,
            "indirect-probe parity on {:?}: this adapter deviates by {worst:e} (at {at}), \
             more than the ESTIMATE {EXPECTED_WORST_UNVERIFIED:e}. That estimate was never \
             run — the final-wave brief forbade building — so if {worst:e} is still well \
             under the {TOLERANCE:e} budget, REPLACE the estimate with this measured \
             number and re-read the error account in its doc comment. Do NOT raise \
             TOLERANCE. Per entry point: {summary}",
            gpu.backend
        );
    }
}
