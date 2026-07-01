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
        // `(clear_mask, set_mask)` table over packed bits (visible=1, focused=2,
        // suspended=4, shutdown=8): new_bits = (self_bits & !clear) | set,
        // indexed by the signal's discriminant.
        const VISIBLE: u8 = 1;
        const FOCUSED: u8 = 2;
        const SUSPENDED: u8 = 4;
        const SHUTDOWN: u8 = 8;
        // Indexed by `HostLifecycleSignal as u8` (Started=0 … ShutdownRequested=7).
        const TRANSITION: [(u8, u8); 8] = [
            (0, VISIBLE),   // Started        -> visible = true
            (0, SUSPENDED), // Suspended      -> suspended = true
            (SUSPENDED, 0), // Resumed        -> suspended = false
            (VISIBLE, 0),   // Hidden         -> visible = false
            (0, VISIBLE),   // Visible        -> visible = true
            (0, FOCUSED),   // Focused        -> focused = true
            (FOCUSED, 0),   // Unfocused      -> focused = false
            (0, SHUTDOWN),  // ShutdownRequested -> shutdown = true
        ];
        let bits = ((self.visible as u8) * VISIBLE)
            | ((self.focused as u8) * FOCUSED)
            | ((self.suspended as u8) * SUSPENDED)
            | ((self.shutdown_requested as u8) * SHUTDOWN);
        let (clear, set) = TRANSITION[signal as usize];
        let next = (bits & !clear) | set;
        HostLifecycleState {
            visible: next & VISIBLE != 0,
            focused: next & FOCUSED != 0,
            suspended: next & SUSPENDED != 0,
            shutdown_requested: next & SHUTDOWN != 0,
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
        let blocked =
            self.shutdown_requested | self.suspended | (!self.visible & !step_while_hidden);
        !blocked
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
