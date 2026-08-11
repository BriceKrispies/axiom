//! What it means for two proposed changes to collide.

use axiom_kernel::StableHash;

use crate::state_id::StateId;
use crate::state_op_kind::StateOpKind;
use crate::state_origin::StateOrigin;

/// Where one side of a conflict came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateOpRef {
    origin: StateOrigin,
    patch_index: u32,
    op_index: u32,
    kind: StateOpKind,
}

impl StateOpRef {
    /// Locate an operation.
    pub const fn new(
        origin: StateOrigin,
        patch_index: u32,
        op_index: u32,
        kind: StateOpKind,
    ) -> Self {
        StateOpRef {
            origin,
            patch_index,
            op_index,
            kind,
        }
    }

    /// Who wrote it.
    pub const fn origin(&self) -> StateOrigin {
        self.origin
    }

    /// Which patch it was in.
    pub const fn patch_index(&self) -> u32 {
        self.patch_index
    }

    /// Its position within that patch.
    pub const fn op_index(&self) -> u32 {
        self.op_index
    }

    /// What change it proposed.
    pub const fn kind(&self) -> StateOpKind {
        self.kind
    }
}

/// Two changes that cannot both be applied.
///
/// Conflicting writes are a **deterministic error**, never last-write-wins:
/// silently keeping one of two disagreeing values is how a simulation diverges
/// from its own replay. There is no resolution policy to configure here — a
/// collision means the caller's decomposition is wrong, and the engine's job is
/// to say exactly where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateConflict {
    state: StateId,
    granule: StableHash,
    left: StateOpRef,
    right: StateOpRef,
}

impl StateConflict {
    /// Record a collision.
    pub const fn new(
        state: StateId,
        granule: StableHash,
        left: StateOpRef,
        right: StateOpRef,
    ) -> Self {
        StateConflict {
            state,
            granule,
            left,
            right,
        }
    }

    /// Which state the two changes disagree about.
    pub const fn state(&self) -> StateId {
        self.state
    }

    /// Which part of it: the digest of the row key or position, or of nothing
    /// for a change that spans the whole state.
    pub const fn granule(&self) -> StableHash {
        self.granule
    }

    /// The earlier of the two changes.
    pub const fn left(&self) -> StateOpRef {
        self.left
    }

    /// The later of the two changes.
    pub const fn right(&self) -> StateOpRef {
        self.right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op_ref(name: &str, patch: u32, op: u32) -> StateOpRef {
        StateOpRef::new(
            StateOrigin::of_name(name),
            patch,
            op,
            StateOpKind::SetCell,
        )
    }

    #[test]
    fn an_operation_reference_locates_its_writer_and_position() {
        let located = op_ref("scoring", 1, 2);
        assert_eq!(located.origin(), StateOrigin::of_name("scoring"));
        assert_eq!(located.patch_index(), 1);
        assert_eq!(located.op_index(), 2);
        assert_eq!(located.kind(), StateOpKind::SetCell);
    }

    #[test]
    fn a_conflict_names_the_state_the_granule_and_both_sides() {
        let state = StateId::of_path("test/score");
        let granule = StableHash::of_bytes(&[]);
        let left = op_ref("scoring", 0, 0);
        let right = op_ref("physics", 1, 0);
        let conflict = StateConflict::new(state, granule, left, right);
        assert_eq!(conflict.state(), state);
        assert_eq!(conflict.granule(), granule);
        assert_eq!(conflict.left(), left);
        assert_eq!(conflict.right(), right);
        assert_ne!(
            conflict.left().origin(),
            conflict.right().origin(),
            "a conflict is between two different writers"
        );
    }
}
