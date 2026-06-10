//! Why a host frame's planner produced zero runtime steps.

/// The deterministic reason a host frame was skipped (planned with zero
/// runtime steps for a lifecycle reason, as opposed to natural under-budget
/// stepping where the accumulator simply hadn't filled yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostSkipReason {
    /// The host is hidden and the boundary config does not opt into
    /// `step_while_hidden`.
    LifecycleHidden,
    /// The host has reported `Suspended`; the boundary unconditionally
    /// blocks stepping.
    LifecycleSuspended,
    /// The host has requested shutdown; no further stepping is permitted.
    ShutdownRequested,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(
            HostSkipReason::LifecycleHidden,
            HostSkipReason::LifecycleSuspended
        );
        assert_ne!(
            HostSkipReason::LifecycleHidden,
            HostSkipReason::ShutdownRequested
        );
        assert_ne!(
            HostSkipReason::LifecycleSuspended,
            HostSkipReason::ShutdownRequested
        );
    }

    #[test]
    fn variants_are_copy_and_equal() {
        let s = HostSkipReason::ShutdownRequested;
        let t = s;
        assert_eq!(s, t);
    }
}
