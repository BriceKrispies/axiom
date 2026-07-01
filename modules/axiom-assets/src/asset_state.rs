//! The per-asset load state and its completion-driven transition table.

/// Where an asset sits in its load lifecycle. `#[repr(u8)]` with explicit
/// discriminants so it indexes the branchless transition table directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum AssetState {
    /// Not yet scheduled for loading.
    Unrequested = 0,
    /// A load has been dispatched; the app is fetching it.
    Requested = 1,
    /// Bytes arrived successfully.
    Ready = 2,
    /// The load failed (terminal in this MVP — no auto-retry).
    Failed = 3,
}

/// The outcome of a dispatched load, used as the completion column of the
/// transition table.
#[derive(Debug, Clone, Copy)]
#[repr(usize)]
pub(crate) enum CompletionOutcome {
    Failure = 0,
    Success = 1,
}

/// `TRANSITIONS[current][outcome]` — the next state when a completion arrives.
/// Only `Requested` advances; every other current state maps to itself, so a
/// duplicate or stray completion is a harmless no-op with no branch.
const TRANSITIONS: [[AssetState; 2]; 4] = [
    [AssetState::Unrequested, AssetState::Unrequested],
    [AssetState::Failed, AssetState::Ready],
    [AssetState::Ready, AssetState::Ready],
    [AssetState::Failed, AssetState::Failed],
];

impl AssetState {
    /// The state after `outcome` completes for an asset currently in `self`.
    pub(crate) fn on_completion(self, outcome: CompletionOutcome) -> AssetState {
        TRANSITIONS[self as usize][outcome as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_requested_advances_on_completion() {
        assert_eq!(
            AssetState::Requested.on_completion(CompletionOutcome::Success),
            AssetState::Ready
        );
        assert_eq!(
            AssetState::Requested.on_completion(CompletionOutcome::Failure),
            AssetState::Failed
        );
    }

    #[test]
    fn non_requested_states_ignore_completions() {
        let untouched = [
            AssetState::Unrequested,
            AssetState::Ready,
            AssetState::Failed,
        ];
        untouched.into_iter().for_each(|state| {
            assert_eq!(state.on_completion(CompletionOutcome::Success), state);
            assert_eq!(state.on_completion(CompletionOutcome::Failure), state);
        });
    }
}
