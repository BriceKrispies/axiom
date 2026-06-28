//! The minimal, deterministic brain contract.
//!
//! `AgentBrain` is the *internal* contract both concrete brains
//! ([`crate::scripted_brain::ScriptedBrain`] and
//! [`crate::replay_brain::ReplayBrain`]) implement. It is deliberately **not**
//! re-exported from `lib.rs`: the module has exactly one public facade
//! ([`crate::AgentApi`]). The agent runtime is generic over this trait, so brain
//! dispatch is monomorphized — there is no dynamic dispatch and no `match` over a
//! brain-kind enum.

use crate::action_intent::ActionIntent;
use crate::agent_id::AgentId;
use crate::agent_memory::AgentMemory;
use crate::agent_profile::AgentProfile;
use crate::observation::Observation;

/// What a brain produces from one observation: the intents it wants to emit (in
/// order) and a `reason_code` (a `DecisionReport::REASON_*` value) explaining the
/// outcome. The runtime queues these and builds the report; a *deciding* brain
/// (the scripted one) also clamps its own emissions to the profile's action
/// budget, whereas the replay brain reproduces its recording verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrainDecision {
    intents: Vec<ActionIntent>,
    reason_code: u16,
}

impl BrainDecision {
    /// Construct a decision from emitted intents and a reason code.
    pub fn new(intents: Vec<ActionIntent>, reason_code: u16) -> Self {
        BrainDecision {
            intents,
            reason_code,
        }
    }

    /// The intents the brain wants to emit, in order.
    pub fn intents(&self) -> &[ActionIntent] {
        &self.intents
    }

    /// The reason the brain decided as it did.
    pub fn reason_code(&self) -> u16 {
        self.reason_code
    }

    /// Consume the decision into its parts for the runtime to finalize.
    pub(crate) fn into_parts(self) -> (Vec<ActionIntent>, u16) {
        (self.intents, self.reason_code)
    }
}

/// A deterministic decision-maker for one agent.
///
/// Implementors map `(agent id, profile, observation, memory)` to a
/// [`BrainDecision`] with no clock, no randomness, and no hidden state beyond
/// their own (a replay brain advances its cursor via `&mut self`). `KIND_CODE`
/// is the stable code the runtime stamps into the decision report.
pub trait AgentBrain {
    /// The stable kind code identifying this brain in a report.
    const KIND_CODE: u16;

    /// Decide what to do given the current observation and memory.
    fn decide(
        &mut self,
        agent_id: AgentId,
        profile: AgentProfile,
        observation: &Observation,
        memory: &AgentMemory,
    ) -> BrainDecision;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_exposes_its_parts() {
        let d = BrainDecision::new(vec![ActionIntent::noop()], 2);
        assert_eq!(d.intents().len(), 1);
        assert_eq!(d.reason_code(), 2);
        let (intents, reason) = d.into_parts();
        assert_eq!(intents.len(), 1);
        assert_eq!(reason, 2);
    }

    #[test]
    fn derives_are_exercised() {
        let d = BrainDecision::new(vec![ActionIntent::noop()], 1);
        let c = d.clone();
        assert_eq!(d, c);
        assert_ne!(d, BrainDecision::new(Vec::new(), 1));
        assert!(format!("{d:?}").contains("BrainDecision"));
    }
}
