//! Target markers at a receiver's feet — the player's read of who the ball can
//! go to, drawn from authoritative simulation state so what is on screen can
//! never disagree with where the ball would actually land.
//!
//! Two modes, because the app runs two loops:
//!
//! - **Decision window** (the prototype): each of the three reads gets its own
//!   coloured ring plus a stack of floating cubes whose COUNT is the read's
//!   number — one cube for `1`, three for `3`. No font, no HUD arrow, and
//!   nothing that says whether the read is a good one; the player still has to
//!   look at the coverage.
//! - **Ambient showcase**: the original throwing-cone read — red on the
//!   receiver the cone would commit to, white on everyone else reachable.
//!
//! Procedural like everything else visible here: a ring is [`RING_SEGMENTS`]
//! small cubes stepped around a circle.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

use crate::data::prototype::READ_COUNT;

use super::snapshot::PresentationSnapshot;

/// Cubes per ring.
pub const RING_SEGMENTS: usize = 20;

/// How many receivers the ambient cone may ring at once.
pub const MAX_RINGS: usize = 4;

/// Pooled cubes for the single red cone-target ring.
pub const TARGET_RING_POOL: usize = RING_SEGMENTS;

/// Pooled cubes for the white rings on the remaining cone-eligible receivers.
pub const ELIGIBLE_RING_POOL: usize = RING_SEGMENTS * (MAX_RINGS - 1);

/// Pooled cubes for one read: its ring plus up to three numeral cubes.
pub const READ_RING_POOL: usize = RING_SEGMENTS + READ_COUNT;

/// Total pooled cubes the scene must allocate for all target markers.
pub const RECEIVER_RING_POOL: usize =
    TARGET_RING_POOL + ELIGIBLE_RING_POOL + READ_RING_POOL * READ_COUNT;

/// Which marker a receiver gets. The three read kinds are separate variants
/// (rather than one kind plus an index) because each is its own pooled material
/// — the scene assigns colour by tag, exactly like the juice pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingKind {
    /// The throwing cone would commit here — drawn red (ambient showcase).
    Target,
    /// Reachable, but not the cone's read — drawn white (ambient showcase).
    Eligible,
    /// Decision-window read one (the short route).
    ReadOne,
    /// Decision-window read two (the intermediate route).
    ReadTwo,
    /// Decision-window read three (the deep route).
    ReadThree,
}

impl RingKind {
    /// The marker kind for read index `0..3`.
    pub fn for_read(read: usize) -> Self {
        match read {
            0 => RingKind::ReadOne,
            1 => RingKind::ReadTwo,
            _ => RingKind::ReadThree,
        }
    }
}

/// One marker cube: where it sits and which marker it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingSegment {
    pub transform: Transform,
    pub kind: RingKind,
}

/// Ring radius at the receiver's feet, yd.
const RING_RADIUS: f32 = 0.95;
/// Height of the ring above the turf, yd.
const RING_HEIGHT: f32 = 0.10;
/// Size of one ring segment cube, yd.
const SEGMENT_SIZE: f32 = 0.26;
/// Where the numeral stack starts above the receiver, yd.
const NUMERAL_BASE_Y: f32 = 2.55;
/// Vertical spacing between numeral cubes, yd.
const NUMERAL_STEP: f32 = 0.42;
/// Size of one numeral cube, yd.
const NUMERAL_SIZE: f32 = 0.30;

fn cube(pos: Vec3, size: f32, kind: RingKind) -> RingSegment {
    RingSegment {
        transform: Transform::new(pos, Quat::IDENTITY, Vec3::new(size, size, size)),
        kind,
    }
}

/// Push a ring at `feet`.
fn push_ring(feet: Vec3, kind: RingKind, out: &mut Vec<RingSegment>) {
    for segment in 0..RING_SEGMENTS {
        let angle = segment as f32 / RING_SEGMENTS as f32 * core::f32::consts::TAU;
        out.push(cube(
            Vec3::new(
                feet.x + angle.cos() * RING_RADIUS,
                RING_HEIGHT,
                feet.z + angle.sin() * RING_RADIUS,
            ),
            SEGMENT_SIZE,
            kind,
        ));
    }
}

/// Push the floating numeral: `read + 1` cubes stacked over the receiver.
fn push_numeral(feet: Vec3, read: usize, kind: RingKind, out: &mut Vec<RingSegment>) {
    for step in 0..=read.min(READ_COUNT - 1) {
        out.push(cube(
            Vec3::new(feet.x, NUMERAL_BASE_Y + step as f32 * NUMERAL_STEP, feet.z),
            NUMERAL_SIZE,
            kind,
        ));
    }
}

/// Build this tick's target markers.
pub fn ring_instances(snapshot: &PresentationSnapshot, out: &mut Vec<RingSegment>) {
    out.clear();
    // In a real session the numbered reads own the markers for the whole live
    // play — from the line, so the player learns which receiver each key
    // belongs to, right through to the throw. Once the ball is gone the field
    // is left clean so the play reads as a play rather than a UI.
    if let Some(step) = snapshot.attempt {
        if !step.phase.shows_reads() {
            return;
        }
        for read in 0..READ_COUNT {
            let state = step.read.read(read);
            if !state.live {
                continue;
            }
            let kind = RingKind::for_read(read);
            let feet = snapshot.player(state.id).pos;
            push_ring(feet, kind, out);
            push_numeral(feet, read, kind, out);
        }
        return;
    }
    // The ambient showcase keeps the original throwing-cone read: the snapshot
    // lists the cone nearest-the-centre-line first, so its head IS the read.
    for (index, id) in snapshot.throwable.iter().take(MAX_RINGS).enumerate() {
        let kind = match index {
            0 => RingKind::Target,
            _ => RingKind::Eligible,
        };
        push_ring(snapshot.player(*id).pos, kind, out);
    }
}
