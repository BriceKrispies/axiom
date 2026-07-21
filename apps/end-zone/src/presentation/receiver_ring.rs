//! The eligible-receiver ring: the white circle drawn at the feet of every
//! receiver the quarterback can currently throw to.
//!
//! This is the player's read of [`crate::football::targeting`]'s cone. The
//! simulation owns the eligibility rule and publishes the resulting list on the
//! snapshot; this module only turns that list into ring geometry, so what the
//! player sees can never disagree with where the ball would actually go.
//!
//! The receiver the pass would actually go to is ringed RED; everyone else the
//! quarterback could legally reach is ringed white. That distinction matters
//! because targeting picks whoever is nearest the centre line — without it, a
//! cone holding three white rings tells the player who is throwable but not
//! where the ball would land.
//!
//! Procedural like everything else visible in this app: a ring is
//! [`RING_SEGMENTS`] small cubes stepped around a circle, the same way the
//! impact-ring juice effect is built.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

use super::snapshot::PresentationSnapshot;

/// Cubes per ring.
pub const RING_SEGMENTS: usize = 20;

/// How many receivers may be ringed at once. The offense fields three route
/// runners, so this has headroom; extra eligible receivers are simply not
/// ringed rather than overflowing the pool.
pub const MAX_RINGS: usize = 4;

/// Total pooled ring cubes the scene must allocate.
pub const RECEIVER_RING_POOL: usize = RING_SEGMENTS * MAX_RINGS;

/// Pooled cubes for the single red target ring.
pub const TARGET_RING_POOL: usize = RING_SEGMENTS;

/// Pooled cubes for the white rings on the remaining eligible receivers.
pub const ELIGIBLE_RING_POOL: usize = RING_SEGMENTS * (MAX_RINGS - 1);

/// Which ring a receiver gets: the one the pass commits to, or merely a
/// receiver the quarterback could reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingKind {
    /// The pass would go here — drawn red.
    Target,
    /// Throwable, but not the current read — drawn white.
    Eligible,
}

/// One ring segment: where it sits and which ring it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingSegment {
    pub transform: Transform,
    pub kind: RingKind,
}

/// Ring radius at the receiver's feet, yd.
const RING_RADIUS: f32 = 0.95;

/// Height of the ring above the turf, yd — just clear of the surface so it
/// reads as painted on the grass rather than floating.
const RING_HEIGHT: f32 = 0.10;

/// Size of one ring segment cube, yd.
const SEGMENT_SIZE: f32 = 0.26;

/// Build the ring geometry for every currently-throwable receiver, in the
/// snapshot's order (nearest the quarterback's centre line first).
pub fn ring_instances(snapshot: &PresentationSnapshot, out: &mut Vec<RingSegment>) {
    out.clear();
    for (index, id) in snapshot.throwable.iter().take(MAX_RINGS).enumerate() {
        // The snapshot lists the cone nearest-the-centre-line first, so the head
        // of the list IS the receiver the pass would commit to.
        let kind = match index {
            0 => RingKind::Target,
            _ => RingKind::Eligible,
        };
        let feet = snapshot.player(*id).pos;
        for segment in 0..RING_SEGMENTS {
            let angle = segment as f32 / RING_SEGMENTS as f32 * core::f32::consts::TAU;
            out.push(RingSegment {
                transform: Transform::new(
                    Vec3::new(
                        feet.x + angle.cos() * RING_RADIUS,
                        RING_HEIGHT,
                        feet.z + angle.sin() * RING_RADIUS,
                    ),
                    Quat::IDENTITY,
                    Vec3::new(SEGMENT_SIZE, SEGMENT_SIZE, SEGMENT_SIZE),
                ),
                kind,
            });
        }
    }
}
