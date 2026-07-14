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
}
