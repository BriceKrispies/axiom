//! A plain-data runtime event.

use axiom_kernel::Tick;

/// A runtime-level event — plain data the runtime queues until the next drain
/// boundary.
///
/// Structurally identical to [`crate::runtime_command::RuntimeCommand`] but
/// kept as a distinct type so producers and consumers do not confuse the two
/// semantic streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEvent {
    kind: u32,
    origin_tick: Tick,
    payload: Vec<u8>,
}

impl RuntimeEvent {
    /// Build an event of the given kind at the given originating tick.
    pub fn new(kind: u32, origin_tick: Tick, payload: Vec<u8>) -> Self {
        RuntimeEvent {
            kind,
            origin_tick,
            payload,
        }
    }

    /// The opaque event kind code.
    pub fn kind(&self) -> u32 {
        self.kind
    }

    /// The kernel-typed tick at which this event was produced.
    pub fn origin_tick(&self) -> Tick {
        self.origin_tick
    }

    /// The opaque payload bytes.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_return_constructed_parts() {
        let e = RuntimeEvent::new(2, Tick::new(5), vec![]);
        assert_eq!(e.kind(), 2);
        assert_eq!(e.origin_tick(), Tick::new(5));
        assert!(e.payload().is_empty());
    }

    #[test]
    fn payload_returns_the_constructed_bytes() {
        let e = RuntimeEvent::new(2, Tick::new(5), vec![9, 8, 7]);
        assert_eq!(e.payload(), &[9, 8, 7]);
    }

    #[test]
    fn equality_is_structural() {
        let a = RuntimeEvent::new(1, Tick::new(0), vec![]);
        let b = RuntimeEvent::new(1, Tick::new(0), vec![]);
        assert_eq!(a, b);
    }
}
