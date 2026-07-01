//! The generic process dependency / subscription model.
//!
//! A process subscribes to *kinds of change* (a fact kind changing, a subject
//! changing, …) keyed by a `u64` selector. When the dirty set reports a matching
//! change, the scheduler can wake the subscribers. Subscriptions are deduplicated
//! deterministically.

use std::collections::{BTreeMap, BTreeSet};

use crate::ids::ProcessId;

/// The category of change a process depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DependencyKind {
    /// A fact of a given kind changed (key = fact kind code).
    FactKindChanged,
    /// A relation of a given kind changed (key = relation kind code).
    RelationKindChanged,
    /// A subject changed (key = entity slot raw id).
    SubjectChanged,
    /// A definition changed (key = definition id raw).
    DefinitionChanged,
    /// A residue changed (key = residue id raw).
    ResidueChanged,
    /// A body surface changed (key = surface id raw).
    BodySurfaceChanged,
    /// A wound changed (key = wound id raw).
    WoundChanged,
    /// An explicit dependency on another process (key = process id raw).
    ExplicitProcess,
    /// A generic dependency (key = caller-defined).
    Generic,
}

const DEPENDENCY_KINDS: [DependencyKind; 9] = [
    DependencyKind::FactKindChanged,
    DependencyKind::RelationKindChanged,
    DependencyKind::SubjectChanged,
    DependencyKind::DefinitionChanged,
    DependencyKind::ResidueChanged,
    DependencyKind::BodySurfaceChanged,
    DependencyKind::WoundChanged,
    DependencyKind::ExplicitProcess,
    DependencyKind::Generic,
];

impl DependencyKind {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<DependencyKind> {
        DEPENDENCY_KINDS.get(code as usize).copied()
    }

    /// The kind's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// A single dependency: a change category plus a `u64` selector key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessDependency {
    kind: DependencyKind,
    key: u64,
}

impl ProcessDependency {
    /// A dependency on `kind` changes selected by `key`.
    pub const fn new(kind: DependencyKind, key: u64) -> Self {
        ProcessDependency { kind, key }
    }
    /// The dependency kind.
    pub const fn kind(self) -> DependencyKind {
        self.kind
    }
    /// The selector key.
    pub const fn key(self) -> u64 {
        self.key
    }

    /// The ordered map key: `(kind code, selector)`.
    fn ordering_key(self) -> (u8, u64) {
        (self.kind.code(), self.key)
    }
}

/// One process's subscription to a dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessSubscription {
    process: ProcessId,
    dependency: ProcessDependency,
}

impl ProcessSubscription {
    /// The subscribing process.
    pub const fn process(&self) -> ProcessId {
        self.process
    }
    /// The dependency subscribed to.
    pub const fn dependency(&self) -> ProcessDependency {
        self.dependency
    }
}

/// The set of all process→dependency subscriptions, indexed both ways.
#[derive(Debug, Clone, Default)]
pub struct DependencySet {
    by_process: BTreeMap<ProcessId, BTreeSet<(u8, u64)>>,
    by_dependency: BTreeMap<(u8, u64), BTreeSet<ProcessId>>,
}

impl DependencySet {
    /// Create an empty dependency set.
    pub fn new() -> Self {
        DependencySet {
            by_process: BTreeMap::new(),
            by_dependency: BTreeMap::new(),
        }
    }

    /// Subscribe `process` to `dependency`. Returns `true` if newly added, `false`
    /// if it was already subscribed (deterministic dedup).
    pub fn subscribe(&mut self, process: ProcessId, dependency: ProcessDependency) -> bool {
        let key = dependency.ordering_key();
        let added = self.by_process.entry(process).or_default().insert(key);
        added.then(|| self.by_dependency.entry(key).or_default().insert(process));
        added
    }

    /// A process's dependencies, ascending by `(kind, key)`.
    pub fn dependencies_of(&self, process: ProcessId) -> Vec<ProcessDependency> {
        self.by_process
            .get(&process)
            .map(|set| {
                set.iter()
                    .filter_map(|(code, key)| {
                        DependencyKind::from_code(*code)
                            .map(|kind| ProcessDependency::new(kind, *key))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The processes subscribed to a dependency, ascending by id.
    pub fn subscribers_of(&self, dependency: ProcessDependency) -> Vec<ProcessId> {
        self.by_dependency
            .get(&dependency.ordering_key())
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// A process's subscriptions, ascending by `(kind, key)`.
    pub fn subscriptions_of(&self, process: ProcessId) -> Vec<ProcessSubscription> {
        self.by_process
            .get(&process)
            .map(|set| {
                set.iter()
                    .filter_map(|(code, key)| {
                        DependencyKind::from_code(*code).map(|kind| ProcessSubscription {
                            process,
                            dependency: ProcessDependency::new(kind, *key),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The number of distinct (process, dependency) subscriptions.
    pub fn len(&self) -> usize {
        self.by_process.values().map(BTreeSet::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(raw: u64) -> ProcessId {
        ProcessId::from_raw(raw)
    }

    #[test]
    fn dependency_kind_codes_round_trip() {
        assert_eq!(
            DependencyKind::from_code(0),
            Some(DependencyKind::FactKindChanged)
        );
        assert_eq!(DependencyKind::from_code(8), Some(DependencyKind::Generic));
        assert_eq!(DependencyKind::from_code(9), None);
        assert_eq!(DependencyKind::SubjectChanged.code(), 2);
    }

    #[test]
    fn subscribe_dedups_and_indexes_both_ways() {
        let mut set = DependencySet::new();
        assert_eq!(set.len(), 0);
        let fact_dep = ProcessDependency::new(DependencyKind::FactKindChanged, 7);
        assert!(set.subscribe(p(1), fact_dep));
        assert!(!set.subscribe(p(1), fact_dep));
        assert!(set.subscribe(p(2), fact_dep));
        assert!(set.subscribe(
            p(1),
            ProcessDependency::new(DependencyKind::SubjectChanged, 3)
        ));
        assert_eq!(set.len(), 3);
        assert_eq!(set.subscribers_of(fact_dep), vec![p(1), p(2)]);
        assert_eq!(
            set.subscribers_of(ProcessDependency::new(DependencyKind::WoundChanged, 0))
                .len(),
            0
        );
        let deps = set.dependencies_of(p(1));
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].kind(), DependencyKind::FactKindChanged);
        assert_eq!(deps[0].key(), 7);
        assert_eq!(deps[1].kind(), DependencyKind::SubjectChanged);
        assert!(set.dependencies_of(p(9)).is_empty());
    }

    #[test]
    fn subscription_record_carries_fields() {
        let dep = ProcessDependency::new(DependencyKind::ResidueChanged, 4);
        let sub = ProcessSubscription {
            process: p(5),
            dependency: dep,
        };
        assert_eq!(sub.process(), p(5));
        assert_eq!(sub.dependency(), dep);
    }
}
