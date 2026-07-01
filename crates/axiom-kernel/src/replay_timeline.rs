//! An ordered recording of values, replayed deterministically by a cursor.
//!
//! A [`ReplayTimeline`] is the data companion to the kernel's clock: you
//! [`record`](ReplayTimeline::record) values in order during a live run, then
//! replay them by advancing a saturating cursor one item at a time
//! ([`advance`](ReplayTimeline::advance)). Given the same recorded sequence,
//! replay is byte-identical every time — the basis for deterministic ghosts,
//! demo playback, input timelines, and replay-based tests. The cursor saturates
//! at the end (it never panics and never runs off), and
//! [`reset`](ReplayTimeline::reset) rewinds it to replay again.
//!
//! It is the kernel's first *type-generic* primitive, deliberately so: the thing
//! being recorded and replayed is the caller's (a move, a command, an event),
//! not a kernel type, so the timeline is generic over the recorded item `T`. It
//! carries no semantics beyond "ordered record, cursored replay" — cadence is
//! the separate [`crate::tick_divider::TickDivider`], and domain meaning stays
//! with the caller.

/// An append-only recording with a saturating replay cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayTimeline<T> {
    items: Vec<T>,
    cursor: usize,
}

impl<T> ReplayTimeline<T> {
    /// An empty timeline: nothing recorded, cursor at the start.
    pub fn new() -> Self {
        ReplayTimeline {
            items: Vec::new(),
            cursor: 0,
        }
    }

    /// A timeline pre-loaded with an already-recorded sequence, cursor at the
    /// start (ready to replay).
    pub fn from_recorded(items: Vec<T>) -> Self {
        ReplayTimeline { items, cursor: 0 }
    }

    /// Append one item to the recording.
    pub fn record(&mut self, item: T) {
        self.items.push(item);
    }

    /// Return the item at the cursor and advance past it; `None` once every
    /// recorded item has been replayed. The cursor saturates at the end, so
    /// calling past the end keeps returning `None` without panicking.
    pub fn advance(&mut self) -> Option<&T> {
        let next = self.cursor;
        self.cursor = (next + 1).min(self.items.len());
        self.items.get(next)
    }

    /// How many recorded items remain to be replayed.
    pub fn remaining(&self) -> usize {
        self.items.len() - self.cursor
    }

    /// Whether every recorded item has been replayed.
    pub fn is_finished(&self) -> bool {
        self.cursor >= self.items.len()
    }

    /// The number of recorded items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The cursor's position: how many items have been replayed so far.
    pub fn position(&self) -> usize {
        self.cursor
    }

    /// The full recorded sequence, in order.
    pub fn recorded(&self) -> &[T] {
        &self.items
    }

    /// Rewind the cursor to the start to replay the recording again.
    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Discard the recording and rewind the cursor.
    pub fn clear(&mut self) {
        self.items.clear();
        self.cursor = 0;
    }
}

impl<T> Default for ReplayTimeline<T> {
    fn default() -> Self {
        ReplayTimeline::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_timeline_is_finished_and_yields_nothing() {
        let mut t: ReplayTimeline<u8> = ReplayTimeline::new();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        assert!(t.is_finished());
        assert_eq!(t.remaining(), 0);
        assert_eq!(t.advance(), None);
        assert_eq!(ReplayTimeline::<u8>::default(), t);
    }

    #[test]
    fn records_then_replays_in_order_and_saturates() {
        let mut t = ReplayTimeline::new();
        t.record(10u8);
        t.record(20u8);
        assert!(!t.is_empty());
        assert_eq!(t.len(), 2);
        assert_eq!(t.remaining(), 2);
        assert!(!t.is_finished());
        assert_eq!(t.recorded(), &[10, 20]);

        assert_eq!(t.advance(), Some(&10));
        assert_eq!(t.position(), 1);
        assert_eq!(t.remaining(), 1);
        assert_eq!(t.advance(), Some(&20));
        assert_eq!(t.position(), 2);
        assert!(t.is_finished());
        assert_eq!(t.remaining(), 0);
        assert_eq!(t.advance(), None);
        assert_eq!(t.position(), 2);
    }

    #[test]
    fn from_recorded_is_ready_to_replay() {
        let mut t = ReplayTimeline::from_recorded(vec![1u8, 2, 3]);
        assert_eq!(t.len(), 3);
        assert_eq!(t.advance(), Some(&1));
    }

    #[test]
    fn reset_rewinds_for_another_replay() {
        let mut t = ReplayTimeline::from_recorded(vec![7u8]);
        assert_eq!(t.advance(), Some(&7));
        assert!(t.is_finished());
        t.reset();
        assert_eq!(t.position(), 0);
        assert!(!t.is_finished());
        assert_eq!(t.advance(), Some(&7));
    }

    #[test]
    fn clear_discards_everything() {
        let mut t = ReplayTimeline::from_recorded(vec![1u8, 2]);
        t.advance();
        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.position(), 0);
        assert_eq!(t.recorded(), &[] as &[u8]);
    }
}
