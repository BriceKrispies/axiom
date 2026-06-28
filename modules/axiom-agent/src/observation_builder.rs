//! A deterministic, bounds-enforcing builder for an [`crate::AgentApi`]
//! observation.

use axiom_kernel::{KernelError, KernelErrorCode, KernelErrorScope, KernelResult, Tick};

use crate::agent_id::AgentId;
use crate::observation::{Observation, ObservationFact};
use crate::observation_channel::ObservationChannel;

/// Accumulates an [`Observation`] under explicit capacity bounds.
///
/// Each `add_*` appends in order and rejects — deterministically, never
/// panicking — once its bound is reached, returning a kernel
/// [`KernelErrorCode::OutOfBounds`] error. Insertion order is preserved, so the
/// built observation is byte-identical for the same call sequence.
#[derive(Debug, Clone)]
pub struct ObservationBuilder {
    agent_id: AgentId,
    tick: Tick,
    max_channels: usize,
    max_facts: usize,
    max_legal_actions: usize,
    channels: Vec<ObservationChannel>,
    legal_actions: Vec<u32>,
    facts: Vec<ObservationFact>,
}

impl ObservationBuilder {
    /// A builder for `agent_id` at `tick`, bounded to at most `max_channels`
    /// channels, `max_facts` facts, and `max_legal_actions` legal actions.
    pub fn new(
        agent_id: AgentId,
        tick: Tick,
        max_channels: usize,
        max_facts: usize,
        max_legal_actions: usize,
    ) -> Self {
        ObservationBuilder {
            agent_id,
            tick,
            max_channels,
            max_facts,
            max_legal_actions,
            channels: Vec::new(),
            legal_actions: Vec::new(),
            facts: Vec::new(),
        }
    }

    /// The deterministic capacity-exceeded error, identical for every overflow.
    fn capacity_error() -> KernelError {
        KernelError::new(
            KernelErrorScope::Memory,
            KernelErrorCode::OutOfBounds,
            "observation builder capacity exceeded",
        )
    }

    /// Append a perception channel, or fail if the channel bound is reached.
    pub fn add_channel(&mut self, channel: ObservationChannel) -> KernelResult<()> {
        let room = self.channels.len() < self.max_channels;
        room.then(|| self.channels.push(channel))
            .ok_or_else(Self::capacity_error)
    }

    /// Append a legal-action code, or fail if the legal-action bound is reached.
    pub fn add_legal_action(&mut self, action_code: u32) -> KernelResult<()> {
        let room = self.legal_actions.len() < self.max_legal_actions;
        room.then(|| self.legal_actions.push(action_code))
            .ok_or_else(Self::capacity_error)
    }

    /// Append a fact, or fail if the fact bound is reached.
    pub fn add_fact(&mut self, fact: ObservationFact) -> KernelResult<()> {
        let room = self.facts.len() < self.max_facts;
        room.then(|| self.facts.push(fact))
            .ok_or_else(Self::capacity_error)
    }

    /// Finish building. Always succeeds — every entry was bounds-checked on the
    /// way in.
    pub fn build(self) -> Observation {
        Observation::from_parts(
            self.agent_id,
            self.tick,
            self.channels,
            self.legal_actions,
            self.facts,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder(max_channels: usize, max_facts: usize, max_legal: usize) -> ObservationBuilder {
        ObservationBuilder::new(AgentId::from_raw(1), Tick::new(3), max_channels, max_facts, max_legal)
    }

    #[test]
    fn preserves_channel_fact_and_action_order() {
        let mut b = builder(2, 2, 3);
        b.add_channel(ObservationChannel::Semantic).unwrap();
        b.add_channel(ObservationChannel::Replay).unwrap();
        b.add_legal_action(10).unwrap();
        b.add_legal_action(20).unwrap();
        b.add_legal_action(30).unwrap();
        b.add_fact(ObservationFact::new(100, 1, 0, 0, 0, 0)).unwrap();
        b.add_fact(ObservationFact::new(200, 2, 0, 0, 0, 0)).unwrap();
        let o = b.build();
        assert_eq!(o.channels(), &[ObservationChannel::Semantic, ObservationChannel::Replay]);
        assert_eq!(o.legal_actions(), &[10, 20, 30]);
        assert_eq!(o.facts()[0].kind_code(), 100);
        assert_eq!(o.facts()[1].kind_code(), 200);
        assert_eq!(o.tick(), Tick::new(3));
    }

    #[test]
    fn channel_overflow_fails_deterministically() {
        let mut b = builder(1, 1, 1);
        assert!(b.add_channel(ObservationChannel::Semantic).is_ok());
        let err = b.add_channel(ObservationChannel::Debug).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::OutOfBounds);
        assert_eq!(err.scope(), KernelErrorScope::Memory);
    }

    #[test]
    fn legal_action_overflow_fails_deterministically() {
        let mut b = builder(1, 1, 1);
        assert!(b.add_legal_action(10).is_ok());
        assert_eq!(b.add_legal_action(20).unwrap_err().code(), KernelErrorCode::OutOfBounds);
    }

    #[test]
    fn fact_overflow_fails_deterministically() {
        let mut b = builder(1, 1, 1);
        assert!(b.add_fact(ObservationFact::new(1, 1, 0, 0, 0, 0)).is_ok());
        assert_eq!(
            b.add_fact(ObservationFact::new(2, 2, 0, 0, 0, 0)).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn derives_are_exercised() {
        let b = builder(1, 1, 1);
        let c = b.clone();
        assert!(format!("{b:?}").contains("ObservationBuilder"));
        assert_eq!(c.build().fact_count(), 0);
    }
}
