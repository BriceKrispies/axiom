//! The one coordinate system, defined once and used by every subsystem.
//!
//! * `X` runs along the goal line, **`+X` is the shooter's right** (with the
//!   camera behind the ball looking down `-Z`, world `+X` projects to the right
//!   of the screen — that identity is what lets "drag right, bend right" be
//!   literally true rather than a sign to remember).
//! * `Y` is up; the turf is `Y = 0`.
//! * `Z` runs away from the goal. The **goal plane is `Z = 0`** and the pitch
//!   extends toward `+Z`, so the ball always travels in `-Z`.
//!
//! One world unit is one metre, and the dimensions are the real laws-of-the-game
//! ones — a 7.32 × 2.44 m goal, a spot 11 m out, a 16.5 m box.

use axiom::prelude::Vec3;

/// Half the goal's inside width, metres (7.32 m between the posts).
pub const GOAL_HALF_WIDTH: f32 = 3.66;
/// Inside height of the goal, metres.
pub const GOAL_HEIGHT: f32 = 2.44;
/// Radius of the posts and crossbar, metres.
pub const POST_RADIUS: f32 = 0.06;
/// How deep the net hangs behind the goal line, metres.
pub const NET_DEPTH: f32 = 1.85;

/// The penalty spot, metres from the goal line.
pub const PENALTY_SPOT_Z: f32 = 11.0;
/// The penalty area: depth from the goal line and half-width, metres.
pub const PENALTY_AREA_DEPTH: f32 = 16.5;
pub const PENALTY_AREA_HALF_WIDTH: f32 = 20.16;
/// The six-yard box: depth and half-width, metres.
pub const GOAL_AREA_DEPTH: f32 = 5.5;
pub const GOAL_AREA_HALF_WIDTH: f32 = 9.16;
/// Radius of the D, centred on the penalty spot, metres.
pub const PENALTY_ARC_RADIUS: f32 = 9.15;

/// Half the pitch width, metres (a 68 m pitch).
pub const PITCH_HALF_WIDTH: f32 = 34.0;
/// How far up the pitch this half extends from the goal line, metres.
pub const PITCH_DEPTH: f32 = 55.0;
/// How far behind the goal line the dead-ball area runs, metres.
pub const BEHIND_GOAL: f32 = 9.0;

/// Where the keeper stands before the strike, metres in front of the goal line.
pub const KEEPER_LINE_Z: f32 = 0.42;

/// Painted line width, metres.
pub const LINE_WIDTH: f32 = 0.12;
/// Height paint is floated above the turf so it never z-fights it, metres.
pub const PAINT_Y: f32 = 0.012;

/// The rectangular mouth of the goal, and the only surface an authored shot is
/// allowed to finish on.
///
/// Normalized goal coordinates are `h ∈ [-1, +1]` across the mouth (`-1` at the
/// left post as the shooter sees it) and `v ∈ [0, +1]` up it (`0` on the turf,
/// `1` at the crossbar). `inset` is the margin held back from the frame on every
/// side, so an endpoint expressed in these coordinates can never land *on* a
/// post no matter how the numbers round.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalMouth {
    inset: f32,
}

impl GoalMouth {
    /// A mouth whose usable rectangle is held `inset` metres inside the frame.
    pub fn new(inset: f32) -> Self {
        GoalMouth {
            inset: inset.clamp(0.0, GOAL_HEIGHT * 0.4),
        }
    }

    /// Half-width of the usable rectangle, metres.
    pub fn half_width(&self) -> f32 {
        GOAL_HALF_WIDTH - POST_RADIUS - self.inset
    }

    /// Lowest usable height, metres.
    pub fn floor(&self) -> f32 {
        self.inset
    }

    /// Highest usable height, metres.
    pub fn ceiling(&self) -> f32 {
        GOAL_HEIGHT - POST_RADIUS - self.inset
    }

    /// Map normalized goal coordinates to the world point on the goal plane.
    /// Inputs outside range are clamped, which is what makes a sloppy touch land
    /// on the nearest legal point rather than nowhere.
    pub fn to_world(&self, h: f32, v: f32) -> Vec3 {
        let h = h.clamp(-1.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        Vec3::new(
            h * self.half_width(),
            self.floor() + v * (self.ceiling() - self.floor()),
            0.0,
        )
    }

    /// The inverse of [`Self::to_world`] for a point on (or off) the goal plane,
    /// clamped into range.
    pub fn to_normalized(&self, world: Vec3) -> (f32, f32) {
        let span = (self.ceiling() - self.floor()).max(1.0e-3);
        (
            (world.x / self.half_width()).clamp(-1.0, 1.0),
            ((world.y - self.floor()) / span).clamp(0.0, 1.0),
        )
    }

    /// The four corners of the *frame's* inside face, in world space, ordered
    /// bottom-left, bottom-right, top-right, top-left as the shooter sees them.
    /// This is the rectangle the aim overlay outlines — the real mouth, not the
    /// inset one, because that is what the player is looking at.
    pub fn frame_corners(&self) -> [Vec3; 4] {
        let x = GOAL_HALF_WIDTH - POST_RADIUS;
        let y = GOAL_HEIGHT - POST_RADIUS;
        [
            Vec3::new(-x, 0.0, 0.0),
            Vec3::new(x, 0.0, 0.0),
            Vec3::new(x, y, 0.0),
            Vec3::new(-x, y, 0.0),
        ]
    }
}

impl Default for GoalMouth {
    fn default() -> Self {
        GoalMouth::new(0.30)
    }
}

/// Where the ball sits before the strike.
pub fn ball_spot(radius: f32) -> Vec3 {
    Vec3::new(0.0, radius, PENALTY_SPOT_Z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_coordinates_round_trip_inside_the_frame() {
        let mouth = GoalMouth::new(0.3);
        for (h, v) in [(-1.0, 0.0), (0.0, 0.5), (1.0, 1.0), (0.42, 0.17)] {
            let world = mouth.to_world(h, v);
            let (rh, rv) = mouth.to_normalized(world);
            assert!((rh - h).abs() < 1.0e-4, "h {h} -> {rh}");
            assert!((rv - v).abs() < 1.0e-4, "v {v} -> {rv}");
            assert_eq!(world.z, 0.0);
        }
    }

    #[test]
    fn every_authored_endpoint_clears_the_frame_by_the_inset() {
        let inset = 0.3;
        let mouth = GoalMouth::new(inset);
        for h in [-2.0f32, -1.0, 0.0, 1.0, 2.0] {
            for v in [-1.0f32, 0.0, 0.5, 1.0, 2.0] {
                let p = mouth.to_world(h, v);
                assert!(p.x.abs() <= GOAL_HALF_WIDTH - POST_RADIUS - inset + 1.0e-5);
                assert!(p.y >= inset - 1.0e-5);
                assert!(p.y <= GOAL_HEIGHT - POST_RADIUS - inset + 1.0e-5);
            }
        }
    }

    #[test]
    fn a_huge_inset_cannot_invert_the_usable_rectangle() {
        let mouth = GoalMouth::new(99.0);
        assert!(mouth.ceiling() >= mouth.floor());
        assert!(mouth.half_width() > 0.0);
        // A degenerate span still resolves rather than dividing by zero.
        assert!(mouth.to_normalized(Vec3::new(0.0, 0.0, 0.0)).1.is_finite());
    }

    #[test]
    fn the_frame_corners_bound_the_usable_rectangle() {
        let mouth = GoalMouth::default();
        let corners = mouth.frame_corners();
        assert_eq!(corners[0].y, 0.0);
        assert!(corners[2].x > mouth.half_width());
        assert!(corners[2].y > mouth.ceiling());
        assert_eq!(ball_spot(0.11), Vec3::new(0.0, 0.11, PENALTY_SPOT_Z));
    }
}
