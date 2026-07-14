//! Stable typed identities. Every cross-subsystem reference — teams, players,
//! the football, plays, assignments, camera targets — travels as one of these
//! newtypes, and players are always resolved in ascending [`PlayerId`] order
//! (fixed arrays indexed by id — never hash-map iteration order).

/// One of the two showcase teams. `TeamId(0)` is the home side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TeamId(pub u8);

/// A player's stable global identity: index `0..PLAYER_COUNT` into the sim's
/// fixed player array. Ordering is the deterministic resolution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub u8);

impl PlayerId {
    /// The array index this id addresses.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// The football (a single ball today; typed so a second ball is an id, not a
/// special case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BallId(pub u8);

/// A play definition's stable identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayId(pub u16);

/// One assignment row inside a play (index into the play's assignment list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssignmentId(pub u8);

/// What the camera director is asked to frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraTargetId {
    Player(PlayerId),
    Ball(BallId),
    /// The line of scrimmage (pre-snap formation framing).
    LineOfScrimmage,
}
