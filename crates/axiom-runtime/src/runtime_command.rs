//! A plain-data runtime command.

use axiom_kernel::Tick;

/// A runtime-level command — plain data the runtime queues until the next
/// drain boundary.
///
/// The runtime attaches no meaning to the `kind` code or payload bytes; future
/// engine layers assign and interpret them. Each command carries the kernel
/// [`Tick`] at which it was produced so replays can correlate commands with
/// the step that emitted them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCommand {
    kind: u32,
    origin_tick: Tick,
    payload: Vec<u8>,
}

impl RuntimeCommand {
    /// Build a command of the given kind at the given originating tick.
    pub fn new(kind: u32, origin_tick: Tick, payload: Vec<u8>) -> Self {
        RuntimeCommand {
            kind,
            origin_tick,
            payload,
        }
    }

    /// The opaque command kind code.
    pub fn kind(&self) -> u32 {
        self.kind
    }

    /// The kernel-typed tick at which this command was produced.
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
        let c = RuntimeCommand::new(7, Tick::new(3), vec![1, 2, 3]);
        assert_eq!(c.kind(), 7);
        assert_eq!(c.origin_tick(), Tick::new(3));
        assert_eq!(c.payload(), &[1, 2, 3]);
    }

    #[test]
    fn equality_is_structural() {
        let a = RuntimeCommand::new(1, Tick::new(0), vec![9]);
        let b = RuntimeCommand::new(1, Tick::new(0), vec![9]);
        assert_eq!(a, b);
    }
}
