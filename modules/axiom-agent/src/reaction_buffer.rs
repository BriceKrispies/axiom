//! **Reaction latency** — the delay between an agent seeing something and being
//! able to act on it.
//!
//! [`AgentProfile`](crate::AgentApi) has carried a `reaction_delay_ticks` since
//! the profile was written, documented as "the stable contract a later stage
//! will honor". Nothing honored it: an agent perceived the world at tick *N* and
//! decided on it at tick *N*, which is a reflex no human has. This is that
//! stage.
//!
//! ## The model
//!
//! It is a **delay line**, not a filter. Every tick the app pushes what the
//! agent can currently see; the brain is then handed what the agent could see
//! `reaction_delay_ticks` ago. Nothing is smoothed, dropped, or invented — the
//! agent acts on a true observation, just an old one — so a decision remains
//! exactly reproducible and a replay stays bit-identical.
//!
//! That is the honest shape of human latency and it has the right consequences
//! for free: an agent with a 500 ms delay will still be turning toward where a
//! threat *was* half a second after it moved, will miss a window that opened and
//! closed inside its delay, and will over-commit to something that has already
//! stopped being true. None of that has to be modelled; it falls out of being
//! late.
//!
//! ## Why a buffer the caller holds
//!
//! `AgentRuntime::step` is a stateless orchestrator, and the module keeps its
//! state in explicit objects the caller owns (a memory, a brain). Latency is
//! state — it is a history — so it is one more such object rather than a hidden
//! field inside the runtime. The caller pushes, the caller reads back, and the
//! delay is visible in the call rather than buried in it.
//!
//! ## Bounded, like everything else here
//!
//! The ring is fixed at construction and never grows. A delay longer than the
//! ring is clamped to the oldest frame remembered rather than failing or
//! allocating: an agent that cannot remember far enough back reacts as slowly as
//! it can, which is the safe direction to be wrong in.

use crate::agent_profile::AgentProfile;
use crate::observation::Observation;

/// A bounded, insertion-ordered delay line of observations.
///
/// Cloneable and comparable so a test can assert on the whole history, and so a
/// caller may snapshot one without reaching inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionBuffer {
    ring: Vec<Observation>,
    head: usize,
    filled: usize,
}

impl ReactionBuffer {
    /// A buffer of `capacity` frames, every slot seeded with `seed`.
    ///
    /// Seeded rather than empty so that reading before anything has been pushed
    /// yields a real observation instead of an `Option` every caller would have
    /// to unwrap. A capacity of `0` is raised to `1`: a delay line has to hold
    /// at least the present.
    pub fn seeded(capacity: usize, seed: Observation) -> Self {
        let capacity = capacity.max(1);
        ReactionBuffer {
            ring: core::iter::repeat_n(seed, capacity).collect(),
            head: 0,
            filled: 0,
        }
    }

    /// How many frames the line can hold.
    pub fn capacity(&self) -> usize {
        self.ring.len()
    }

    /// How many frames have been pushed, saturating at the capacity.
    pub fn filled(&self) -> usize {
        self.filled
    }

    /// Record what the agent can see this tick.
    pub fn perceive(&mut self, observation: Observation) {
        self.head = (self.head + 1) % self.ring.len();
        self.ring[self.head] = observation;
        self.filled = (self.filled + 1).min(self.ring.len());
    }

    /// What the agent may act on now: the frame `delay_ticks` ago, clamped to
    /// the oldest one remembered.
    pub fn delayed(&self, delay_ticks: u32) -> &Observation {
        let reachable = self.filled.saturating_sub(1).min(self.ring.len() - 1);
        let clamped = (delay_ticks as usize).min(reachable);
        let index = (self.head + self.ring.len() - clamped) % self.ring.len();
        &self.ring[index]
    }

    /// What the agent may act on now under `profile`'s reaction delay — the
    /// form a caller actually wants, so the delay cannot be read from one place
    /// and applied from another.
    pub fn reacted(&self, profile: AgentProfile) -> &Observation {
        self.delayed(profile.reaction_delay_ticks())
    }
}

/// Convert a human-scale reaction time in **milliseconds** into the module's
/// ticks, given the fixed step's delta in nanoseconds.
///
/// Milliseconds are the unit reaction time is actually discussed in — a person
/// is "about 250 ms on a visual cue", not "about fifteen ticks" — and the tick
/// rate is a property of the runtime rather than of the human. Rounding is
/// nearest rather than truncating so a delay does not quietly come out short,
/// and the result is at least one tick for any non-zero request: asking for
/// latency and getting none is the one answer that would be wrong.
pub fn ticks_for_millis(millis: u32, step_delta_nanos: u64) -> u32 {
    let nanos = u64::from(millis) * 1_000_000;
    let delta = step_delta_nanos.max(1);
    let ticks = (nanos + delta / 2) / delta;
    let floor = u64::from(u32::from(millis > 0));
    ticks.max(floor).min(u64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_id::AgentId;
    use axiom_kernel::Tick;

    fn frame(tick: u64) -> Observation {
        Observation::empty(AgentId::from_raw(1), Tick::new(tick))
    }

    #[test]
    fn a_zero_capacity_buffer_still_holds_the_present() {
        let buffer = ReactionBuffer::seeded(0, frame(0));
        assert_eq!(buffer.capacity(), 1);
        assert_eq!(buffer.filled(), 0);
    }

    #[test]
    fn an_unpushed_buffer_reads_back_its_seed() {
        let buffer = ReactionBuffer::seeded(4, frame(7));
        assert_eq!(buffer.delayed(0), &frame(7));
        assert_eq!(buffer.delayed(3), &frame(7));
    }

    #[test]
    fn zero_delay_reads_the_newest_frame() {
        let mut buffer = ReactionBuffer::seeded(4, frame(0));
        buffer.perceive(frame(1));
        buffer.perceive(frame(2));
        assert_eq!(buffer.delayed(0), &frame(2));
        assert_eq!(buffer.filled(), 2);
    }

    #[test]
    fn a_delay_reads_the_frame_that_many_ticks_ago() {
        let mut buffer = ReactionBuffer::seeded(8, frame(0));
        (1..=5).for_each(|tick| buffer.perceive(frame(tick)));
        assert_eq!(buffer.delayed(0), &frame(5));
        assert_eq!(buffer.delayed(1), &frame(4));
        assert_eq!(buffer.delayed(4), &frame(1));
    }

    #[test]
    fn a_delay_longer_than_the_history_clamps_to_the_oldest_remembered() {
        let mut buffer = ReactionBuffer::seeded(8, frame(0));
        buffer.perceive(frame(1));
        buffer.perceive(frame(2));
        // Only two frames exist, so a 60-tick delay is as old as it can be.
        assert_eq!(buffer.delayed(60), &frame(1));
    }

    #[test]
    fn the_ring_wraps_and_forgets_the_oldest() {
        let mut buffer = ReactionBuffer::seeded(3, frame(0));
        (1..=5).for_each(|tick| buffer.perceive(frame(tick)));
        assert_eq!(buffer.filled(), 3);
        assert_eq!(buffer.delayed(0), &frame(5));
        assert_eq!(buffer.delayed(2), &frame(3));
        // Beyond the ring, the oldest slot still standing.
        assert_eq!(buffer.delayed(9), &frame(3));
    }

    #[test]
    fn reacted_applies_the_profiles_own_delay() {
        let mut buffer = ReactionBuffer::seeded(64, frame(0));
        (1..=40).for_each(|tick| buffer.perceive(frame(tick)));
        let perfect = AgentProfile::debug_perfect();
        assert_eq!(perfect.reaction_delay_ticks(), 0);
        assert_eq!(buffer.reacted(perfect), &frame(40));

        let human = AgentProfile::human_like_default();
        assert_eq!(human.reaction_delay_ticks(), 12);
        assert_eq!(buffer.reacted(human), &frame(28));
    }

    #[test]
    fn milliseconds_convert_to_ticks_at_the_steps_rate() {
        // The engine's fixed 60 Hz step.
        let step = 16_666_667u64;
        assert_eq!(ticks_for_millis(0, step), 0);
        assert_eq!(ticks_for_millis(500, step), 30);
        assert_eq!(ticks_for_millis(250, step), 15);
        // Nearest, not truncating: 100 ms is 6.0 ticks.
        assert_eq!(ticks_for_millis(100, step), 6);
        // Any non-zero request buys at least one tick of lateness.
        assert_eq!(ticks_for_millis(1, step), 1);
        // A degenerate zero-delta step cannot divide by zero.
        assert_eq!(ticks_for_millis(1, 0), 1_000_000);
    }

    #[test]
    fn derives_are_exercised() {
        let buffer = ReactionBuffer::seeded(2, frame(0));
        let copy = buffer.clone();
        assert_eq!(buffer, copy);
        assert!(format!("{buffer:?}").contains("ReactionBuffer"));
        let mut other = ReactionBuffer::seeded(2, frame(0));
        other.perceive(frame(9));
        assert_ne!(buffer, other);
    }
}
