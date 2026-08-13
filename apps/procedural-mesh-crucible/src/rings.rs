//! The two counter-rotating rings: how many dogs walk each one, where each dog
//! starts, which way round it faces, and what colour it is.
//!
//! ## The layout is derived, not typed
//!
//! Nothing here is a hand-tuned dog count. A ring knows its radius; a dog knows
//! how long it is; the count is the circumference divided by the space one dog
//! needs, rounded to a whole animal. Change [`OUTER`]'s radius and the ring
//! re-populates itself with the right number of dogs at the right spacing,
//! because the count was never authored in the first place.
//!
//! ## Which way is which
//!
//! Seen from `+Y` looking down at the `XZ` plane — the way the framing camera
//! sees it — a point at `(R·cos θ, R·sin θ)` traversed with **increasing** `θ`
//! goes **clockwise**. (Take screen-right as `+X` and screen-up as `-Z`, the
//! ordinary map orientation: the point's screen angle is `-θ`, so advancing `θ`
//! turns the short way round the clock.) That single fact is what
//! [`Winding::sign`] encodes, and it is the whole difference between the two
//! rings.
//!
//! The direction is then testable without trusting any of this prose: for a
//! position `p` measured from the ring centre and a heading `h`, the `y`
//! component of `p × h` is `p.z·h.x − p.x·h.z`, which is `−R²` for a clockwise
//! walk and `+R²` for a counter-clockwise one. `tests/rings.rs` asserts exactly
//! that sign on the real posed bones.

use crate::creature_pose::DOG_GAIT;
use crate::rainbow::hue_to_rgb;

/// Which way round a ring is walked, seen from `+Y` looking down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Winding {
    /// Anticlockwise from above — the outer ring.
    CounterClockwise,
    /// Clockwise from above — the inner ring.
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

/// One ring of dogs: how wide it is, which way it is walked, and where its
/// rainbow starts.
#[derive(Debug, Clone, Copy)]
pub struct Ring {
    /// A stable name, used by the tests and the page legend.
    pub name: &'static str,
    /// The ring's radius about the scene origin, in world units.
    pub radius: f32,
    /// Which way round the dogs walk.
    pub winding: Winding,
    /// Where this ring's sweep of the hue circle begins, in turns. The two rings
    /// are offset so the pair does not read as one palette drawn twice.
    pub hue_phase: f32,
}

impl Ring {
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
    /// authored radius; it is there so the arithmetic cannot produce a "ring"
    /// of one dog chasing itself.
    pub fn count(self) -> usize {
        (self.circumference() / DOG_SPACING).round().max(3.0) as usize
    }
}

/// The outer ring: the wide one, walked anticlockwise.
pub const OUTER: Ring = Ring {
    name: "outer",
    radius: 46.0,
    winding: Winding::CounterClockwise,
    hue_phase: 0.0,
};

/// The inner ring: the tight one, walked the other way, with its rainbow half a
/// turn out of phase with the outer ring's.
pub const INNER: Ring = Ring {
    name: "inner",
    radius: 26.0,
    winding: Winding::Clockwise,
    hue_phase: 0.5,
};

/// Both rings, in spawn order. Every dog's identity is `(ring index, slot)`.
pub const RINGS: [Ring; 2] = [OUTER, INNER];

/// The dog's nose-to-tail length in its own authored units, before the
/// presentation scale. The authored figure is a ~1.05-unit muzzle reach in front
/// of the origin and a ~1.14-unit tail behind it; `tests/rings.rs` measures the
/// real assembled bounds against this number, so it cannot drift away from the
/// animal it is supposed to describe.
pub const DOG_BODY_LENGTH: f32 = 2.1;

/// The scale the dogs are presented at. Read from the gait rather than typed
/// again: the stride, the crouch and the leg reach are all sized against this
/// number, and a spacing that disagreed with it would space the ring by a dog
/// that is not the dog being drawn.
pub const DOG_SCALE: f32 = DOG_GAIT.scale;

/// The dog's world-space length: 21 units.
pub const DOG_LENGTH: f32 = DOG_BODY_LENGTH * DOG_SCALE;

/// The clear air between one dog's tail and the next dog's nose, in world units.
/// Small enough that the ring reads as one chain, wide enough that a stride's
/// worth of gait never closes it.
pub const DOG_GAP: f32 = 3.0;

/// The arc one dog occupies on its ring: its own length plus the gap behind it.
pub const DOG_SPACING: f32 = DOG_LENGTH + DOG_GAP;

/// One dog in the crowd: which ring it walks, where in the chain it is, and what
/// colour it is painted.
///
/// It deliberately carries **no geometry**. Every dog in both rings is the same
/// 23 registered bone meshes drawn again at another transform — this struct is
/// the whole of what makes one dog different from the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingDog {
    /// Which ring this dog walks: an index into [`RINGS`].
    pub ring: usize,
    /// Its place in the chain, `0..ring.count()`.
    pub slot: usize,
    /// Its linear-RGB coat colour.
    pub color: [f32; 3],
}

/// Every dog in both rings, in spawn order: the outer ring's chain, then the
/// inner ring's.
///
/// A pure function of the authored constants above — no clock, no randomness, no
/// environment — so the crowd is byte-identical in every process.
pub fn ring_dogs() -> Vec<RingDog> {
    RINGS
        .iter()
        .enumerate()
        .flat_map(|(ring, spec)| {
            let count = spec.count();
            (0..count).map(move |slot| RingDog {
                ring,
                slot,
                // The full hue circle, once, around each ring — so the chain
                // reads as a rainbow rather than as a gradient with a seam.
                color: hue_to_rgb(spec.hue_phase + slot as f32 / count as f32),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ring_is_populated_from_its_own_circumference() {
        // The arithmetic, stated: a 21-unit dog plus a 3-unit gap is 24 units of
        // ring, and the ring is as long as it is round.
        assert_eq!(DOG_LENGTH, 21.0);
        assert_eq!(DOG_SPACING, 24.0);
        assert_eq!(OUTER.count(), 12, "outer: {} long", OUTER.circumference());
        assert_eq!(INNER.count(), 7, "inner: {} long", INNER.circumference());
        // And the realised spacing is within a tenth of the target either way,
        // which is what "evenly spaced, nose to tail" means in numbers.
        for ring in RINGS {
            let spacing = ring.circumference() / ring.count() as f32;
            assert!(
                (spacing - DOG_SPACING).abs() < 0.1 * DOG_SPACING,
                "{} spaces its dogs {spacing} apart",
                ring.name
            );
            assert!(spacing > DOG_LENGTH, "{} dogs overlap", ring.name);
        }
    }

    #[test]
    fn the_two_rings_turn_opposite_ways() {
        assert_eq!(OUTER.winding.sign(), -1.0);
        assert_eq!(INNER.winding.sign(), 1.0);
        assert_eq!(OUTER.winding.cross_sign(), 1.0);
        assert_eq!(INNER.winding.cross_sign(), -1.0);
        assert_ne!(OUTER.winding, INNER.winding);
    }

    #[test]
    fn the_crowd_is_both_rings_chains_end_to_end() {
        let dogs = ring_dogs();
        assert_eq!(dogs.len(), OUTER.count() + INNER.count());
        assert_eq!(dogs.len(), 19);
        assert!(dogs[..OUTER.count()].iter().all(|dog| dog.ring == 0));
        assert!(dogs[OUTER.count()..].iter().all(|dog| dog.ring == 1));
        // Slots run 0.. within each ring, and the crowd is reproducible.
        assert_eq!(dogs[OUTER.count()].slot, 0);
        assert_eq!(dogs, ring_dogs());
    }
}
