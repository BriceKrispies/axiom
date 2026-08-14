//! The concentric field of counter-rotating rings: how many rings there are, how
//! many dogs walk each one, where each dog starts, which way round it faces, and
//! which entry of the shared colour palette it is painted from.
//!
//! ## The layout is derived, not typed
//!
//! Nothing here is a hand-tuned ring list or dog count. Three measured facts —
//! how long a dog is, how wide it is, and how far the terrain reaches — produce
//! the whole field:
//!
//! * a ring knows its radius, and its dog count is its circumference divided by
//!   the room one dog needs ([`DOG_SPACING`]), rounded to a whole animal;
//! * the rings are a fixed radial pitch apart ([`RING_SPACING`]), and that pitch
//!   is set by the dog's **width** plus the outward bulge a rigid body makes on a
//!   curve — not by its length;
//! * the innermost radius is the tightest curve the gait is tuned for, and the
//!   outermost is the largest circle that still leaves clear ground before the
//!   terrain's rim.
//!
//! Move [`RING_MAX_RADIUS`] and the field re-populates itself with the right
//! number of rings, each holding the right number of dogs, because none of those
//! numbers was ever authored.
//!
//! ## The radial pitch: bounded by width, not length
//!
//! A dog is [`DOG_WIDTH`] (4.24) across and [`DOG_LENGTH`] (21.0) long, and it is
//! laid **along** its ring — so what separates two rings is the width, plus one
//! correction. A rigid body of length `L` standing on a circle of radius `R` has
//! its centre on the circle and its nose and tail *outside* it, by
//! `sqrt(R² + (L/2)²) − R ≈ (L/2)² / (2R)` — see [`body_bulge`]. Two rings
//! therefore clear each other when
//!
//! ```text
//! RING_SPACING  ≥  DOG_WIDTH  +  body_bulge(inner radius)
//! ```
//!
//! At the tightest pair (26 → 34) that is `4.24 + 2.12 = 6.36`, and
//! [`RING_SPACING`] is `8.0`: **1.64 units of clear air** between the outermost
//! point of one ring's dogs and the innermost point of the next ring's. At the
//! widest pair (74 → 82) the bulge is `0.74` and the air is `3.02`.
//!
//! ## Which way is which
//!
//! Seen from `+Y` looking down at the `XZ` plane — the way the framing camera
//! sees it — a point at `(R·cos θ, R·sin θ)` traversed with **increasing** `θ`
//! goes **clockwise**. (Take screen-right as `+X` and screen-up as `-Z`, the
//! ordinary map orientation: the point's screen angle is `-θ`, so advancing `θ`
//! turns the short way round the clock.) That single fact is what
//! [`Winding::sign`] encodes, and the ring's **index parity** is the whole of
//! what picks one — so every ring necessarily turns against both its neighbours.
//!
//! The direction is then testable without trusting any of this prose: for a
//! position `p` measured from the ring centre and a heading `h`, the `y`
//! component of `p × h` is `p.z·h.x − p.x·h.z`, which is `−R²` for a clockwise
//! walk and `+R²` for a counter-clockwise one. `tests/rings.rs` asserts exactly
//! that sign on the real posed bones, for every adjacent pair.
//!
//! ## Why the colours come from a bounded palette
//!
//! A dog's colour reaches the GPU **only** through its material: the per-instance
//! `colour[4]` in the instance stream is filled from the material the draw names
//! (`axiom-render-pipeline`'s `MaterialSlot`), and draws batch on the
//! `(mesh_id, material_id)` pair. One material per dog would therefore mean
//! `23 bones × dogs` single-instance batches — 2760 draw calls at this crowd
//! size, which throws instancing away entirely.
//!
//! So the field is painted from a fixed [`PALETTE_SIZE`]-entry palette that every
//! dog shares, and the batch count is `23 × PALETTE_SIZE + 1` **whatever the
//! crowd size** (at most 415; 392 for the field as laid out, which wears 17 of
//! the 18 coats). The palette is split into two interleaved combs of
//! [`RING_COMB`] hues; a ring uses the comb its index parity names, so a dog and
//! any dog on an adjacent ring are at least `1/PALETTE_SIZE` of a turn apart in
//! hue, and two dogs adjacent *along* a ring are at least `2/PALETTE_SIZE` apart.

use crate::creature_pose::DOG_GAIT;
use crate::rainbow::hue_to_rgb;
use crate::terrain::TERRAIN_HALF_EXTENT;

/// Which way round a ring is walked, seen from `+Y` looking down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Winding {
    /// Anticlockwise from above — every even-indexed ring.
    CounterClockwise,
    /// Clockwise from above — every odd-indexed ring.
    Clockwise,
}

impl Winding {
    /// The sign the ring's authored angle advances with. Increasing angle is a
    /// clockwise walk (see the module note), so counter-clockwise is `-1`.
    pub fn sign(self) -> f32 {
        [-1.0, 1.0][self as usize]
    }

    /// The sign of `(position − centre) × heading` a dog on this ring must
    /// produce. This is the *observable* form of the winding: it is what a test
    /// measures on a posed bone, and it is the inverse of [`Self::sign`]
    /// because the cross product of a radius with a clockwise tangent points
    /// down.
    pub fn cross_sign(self) -> f32 {
        -self.sign()
    }
}

/// One ring of dogs: where it sits in the field, and how wide it is.
///
/// Everything else about it — which way it is walked, how many dogs it holds,
/// which palette entries they wear — is derived from those two numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ring {
    /// Its place in [`RINGS`], counting outward from the innermost ring.
    pub index: usize,
    /// Its radius about the scene origin, in world units.
    pub radius: f32,
}

impl Ring {
    /// Which way round the dogs walk: alternating with the index, so no ring
    /// ever turns the same way as a neighbour.
    pub const fn winding(self) -> Winding {
        [Winding::CounterClockwise, Winding::Clockwise][self.index % 2]
    }

    /// The ring's circumference — the length of the walk, before the terrain's
    /// relief adds its own fraction of a percent.
    pub fn circumference(self) -> f32 {
        core::f32::consts::TAU * self.radius
    }

    /// How many dogs walk this ring: the circumference divided by the room one
    /// dog needs, rounded to the nearest whole animal.
    ///
    /// Rounding rather than flooring is deliberate — the leftover is shared out
    /// between every gap instead of being dropped into one, so the chain stays
    /// evenly spaced either way. The floor of three is not a real case at any
    /// derived radius; it is there so the arithmetic cannot produce a "ring"
    /// of one dog chasing itself.
    pub fn count(self) -> usize {
        (self.circumference() / DOG_SPACING).round().max(3.0) as usize
    }

    /// How far outside its own circle this ring's dogs reach, in world units:
    /// the nose-and-tail bulge of a rigid body standing on a curve.
    pub fn bulge(self) -> f32 {
        body_bulge(self.radius)
    }

    /// The radial band this ring's dogs occupy: `(inner, outer)` world radii,
    /// from the inside flank of a dog to the tip of its outward-bulging nose.
    pub fn band(self) -> (f32, f32) {
        (
            self.radius - DOG_WIDTH * 0.5,
            self.radius + self.bulge() + DOG_WIDTH * 0.5,
        )
    }

    /// Which palette entry the dog in `slot` wears.
    ///
    /// The ring walks its own comb of [`RING_COMB`] hues, sweeping it as many
    /// whole times as it takes for the step between neighbours to be at least
    /// one comb entry — so however many dogs a ring holds, **no two adjacent
    /// dogs share a colour**, and the seam where the chain closes is no
    /// different from any other gap. The comb is then interleaved into the full
    /// palette by the ring's parity, which is also its winding: a ring and its
    /// neighbours are drawn from disjoint hue sets.
    pub fn palette_at(self, slot: usize) -> usize {
        let count = self.count();
        let sweeps = count.div_ceil(RING_COMB).max(1);
        let step = (slot * RING_COMB * sweeps / count) % RING_COMB;
        step * 2 + self.index % 2
    }
}

/// How far outside a circle of `radius` the ends of a rigid [`DOG_LENGTH`] body
/// standing on it reach.
///
/// The body's centre is on the circle and its nose is `DOG_LENGTH / 2` along the
/// tangent, so the nose sits at `sqrt(radius² + (L/2)²)`. This is the correction
/// that makes a tight ring take more radial room than a wide one, and it is what
/// [`RING_SPACING`] is sized against.
pub fn body_bulge(radius: f32) -> f32 {
    let half = DOG_LENGTH * 0.5;
    (radius * radius + half * half).sqrt() - radius
}

/// The tightest ring in the field, in world units.
///
/// **This is a floor, not a preference.** A dog is a rigid 21-unit body whose
/// paws are planted on the ring itself, and the mismatch between the two grows
/// as the curve tightens: [`body_bulge`] is 2.12 units here, 3.06 at radius 18
/// and 5.51 at radius 10 — and at radius 10.5 the body is longer than the circle
/// it is standing on and the geometry stops existing at all.
///
/// 26 is where the gait is *tuned*: [`DOG_GAIT`]'s shortened stride and deepened
/// crouch were sized for exactly this curve (see `creature_pose.rs`), and
/// `tests/locomotion.rs` measures every limb of every dog on every ring over
/// more than a lap and fails if one is asked to reach further than it is long.
/// A tighter innermost ring is not a constant change: it is a re-tuned gait with
/// that measurement re-run, and until that work is done the field stops here.
pub const RING_MIN_RADIUS: f32 = 26.0;

/// The widest ring in the field, in world units.
///
/// The terrain's top surface is a square of half-extent [`TERRAIN_HALF_EXTENT`]
/// (96) — the skirt hangs straight down from that border, so the usable ground is
/// the inscribed disc of radius 96 and nothing beyond it. A dog on the outermost
/// ring reaches `radius + bulge + half a width` ≈ 84.9 from the origin, which
/// leaves **11.1 units — half a dog's length — of clear ground** between the
/// outermost paw and the rim. That margin is the point: the field has to read as
/// standing *on* a plain, not balanced on its lip.
pub const RING_MAX_RADIUS: f32 = 82.0;

/// The radial pitch between neighbouring rings, in world units. See the module
/// note for the arithmetic: it is the dog's **width** plus the worst-case rigid
/// body bulge, plus air.
pub const RING_SPACING: f32 = 8.0;

/// How many concentric rings the field holds: the innermost, the outermost, and
/// every pitch in between. Asserted against the radii themselves in the tests
/// below, so it cannot drift away from the three constants that produce it.
pub const RING_COUNT: usize = 8;

/// Every ring, innermost first. A dog's identity is `(ring index, slot)`.
pub const RINGS: [Ring; RING_COUNT] = concentric_rings();

/// Lay the rings out from [`RING_MIN_RADIUS`] outward at [`RING_SPACING`].
const fn concentric_rings() -> [Ring; RING_COUNT] {
    let mut rings = [Ring {
        index: 0,
        radius: 0.0,
    }; RING_COUNT];
    let mut index = 0;
    while index < RING_COUNT {
        rings[index] = Ring {
            index,
            radius: RING_MIN_RADIUS + index as f32 * RING_SPACING,
        };
        index += 1;
    }
    rings
}

/// The dog's nose-to-tail length in its own authored units, before the
/// presentation scale. The authored figure is a ~1.05-unit muzzle reach in front
/// of the origin and a ~1.14-unit tail behind it; `tests/creatures.rs` measures
/// the real assembled bounds against this number, so it cannot drift away from
/// the animal it is supposed to describe.
pub const DOG_BODY_LENGTH: f32 = 2.1;

/// The dog's flank-to-flank width in its own authored units — measured, like the
/// length, off the assembled bounds. This is the number [`RING_SPACING`] is
/// built on, because a dog laid along its ring separates two rings by its width
/// and not by its length.
pub const DOG_BODY_WIDTH: f32 = 0.424;

/// The scale the dogs are presented at. Read from the gait rather than typed
/// again: the stride, the crouch and the leg reach are all sized against this
/// number, and a spacing that disagreed with it would space the field by a dog
/// that is not the dog being drawn.
pub const DOG_SCALE: f32 = DOG_GAIT.scale;

/// The dog's world-space length: 21 units.
pub const DOG_LENGTH: f32 = DOG_BODY_LENGTH * DOG_SCALE;

/// The dog's world-space width: 4.24 units.
pub const DOG_WIDTH: f32 = DOG_BODY_WIDTH * DOG_SCALE;

/// The clear air between one dog's tail and the next dog's nose, in world units.
/// Small enough that the chain reads as one packed queue, wide enough that a
/// stride's worth of gait never closes it — a paw's fore-aft excursion is
/// ±2.13 units about its own neutral, and every paw's neutral sits well inside
/// the nose-to-tail envelope, so the gap is never eaten by a swinging leg.
pub const DOG_GAP: f32 = 1.5;

/// The arc one dog occupies on its ring: its own length plus the gap behind it.
pub const DOG_SPACING: f32 = DOG_LENGTH + DOG_GAP;

/// How many hue materials the whole field shares.
///
/// This is the app's answer to the batching constraint in the module note: the
/// live backend draws one batch per `(mesh, material)` pair, so the palette size
/// — not the crowd size — sets the draw-call count, at no more than
/// `23 bones × PALETTE_SIZE + 1 terrain = 415` (392 as laid out). Eighteen is
/// chosen so each of the
/// two interleaved combs is nine hues (40° apart along a ring) and the combs are
/// 20° apart from each other, which is comfortably past the point where two
/// coats read as the same colour.
pub const PALETTE_SIZE: usize = 18;

/// How many hues one ring's comb holds: half the palette, because neighbouring
/// rings take alternate entries.
pub const RING_COMB: usize = PALETTE_SIZE / 2;

/// The linear-RGB coat colour of palette entry `index`.
pub fn palette_color(index: usize) -> [f32; 3] {
    hue_to_rgb(index as f32 / PALETTE_SIZE as f32)
}

/// Every palette entry, in order — the material set `install.rs` registers once
/// and every dog in the field draws from.
pub fn palette() -> Vec<[f32; 3]> {
    (0..PALETTE_SIZE).map(palette_color).collect()
}

/// One dog in the crowd: which ring it walks, where in the chain it is, and
/// which palette entry it is painted from.
///
/// It deliberately carries **no geometry and no colour of its own**. Every dog in
/// the field is the same 23 registered bone meshes drawn again at another
/// transform, in one of [`PALETTE_SIZE`] shared coats — this struct is the whole
/// of what makes one dog different from the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingDog {
    /// Which ring this dog walks: an index into [`RINGS`].
    pub ring: usize,
    /// Its place in the chain, `0..ring.count()`.
    pub slot: usize,
    /// Its coat: an index into the shared palette, `0..PALETTE_SIZE`.
    pub palette: usize,
}

impl RingDog {
    /// Its linear-RGB coat colour — a lookup, not a property of the dog.
    pub fn color(self) -> [f32; 3] {
        palette_color(self.palette)
    }
}

/// Every dog in the field, in spawn order: the innermost ring's chain first,
/// then each ring outward.
///
/// A pure function of the authored constants above — no clock, no randomness, no
/// environment — so the crowd is byte-identical in every process.
pub fn ring_dogs() -> Vec<RingDog> {
    RINGS
        .iter()
        .flat_map(|ring| {
            (0..ring.count()).map(move |slot| RingDog {
                ring: ring.index,
                slot,
                palette: ring.palette_at(slot),
            })
        })
        .collect()
}

/// How many dogs the whole field holds.
pub fn dog_total() -> usize {
    RINGS.iter().map(|ring| ring.count()).sum()
}

/// The clear ground between the outermost dog's furthest reach and the terrain's
/// rim, in world units. Positive by construction — [`RING_MAX_RADIUS`] is chosen
/// against this number, and the test below holds it to half a dog's length.
pub fn outer_clearance() -> f32 {
    TERRAIN_HALF_EXTENT - RINGS[RING_COUNT - 1].band().1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_is_laid_out_from_three_measured_numbers() {
        // The dog, stated: 21 units long, 4.24 wide, needing 22.5 units of ring.
        assert_eq!(DOG_LENGTH, 21.0);
        assert!((DOG_WIDTH - 4.24).abs() < 1.0e-4, "{DOG_WIDTH}");
        assert_eq!(DOG_SPACING, 22.5);

        // The ring count is the pitch stepped from the floor to the ceiling.
        let derived = ((RING_MAX_RADIUS - RING_MIN_RADIUS) / RING_SPACING) as usize + 1;
        assert_eq!(RING_COUNT, derived);
        assert_eq!(RINGS[0].radius, RING_MIN_RADIUS);
        assert_eq!(RINGS[RING_COUNT - 1].radius, RING_MAX_RADIUS);
        RINGS.iter().enumerate().for_each(|(index, ring)| {
            assert_eq!(ring.index, index);
            assert_eq!(ring.radius, RING_MIN_RADIUS + index as f32 * RING_SPACING);
        });
    }

    #[test]
    fn no_two_rings_intersect_and_the_outermost_stays_on_the_ground() {
        RINGS.windows(2).for_each(|pair| {
            let (_, outer_edge) = pair[0].band();
            let (inner_edge, _) = pair[1].band();
            let air = inner_edge - outer_edge;
            assert!(
                air > 1.0,
                "rings {} and {} clear each other by only {air}",
                pair[0].index,
                pair[1].index
            );
        });
        // ...and the widest ring's dogs stay well inside the terrain's rim.
        let clear = outer_clearance();
        assert!(
            clear > DOG_LENGTH * 0.5,
            "the outer ring leaves only {clear} units of ground before the rim"
        );
    }

    #[test]
    fn each_ring_is_populated_from_its_own_circumference() {
        assert_eq!(
            RINGS.map(|ring| ring.count()),
            [7, 9, 12, 14, 16, 18, 21, 23]
        );
        assert_eq!(dog_total(), 120);
        RINGS.iter().for_each(|ring| {
            let spacing = ring.circumference() / ring.count() as f32;
            assert!(
                (spacing - DOG_SPACING).abs() < 0.1 * DOG_SPACING,
                "ring {} spaces its dogs {spacing} apart",
                ring.index
            );
            assert!(spacing > DOG_LENGTH, "ring {} dogs overlap", ring.index);
        });
    }

    #[test]
    fn every_ring_turns_against_both_its_neighbours() {
        RINGS.windows(2).for_each(|pair| {
            assert_ne!(pair[0].winding(), pair[1].winding());
            assert_eq!(pair[0].winding().sign(), -pair[1].winding().sign());
            assert_eq!(pair[0].winding().cross_sign(), -pair[0].winding().sign());
        });
        assert_eq!(RINGS[0].winding(), Winding::CounterClockwise);
    }

    #[test]
    fn the_palette_is_bounded_and_no_neighbour_shares_a_coat() {
        let dogs = ring_dogs();
        assert_eq!(dogs.len(), dog_total());
        assert!(dogs.iter().all(|dog| dog.palette < PALETTE_SIZE));
        // Adjacent rings draw from disjoint combs — a dog and *any* dog on a
        // neighbouring ring differ, whatever the two chains' alignment.
        RINGS.iter().for_each(|ring| {
            let chain: Vec<usize> = (0..ring.count()).map(|s| ring.palette_at(s)).collect();
            assert!(chain.iter().all(|entry| entry % 2 == ring.index % 2));
            (0..chain.len()).for_each(|slot| {
                assert_ne!(
                    chain[slot],
                    chain[(slot + 1) % chain.len()],
                    "ring {} repeats a coat at slot {slot}",
                    ring.index
                );
            });
        });
        assert_eq!(dogs, ring_dogs());
    }
}
