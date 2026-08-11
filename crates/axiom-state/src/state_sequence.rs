//! An explicitly ordered sequence, where position carries meaning.

/// An ordered list of typed items.
///
/// Unlike a table, order here is *semantic* — a batting order, a checkpoint
/// list, an event log — so items are addressed by position and never reordered
/// behind the caller's back.
///
/// Value-semantic like [`crate::StateTable`]: the builders consume and return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSequence<T> {
    items: Vec<T>,
}

impl<T> StateSequence<T> {
    /// An empty sequence.
    pub fn new() -> Self {
        StateSequence { items: Vec::new() }
    }

    /// The sequence with `item` appended at the end.
    pub fn appended(mut self, item: T) -> Self {
        self.items.push(item);
        self
    }

    /// Every item, in order.
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// The item at `index`.
    pub fn get(&self, index: usize) -> Option<&T> {
        self.items.get(index)
    }

    /// How many items the sequence holds.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the sequence is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl<T> Default for StateSequence<T> {
    fn default() -> Self {
        StateSequence::new()
    }
}

impl<T> FromIterator<T> for StateSequence<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        StateSequence {
            items: items.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_sequence_is_empty() {
        let empty: StateSequence<u32> = StateSequence::new();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert!(empty.items().is_empty());
        assert_eq!(empty.get(0), None);
    }

    #[test]
    fn the_default_sequence_is_the_empty_one() {
        let defaulted: StateSequence<u32> = StateSequence::default();
        assert_eq!(defaulted, StateSequence::new());
    }

    #[test]
    fn appending_preserves_order() {
        let sequence = StateSequence::new().appended(10).appended(20).appended(30);
        assert_eq!(sequence.items(), &[10, 20, 30]);
        assert_eq!(sequence.len(), 3);
        assert_eq!(sequence.get(1), Some(&20));
    }

    #[test]
    fn order_is_semantic_so_a_different_order_is_a_different_sequence() {
        let forwards = StateSequence::new().appended(1).appended(2);
        let backwards = StateSequence::new().appended(2).appended(1);
        assert_ne!(forwards, backwards);
    }

    #[test]
    fn reading_past_the_end_yields_nothing() {
        let sequence = StateSequence::new().appended(1);
        assert_eq!(sequence.get(1), None);
    }

    #[test]
    fn a_sequence_can_be_collected_in_order() {
        let collected: StateSequence<u32> = [3, 1, 2].into_iter().collect();
        assert_eq!(collected.items(), &[3, 1, 2]);
    }
}
