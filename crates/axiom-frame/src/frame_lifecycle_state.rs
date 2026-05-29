//! Frame-level coarse lifecycle state.

use axiom_host::HostLifecycleState;

/// The coarse lifecycle state of the engine frame boundary.
///
/// `FrameLifecycleState` is the projection of a [`HostLifecycleState`] into
/// the four states the engine frame boundary actually distinguishes:
///
/// - [`Active`](Self::Active) — the engine should produce a real frame.
/// - [`Hidden`](Self::Hidden) — the host is not visible and the boundary
///   policy did not opt into stepping while hidden.
/// - [`Suspended`](Self::Suspended) — the host explicitly asked the
///   engine to stand down.
/// - [`ShutdownRequested`](Self::ShutdownRequested) — the host is asking
///   the engine to stop at the next safe boundary.
///
/// Mapping is total and deterministic: `ShutdownRequested` wins, then
/// `Suspended`, then `Hidden`, then `Active`. Input mapping (keyboard,
/// mouse, gamepad, touch) is deliberately out of scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameLifecycleState {
    Active,
    Hidden,
    Suspended,
    ShutdownRequested,
}

impl FrameLifecycleState {
    /// Project a host lifecycle state onto the four frame-level states.
    pub const fn from_host(state: HostLifecycleState) -> Self {
        if state.shutdown_requested() {
            FrameLifecycleState::ShutdownRequested
        } else if state.suspended() {
            FrameLifecycleState::Suspended
        } else if !state.visible() {
            FrameLifecycleState::Hidden
        } else {
            FrameLifecycleState::Active
        }
    }

    /// `true` iff the frame boundary considers this state safe to step
    /// (i.e. the host is visible, not suspended, not shutting down).
    pub const fn is_active(self) -> bool {
        matches!(self, FrameLifecycleState::Active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::HostLifecycleSignal;

    fn visible() -> HostLifecycleState {
        HostLifecycleState::initial().apply(HostLifecycleSignal::Started)
    }

    #[test]
    fn active_state_maps_from_visible_host() {
        assert_eq!(
            FrameLifecycleState::from_host(visible()),
            FrameLifecycleState::Active
        );
        assert!(FrameLifecycleState::Active.is_active());
    }

    #[test]
    fn hidden_state_maps_from_invisible_host() {
        let hidden = HostLifecycleState::initial();
        assert_eq!(
            FrameLifecycleState::from_host(hidden),
            FrameLifecycleState::Hidden
        );
        assert!(!FrameLifecycleState::Hidden.is_active());
    }

    #[test]
    fn suspended_state_maps_from_suspended_host() {
        let suspended = visible().apply(HostLifecycleSignal::Suspended);
        assert_eq!(
            FrameLifecycleState::from_host(suspended),
            FrameLifecycleState::Suspended
        );
    }

    #[test]
    fn shutdown_state_maps_from_shutdown_host() {
        let shutting = visible().apply(HostLifecycleSignal::ShutdownRequested);
        assert_eq!(
            FrameLifecycleState::from_host(shutting),
            FrameLifecycleState::ShutdownRequested
        );
    }

    #[test]
    fn shutdown_wins_over_suspended_wins_over_hidden() {
        let s = HostLifecycleState::initial()
            .apply(HostLifecycleSignal::Started)
            .apply(HostLifecycleSignal::Hidden)
            .apply(HostLifecycleSignal::Suspended)
            .apply(HostLifecycleSignal::ShutdownRequested);
        assert_eq!(
            FrameLifecycleState::from_host(s),
            FrameLifecycleState::ShutdownRequested
        );
    }

    #[test]
    fn mapping_is_deterministic_for_equal_host_states() {
        let h = visible();
        assert_eq!(
            FrameLifecycleState::from_host(h),
            FrameLifecycleState::from_host(h)
        );
    }
}
