//! The marker at a player's feet — drawn from authoritative simulation state,
//! so what is on screen can never disagree with who the game thinks you are.
//!
//! Two modes, because the app runs two loops:
//!
//! - **The run game.** One ring, under the running back. Before the snap it is
//!   *amber*: that is the man you are about to be, and picking him out of eleven
//!   bodies at the line is otherwise genuinely hard. From the exchange onward it
//!   is *white*: that is the man you **are**. The colour change is the clearest
//!   signal in the game that control has arrived, and it costs no screen space.
//! - **Ambient showcase.** The original throwing-cone read — red on the receiver
//!   the cone would commit to, white on everyone else reachable.
//!
//! Nothing here says whether anything is a *good* idea; the ring identifies WHO,
//! never what to do about it. Procedural like everything else visible in this
//! app: a ring is [`RING_SEGMENTS`] small cubes stepped around a circle.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

use super::snapshot::PresentationSnapshot;

/// Cubes per ring.
pub const RING_SEGMENTS: usize = 20;

/// How many receivers the ambient cone may ring at once.
pub const MAX_RINGS: usize = 4;

/// Pooled cubes for the single red cone-target ring.
pub const TARGET_RING_POOL: usize = RING_SEGMENTS;

/// Pooled cubes for the white rings on the remaining cone-eligible receivers,
/// and for the run game's live carrier ring.
pub const ELIGIBLE_RING_POOL: usize = RING_SEGMENTS * (MAX_RINGS - 1);

/// Pooled cubes for the pre-snap amber ring on the back.
pub const BACK_RING_POOL: usize = RING_SEGMENTS;

/// Cubes in the **charge tell** — the marker under a defender the back could run
/// through right now. Fewer and smaller than a foot ring on purpose: it has to
/// be noticed without being read, so it reads as a texture change at the man's
/// feet rather than as a second thing to look at.
pub const CHARGE_SEGMENTS: usize = 10;
pub const CHARGE_RING_POOL: usize = CHARGE_SEGMENTS;

/// Total pooled cubes the scene must allocate for all foot markers.
pub const CARRIER_RING_POOL: usize =
    TARGET_RING_POOL + ELIGIBLE_RING_POOL + BACK_RING_POOL + CHARGE_RING_POOL;

/// Which marker a player gets. Each is its own pooled material — the scene
/// assigns colour by tag, exactly like the juice pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingKind {
    /// The throwing cone would commit here — drawn red (ambient showcase).
    Target,
    /// The player-controlled runner, live — drawn white.
    Carrier,
    /// The back you are about to become — drawn amber (pre-exchange).
    Back,
    /// **The charge tell**: a defender the shoulder would go through. Drawn hot,
    /// small, and tight to his feet.
    ChargeTarget,
}

/// One marker cube: where it sits and which marker it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingSegment {
    pub transform: Transform,
    pub kind: RingKind,
}

/// Ring radius at the player's feet, yd.
const RING_RADIUS: f32 = 0.95;
/// The charge tell sits tighter and lower than a foot ring.
const CHARGE_RADIUS: f32 = 0.62;
const CHARGE_SEGMENT_SIZE: f32 = 0.2;
/// Height of the ring above the turf, yd.
const RING_HEIGHT: f32 = 0.10;
/// Size of one ring segment cube, yd.
const SEGMENT_SIZE: f32 = 0.26;

fn cube(pos: Vec3, size: f32, kind: RingKind) -> RingSegment {
    RingSegment {
        transform: Transform::new(pos, Quat::IDENTITY, Vec3::new(size, size, size)),
        kind,
    }
}

/// Push a ring at `feet`. The ring stays on the **turf** even when the runner
/// is airborne — a marker that left the ground with him would stop telling the
/// player where on the field he actually is, which during a leap is exactly
/// when that is hardest to judge and most worth knowing.
fn push_ring(feet: Vec3, kind: RingKind, out: &mut Vec<RingSegment>) {
    (0..RING_SEGMENTS).for_each(|segment| {
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
    });
}

/// The charge tell: a tight hot arc at the feet of a defender the shoulder would
/// go through.
///
/// **It is a promise, not a hint.** The simulation resolves a charge on the
/// geometry as it was when the player pressed, and this is drawn from that same
/// resolution (`runback::read::charge_window`), so the tell being lit and the
/// charge being won are one fact rather than two that have to agree.
///
/// That is also what earns it the right to exist. End Zone otherwise refuses to
/// tell the player whether a move will work, because reading the field IS the
/// game — but the charge's inputs, closing speed and the defender's brace, are
/// the one set a low chase camera genuinely cannot show you. Two bodies about to
/// collide look identical from behind whether one of them is squared up and
/// braced or turned and coasting. The tell gives back information the camera
/// takes away; it never answers a question the player could have answered by
/// looking.
///
/// Its arc length scales with how decisively the charge would be won, so a
/// comfortable window reads as a fuller mark than a marginal one — without ever
/// putting a number on the screen.
fn push_charge_tell(feet: Vec3, overload: f32, out: &mut Vec<RingSegment>) {
    let filled = ((overload - 1.0) * 6.0).clamp(3.0, CHARGE_SEGMENTS as f32) as usize;
    (0..filled).for_each(|segment| {
        let angle = segment as f32 / CHARGE_SEGMENTS as f32 * core::f32::consts::TAU;
        out.push(cube(
            Vec3::new(
                feet.x + angle.cos() * CHARGE_RADIUS,
                RING_HEIGHT,
                feet.z + angle.sin() * CHARGE_RADIUS,
            ),
            CHARGE_SEGMENT_SIZE,
            RingKind::ChargeTarget,
        ));
    });
}

/// Build this tick's foot markers.
pub fn ring_instances(snapshot: &PresentationSnapshot, out: &mut Vec<RingSegment>) {
    out.clear();
    if let Some(step) = snapshot.attempt {
        let Some(back) = step.runback.back else {
            return;
        };
        let kind = match snapshot.possession == Some(back) {
            true => RingKind::Carrier,
            false => RingKind::Back,
        };
        push_ring(snapshot.player(back).pos, kind, out);
        if let Some(window) = step.runback.charge_window {
            push_charge_tell(snapshot.player(window.defender).pos, window.overload, out);
        }
        return;
    }
    // The ambient showcase keeps the original throwing-cone read: the snapshot
    // lists the cone nearest-the-centre-line first, so its head IS the read.
    snapshot
        .throwable
        .iter()
        .take(MAX_RINGS)
        .enumerate()
        .for_each(|(index, id)| {
            let kind = match index {
                0 => RingKind::Target,
                _ => RingKind::Carrier,
            };
            push_ring(snapshot.player(*id).pos, kind, out);
        });
}
