//! A typed, deterministic FIFO event buffer.

/// A buffer of events of one type `E`, drained in first-in-first-out order.
///
/// This is the minimal event primitive future systems consume: a producer
/// [`push`](Self::push)es events during a frame, a consumer [`drain`](Self::drain)s
/// them at an explicit boundary in insertion order, and [`clear`](Self::clear)
/// discards any that were not drained. Ordering is exactly insertion order — a
/// `Vec`, never a hashed container — so replay is deterministic. The ECS owns the
/// mechanism; the event *types* are defined by consumers.
#[derive(Debug, Clone)]
pub struct EventBuffer<E> {
    events: Vec<E>,
}

impl<E> EventBuffer<E> {
    /// Create an empty event buffer.
    pub fn new() -> Self {
        EventBuffer { events: Vec::new() }
    }

    /// Append an event to the back of the buffer.
    pub fn push(&mut self, event: E) {
        self.events.push(event);
    }

    /// Remove and yield every buffered event in FIFO order, emptying the buffer.
    pub fn drain(&mut self) -> impl Iterator<Item = E> + '_ {
        self.events.drain(..)
    }

    /// Discard all buffered events without yielding them.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// The number of buffered events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the buffer holds no events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl<E> Default for EventBuffer<E> {
    fn default() -> Self {
        EventBuffer::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Tick(u32);

    #[test]
    fn new_and_default_are_empty() {
        let a: EventBuffer<Tick> = EventBuffer::new();
        let b: EventBuffer<Tick> = EventBuffer::default();
        assert!(a.is_empty());
        assert_eq!(a.len(), 0);
        assert!(b.is_empty());
    }

    #[test]
    fn drains_in_fifo_order_and_empties() {
        let mut buffer = EventBuffer::new();
        buffer.push(Tick(1));
        buffer.push(Tick(2));
        buffer.push(Tick(3));
        assert_eq!(buffer.len(), 3);
        let drained: Vec<Tick> = buffer.drain().collect();
        assert_eq!(drained, vec![Tick(1), Tick(2), Tick(3)]);
        assert!(buffer.is_empty(), "drain empties the buffer");
    }

    #[test]
    fn clear_discards_without_yielding() {
        let mut buffer = EventBuffer::new();
        buffer.push(Tick(9));
        buffer.clear();
        assert!(buffer.is_empty());
        let drained: Vec<Tick> = buffer.drain().collect();
        assert!(drained.is_empty(), "nothing remains to drain after clear");
    }
}
