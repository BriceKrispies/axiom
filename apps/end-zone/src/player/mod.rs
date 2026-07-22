//! The player subsystem. Simulation side: [`PlayerSim`] (the authoritative
//! kinematic record) and [`controller`] (the ONLY writer of player movement).
//! Presentation side: [`model`] (procedural box-figure construction),
//! [`rig`] (per-tick pose resolution), and [`animation`] (state-driven
//! procedural poses).

pub mod animation;
pub mod contact;
pub mod contact_stage;
pub mod controller;
pub mod lineup;
pub mod model;
pub mod rig;

use axiom::prelude::Vec3;

use crate::data::PlayerArchetype;
use crate::identity::{PlayerId, TeamId};

/// Procedural animation states, derived from fixed simulation ticks and
/// explicit state — never a wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimState {
    ReadyStance,
    Idle,
    Jog,
    Sprint,
    DropBack,
    Throw,
    Catch,
    Block,
    Tackle,
    /// A committed diving tackle: the defender has left their feet in a
    /// forward lunge (ballistic arc). It can land a tackle from extended reach;
    /// a miss lands the diver prone.
    Dive,
    HitReaction,
    Stumble,
    AirborneFall,
    GroundImpact,
    Recovery,
}

impl AnimState {
    /// Whether the player can act (run routes, catch, tackle) in this state.
    /// A committed diver is ballistic — the controller does not steer them and
    /// they are not overlap-resolved; their tackle is landed by the contact
    /// framework's dedicated dive path, not the standard tackle gate.
    pub fn can_act(self) -> bool {
        !matches!(
            self,
            AnimState::Dive
                | AnimState::HitReaction
                | AnimState::Stumble
                | AnimState::AirborneFall
                | AnimState::GroundImpact
                | AnimState::Recovery
        )
    }

    /// Whether a ball carrier in this state is holding the ball in hand —
    /// running, standing, or dropping back — as opposed to throwing it, catching
    /// it, or being down (states that pose their own arms). *How* the held ball
    /// is carried (throw-ready by the ear vs cradled in the crook) is decided by
    /// `animation::ball_hold` from the carrier's role and field position, not
    /// here.
    pub fn holds_ball(self) -> bool {
        matches!(
            self,
            AnimState::ReadyStance
                | AnimState::Idle
                | AnimState::Jog
                | AnimState::Sprint
                | AnimState::DropBack
        )
    }

    /// Whether the player is on (or heading for) the turf.
    pub fn is_down(self) -> bool {
        matches!(
            self,
            AnimState::AirborneFall | AnimState::GroundImpact | AnimState::Recovery
        )
    }
}

/// One player's authoritative simulation record. Position `y` is height above
/// the field surface (non-zero only when knocked airborne).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerSim {
    pub id: PlayerId,
    pub team: TeamId,
    pub jersey: u8,
    pub archetype: PlayerArchetype,
    /// Ground position + airborne height, yards.
    pub pos: Vec3,
    /// Horizontal velocity, yd/s.
    pub vel: Vec3,
    /// Vertical velocity for knockdown arcs, yd/s.
    pub vertical_vel: f32,
    /// Facing yaw, radians (`0` faces `+Z`).
    pub facing: f32,
    /// Animation state + ticks spent in it.
    pub anim: AnimState,
    pub anim_ticks: u32,
    /// Balance `0..=1`; depleted by contact, restored over time.
    pub balance: f32,
    /// The strength of the hit that put this player down (drives the ground
    /// impact event when the fall completes).
    pub impact_strength: f32,
}

impl PlayerSim {
    /// A player standing ready at `pos`, facing `facing`.
    pub fn at(
        id: PlayerId,
        team: TeamId,
        jersey: u8,
        archetype: PlayerArchetype,
        pos: Vec3,
        facing: f32,
    ) -> Self {
        PlayerSim {
            id,
            team,
            jersey,
            archetype,
            pos,
            vel: Vec3::ZERO,
            vertical_vel: 0.0,
            facing,
            anim: AnimState::ReadyStance,
            anim_ticks: 0,
            balance: 1.0,
            impact_strength: 0.0,
        }
    }

    /// The unit facing direction on the ground plane.
    pub fn facing_dir(&self) -> Vec3 {
        Vec3::new(self.facing.sin(), 0.0, self.facing.cos())
    }

    /// Horizontal speed, yd/s.
    pub fn speed(&self) -> f32 {
        Vec3::new(self.vel.x, 0.0, self.vel.z).length()
    }

    /// Switch animation state (resets the in-state tick counter).
    pub fn set_anim(&mut self, anim: AnimState) {
        if self.anim != anim {
            self.anim = anim;
            self.anim_ticks = 0;
        }
    }
}
