//! Deterministic projection of host lifecycle signals into an engine state.

use crate::host_lifecycle_signal::HostLifecycleSignal;

/// The host's *current* lifecycle state, after a sequence of
/// [`HostLifecycleSignal`]s has been applied.
///
/// Pure data; the projection from signals to state is total and
/// deterministic. The engine boundary reads this to decide whether it is
/// allowed to drive a runtime step (see
/// [`HostLifecycleState::allows_stepping`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostLifecycleState {
    visible: bool,
    focused: bool,
    suspended: bool,
    shutdown_requested: bool,
}

impl HostLifecycleState {
    /// The state a host boundary is in before any signal has been observed:
    /// not visible, not focused, not suspended, no shutdown.
    pub const fn initial() -> Self {
        HostLifecycleState {
            visible: false,
            focused: false,
            suspended: false,
            shutdown_requested: false,
        }
    }

    /// Apply one signal and return the resulting state.
    ///
    /// The projection is monotonic for `ShutdownRequested` — once a shutdown
    /// has been observed it cannot be undone by later signals; the host must
    /// explicitly construct a fresh state to recover. This keeps replay
    /// deterministic and prevents a stuck-shutdown bug from being masked by
    /// an out-of-order `Started`.
    pub const fn apply(self, signal: HostLifecycleSignal) -> Self {
        match signal {
            HostLifecycleSignal::Started => HostLifecycleState {
                visible: true,
                ..self
            },
            HostLifecycleSignal::Visible => HostLifecycleState {
                visible: true,
                ..self
            },
            HostLifecycleSignal::Hidden => HostLifecycleState {
                visible: false,
                ..self
            },
            HostLifecycleSignal::Focused => HostLifecycleState {
                focused: true,
                ..self
            },
            HostLifecycleSignal::Unfocused => HostLifecycleState {
                focused: false,
                ..self
            },
            HostLifecycleSignal::Suspended => HostLifecycleState {
                suspended: true,
                ..self
            },
            HostLifecycleSignal::Resumed => HostLifecycleState {
                suspended: false,
                ..self
            },
            HostLifecycleSignal::ShutdownRequested => HostLifecycleState {
                shutdown_requested: true,
                ..self
            },
        }
    }

    pub const fn visible(&self) -> bool {
        self.visible
    }

    pub const fn focused(&self) -> bool {
        self.focused
    }

    pub const fn suspended(&self) -> bool {
        self.suspended
    }

    pub const fn shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    /// Whether the host boundary should permit a runtime step right now.
    ///
    /// - A `ShutdownRequested` always blocks stepping.
    /// - `Suspended` always blocks stepping (the host is asking the engine
    ///   to stand down).
    /// - `!visible` blocks stepping unless the caller has opted into
    ///   `step_while_hidden`.
    pub const fn allows_stepping(&self, step_while_hidden: bool) -> bool {
        if self.shutdown_requested {
            return false;
        }
        if self.suspended {
            return false;
        }
        if !self.visible && !step_while_hidden {
            return false;
        }
        true
    }
}

impl Default for HostLifecycleState {
    fn default() -> Self {
        HostLifecycleState::initial()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_quiescent() {
        let s = HostLifecycleState::initial();
        assert!(!s.visible());
        assert!(!s.focused());
        assert!(!s.suspended());
        assert!(!s.shutdown_requested());
    }

    #[test]
    fn started_signal_makes_state_visible() {
        let s = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
        assert!(s.visible());
    }

    #[test]
    fn visible_and_hidden_toggle_visibility() {
        let s = HostLifecycleState::initial()
            .apply(HostLifecycleSignal::Visible)
            .apply(HostLifecycleSignal::Hidden);
        assert!(!s.visible());
        let s = s.apply(HostLifecycleSignal::Visible);
        assert!(s.visible());
    }

    #[test]
    fn focused_and_unfocused_toggle_focus() {
        let s = HostLifecycleState::initial().apply(HostLifecycleSignal::Focused);
        assert!(s.focused());
        assert!(!s.apply(HostLifecycleSignal::Unfocused).focused());
    }

    #[test]
    fn suspended_and_resumed_toggle_suspension() {
        let s = HostLifecycleState::initial().apply(HostLifecycleSignal::Suspended);
        assert!(s.suspended());
        assert!(!s.apply(HostLifecycleSignal::Resumed).suspended());
    }

    #[test]
    fn shutdown_requested_is_sticky() {
        let s = HostLifecycleState::initial()
            .apply(HostLifecycleSignal::ShutdownRequested)
            .apply(HostLifecycleSignal::Started)
            .apply(HostLifecycleSignal::Visible);
        assert!(
            s.shutdown_requested(),
            "shutdown must survive later signals"
        );
        assert!(!s.allows_stepping(true));
    }

    #[test]
    fn allows_stepping_blocks_hidden_unless_opted_in() {
        let hidden = HostLifecycleState::initial();
        assert!(!hidden.allows_stepping(false));
        assert!(hidden.allows_stepping(true));
    }

    #[test]
    fn allows_stepping_blocks_suspended_unconditionally() {
        let suspended = HostLifecycleState::initial()
            .apply(HostLifecycleSignal::Started)
            .apply(HostLifecycleSignal::Suspended);
        assert!(!suspended.allows_stepping(true));
    }

    #[test]
    fn visible_state_allows_stepping() {
        let visible = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
        assert!(visible.allows_stepping(false));
    }

    #[test]
    fn equal_signal_sequences_produce_equal_state() {
        let a = HostLifecycleState::initial()
            .apply(HostLifecycleSignal::Started)
            .apply(HostLifecycleSignal::Focused);
        let b = HostLifecycleState::initial()
            .apply(HostLifecycleSignal::Started)
            .apply(HostLifecycleSignal::Focused);
        assert_eq!(a, b);
    }

    #[test]
    fn default_matches_initial() {
        assert_eq!(HostLifecycleState::default(), HostLifecycleState::initial());
    }
}
