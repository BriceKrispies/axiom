//! A single coarse host lifecycle signal.

/// One coarse host lifecycle fact, supplied to the host boundary as an
/// explicit value.
///
/// These are **only** the lifecycle / visibility / focus facts the engine
/// boundary needs to drive deterministic stepping. Keyboard, mouse, touch,
/// gamepad, and any other input mapping are out of scope: input belongs to a
/// dedicated higher layer that has not been built yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostLifecycleSignal {
    /// The host has started and the engine is allowed to begin stepping.
    Started,
    /// The host has been suspended (background, OS sleep, tab discard…).
    Suspended,
    /// The host has resumed from a suspended state.
    Resumed,
    /// The host surface is no longer visible (tab hidden, minimized…).
    Hidden,
    /// The host surface is visible again.
    Visible,
    /// The host surface gained focus.
    Focused,
    /// The host surface lost focus.
    Unfocused,
    /// The host is asking the engine to shut down at the next safe boundary.
    ShutdownRequested,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_are_copyable_value_types() {
        let s = HostLifecycleSignal::Started;
        let t = s;
        assert_eq!(s, t);
    }

    #[test]
    fn distinct_variants_are_not_equal() {
        assert_ne!(HostLifecycleSignal::Hidden, HostLifecycleSignal::Visible);
        assert_ne!(HostLifecycleSignal::Focused, HostLifecycleSignal::Unfocused);
        assert_ne!(
            HostLifecycleSignal::Suspended,
            HostLifecycleSignal::Resumed
        );
        assert_ne!(
            HostLifecycleSignal::Started,
            HostLifecycleSignal::ShutdownRequested
        );
    }

    #[test]
    fn ordering_is_preserved_when_queued() {
        // Layer 03 does not impose semantic ordering on lifecycle signals; it
        // only requires that an externally-supplied sequence is replayed in
        // insertion order. A Vec is the queue contract.
        let queue = vec![
            HostLifecycleSignal::Started,
            HostLifecycleSignal::Visible,
            HostLifecycleSignal::Focused,
            HostLifecycleSignal::Hidden,
            HostLifecycleSignal::Resumed,
        ];
        let replayed: Vec<_> = queue.iter().copied().collect();
        assert_eq!(queue, replayed);
    }
}
