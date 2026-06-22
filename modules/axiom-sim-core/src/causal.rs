//! The causal journal: structured cause tracking for every meaningful mutation.

use std::collections::BTreeMap;

use axiom_ecs::EntityHandle;

use crate::cause::CauseRef;
use crate::fact::FactValue;
use crate::ids::CausalEventId;

/// The domain-defined *kind* of a causal event, as an opaque deterministic code.
/// sim-core attaches no narrative — this is structured cause tracking, not prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CausalEventKind(u32);

impl CausalEventKind {
    /// A causal-event kind from a deterministic code.
    pub const fn new(code: u32) -> Self {
        CausalEventKind(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// A recorded "this happened, because of that" entry.
///
/// It names the event [`kind`](Self::kind), the logical [`tick`](Self::tick), an
/// optional primary and secondary [`subject`](Self::subject), an optional
/// [`parent`](Self::parent) cause (linking it into a causal chain), a short
/// deterministic [`code`](Self::code) symbol, and an optional compact
/// [`payload`](Self::payload). Its [`CausalEventId`] is the ordering key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CausalEvent {
    id: CausalEventId,
    kind: CausalEventKind,
    tick: u64,
    subject: Option<EntityHandle>,
    secondary: Option<EntityHandle>,
    parent: Option<CauseRef>,
    code: u64,
    payload: Option<FactValue>,
}

impl CausalEvent {
    /// This event's stable id (its deterministic ordering key).
    pub const fn id(&self) -> CausalEventId {
        self.id
    }

    /// The event kind.
    pub const fn kind(&self) -> CausalEventKind {
        self.kind
    }

    /// The logical tick the event occurred on.
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// The primary subject, if any.
    pub const fn subject(&self) -> Option<EntityHandle> {
        self.subject
    }

    /// The secondary subject, if any.
    pub const fn secondary(&self) -> Option<EntityHandle> {
        self.secondary
    }

    /// The parent cause that links this event into a chain, if any.
    pub const fn parent(&self) -> Option<CauseRef> {
        self.parent
    }

    /// The short deterministic code/symbol.
    pub const fn code(&self) -> u64 {
        self.code
    }

    /// The compact payload, if any.
    pub const fn payload(&self) -> Option<FactValue> {
        self.payload
    }
}

/// A deterministic, append-only journal of causal events, keyed and iterated by
/// ascending [`CausalEventId`].
#[derive(Debug, Clone, Default)]
pub struct CausalJournal {
    events: BTreeMap<CausalEventId, CausalEvent>,
    next: u64,
}

impl CausalJournal {
    /// Create an empty journal. The first appended event has id 1.
    pub fn new() -> Self {
        CausalJournal {
            events: BTreeMap::new(),
            next: 1,
        }
    }

    /// Append a causal event, minting and returning its deterministic id.
    pub fn append(
        &mut self,
        kind: CausalEventKind,
        tick: u64,
        subject: Option<EntityHandle>,
        secondary: Option<EntityHandle>,
        parent: Option<CauseRef>,
        code: u64,
        payload: Option<FactValue>,
    ) -> CausalEventId {
        let id = CausalEventId::from_raw(self.next);
        self.next += 1;
        self.events.insert(
            id,
            CausalEvent {
                id,
                kind,
                tick,
                subject,
                secondary,
                parent,
                code,
                payload,
            },
        );
        id
    }

    /// Borrow a causal event by id, if present.
    pub fn get(&self, id: CausalEventId) -> Option<&CausalEvent> {
        self.events.get(&id)
    }

    /// Events whose primary subject is `subject`, in ascending id order.
    pub fn by_subject(&self, subject: EntityHandle) -> impl Iterator<Item = &CausalEvent> {
        self.events
            .values()
            .filter(move |event| event.subject == Some(subject))
    }

    /// Events whose parent cause is `cause`, in ascending id order.
    pub fn by_parent(&self, cause: CauseRef) -> impl Iterator<Item = &CausalEvent> {
        self.events
            .values()
            .filter(move |event| event.parent == Some(cause))
    }

    /// All events, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &CausalEvent> {
        self.events.values()
    }

    /// The number of recorded events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the journal is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProcessId;
    use axiom_ecs::EntityRegistry;

    #[test]
    fn new_and_default_are_empty() {
        assert!(CausalJournal::new().is_empty());
        assert_eq!(CausalJournal::new().len(), 0);
        assert!(CausalJournal::default().is_empty());
    }

    #[test]
    fn append_get_and_field_round_trip() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let b = reg.spawn_handle();
        let mut journal = CausalJournal::new();
        let parent = CauseRef::Process(ProcessId::from_raw(7));
        let id = journal.append(
            CausalEventKind::new(1),
            42,
            Some(a),
            Some(b),
            Some(parent),
            0xABCD,
            Some(FactValue::Unsigned(9)),
        );
        assert_eq!(id.raw(), 1);
        let event = journal.get(id).unwrap();
        assert_eq!(event.kind(), CausalEventKind::new(1));
        assert_eq!(event.tick(), 42);
        assert_eq!(event.subject(), Some(a));
        assert_eq!(event.secondary(), Some(b));
        assert_eq!(event.parent(), Some(parent));
        assert_eq!(event.code(), 0xABCD);
        assert_eq!(event.payload(), Some(FactValue::Unsigned(9)));
        assert!(journal.get(CausalEventId::from_raw(99)).is_none());
    }

    #[test]
    fn queries_by_subject_and_parent_are_ascending() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let b = reg.spawn_handle();
        let parent = CauseRef::Process(ProcessId::from_raw(1));
        let mut journal = CausalJournal::new();
        let e1 = journal.append(
            CausalEventKind::new(1),
            0,
            Some(a),
            None,
            Some(parent),
            1,
            None,
        );
        let _e2 = journal.append(CausalEventKind::new(1), 0, Some(b), None, None, 2, None);
        let e3 = journal.append(
            CausalEventKind::new(1),
            0,
            Some(a),
            None,
            Some(parent),
            3,
            None,
        );

        let by_subject: Vec<CausalEventId> = journal.by_subject(a).map(CausalEvent::id).collect();
        assert_eq!(by_subject, vec![e1, e3]);
        let by_parent: Vec<CausalEventId> =
            journal.by_parent(parent).map(CausalEvent::id).collect();
        assert_eq!(by_parent, vec![e1, e3]);
        // An unrelated cause matches nothing.
        assert_eq!(journal.by_parent(CauseRef::Command).count(), 0);
        let all: Vec<u64> = journal.iter().map(|e| e.id().raw()).collect();
        assert_eq!(all, vec![1, 2, 3]);
    }

    #[test]
    fn parent_child_chain_links_events() {
        let mut journal = CausalJournal::new();
        // A root event, then a child whose parent is the root event.
        let root = journal.append(CausalEventKind::new(1), 0, None, None, None, 1, None);
        let child = journal.append(
            CausalEventKind::new(2),
            1,
            None,
            None,
            Some(CauseRef::Event(root)),
            2,
            None,
        );
        let children: Vec<CausalEventId> = journal
            .by_parent(CauseRef::Event(root))
            .map(CausalEvent::id)
            .collect();
        assert_eq!(children, vec![child]);
    }
}
