//! The soccer environment: one documented coordinate system, the generator that
//! builds every static piece once, and the goal — the frame the ball can hit and
//! the net it bulges.

pub mod coordinates;
pub mod generator;
pub mod goal;

pub use coordinates::{
    ball_spot, GoalMouth, BEHIND_GOAL, GOAL_HALF_WIDTH, GOAL_HEIGHT, KEEPER_LINE_Z, NET_DEPTH,
    PENALTY_SPOT_Z, PITCH_DEPTH, PITCH_HALF_WIDTH, POST_RADIUS,
};
pub use generator::{generate_pitch, PitchMaterial, PitchMesh, PitchPiece};
pub use goal::{
    frame_hit, inside_mouth, net_strands, FrameHit, FrameMember, NetImpulse, NetStrand,
};
