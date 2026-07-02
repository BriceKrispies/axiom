//! Pass 3 — faked blob shadows.
//!
//! A blob shadow is a flat, dark, translucent ellipse laid on the ground under
//! an actor. It is pure decoration: no real-time lighting, no shadow maps, no
//! projection math. Each shadow is a fixed [`PenaltyBlobShadow`] descriptor; the
//! scene emits it as a ground quad in the Pass 2 `ActorShadow` layer with a
//! stable ordinal, so it always draws after the field/lines and before the
//! actors.

use axiom_math::Vec3;

use crate::soccer_penalty::penalty_scene::{BALL_RADIUS, GOALIE_X, GOALIE_Z, GROUND_Y, KICKER_X, KICKER_Z, PENALTY_SPOT_Z};

/// A single fake blob shadow: a flat ground ellipse (`radius_x` across the
/// field, `radius_z` along depth). Lifted a hair off the ground to avoid
/// z-fighting with the pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyBlobShadow {
    pub label: &'static str,
    pub center: Vec3,
    pub radius_x: f32,
    pub radius_z: f32,
}

/// Height of the shadow quads above the pitch.
pub const SHADOW_Y: f32 = GROUND_Y + 0.03;

/// The three fixed blob shadows, in stable order: kicker (elongated on the
/// field), ball (small, directly under the static ball), goalie (near the goal
/// line under the goalie).
pub const BLOB_SHADOWS: [PenaltyBlobShadow; 3] = [
    PenaltyBlobShadow {
        label: "shadow.kicker",
        center: Vec3::new(KICKER_X, SHADOW_Y, KICKER_Z),
        radius_x: 0.42,
        radius_z: 0.86, // elongated along the field
    },
    PenaltyBlobShadow {
        label: "shadow.ball",
        center: Vec3::new(0.0, SHADOW_Y, PENALTY_SPOT_Z),
        radius_x: BALL_RADIUS * 1.1,
        radius_z: BALL_RADIUS * 1.0, // small, directly under the ball
    },
    PenaltyBlobShadow {
        label: "shadow.goalie",
        center: Vec3::new(GOALIE_X, SHADOW_Y, GOALIE_Z),
        radius_x: 0.62,
        radius_z: 0.44, // near the goal line under the goalie
    },
];
