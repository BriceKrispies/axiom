//! A deterministically ordered keyed collection.

use std::collections::BTreeMap;

/// A keyed collection of typed rows.
///
/// Ordering is by `K: Ord`, never by hash: a hash map's iteration order is not
/// guaranteed, and an unstable order would make snapshots, hashes and diffs
/// disagree between runs for no reason.
///
/// The API is value-semantic — [`Self::with`] and [`Self::without`] consume the
/// table and return a new one — so no caller can ever hold a mutable reference
/// into stored state. Internally they mutate the owned map they are about to
/// return, which is ordinary local construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTable<K, V> {
    rows: BTreeMap<K, V>,
}

impl<K: Ord, V> StateTable<K, V> {
    /// An empty table.
    pub fn new() -> Self {
        StateTable {
            rows: BTreeMap::new(),
        }
    }

    /// The table with `key` set to `value`, inserting or replacing.
    pub fn with(mut self, key: K, value: V) -> Self {
        self.rows.insert(key, value);
        self
    }

    /// The table without `key`, whether or not it was present.
    pub fn without(mut self, key: &K) -> Self {
        self.rows.remove(key);
        self
    }

    /// The value stored under `key`.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.rows.get(key)
    }

    /// Whether `key` is present.
    pub fn contains(&self, key: &K) -> bool {
        self.rows.contains_key(key)
    }

    /// How many rows the table holds.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Every row, ascending by key.
    pub fn rows(&self) -> Vec<(&K, &V)> {
        self.rows.iter().collect()
    }

    /// Every key, ascending.
    pub fn keys(&self) -> Vec<&K> {
        self.rows.keys().collect()
    }
}

impl<K: Ord, V> Default for StateTable<K, V> {
    fn default() -> Self {
        StateTable::new()
    }
}

/// Build a table from rows. Later rows win, so this is total.
impl<K: Ord, V> FromIterator<(K, V)> for StateTable<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(rows: I) -> Self {
        StateTable {
            rows: rows.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> StateTable<u32, u64> {
        StateTable::new().with(2, 20).with(1, 10).with(3, 30)
    }

    #[test]
    fn a_new_table_is_empty() {
        let empty: StateTable<u32, u64> = StateTable::new();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert!(empty.rows().is_empty());
        assert!(empty.keys().is_empty());
    }

    #[test]
    fn the_default_table_is_the_empty_one() {
        let defaulted: StateTable<u32, u64> = StateTable::default();
        assert_eq!(defaulted, StateTable::new());
    }

    #[test]
    fn a_row_can_be_inserted_and_read_back() {
        let table = table();
        assert_eq!(table.get(&1), Some(&10));
        assert!(table.contains(&2));
        assert_eq!(table.len(), 3);
        assert!(!table.is_empty());
    }

    #[test]
    fn inserting_an_existing_key_replaces_its_value() {
        let table = table().with(1, 99);
        assert_eq!(table.get(&1), Some(&99));
        assert_eq!(table.len(), 3);
    }

    #[test]
    fn a_missing_key_reads_as_nothing() {
        let table = table();
        assert_eq!(table.get(&404), None);
        assert!(!table.contains(&404));
    }

    #[test]
    fn a_row_can_be_removed_and_removing_a_missing_row_is_harmless() {
        let table = table().without(&2);
        assert_eq!(table.len(), 2);
        assert!(!table.contains(&2));
        assert_eq!(table.without(&404).len(), 2);
    }

    #[test]
    fn iteration_is_ascending_by_key_regardless_of_insertion_order() {
        let forwards = StateTable::new().with(1, 10).with(2, 20).with(3, 30);
        let backwards = StateTable::new().with(3, 30).with(2, 20).with(1, 10);
        let expected: Vec<(&u32, &u64)> = vec![(&1, &10), (&2, &20), (&3, &30)];
        assert_eq!(forwards.rows(), expected);
        assert_eq!(backwards.rows(), expected);
        assert_eq!(forwards.keys(), vec![&1, &2, &3]);
        assert_eq!(forwards, backwards);
    }

    #[test]
    fn a_table_can_be_collected_from_rows_with_later_rows_winning() {
        let collected: StateTable<u32, u64> = [(1, 10), (2, 20), (1, 11)].into_iter().collect();
        assert_eq!(collected.len(), 2);
        assert_eq!(collected.get(&1), Some(&11));
    }
}
