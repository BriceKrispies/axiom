//! The verdict of comparing peers' state hashes at a tick.

/// The result of reconciling per-peer state hashes for one confirmed tick.
///
/// `Pending` means not every peer has reported a hash yet (no verdict possible).
/// `InSync` means every peer reported and all hashes agree. `Desync` means every
/// peer reported but at least two disagree — the determinism guarantee broke and
/// the session must halt/resync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncStatus {
    /// Not all peers have reported a hash for this tick yet.
    Pending,
    /// Every peer reported and all hashes agree.
    InSync,
    /// Every peer reported but hashes disagree at this tick.
    Desync {
        /// The tick at which the divergence was observed.
        tick: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(SyncStatus::Pending, SyncStatus::InSync);
        assert_ne!(SyncStatus::InSync, SyncStatus::Desync { tick: 3 });
        assert_eq!(
            SyncStatus::Desync { tick: 3 },
            SyncStatus::Desync { tick: 3 }
        );
        assert_ne!(
            SyncStatus::Desync { tick: 3 },
            SyncStatus::Desync { tick: 4 }
        );
    }
}
