//! A bounded, deterministic agent memory store.

use axiom_kernel::Tick;

/// One machine-readable memory record.
///
/// Memory is keyed by stable numeric codes, never by strings: a `key_code`
/// names *what kind* of thing is remembered and a `value_code` carries its
/// value, both defined by the app. The `tick` stamps when it was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryEntry {
    tick: Tick,
    key_code: u32,
    value_code: i64,
}

impl MemoryEntry {
    /// Construct a memory entry.
    pub const fn new(tick: Tick, key_code: u32, value_code: i64) -> Self {
        MemoryEntry {
            tick,
            key_code,
            value_code,
        }
    }

    /// The tick this entry was recorded at.
    pub const fn tick(self) -> Tick {
        self.tick
    }

    /// The stable code naming what is remembered.
    pub const fn key_code(self) -> u32 {
        self.key_code
    }

    /// The stable value associated with the key.
    pub const fn value_code(self) -> i64 {
        self.value_code
    }
}

/// A fixed-capacity, insertion-ordered memory of the most recent entries.
///
/// It is a plain `Vec` with an explicit `capacity` — never a hash map — so
/// iteration order is exactly insertion order and is identical across runs. When
/// a `remember` would exceed `capacity`, the oldest entry is dropped first, so
/// the store always holds at most `capacity` entries and always the newest ones.
/// A `capacity` of `0` stores nothing.
#[derive(Debug, Clone)]
pub struct AgentMemory {
    entries: Vec<MemoryEntry>,
    capacity: usize,
}

impl AgentMemory {
    /// An empty memory bounded to at most `capacity` entries.
    pub fn empty_with_capacity(capacity: usize) -> Self {
        AgentMemory {
            entries: Vec::new(),
            capacity,
        }
    }

    /// Record `entry`. If the store is already at capacity, the oldest entry is
    /// dropped first (branchlessly), preserving insertion order of the rest. A
    /// zero-capacity store records nothing.
    pub fn remember(&mut self, entry: MemoryEntry) {
        let at_capacity = self.entries.len() >= self.capacity;
        let has_entries = !self.entries.is_empty();
        (at_capacity & has_entries).then(|| self.entries.remove(0));
        (self.capacity > 0).then(|| self.entries.push(entry));
    }

    /// Drop every entry, keeping the capacity.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The bound on how many entries may be stored.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The stored entries, in insertion order (oldest first).
    pub fn entries(&self) -> &[MemoryEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(raw_tick: u64, key: u32, value: i64) -> MemoryEntry {
        MemoryEntry::new(Tick::new(raw_tick), key, value)
    }

    #[test]
    fn empty_store_reports_empty() {
        let m = AgentMemory::empty_with_capacity(4);
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
        assert_eq!(m.capacity(), 4);
        assert!(m.entries().is_empty());
    }

    #[test]
    fn remember_preserves_insertion_order_under_capacity() {
        let mut m = AgentMemory::empty_with_capacity(4);
        m.remember(entry(1, 10, 100));
        m.remember(entry(2, 11, 101));
        m.remember(entry(3, 12, 102));
        assert!(!m.is_empty());
        assert_eq!(m.len(), 3);
        let keys: Vec<u32> = m.entries().iter().map(|e| e.key_code()).collect();
        assert_eq!(keys, vec![10, 11, 12]);
    }

    #[test]
    fn remember_drops_oldest_when_capacity_exceeded() {
        let mut m = AgentMemory::empty_with_capacity(2);
        m.remember(entry(1, 10, 100));
        m.remember(entry(2, 11, 101));
        m.remember(entry(3, 12, 102));
        assert_eq!(m.len(), 2);
        let keys: Vec<u32> = m.entries().iter().map(|e| e.key_code()).collect();
        assert_eq!(keys, vec![11, 12]);
        assert_eq!(m.entries()[0].tick(), Tick::new(2));
        assert_eq!(m.entries()[1].value_code(), 102);
    }

    #[test]
    fn zero_capacity_stores_nothing() {
        let mut m = AgentMemory::empty_with_capacity(0);
        m.remember(entry(1, 10, 100));
        m.remember(entry(2, 11, 101));
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn clear_empties_but_keeps_capacity() {
        let mut m = AgentMemory::empty_with_capacity(3);
        m.remember(entry(1, 10, 100));
        m.clear();
        assert!(m.is_empty());
        assert_eq!(m.capacity(), 3);
    }

    #[test]
    fn entry_derives_are_exercised() {
        let e = entry(5, 1, -7);
        let c = e;
        assert_eq!(e, c);
        assert_ne!(e, entry(6, 1, -7));
        assert!(format!("{e:?}").contains("MemoryEntry"));
    }

    #[test]
    fn memory_derives_are_exercised() {
        let m = AgentMemory::empty_with_capacity(2);
        let c = m.clone();
        assert_eq!(c.capacity(), 2);
        assert!(format!("{m:?}").contains("AgentMemory"));
    }
}
