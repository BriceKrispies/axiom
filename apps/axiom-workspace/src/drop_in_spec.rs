//! [`DropInSpec`] — the editor-derived spawn context attached to a "Drop In"
//! launch, and [`DropLevel`] — the fixed-size, `Copy` level identifier it holds.
//!
//! "Drop In" is a launch spec **plus** this editor-derived spawn context: where
//! in a level, facing which way, with which entity selected, a launched session
//! should spawn into. Attaching a drop context is additive — it never changes the
//! launch identity (see [`crate::launch_spec::LaunchSpec`]).

use axiom_kernel::{EntityId, Meters, Radians};

/// The editor-derived spawn context attached to a "Drop In" launch.
///
/// It captures where a launched session should spawn: a level id, a world
/// position and yaw (typically camera-derived in an editor), and an optionally
/// selected entity to drop in as / near. Positions and rotations are carried as
/// the kernel's dimensioned scalars so the units are explicit at the boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropInSpec {
    level_id: DropLevel,
    position: [Meters; 3],
    yaw: Radians,
    selected_entity: Option<EntityId>,
}

/// A fixed-size, `Copy` level identifier used inside [`DropInSpec`]. It stores a
/// small level id as raw bytes so the whole drop context stays `Copy` and free of
/// heap state, keeping the spawn context a cheap, comparable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropLevel {
    bytes: [u8; DropLevel::CAP],
    len: usize,
}

impl DropLevel {
    /// Maximum stored level-id length; longer ids are truncated to this bound.
    pub const CAP: usize = 32;

    /// Store a level id, truncated to [`DropLevel::CAP`] bytes.
    #[must_use]
    pub fn new(level_id: &str) -> Self {
        let src = level_id.as_bytes();
        let len = src.len().min(Self::CAP);
        let mut bytes = [0u8; Self::CAP];
        bytes[..len].copy_from_slice(&src[..len]);
        DropLevel { bytes, len }
    }

    /// The stored level id as bytes (already truncated to the cap).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl DropInSpec {
    /// Build a spawn context. Non-finite position/yaw coordinates collapse to
    /// zero via the kernel's dimensioned scalars, so a drop context is always a
    /// well-formed value.
    #[must_use]
    pub fn new(
        level_id: &str,
        position: [f32; 3],
        yaw: f32,
        selected_entity: Option<EntityId>,
    ) -> Self {
        DropInSpec {
            level_id: DropLevel::new(level_id),
            position: position.map(Meters::finite_or_zero),
            yaw: Radians::finite_or_zero(yaw),
            selected_entity,
        }
    }

    /// The level id the session drops into.
    #[must_use]
    pub fn level_id(&self) -> DropLevel {
        self.level_id
    }

    /// The spawn position (metres).
    #[must_use]
    pub fn position(&self) -> [Meters; 3] {
        self.position
    }

    /// The spawn yaw (radians).
    #[must_use]
    pub fn yaw(&self) -> Radians {
        self.yaw
    }

    /// The selected entity to drop in as / near, if any.
    #[must_use]
    pub fn selected_entity(&self) -> Option<EntityId> {
        self.selected_entity
    }
}
