//! Deterministic roadside scenery: what stands beside the road, and where.
//!
//! Scenery in a racing game is not decoration — it is the **speed cue**. A car
//! at 320 km/h on an empty plain looks stationary; the same car with reflector
//! posts flicking past two metres from the camera looks terrifying. So the
//! priorities here are, in order: *frequent small things close to the road*,
//! *occasional large landmarks*, and only then *anything on the horizon*.
//!
//! ## Determinism
//!
//! A prop's existence, kind, position, scale and rotation are a pure function of
//! `(seed, chunk index, side, slot)`. Nothing consults a frame counter or a
//! clock. That is what makes chunk recycling safe: chunk 41 regenerates exactly
//! the same trees whether you reach it in the first minute or after three
//! resets, and the test suite asserts it.
//!
//! ## Bounded by construction
//!
//! Every generator here is a `for` loop over a count derived from the chunk's
//! length — never a "keep placing until it looks full" loop. The per-kind pool
//! capacities below are the hard ceiling on instances, and anything that does
//! not fit a pool slot simply is not drawn.

use axiom::prelude::Vec3;

use crate::draw::Draw;
use crate::track::{SectionKind, Track, TrackSample, Zone};
use crate::tuning::CourseTuning;

use super::prop_meshes::CROWN_ROOT_HEIGHT;
use super::road_mesh::{chunk_sample_range, CHUNK_LENGTH};

/// The prop archetypes. Each is one mesh and one material, so a pool of them is
/// a single draw call however many are on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropKind {
    /// A short reflector post at the road edge. The workhorse speed cue.
    Post,
    /// A tree: trunk plus a conical crown.
    Tree,
    /// A boulder — the one prop whose mesh is *baked from a recipe* rather than
    /// authored or taken from the engine's primitives. See
    /// [`super::rock_mesh`]. This kind also carries the distant horizon hills
    /// ([`distant_hills`]), which are spawned statically against the cube.
    Rock,
    /// A utility pole with a crossbar.
    Pole,
    /// A roadside sign board.
    Sign,
    /// A ceiling light inside a tunnel.
    TunnelLight,
    /// A low industrial building.
    Building,
    /// The bare stem of a coastal palm. Half of a palm; the other half is a
    /// [`PropKind::PalmCrown`] seated on its top, because one pool draws one
    /// mesh with one material and a palm is emphatically two of each.
    PalmTrunk,
    /// The frond fan of a coastal palm.
    PalmCrown,
    /// A clump of roadside undergrowth: one splayed rosette of leaf blades.
    ///
    /// The floor of the roadside. A palm avenue gives the verge its vertical
    /// beat; without something growing between the stems the ground under it is
    /// an unbroken sheet of one colour, which is the one thing no coast road has
    /// ever looked like.
    Shrub,
}

impl PropKind {
    /// Every kind, in a stable order — the pool is laid out in this order.
    pub const ALL: [PropKind; 10] = [
        PropKind::Post,
        PropKind::Tree,
        PropKind::Rock,
        PropKind::Pole,
        PropKind::Sign,
        PropKind::TunnelLight,
        PropKind::Building,
        PropKind::PalmTrunk,
        PropKind::PalmCrown,
        PropKind::Shrub,
    ];

    /// The hard ceiling on live instances of this kind.
    pub const fn pool_capacity(self) -> usize {
        match self {
            PropKind::Post => 460,
            PropKind::Tree => 200,
            PropKind::Rock => 240,
            PropKind::Pole => 60,
            PropKind::Sign => 24,
            PropKind::TunnelLight => 220,
            PropKind::Building => 60,
            // Two ranks on two shoulders at [`PALM_SPACING`] across the whole
            // active window, with headroom. A pool that runs out does not fail
            // loudly — it silently stops drawing palms partway down the road,
            // which is the one thing an avenue must never do.
            PropKind::PalmTrunk | PropKind::PalmCrown => 380,
            // The densest kind on the course, because ground cover is the one
            // thing that only works in quantity. Sized to the whole active
            // window at [`SHRUB_SPACING`] on both shoulders, with headroom.
            PropKind::Shrub => 480,
        }
    }

    /// Half-extents of this kind's bounding box (m), for culling and LOD.
    pub const fn half_extents(self) -> Vec3 {
        match self {
            PropKind::Post => Vec3::new(0.16, 0.6, 0.16),
            PropKind::Tree => Vec3::new(2.2, 5.0, 2.2),
            PropKind::Rock => Vec3::new(1.8, 1.4, 1.8),
            PropKind::Pole => Vec3::new(1.4, 5.5, 0.3),
            PropKind::Sign => Vec3::new(1.8, 1.6, 0.2),
            PropKind::TunnelLight => Vec3::new(0.7, 0.2, 0.25),
            PropKind::Building => Vec3::new(9.0, 5.0, 9.0),
            // The *unit* palm: mostly stem, 11.2 m of it, 0.44 m thick, so the
            // crown clears the car's roofline and the road is seen *through*
            // the avenue rather than behind a hedge. What is actually planted
            // is this box times a rank's size draw — see [`PALM_RANKS`], where
            // the avenue's real height is set.
            PropKind::PalmTrunk => Vec3::new(0.22, 5.6, 0.22),
            PropKind::PalmCrown => Vec3::new(3.0, 1.7, 3.0),
            // Knee-to-waist high and wider than it is tall — a plant, not a
            // bush-shaped tree. Anything taller hides the road edge from the
            // chase camera, which is the one thing the verge must never do.
            PropKind::Shrub => Vec3::new(1.05, 0.62, 1.05),
        }
    }

    /// The distance band (m) past which this kind stops being drawn at all.
    ///
    /// Small things vanish early because they are invisible at range anyway;
    /// buildings and trees survive further out because their silhouettes are
    /// what give the middle distance any shape.
    pub const fn draw_distance(self) -> f32 {
        match self {
            PropKind::Post => 380.0,
            PropKind::Tree => 700.0,
            PropKind::Rock => 620.0,
            PropKind::Pole => 700.0,
            PropKind::Sign => 460.0,
            PropKind::TunnelLight => 400.0,
            PropKind::Building => 900.0,
            // An avenue has to recede all the way to the vanishing point or it
            // is not an avenue, so a palm outlives every other roadside prop.
            PropKind::PalmTrunk | PropKind::PalmCrown => 900.0,
            // A metre-high plant is under a pixel well before this, and there
            // are hundreds of them: the near verge is the only place they earn
            // their draw, so they stop early and stay cheap.
            PropKind::Shrub => 240.0,
        }
    }
}

/// One placed prop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropInstance {
    pub kind: PropKind,
    /// Base position: the point the prop stands on.
    pub position: Vec3,
    /// Yaw (radians) the prop faces.
    pub yaw: f32,
    /// Per-instance scale multiplier.
    pub scale: Vec3,
}

/// Salt separating the scenery stream from the traffic and course streams.
const SCENERY_SALT: u64 = 0x51E4_A72B_0D93_6C11;

/// Generate every prop in chunk `index`. A pure function of its arguments.
pub fn props_for_chunk(
    seed: u64,
    track: &Track,
    index: usize,
    tuning: &CourseTuning,
    out: &mut Vec<PropInstance>,
) {
    out.clear();
    let (start, end) = chunk_sample_range(track, index);
    if start >= end {
        return;
    }
    let from = track.samples()[start].distance;
    let to = track.samples()[end].distance;
    let mut draw = Draw::seeded(seed).fork(SCENERY_SALT ^ index as u64);

    reflector_posts(track, from, to, tuning, out);
    tunnel_lights(track, from, to, out);
    coastal_palms(seed, track, index, from, to, out);
    verge_undergrowth(seed, track, index, from, to, out);
    zone_props(&mut draw, track, from, to, out);
}

/// Reflector posts, marching along both edges at a fixed metre spacing.
///
/// These are placed by *distance*, not randomly, and deliberately so: their
/// value is entirely in the regularity. A post every eight metres at 90 m/s is
/// eleven a second past each shoulder, and that beat is the most direct speed
/// signal the game has.
fn reflector_posts(
    track: &Track,
    from: f32,
    to: f32,
    tuning: &CourseTuning,
    out: &mut Vec<PropInstance>,
) {
    let spacing = tuning.post_spacing.max(1.0);
    let first = (from / spacing).ceil();
    let count = ((to - from) / spacing).ceil().max(0.0) as usize;
    for i in 0..count {
        let distance = (first + i as f32) * spacing;
        if distance > to {
            break;
        }
        let sample = track.interpolated_at(distance);
        // Inside the barrier, right at the edge of the shoulder — close enough
        // to the camera to blur past, far enough not to be hit.
        let offset = sample.half_width + track.shoulder() + POST_INSET;
        for side in [-1.0f32, 1.0] {
            out.push(PropInstance {
                kind: PropKind::Post,
                position: sample.at_lateral(side * offset),
                yaw: sample.heading,
                scale: Vec3::ONE,
            });
        }
    }
}

/// How far outside the shoulder a reflector post stands (m).
const POST_INSET: f32 = 0.35;

/// Ceiling lights through a tunnel, at a tight spacing so they strobe.
fn tunnel_lights(track: &Track, from: f32, to: f32, out: &mut Vec<PropInstance>) {
    let count = ((to - from) / TUNNEL_LIGHT_SPACING).ceil().max(0.0) as usize;
    let first = (from / TUNNEL_LIGHT_SPACING).ceil();
    for i in 0..count {
        let distance = (first + i as f32) * TUNNEL_LIGHT_SPACING;
        if distance > to {
            break;
        }
        let sample = track.interpolated_at(distance);
        if !sample.section.walled() {
            continue;
        }
        out.push(PropInstance {
            kind: PropKind::TunnelLight,
            position: sample
                .position
                .add(sample.up.mul_scalar(super::road_mesh::TUNNEL_HEIGHT - 0.35)),
            yaw: sample.heading,
            scale: Vec3::ONE,
        });
    }
}

/// Spacing of tunnel ceiling lights (m).
const TUNNEL_LIGHT_SPACING: f32 = 11.0;

/// The coastal grove: **two** ranks of palms down both shoulders of every
/// coastal section.
///
/// This is placed **by distance**, like the reflector posts and unlike the
/// random zone scatter, and that is the whole point. A coast road is not a
/// clearing with some trees in it; it is a corridor, and a corridor is made by
/// regular repetition marching to the vanishing point. Scattering the same
/// number of palms at random would cost the same and read as a swamp.
///
/// But a *single* file of stems at the barrier line is not a coast either — it
/// is a colonnade, and a colonnade has no depth. Every palm sits at the same
/// distance from the road, so they all subtend the same angle, all cross the
/// horizon at the same height, and the eye reads one thin picket fence of
/// aerials with empty ground behind it. What makes a coast road look planted is
/// that the trees stand at *several* depths: a near rank you drive past and a
/// further rank showing through the gaps between them, so the verge has a back
/// wall instead of a sky-coloured hole.
///
/// So the grove is authored as [`PALM_RANKS`] — a near avenue and a back rank
/// half a spacing out of step with it, set well back and drawn from a larger
/// size range so it reads *over* the avenue rather than hiding inside it. The
/// half-spacing phase is what stops the two ranks pairing up into one thick
/// row: along the road the palms now beat past at half the old interval.
///
/// Each palm is emitted as two instances — a stem and a crown seated on its top
/// — so a palm's two materials are two pools and therefore still two draw calls
/// for the whole grove, however many ranks it is planted in.
fn coastal_palms(
    seed: u64,
    track: &Track,
    index: usize,
    from: f32,
    to: f32,
    out: &mut Vec<PropInstance>,
) {
    let mut draw = Draw::seeded(seed).fork(PALM_SALT ^ index as u64);
    let first = (from / PALM_SPACING).ceil();
    let count = ((to - from) / PALM_SPACING).ceil().max(0.0) as usize;
    for i in 0..count {
        let slot = (first + i as f32) * PALM_SPACING;
        if slot > to {
            break;
        }
        // Each rank owns the same slot index, so a rank's palms belong to
        // exactly one chunk however far past the chunk's own end its phase
        // carries them — no seam duplicates, no seam gaps.
        for rank in PALM_RANKS {
            let sample = track.interpolated_at(slot + rank.phase * PALM_SPACING);
            if sample.section.zone() != Zone::Coast {
                continue;
            }
            for side in [-1.0f32, 1.0] {
                // Drawn unconditionally, before the side is known to be used, so
                // the stream advances the same way for both shoulders.
                let size = draw.range(rank.smallest, rank.largest);
                let depth = rank.inset + draw.range(0.0, rank.spread);
                let spin = draw.range(0.0, std::f32::consts::TAU);
                let scale = Vec3::ONE.mul_scalar(size);
                let base = sample
                    .at_lateral(side * (track.barrier_offset(&sample) + depth))
                    .add(Vec3::new(0.0, -PROP_SINK, 0.0));
                let trunk_top = base.y + PropKind::PalmTrunk.half_extents().y * 2.0 * size;
                let crown_height = PropKind::PalmCrown.half_extents().y * 2.0 * size;
                out.push(PropInstance {
                    kind: PropKind::PalmTrunk,
                    position: base,
                    yaw: sample.heading,
                    scale,
                });
                out.push(PropInstance {
                    kind: PropKind::PalmCrown,
                    // Seated so the fronds *leave the stem at its top*: the crown
                    // box's root height is where the blades meet, and it is the same
                    // constant the mesh is authored around.
                    position: Vec3::new(
                        base.x,
                        trunk_top - crown_height * CROWN_ROOT_HEIGHT,
                        base.z,
                    ),
                    yaw: spin,
                    scale,
                });
            }
        }
    }
}

/// One rank of the coastal grove — a row of palms at its own depth, its own
/// phase along the road and its own size range.
struct PalmRank {
    /// Where this rank stands along the road relative to the slot, as a
    /// fraction of [`PALM_SPACING`]. Two ranks at different phases interleave
    /// instead of standing in pairs, which is what halves the visible interval.
    phase: f32,
    /// How far beyond the barrier this rank's nearest palm stands (m).
    inset: f32,
    /// How much further out a palm in this rank may be set (m), so the rank is
    /// a rank and not a ruler.
    spread: f32,
    /// The smallest and largest a palm in this rank may be drawn.
    smallest: f32,
    largest: f32,
}

/// The two ranks the coast is planted in: the avenue at the barrier line, and
/// the back rank showing through the gaps in it.
///
/// ## The sizes are a **framing** decision, and they are forced by arithmetic
///
/// A roadside tree's job in this shot is not to be scenery, it is to be the
/// **proscenium**: the pair of verticals at the frame edges that turn an open
/// plain into a corridor. Whether it can do that job is not a matter of taste —
/// it is a single ratio, and the ratio is decided here.
///
/// A palm of height `H` standing `x` metres off the centreline, seen by an eye
/// at height `h`, reaches its greatest height in frame at the moment it crosses
/// the frame edge, and there it stands exactly
///
/// ```text
///     y_above_horizon = 0.5 · (H − h) · aspect / x     (fractions of frame)
/// ```
///
/// above the horizon — **at every field of view**, because both the palm's
/// height and its lateral offset are magnified by the same lens. No camera
/// change can lift it: only the height-to-offset ratio can.
///
/// Measured on the judged arm against the era-C reference, on a 0.562 phone
/// frame with the horizon at 0.47:
///
/// | | reference | champion (0.80…1.52) |
/// |---|---|---|
/// | topmost roadside foliage | **0.17** of frame | **0.35** of frame |
/// | foliage as a share of the frame | 4.45% | 0.73% |
/// | implied `(H − h) / x` | ~1.4 | 0.43 |
///
/// The champion was not missing palms and was not framed wrong — the horizon,
/// the car's width and the car's contact point are all already at parity. It
/// was planting **young** palms: a 9–14 m stem standing 20 m off the road can
/// only ever reach 0.12 of frame above the horizon, which is precisely the 0.35
/// that was measured. Every pixel above that was sky, so the top third of the
/// picture carried nothing at all.
///
/// The offset is not free to change — a prop must stand outside
/// [`crate::track::Track::barrier_offset`], and on this road that is 17.8 m of
/// tarmac, shoulder and verge before a palm may be planted. So the avenue is
/// grown into the mature coastal palm the reference is plainly drawing: stems of
/// 16–21 m in the avenue and 18–25 m behind it, which are ordinary heights for
/// a coconut or royal palm and take the crowns to ~0.27 of frame — about half
/// the gap closed, honestly, without a tree that could not exist.
///
/// The crowns still clear the road: a crown's fan is 3.0 m of unit half-extent,
/// so even at 1.90 it reaches 5.7 m from its stem and stops ~2 m short of the
/// tarmac edge, eighteen metres up. The avenue is still seen *through*.
const PALM_RANKS: [PalmRank; 2] = [
    // The avenue. Close enough to the barrier to sweep past the camera, and the
    // rank that carries the corridor's beat.
    PalmRank {
        phase: 0.0,
        inset: PALM_INSET,
        spread: PALM_DEPTH_JITTER,
        smallest: 1.45,
        largest: 1.90,
    },
    // The back rank, half a spacing out of step. Set far enough back that it is
    // plainly *behind* the avenue rather than beside it, jittered across a much
    // deeper band so it never lines up, and drawn from a taller size range so
    // its crowns clear the near rank's instead of vanishing under them. Standing
    // 13–27 m further out, it needs the extra height twice over: to clear the
    // avenue, and to hold the same ratio at its own larger offset.
    PalmRank {
        phase: 0.5,
        inset: PALM_BACK_INSET,
        spread: PALM_BACK_BAND,
        smallest: 1.62,
        largest: 2.25,
    },
];

/// Salt separating the palm stream from the rest of the scenery.
const PALM_SALT: u64 = 0x7A19_C4E0_2B85_D33F;
/// Along-course spacing of a rank's palm pair (m). Wide enough that the avenue
/// is seen through rather than along a wall; with two ranks half a spacing out
/// of step, a palm beats past every twelve metres.
const PALM_SPACING: f32 = 24.0;
/// How far beyond the barrier the nearest palm stands (m).
const PALM_INSET: f32 = 2.6;
/// How much further out a palm may be set, so the row is a row and not a ruler.
const PALM_DEPTH_JITTER: f32 = 3.4;
/// How far beyond the barrier the back rank starts (m). Past the undergrowth
/// band, so the grove reads as three receding layers — plants, avenue, grove.
const PALM_BACK_INSET: f32 = 13.0;
/// How deep the back rank's band runs beyond its inset (m).
const PALM_BACK_BAND: f32 = 14.0;

/// The undergrowth band: ground cover crowding the verge through every green
/// zone, from the barrier line out into the middle distance.
///
/// This is the floor the palm avenue stands on. It is placed by distance for
/// the same reason the avenue is — the band has to be continuous, and a random
/// scatter at this density leaves bald patches that read as mown lawn — but
/// unlike the avenue every clump is jittered hard in depth, size and spin. The
/// beat here is not the point; the *cover* is, and a visible rhythm in ground
/// cover is a hedge.
///
/// Rock and tunnel zones are skipped outright: a canyon floor and a tunnel are
/// bare by definition, and planting them would undo the thing that makes those
/// stretches feel different from the coast.
fn verge_undergrowth(
    seed: u64,
    track: &Track,
    index: usize,
    from: f32,
    to: f32,
    out: &mut Vec<PropInstance>,
) {
    let mut draw = Draw::seeded(seed).fork(SHRUB_SALT ^ index as u64);
    let first = (from / SHRUB_SPACING).ceil();
    let count = ((to - from) / SHRUB_SPACING).ceil().max(0.0) as usize;
    for i in 0..count {
        let distance = (first + i as f32) * SHRUB_SPACING;
        if distance > to {
            break;
        }
        let sample = track.interpolated_at(distance);
        let planted = matches!(
            sample.section.zone(),
            Zone::Coast | Zone::Meadow | Zone::Forest
        );
        for side in [-1.0f32, 1.0] {
            // Every draw happens before the zone is consulted, so the stream
            // advances identically whether or not this slot is planted — the
            // undergrowth of a coastal chunk cannot depend on what zone the
            // chunk before it was.
            let size = draw.range(0.70, 1.45);
            let depth = SHRUB_INSET + draw.range(0.0, SHRUB_BAND);
            let spin = draw.range(0.0, std::f32::consts::TAU);
            if !planted {
                continue;
            }
            out.push(PropInstance {
                kind: PropKind::Shrub,
                position: sample
                    .at_lateral(side * (track.barrier_offset(&sample) + depth))
                    .add(Vec3::new(0.0, -PROP_SINK, 0.0)),
                yaw: spin,
                scale: Vec3::ONE.mul_scalar(size),
            });
        }
    }
}

/// Salt separating the undergrowth stream from the palms and the scatter.
const SHRUB_SALT: u64 = 0x3D62_18BC_9F04_A7E5;
/// Along-course spacing of an undergrowth pair (m). One clump per shoulder every
/// eight metres, jittered across a wide band, is what turns a green sheet into a
/// verge without turning it into a wall.
const SHRUB_SPACING: f32 = 8.0;
/// How far beyond the barrier the undergrowth band starts (m). Close, because
/// the plants nearest the camera are the ones doing the work.
const SHRUB_INSET: f32 = 0.9;
/// How deep the band runs beyond its inset (m) — wide enough that consecutive
/// clumps never line up into a row.
const SHRUB_BAND: f32 = 11.0;

/// The zone-specific scatter: trees, rocks, poles, signs, buildings.
fn zone_props(
    draw: &mut Draw,
    track: &Track,
    from: f32,
    to: f32,
    out: &mut Vec<PropInstance>,
) {
    let span = (to - from).max(1.0);
    let slots = ((span / SLOT_SPACING).round().max(1.0)) as usize;
    for slot in 0..slots {
        let distance = from + (slot as f32 + 0.5) * (span / slots as f32);
        let sample = track.interpolated_at(distance);
        let zone = sample.section.zone();
        // A tunnel's walls leave no room for anything outside them.
        if sample.section.walled() && zone == Zone::Tunnel {
            continue;
        }
        for side in [-1.0f32, 1.0] {
            let Some(kind) = pick_kind(draw, zone, sample.section) else {
                continue;
            };
            let margin = track.barrier_offset(&sample) + kind.half_extents().x + PROP_MARGIN;
            let reach = margin + draw.range(0.0, scatter_depth(kind));
            let position = sample.at_lateral(side * reach).add(Vec3::new(0.0, -PROP_SINK, 0.0));
            out.push(PropInstance {
                kind,
                position,
                yaw: sample.heading + draw.range(-0.6, 0.6),
                scale: Vec3::ONE.mul_scalar(draw.range(0.78, 1.35)),
            });
        }
    }
}

/// Along-course spacing of a zone-prop slot (m).
const SLOT_SPACING: f32 = 14.0;
/// Clearance kept between a prop and the barrier (m).
const PROP_MARGIN: f32 = 1.2;
/// How far props are sunk into the verge so they never appear to float (m).
const PROP_SINK: f32 = 0.6;

/// How far out from the barrier a kind may scatter (m).
fn scatter_depth(kind: PropKind) -> f32 {
    match kind {
        PropKind::Tree => 26.0,
        PropKind::Rock => 14.0,
        PropKind::Pole => 4.0,
        PropKind::Sign => 1.5,
        PropKind::Building => 22.0,
        _ => 6.0,
    }
}

/// Choose what stands at a slot in this zone, or nothing.
///
/// The `None` arm is what stops the roadside becoming uniform visual noise: most
/// slots in an open zone are deliberately empty, so the ones that are filled
/// read as landmarks rather than as wallpaper.
fn pick_kind(draw: &mut Draw, zone: Zone, section: SectionKind) -> Option<PropKind> {
    let roll = draw.unit();
    let _ = section;
    match zone {
        Zone::Meadow => (roll < 0.30).then(|| {
            if roll < 0.06 {
                PropKind::Pole
            } else if roll < 0.09 {
                PropKind::Sign
            } else {
                PropKind::Tree
            }
        }),
        // No conifers on a beach. The coast's trees are the palm avenue, placed
        // by distance in `coastal_palms`; what is left to scatter here is the
        // shoreline boulders and the occasional pole.
        Zone::Coast => (roll < 0.18).then(|| {
            if roll < 0.05 {
                PropKind::Pole
            } else {
                PropKind::Rock
            }
        }),
        Zone::Forest => (roll < 0.82).then(|| {
            if roll < 0.05 {
                PropKind::Rock
            } else {
                PropKind::Tree
            }
        }),
        Zone::Industrial => (roll < 0.46).then(|| {
            if roll < 0.14 {
                PropKind::Building
            } else if roll < 0.30 {
                PropKind::Pole
            } else if roll < 0.34 {
                PropKind::Sign
            } else {
                PropKind::Tree
            }
        }),
        Zone::Canyon => (roll < 0.92).then_some(PropKind::Rock),
        // A tunnel's props are its ceiling lights, placed separately.
        Zone::Tunnel => None,
    }
}

/// Distant hills, generated once for the whole course rather than per chunk.
///
/// These exist only to close the horizon; they are large, sparse, far from the
/// road, and never move. Generating them up front (there are a few dozen for a
/// nine-kilometre course) is cheaper and simpler than streaming something the
/// player can see from anywhere anyway.
pub fn distant_hills(seed: u64, track: &Track) -> Vec<PropInstance> {
    let mut draw = Draw::seeded(seed).fork(HILL_SALT);
    let count = ((track.length() / HILL_SPACING).round().max(1.0)) as usize;
    (0..count)
        .flat_map(|i| {
            let distance = (i as f32 + 0.5) * HILL_SPACING;
            let sample = track.sample_at(distance);
            [-1.0f32, 1.0].map(|side| {
                let out = draw.range(HILL_MIN_OFFSET, HILL_MAX_OFFSET);
                let height = draw.range(24.0, 70.0);
                PropInstance {
                    kind: PropKind::Rock,
                    position: sample
                        .at_lateral(side * out)
                        .add(Vec3::new(0.0, -height * 0.45, 0.0)),
                    yaw: draw.range(0.0, std::f32::consts::TAU),
                    scale: Vec3::new(height * 0.9, height, height * 0.9),
                }
            })
        })
        .collect()
}

/// Salt for the distant-hill stream.
const HILL_SALT: u64 = 0x2C77_9F31_84AE_0052;
/// Along-course spacing of hill pairs (m).
const HILL_SPACING: f32 = 260.0;
/// Closest a hill may sit to the road (m).
const HILL_MIN_OFFSET: f32 = 190.0;
/// Furthest a hill may sit from the road (m).
const HILL_MAX_OFFSET: f32 = 560.0;

/// A prop's world bounding box, for culling and LOD.
pub fn prop_bounds(prop: &PropInstance) -> (Vec3, Vec3) {
    let half = Vec3::new(
        prop.kind.half_extents().x * prop.scale.x,
        prop.kind.half_extents().y * prop.scale.y,
        prop.kind.half_extents().z * prop.scale.z,
    );
    (prop.position.add(Vec3::new(0.0, half.y, 0.0)), half)
}

/// The reference sample a chunk's props are keyed to — used by diagnostics.
pub fn chunk_reference(track: &Track, index: usize) -> TrackSample {
    let (start, _) = chunk_sample_range(track, index);
    track.samples()[start]
}

/// How many chunks of scenery fit around the player at once, given the render
/// range. Used to size the per-chunk cache.
pub fn chunk_cache_capacity(ahead: usize, behind: usize) -> usize {
    ahead + behind + 2
}

/// The along-course span one chunk of scenery covers (m).
pub const SCENERY_CHUNK_LENGTH: f32 = CHUNK_LENGTH;

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track::fixture(crate::DEFAULT_SEED)
    }

    fn props(index: usize) -> Vec<PropInstance> {
        let mut out = Vec::new();
        props_for_chunk(
            crate::DEFAULT_SEED,
            &track(),
            index,
            &CourseTuning::DEFAULT,
            &mut out,
        );
        out
    }

    /// The property that makes chunk recycling safe.
    #[test]
    fn a_chunks_scenery_is_a_pure_function_of_seed_and_chunk() {
        for index in [0usize, 7, 33, 61] {
            assert_eq!(props(index), props(index), "chunk {index} is reproducible");
        }
        let track = track();
        let mut a = Vec::new();
        let mut b = Vec::new();
        props_for_chunk(1, &track, 20, &CourseTuning::DEFAULT, &mut a);
        props_for_chunk(2, &track, 20, &CourseTuning::DEFAULT, &mut b);
        assert_ne!(a, b, "a different seed gives different scenery");
    }

    #[test]
    fn generating_into_a_dirty_buffer_clears_it_first() {
        let track = track();
        let mut out = vec![PropInstance {
            kind: PropKind::Sign,
            position: Vec3::new(999.0, 999.0, 999.0),
            yaw: 0.0,
            scale: Vec3::ONE,
        }];
        props_for_chunk(crate::DEFAULT_SEED, &track, 5, &CourseTuning::DEFAULT, &mut out);
        assert!(!out.iter().any(|p| p.position.x == 999.0), "stale props are gone");
    }

    /// Reflector posts sit at the **shoulder edge**, inside the barrier line —
    /// deliberately, because their whole value is passing close to the camera.
    /// Everything else stands beyond the barrier where the car cannot reach it.
    #[test]
    fn every_prop_is_finite_and_clear_of_the_driving_surface() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let mut out = Vec::new();
        for index in 0..super::super::road_mesh::chunk_count(&track) {
            props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
            for prop in &out {
                let p = prop.position;
                assert!(
                    p.x.is_finite() && p.y.is_finite() && p.z.is_finite(),
                    "chunk {index}: {prop:?}"
                );
                assert!(prop.scale.x > 0.0 && prop.scale.y > 0.0 && prop.scale.z > 0.0);
                assert!(prop.yaw.is_finite());
                // Ceiling lights are the one kind that lives over the road.
                if prop.kind == PropKind::TunnelLight {
                    continue;
                }
                let (distance, lateral) =
                    track.localise(p, chunk_reference(&track, index).distance, 220.0);
                let sample = track.sample_at(distance);
                let floor = if prop.kind == PropKind::Post {
                    // At the shoulder edge: off the tarmac, close to the camera.
                    sample.half_width
                } else {
                    // Beyond the barrier, where the car cannot reach it.
                    track.barrier_offset(&sample) - 1.0
                };
                assert!(
                    lateral.abs() >= floor,
                    "chunk {index}: a {:?} at {lateral} m is inside the {floor} m line",
                    prop.kind
                );
            }
        }
    }

    #[test]
    fn reflector_posts_march_along_both_edges_at_a_constant_spacing() {
        let t = CourseTuning::DEFAULT;
        let posts: Vec<PropInstance> = props(3)
            .into_iter()
            .filter(|p| p.kind == PropKind::Post)
            .collect();
        assert!(posts.len() > 20, "a 100 m chunk has many posts: {}", posts.len());
        assert_eq!(posts.len() % 2, 0, "they come in pairs");

        // Consecutive pairs are one post spacing apart along the road.
        let track = track();
        let along: Vec<f32> = posts
            .iter()
            .step_by(2)
            .map(|p| track.localise(p.position, 0.0, 4_000.0).0)
            .collect();
        for w in along.windows(2) {
            assert!(
                (w[1] - w[0] - t.post_spacing).abs() < 1.0,
                "posts are {} m apart, expected {}",
                w[1] - w[0],
                t.post_spacing
            );
        }
    }

    /// The avenue: a coastal chunk carries palms down *both* shoulders at a
    /// constant spacing, and every stem has exactly one crown.
    #[test]
    fn the_coast_is_lined_with_a_palm_avenue_on_both_sides() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let mut out = Vec::new();
        let mut stems = 0usize;
        let mut crowns = 0usize;
        let mut sides = (0usize, 0usize);
        for index in 0..super::super::road_mesh::chunk_count(&track) {
            props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
            for prop in &out {
                let (distance, lateral) =
                    track.localise(prop.position, chunk_reference(&track, index).distance, 220.0);
                if prop.kind == PropKind::PalmTrunk {
                    stems += 1;
                    *[&mut sides.0, &mut sides.1][usize::from(lateral > 0.0)] += 1;
                    assert_eq!(
                        track.sample_at(distance).section.zone(),
                        Zone::Coast,
                        "a palm at {distance} m is inland"
                    );
                }
                crowns += usize::from(prop.kind == PropKind::PalmCrown);
            }
        }
        assert!(stems > 200, "the coast is genuinely lined: {stems} palms");
        assert_eq!(stems, crowns, "every stem carries exactly one crown");
        assert!(
            sides.0 > 0 && sides.1 > 0 && sides.0.abs_diff(sides.1) < stems / 4,
            "both shoulders are planted: {sides:?}"
        );
    }

    /// The grove has *depth*. This is the property a single-file avenue cannot
    /// have and the reason the second rank exists: palms stand at plainly
    /// different distances from the road, so the verge has a back wall showing
    /// through the gaps in the near row rather than open sky behind it.
    #[test]
    fn the_coastal_grove_stands_in_two_distinct_ranks() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let mut out = Vec::new();
        let mut near = 0usize;
        let mut back = 0usize;
        let mut alongs: Vec<f32> = Vec::new();
        for index in 0..60 {
            props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
            let origin = chunk_reference(&track, index).distance;
            for stem in out.iter().filter(|p| p.kind == PropKind::PalmTrunk) {
                let (distance, lateral) = track.localise(stem.position, origin, 240.0);
                let beyond = lateral.abs() - track.barrier_offset(&track.sample_at(distance));
                // The two bands do not overlap: the avenue ends well inside
                // where the back rank begins.
                let in_near = beyond < PALM_INSET + PALM_DEPTH_JITTER + 0.01;
                near += usize::from(in_near);
                back += usize::from(beyond >= PALM_BACK_INSET - 0.01);
                assert!(
                    in_near || beyond >= PALM_BACK_INSET - 0.01,
                    "a palm at {beyond} m sits between the ranks"
                );
                alongs.push(distance);
            }
        }
        assert!(near > 100, "the avenue is still planted: {near} palms");
        assert!(back > 100, "and a back rank stands behind it: {back} palms");
        assert!(
            near.abs_diff(back) < near / 3,
            "neither rank dominates: {near} near, {back} back"
        );
        // Out of step, not paired: consecutive palms along the road are about
        // half a spacing apart, which is the interval the driver actually sees.
        alongs.sort_by(f32::total_cmp);
        let mut steps: Vec<f32> = alongs
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|d| *d > 0.5)
            .collect();
        // The *median* step, not the mean: the course leaves the coast and comes
        // back, and one inland stretch is a single enormous gap that would drag
        // an average anywhere. The middle step is what the driver sees.
        steps.sort_by(f32::total_cmp);
        let typical = steps[steps.len() / 2];
        assert!(
            typical < PALM_SPACING * 0.75,
            "the ranks interleave: palms every {typical} m, spacing {PALM_SPACING}"
        );
    }

    /// **The avenue is tall enough to frame the shot.** The composition claim
    /// [`PALM_RANKS`] is authored against, pinned as arithmetic rather than left
    /// in prose: a palm crossing the frame edge stands
    /// `0.5 · (H − h) · aspect / x` of frame above the horizon, so the avenue is
    /// a proscenium only if its height-to-offset ratio is large enough. At the
    /// 9–14 m stems this replaced the ratio was 0.43 and the crowns could not
    /// reach past 0.12 of frame above the horizon — the top third of a phone
    /// frame was sky by construction.
    #[test]
    fn the_palm_avenue_stands_tall_enough_against_its_own_stand_off() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let eye = crate::tuning::CameraTuning::DEFAULT
            .framed_for_aspect(470.0 / 836.0)
            .height;
        let mut out = Vec::new();
        let mut ratios: Vec<f32> = Vec::new();
        let mut shortest = f32::MAX;
        for index in 0..60 {
            props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
            let origin = chunk_reference(&track, index).distance;
            for stem in out.iter().filter(|p| p.kind == PropKind::PalmTrunk) {
                let (_, lateral) = track.localise(stem.position, origin, 240.0);
                let height = PropKind::PalmTrunk.half_extents().y * 2.0 * stem.scale.y;
                shortest = shortest.min(height);
                ratios.push((height - eye) / lateral.abs().max(1.0));
            }
        }
        assert!(ratios.len() > 100, "the coast was reached: {}", ratios.len());
        assert!(
            shortest >= 15.0,
            "the shortest planted palm is {shortest} m — a sapling, not an avenue"
        );
        ratios.sort_by(f32::total_cmp);
        let typical = ratios[ratios.len() / 2];
        assert!(
            typical > 0.60,
            "the avenue's height-to-offset ratio is {typical}; below 0.6 its crowns \
             cannot break the skyline and the frame's upper half is sky"
        );
    }

    /// And it is still an avenue you see *through*, not a hedge: however tall a
    /// palm is drawn, its crown must stop short of the tarmac it stands beside.
    #[test]
    fn no_crown_however_large_reaches_over_the_road() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let mut out = Vec::new();
        let mut checked = 0usize;
        for index in 0..60 {
            props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
            let origin = chunk_reference(&track, index).distance;
            for crown in out.iter().filter(|p| p.kind == PropKind::PalmCrown) {
                let (distance, lateral) = track.localise(crown.position, origin, 240.0);
                let sample = track.sample_at(distance);
                let reach = lateral.abs() - PropKind::PalmCrown.half_extents().x * crown.scale.x;
                assert!(
                    reach > sample.half_width + track.shoulder(),
                    "a crown reaches to {reach} m over a {} m carriageway",
                    sample.half_width + track.shoulder()
                );
                checked += 1;
            }
        }
        assert!(checked > 100, "the coast was reached: {checked}");
    }

    /// The verge is *covered*, not decorated. This is the count test: ground
    /// cover only works in quantity, and a handful of plants per chunk is worse
    /// than none because it reads as litter on a lawn.
    #[test]
    fn the_green_verge_is_planted_thickly_on_both_shoulders() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let mut out = Vec::new();
        let mut clumps = 0usize;
        let mut sides = (0usize, 0usize);
        let mut depths: Vec<f32> = Vec::new();
        for index in 0..60 {
            props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
            let origin = chunk_reference(&track, index).distance;
            for prop in out.iter().filter(|p| p.kind == PropKind::Shrub) {
                clumps += 1;
                let (distance, lateral) = track.localise(prop.position, origin, 220.0);
                let sample = track.sample_at(distance);
                assert!(
                    matches!(
                        sample.section.zone(),
                        Zone::Coast | Zone::Meadow | Zone::Forest
                    ),
                    "a shrub at {distance} m is planted in bare ground"
                );
                let beyond = lateral.abs() - track.barrier_offset(&sample);
                assert!(beyond >= 0.0, "a shrub at {beyond} m is inside the barrier");
                depths.push(beyond);
                *[&mut sides.0, &mut sides.1][usize::from(lateral > 0.0)] += 1;
            }
        }
        assert!(clumps > 400, "the verge is genuinely covered: {clumps} clumps");
        assert!(
            sides.0 > 0 && sides.1 > 0 && sides.0.abs_diff(sides.1) < clumps / 4,
            "both shoulders are planted: {sides:?}"
        );
        // Scattered across the band rather than lined up: a constant depth is a
        // hedge, and a hedge is the failure mode this jitter exists to avoid.
        let spread = depths.iter().fold(f32::MIN, |m, d| m.max(*d))
            - depths.iter().fold(f32::MAX, |m, d| m.min(*d));
        assert!(spread > SHRUB_BAND * 0.5, "the band has depth: {spread} m");
    }

    /// A crown floating off its stem is the one way this can look broken, and
    /// the seating is pure arithmetic, so it is worth pinning exactly.
    #[test]
    fn a_palm_crown_is_seated_on_the_top_of_its_own_stem() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let mut out = Vec::new();
        let mut checked = 0usize;
        for index in 0..40 {
            props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
            let stems: Vec<PropInstance> = out
                .iter()
                .copied()
                .filter(|p| p.kind == PropKind::PalmTrunk)
                .collect();
            let crowns: Vec<PropInstance> = out
                .iter()
                .copied()
                .filter(|p| p.kind == PropKind::PalmCrown)
                .collect();
            for (stem, crown) in stems.iter().zip(&crowns) {
                assert!((stem.position.x - crown.position.x).abs() < 1.0e-4);
                assert!((stem.position.z - crown.position.z).abs() < 1.0e-4);
                assert_eq!(stem.scale, crown.scale, "one palm, one size");
                let top = stem.position.y
                    + PropKind::PalmTrunk.half_extents().y * 2.0 * stem.scale.y;
                let roots = crown.position.y
                    + PropKind::PalmCrown.half_extents().y * 2.0 * crown.scale.y * CROWN_ROOT_HEIGHT;
                assert!(
                    (top - roots).abs() < 1.0e-3,
                    "the fronds leave the stem at {roots} m but its top is {top} m"
                );
                checked += 1;
            }
        }
        assert!(checked > 20, "the coast was actually reached: {checked}");
    }

    #[test]
    fn the_pool_capacities_cover_what_the_active_range_generates() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let mut worst = std::collections::HashMap::new();
        let mut out = Vec::new();
        // The worst case is the densest window of consecutive chunks.
        let window = super::super::chunks::CHUNKS_AHEAD + super::super::chunks::CHUNKS_BEHIND + 1;
        let total = super::super::road_mesh::chunk_count(&track);
        for start in 0..total.saturating_sub(window) {
            let mut counts = std::collections::HashMap::new();
            for index in start..start + window {
                props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
                for prop in &out {
                    *counts.entry(prop.kind).or_insert(0usize) += 1;
                }
            }
            for (kind, count) in counts {
                let entry = worst.entry(kind).or_insert(0usize);
                *entry = (*entry).max(count);
            }
        }
        for (kind, count) in worst {
            assert!(
                count <= kind.pool_capacity(),
                "{kind:?} peaks at {count} instances but its pool holds {}",
                kind.pool_capacity()
            );
        }
    }

    #[test]
    fn each_zone_gets_its_own_vocabulary() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let mut out = Vec::new();
        let mut by_zone: std::collections::HashMap<Zone, std::collections::HashSet<PropKind>> =
            std::collections::HashMap::new();
        for index in 0..super::super::road_mesh::chunk_count(&track) {
            props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
            for prop in &out {
                // A 100 m chunk can straddle a section boundary, so a prop's
                // zone is read at the prop, not at the chunk.
                let (distance, _) =
                    track.localise(prop.position, chunk_reference(&track, index).distance, 220.0);
                by_zone
                    .entry(track.sample_at(distance).section.zone())
                    .or_default()
                    .insert(prop.kind);
            }
        }
        assert!(
            by_zone[&Zone::Forest].contains(&PropKind::Tree),
            "the forest has trees"
        );
        assert!(
            by_zone[&Zone::Canyon].contains(&PropKind::Rock),
            "the canyon has rocks"
        );
        assert!(
            !by_zone[&Zone::Canyon].contains(&PropKind::Tree),
            "and no trees"
        );
        assert!(
            by_zone[&Zone::Tunnel].contains(&PropKind::TunnelLight),
            "the tunnel has lights"
        );
        assert!(
            by_zone[&Zone::Industrial].contains(&PropKind::Building),
            "the industrial stretch has buildings"
        );
    }

    #[test]
    fn tunnel_lights_only_appear_inside_tunnels_and_above_the_road() {
        let track = track();
        let t = CourseTuning::DEFAULT;
        let mut out = Vec::new();
        let mut found = 0;
        for index in 0..super::super::road_mesh::chunk_count(&track) {
            props_for_chunk(crate::DEFAULT_SEED, &track, index, &t, &mut out);
            for prop in out.iter().filter(|p| p.kind == PropKind::TunnelLight) {
                found += 1;
                let (distance, _) = track.localise(prop.position, chunk_reference(&track, index).distance, 220.0);
                assert!(
                    track.sample_at(distance).section.walled(),
                    "a tunnel light at {distance} m is not in a tunnel"
                );
                assert!(prop.position.y > track.sample_at(distance).position.y + 3.0);
            }
        }
        assert!(found > 40, "the tunnel is genuinely lit: {found} lights");
    }

    #[test]
    fn distant_hills_are_far_from_the_road_and_deterministic() {
        let track = track();
        let a = distant_hills(crate::DEFAULT_SEED, &track);
        assert_eq!(a, distant_hills(crate::DEFAULT_SEED, &track));
        assert_ne!(a, distant_hills(9, &track));
        assert!(a.len() > 40, "the horizon is populated: {}", a.len());
        for hill in &a {
            assert!(hill.scale.y >= 24.0, "hills are large");
            assert!(hill.position.x.is_finite() && hill.position.z.is_finite());
        }
    }

    #[test]
    fn prop_bounds_sit_on_top_of_the_prop_position() {
        let prop = PropInstance {
            kind: PropKind::Tree,
            position: Vec3::new(4.0, 1.0, -2.0),
            yaw: 0.0,
            scale: Vec3::ONE.mul_scalar(2.0),
        };
        let (centre, half) = prop_bounds(&prop);
        assert!((half.y - PropKind::Tree.half_extents().y * 2.0).abs() < 1.0e-4);
        assert!((centre.y - (1.0 + half.y)).abs() < 1.0e-4, "the box stands on the ground");
    }

    #[test]
    fn every_kind_declares_a_pool_a_size_and_a_draw_distance() {
        for kind in PropKind::ALL {
            assert!(kind.pool_capacity() > 0);
            let half = kind.half_extents();
            assert!(half.x > 0.0 && half.y > 0.0 && half.z > 0.0);
            assert!(kind.draw_distance() > 100.0);
            assert!(scatter_depth(kind) >= 0.0);
        }
        // Small things vanish before big things.
        assert!(PropKind::Post.draw_distance() < PropKind::Building.draw_distance());
    }

    #[test]
    fn a_degenerate_chunk_produces_nothing_rather_than_panicking() {
        let track = track();
        let mut out = Vec::new();
        let beyond = super::super::road_mesh::chunk_count(&track) + 20;
        props_for_chunk(crate::DEFAULT_SEED, &track, beyond, &CourseTuning::DEFAULT, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn the_chunk_cache_is_sized_for_the_render_range() {
        assert!(chunk_cache_capacity(14, 2) >= 16);
        assert_eq!(SCENERY_CHUNK_LENGTH, CHUNK_LENGTH);
    }
}
