//! Explicit football states and the ball's simulation record. Transitions are
//! owned by [`crate::state::SimState`]; nothing outside the simulation mutates
//! a `BallSim`.

use axiom::prelude::Vec3;

use crate::identity::PlayerId;

use super::flight::FlightInfo;

/// The football's explicit state machine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BallState {
    /// Pre-play: on the ground at the line of scrimmage, not live.
    Dead,
    /// In a player's hands, following the carry socket.
    Held { carrier: PlayerId },
    /// Traveling from the snapper to the quarterback (deterministic lerp).
    Snap {
        from: PlayerId,
        to: PlayerId,
        start: Vec3,
        elapsed: u32,
        total: u32,
    },
    /// **The exchange.** Travelling from the quarterback's hands into the
    /// back's, over a real, visible number of ticks.
    ///
    /// Its own state rather than a flag on `Held`, and deliberately the same
    /// shape as [`BallState::Snap`], because it is the same football fact: the
    /// ball is between two people and belongs to neither. Modelling it that way
    /// is what stops the handoff being a teleport — the ball is drawn where the
    /// lerp puts it, so the player watches it leave one pair of hands and arrive
    /// in the other, and possession only changes when it lands.
    Handoff {
        from: PlayerId,
        to: PlayerId,
        start: Vec3,
        elapsed: u32,
        total: u32,
    },
    /// A live forward pass in ballistic flight.
    Airborne { flight: FlightInfo },
    /// Live on the turf with no possessor (bouncing/rolling).
    Loose,
    /// Settled on the turf; the play is over.
    Grounded,
}

/// The ball's authoritative simulation record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallSim {
    pub state: BallState,
    /// World position (center), yards.
    pub pos: Vec3,
    /// World velocity, yd/s (meaningful while airborne/loose).
    pub vel: Vec3,
    /// Accumulated spiral angle, radians.
    pub spin_angle: f32,
    /// Spiral rate, rad/s (deterministic, set at release).
    pub spin_rate: f32,
    /// The long-axis direction while in flight.
    pub flight_axis: Vec3,
}

/// Physics collider radius (yards) — the ball flies and bounces as a sphere
/// (the engine has no prolate collider); the silhouette is visual scale.
pub const BALL_RADIUS: f32 = 0.21;

/// Visual full extents of the prolate silhouette (x, long-axis, z), yards.
pub const BALL_VISUAL_SCALE: Vec3 = Vec3::new(0.42, 0.66, 0.42);

impl BallSim {
    /// A dead ball resting at `pos`.
    pub fn dead_at(pos: Vec3) -> Self {
        BallSim {
            state: BallState::Dead,
            pos,
            vel: Vec3::ZERO,
            spin_angle: 0.0,
            spin_rate: 0.0,
            flight_axis: Vec3::UNIT_Z,
        }
    }

    /// The current carrier, if any.
    pub fn carrier(&self) -> Option<PlayerId> {
        match self.state {
            BallState::Held { carrier } => Some(carrier),
            _ => None,
        }
    }

    /// Whether the ball is in the air on a pass.
    pub fn is_airborne(&self) -> bool {
        matches!(self.state, BallState::Airborne { .. })
    }

    /// The back an exchange in progress is travelling to.
    ///
    /// Distinct from [`Self::carrier`] on purpose: mid-exchange nobody *has* the
    /// ball (exactly as during the snap), but the defense must already be
    /// rallying to the man about to. Conflating the two would either let a
    /// defender tackle a player who is not yet carrying, or leave the whole
    /// defense standing still through the most important beat of the play.
    pub fn handoff_target(&self) -> Option<PlayerId> {
        match self.state {
            BallState::Handoff { to, .. } => Some(to),
            _ => None,
        }
    }

    /// Whether the ball is mid-exchange between two players.
    pub fn is_exchanging(&self) -> bool {
        matches!(self.state, BallState::Handoff { .. })
    }
}
