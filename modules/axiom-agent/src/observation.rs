//! A bounded, game-neutral observation packet.

use axiom_kernel::Tick;

use crate::agent_id::AgentId;
use crate::observation_channel::ObservationChannel;

/// One neutral, machine-readable fact in an [`Observation`].
///
/// Facts use neutral nouns only — a `kind_code` names *what kind* of fact it is
/// and a `subject_code` names *which* subject — never game nouns. Coordinates are
/// fixed-point integers (micro-units) and `value` is a generic signed magnitude
/// the app interprets. There is no enemy/door/coin/weapon/health vocabulary here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservationFact {
    kind_code: u16,
    subject_code: u32,
    x: i64,
    y: i64,
    z: i64,
    value: i64,
}

impl ObservationFact {
    /// Construct a fact from its neutral codes and fixed-point fields.
    pub const fn new(
        kind_code: u16,
        subject_code: u32,
        x: i64,
        y: i64,
        z: i64,
        value: i64,
    ) -> Self {
        ObservationFact {
            kind_code,
            subject_code,
            x,
            y,
            z,
            value,
        }
    }

    /// The fact's kind discriminant.
    pub const fn kind_code(self) -> u16 {
        self.kind_code
    }

    /// The subject this fact is about.
    pub const fn subject_code(self) -> u32 {
        self.subject_code
    }

    /// The x coordinate (micro-units).
    pub const fn x(self) -> i64 {
        self.x
    }

    /// The y coordinate (micro-units).
    pub const fn y(self) -> i64 {
        self.y
    }

    /// The z coordinate (micro-units).
    pub const fn z(self) -> i64 {
        self.z
    }

    /// The generic signed magnitude.
    pub const fn value(self) -> i64 {
        self.value
    }
}

/// A bounded packet of what one agent perceived at one tick.
///
/// It carries the perceiving agent's id, the tick, the active perception
/// channels, the codes of the actions currently *legal* for the agent, and the
/// neutral facts themselves. Every collection is an insertion-ordered `Vec`, so
/// two equal observations are byte-identical and comparison is deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    agent_id: AgentId,
    tick: Tick,
    channels: Vec<ObservationChannel>,
    legal_actions: Vec<u32>,
    facts: Vec<ObservationFact>,
}

impl Observation {
    /// An empty observation for `agent_id` at `tick` (no channels, no legal
    /// actions, no facts).
    pub fn empty(agent_id: AgentId, tick: Tick) -> Self {
        Observation {
            agent_id,
            tick,
            channels: Vec::new(),
            legal_actions: Vec::new(),
            facts: Vec::new(),
        }
    }

    /// Assemble an observation from already-bounded parts. The
    /// [`crate::AgentApi`] observation builder is the only producer, so the
    /// bounds are enforced before this is reached.
    pub(crate) fn from_parts(
        agent_id: AgentId,
        tick: Tick,
        channels: Vec<ObservationChannel>,
        legal_actions: Vec<u32>,
        facts: Vec<ObservationFact>,
    ) -> Self {
        Observation {
            agent_id,
            tick,
            channels,
            legal_actions,
            facts,
        }
    }

    /// The perceiving agent.
    pub fn agent_id(&self) -> AgentId {
        self.agent_id
    }

    /// The tick this observation is for.
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The active perception channels, in insertion order.
    pub fn channels(&self) -> &[ObservationChannel] {
        &self.channels
    }

    /// The codes of the actions currently legal for the agent.
    pub fn legal_actions(&self) -> &[u32] {
        &self.legal_actions
    }

    /// The neutral facts, in insertion order.
    pub fn facts(&self) -> &[ObservationFact] {
        &self.facts
    }

    /// The number of facts.
    pub fn fact_count(&self) -> usize {
        self.facts.len()
    }

    /// The number of legal actions.
    pub fn legal_action_count(&self) -> usize {
        self.legal_actions.len()
    }

    /// The first fact whose kind matches `kind_code`, if any. Used by the
    /// scripted brain to resolve a rule against the observation.
    pub(crate) fn first_fact_with_kind(&self, kind_code: u16) -> Option<&ObservationFact> {
        self.facts.iter().find(|fact| fact.kind_code() == kind_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> AgentId {
        AgentId::from_raw(1)
    }

    #[test]
    fn empty_observation_has_no_entries() {
        let o = Observation::empty(id(), Tick::new(5));
        assert_eq!(o.agent_id(), id());
        assert_eq!(o.tick(), Tick::new(5));
        assert!(o.channels().is_empty());
        assert!(o.legal_actions().is_empty());
        assert!(o.facts().is_empty());
        assert_eq!(o.fact_count(), 0);
        assert_eq!(o.legal_action_count(), 0);
    }

    #[test]
    fn from_parts_preserves_order_and_counts() {
        let o = Observation::from_parts(
            id(),
            Tick::new(2),
            vec![ObservationChannel::Semantic, ObservationChannel::Geometric],
            vec![10, 20, 30],
            vec![
                ObservationFact::new(100, 1, 0, 0, 0, 0),
                ObservationFact::new(200, 2, 0, 0, 0, 0),
            ],
        );
        assert_eq!(
            o.channels(),
            &[ObservationChannel::Semantic, ObservationChannel::Geometric]
        );
        assert_eq!(o.legal_actions(), &[10, 20, 30]);
        assert_eq!(o.fact_count(), 2);
        assert_eq!(o.legal_action_count(), 3);
        assert_eq!(o.facts()[1].kind_code(), 200);
    }

    #[test]
    fn first_fact_with_kind_finds_first_match_or_none() {
        let o = Observation::from_parts(
            id(),
            Tick::new(0),
            Vec::new(),
            Vec::new(),
            vec![
                ObservationFact::new(100, 1, 0, 0, 0, 0),
                ObservationFact::new(100, 2, 0, 0, 0, 0),
                ObservationFact::new(200, 3, 0, 0, 0, 0),
            ],
        );
        assert_eq!(
            o.first_fact_with_kind(100).map(|f| f.subject_code()),
            Some(1)
        );
        assert_eq!(
            o.first_fact_with_kind(200).map(|f| f.subject_code()),
            Some(3)
        );
        assert!(o.first_fact_with_kind(999).is_none());
    }

    #[test]
    fn fact_accessors_round_trip() {
        let f = ObservationFact::new(7, 8, -1, -2, -3, 99);
        assert_eq!(f.kind_code(), 7);
        assert_eq!(f.subject_code(), 8);
        assert_eq!((f.x(), f.y(), f.z()), (-1, -2, -3));
        assert_eq!(f.value(), 99);
    }

    #[test]
    fn fact_derives_are_exercised() {
        let f = ObservationFact::new(1, 1, 0, 0, 0, 0);
        let c = f;
        assert_eq!(f, c);
        assert_ne!(f, ObservationFact::new(2, 1, 0, 0, 0, 0));
        assert!(format!("{f:?}").contains("ObservationFact"));
    }

    #[test]
    fn observation_derives_are_exercised() {
        let o = Observation::empty(id(), Tick::new(0));
        let c = o.clone();
        assert_eq!(o, c);
        assert_ne!(o, Observation::empty(id(), Tick::new(1)));
        assert!(format!("{o:?}").contains("Observation"));
    }
}
