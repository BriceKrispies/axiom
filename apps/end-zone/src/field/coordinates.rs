//! The one field coordinate system, used by every subsystem:
//!
//! * `X` runs sideline to sideline (|X| ≤ 26.667).
//! * `Y` is vertical; the field surface is `Y = 0`.
//! * `Z` runs end zone to end zone (|Z| ≤ 60); the origin is midfield.
//! * One world unit = one yard.
//!
//! The playing field is 100 yards between the goal lines at `Z = ±50`, with a
//! 10-yard end zone beyond each. Every conversion between yard lines,
//! normalized coordinates, and offense-relative coordinates lives HERE — no
//! sign inversions or field constants are scattered elsewhere.

use axiom::prelude::Vec3;

/// Total field length including both end zones, yards.
pub const FIELD_LENGTH: f32 = 120.0;
/// Half the total length: the end lines sit at `Z = ±60`.
pub const FIELD_HALF_LENGTH: f32 = FIELD_LENGTH / 2.0;
/// Field width, yards (53 1/3).
pub const FIELD_WIDTH: f32 = 160.0 / 3.0;
/// Half the width: the sidelines sit at `X = ±26.667`.
pub const FIELD_HALF_WIDTH: f32 = FIELD_WIDTH / 2.0;
/// The goal lines sit at `Z = ±50`.
pub const GOAL_LINE_Z: f32 = 50.0;
/// Hash marks inset: NFL hashes are 23.583 yd in from each sideline.
pub const HASH_X: f32 = FIELD_HALF_WIDTH - 70.75 / 3.0;

/// Which end zone the offense is driving toward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveDirection {
    /// Attacking the end zone at `Z = +60`.
    PlusZ,
    /// Attacking the end zone at `Z = -60`.
    MinusZ,
}

impl DriveDirection {
    /// `+1.0` for [`DriveDirection::PlusZ`], `-1.0` for [`DriveDirection::MinusZ`].
    pub fn sign(self) -> f32 {
        match self {
            DriveDirection::PlusZ => 1.0,
            DriveDirection::MinusZ => -1.0,
        }
    }

    /// The opposite drive direction.
    pub fn flipped(self) -> DriveDirection {
        match self {
            DriveDirection::PlusZ => DriveDirection::MinusZ,
            DriveDirection::MinusZ => DriveDirection::PlusZ,
        }
    }
}

/// The broadcast yard-line number for a world position: `50` at midfield,
/// `0` at either goal line, negative inside an end zone (`-10` at the end
/// lines). Symmetric in `Z`, so it names the line, not the direction.
pub fn world_to_yard_line(position: Vec3) -> f32 {
    GOAL_LINE_Z - position.z.abs()
}

/// The world `Z` of a yard line as seen by an offense driving `direction`:
/// `yards_from_own_goal` counts from the offense's own goal line (`0`) through
/// midfield (`50`) to the opponent goal line (`100`).
pub fn yard_line_to_z(yards_from_own_goal: f32, direction: DriveDirection) -> f32 {
    (yards_from_own_goal - GOAL_LINE_Z) * direction.sign()
}

/// The inverse of [`yard_line_to_z`]: how far a world `Z` sits from the
/// offense's own goal line, in yards (`0` own goal, `50` midfield, `100`
/// opponent goal, `>100` inside the attacked end zone).
pub fn z_to_yards_from_own_goal(world_z: f32, direction: DriveDirection) -> f32 {
    world_z * direction.sign() + GOAL_LINE_Z
}

/// Map normalized field coordinates to world: `u ∈ [0,1]` spans sideline to
/// sideline (`-X` to `+X`), `v ∈ [0,1]` spans end line to end line
/// (`-Z` to `+Z`). Returns a point on the surface (`Y = 0`).
pub fn normalized_to_world(u: f32, v: f32) -> Vec3 {
    Vec3::new((u - 0.5) * FIELD_WIDTH, 0.0, (v - 0.5) * FIELD_LENGTH)
}

/// A point in offense-relative coordinates: `lateral` is yards toward the
/// offense's right hand, `downfield` is yards toward the opponent end zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffensePoint {
    pub lateral: f32,
    pub downfield: f32,
}

impl OffensePoint {
    pub fn new(lateral: f32, downfield: f32) -> Self {
        OffensePoint { lateral, downfield }
    }
}

/// The offense-relative frame for a drive: anchored at the line of scrimmage,
/// facing the opponent end zone. Works in either drive direction — routes and
/// formations are authored once and mirror through this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffenseFrame {
    pub direction: DriveDirection,
    /// World `Z` of the line of scrimmage.
    pub line_of_scrimmage_z: f32,
}

impl OffenseFrame {
    /// A frame at `yards_from_own_goal` for an offense driving `direction`.
    pub fn at_yard_line(yards_from_own_goal: f32, direction: DriveDirection) -> Self {
        OffenseFrame {
            direction,
            line_of_scrimmage_z: yard_line_to_z(yards_from_own_goal, direction),
        }
    }

    /// The world-space forward (downfield) direction.
    pub fn forward(&self) -> Vec3 {
        Vec3::new(0.0, 0.0, self.direction.sign())
    }

    /// The world-space direction of the offense's right hand. Facing `+Z`
    /// with `Y` up, right is `-X`; facing `-Z`, right is `+X`.
    pub fn right(&self) -> Vec3 {
        Vec3::new(-self.direction.sign(), 0.0, 0.0)
    }

    /// Offense-relative → world (on the surface, `Y = 0`).
    pub fn to_world(&self, point: OffensePoint) -> Vec3 {
        let s = self.direction.sign();
        Vec3::new(
            -s * point.lateral,
            0.0,
            self.line_of_scrimmage_z + s * point.downfield,
        )
    }

    /// World → offense-relative (drops `Y`).
    pub fn from_world(&self, world: Vec3) -> OffensePoint {
        let s = self.direction.sign();
        OffensePoint {
            lateral: -s * world.x,
            downfield: s * (world.z - self.line_of_scrimmage_z),
        }
    }

    /// Clamp a world point into the playing surface (keeps steering in bounds;
    /// `margin` shrinks the boundary, in yards).
    pub fn clamp_in_bounds(world: Vec3, margin: f32) -> Vec3 {
        Vec3::new(
            world
                .x
                .clamp(-(FIELD_HALF_WIDTH - margin), FIELD_HALF_WIDTH - margin),
            world.y,
            world
                .z
                .clamp(-(FIELD_HALF_LENGTH - margin), FIELD_HALF_LENGTH - margin),
        )
    }
}
