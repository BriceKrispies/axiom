//! Deterministic per-step diagnostics.

use crate::physics_step_result::PhysicsStepResult;

/// A deterministic record of one world step, captured by [`crate::PhysicsApi`]
/// after each step and readable via `latest_step_record`.
///
/// Every field is an integer count, so the record is byte-stable and
/// replay-friendly. The counts report **real per-step work**:
/// `broad_phase_pair_count` and `contact_pair_count` are the actual candidate
/// pairs and generated contacts this step; `solved_contact_count` is the number
/// of contacts the solver actually resolved (approaching contacts that received a
/// normal impulse); `integration_count` is the number of bodies integrated; and
/// `substep_count` is how many substeps the step was split into (these four are
/// summed across substeps). `solver_iteration_count` is the **configured**
/// sequential-impulse iteration count — diagnostic metadata describing the solver
/// budget, **not** a proof that any contact was solved (use
/// `solved_contact_count` for that).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsStepRecord {
    step_index: u64,
    body_count: u32,
    collider_count: u32,
    dynamic_body_count: u32,
    command_count: u32,
    event_count: u32,
    integration_count: u32,
    broad_phase_pair_count: u32,
    contact_pair_count: u32,
    solved_contact_count: u32,
    solver_iteration_count: u32,
    substep_count: u32,
}

impl PhysicsStepRecord {
    /// The empty record, before any step has run (every count zero).
    pub(crate) fn empty() -> Self {
        PhysicsStepRecord {
            step_index: 0,
            body_count: 0,
            collider_count: 0,
            dynamic_body_count: 0,
            command_count: 0,
            event_count: 0,
            integration_count: 0,
            broad_phase_pair_count: 0,
            contact_pair_count: 0,
            solved_contact_count: 0,
            solver_iteration_count: 0,
            substep_count: 0,
        }
    }

    /// Build a step record from the world's own counts plus the dynamics
    /// [`PhysicsStepResult`] (which carries the integration / broad-phase /
    /// contact / solver counts). Folding the dynamics counts into one `result`
    /// argument keeps the constructor's arity small.
    pub(crate) fn new(
        step_index: u64,
        body_count: u32,
        collider_count: u32,
        dynamic_body_count: u32,
        command_count: u32,
        event_count: u32,
        result: &PhysicsStepResult,
    ) -> Self {
        PhysicsStepRecord {
            step_index,
            body_count,
            collider_count,
            dynamic_body_count,
            command_count,
            event_count,
            integration_count: result.integration_count(),
            broad_phase_pair_count: result.broad_phase_pair_count(),
            contact_pair_count: result.contact_pair_count(),
            solved_contact_count: result.solved_contact_count(),
            solver_iteration_count: result.solver_iteration_count(),
            substep_count: result.substep_count(),
        }
    }

    /// The completed step index (the world's step counter after this step).
    pub fn step_index(&self) -> u64 {
        self.step_index
    }

    /// The number of bodies in the world.
    pub fn body_count(&self) -> u32 {
        self.body_count
    }

    /// The number of colliders in the world.
    pub fn collider_count(&self) -> u32 {
        self.collider_count
    }

    /// The number of dynamic bodies in the world.
    pub fn dynamic_body_count(&self) -> u32 {
        self.dynamic_body_count
    }

    /// The number of commands drained and applied this step.
    pub fn command_count(&self) -> u32 {
        self.command_count
    }

    /// The number of events emitted during this step.
    pub fn event_count(&self) -> u32 {
        self.event_count
    }

    /// The number of bodies integrated (enabled dynamic bodies), summed across
    /// substeps.
    pub fn integration_count(&self) -> u32 {
        self.integration_count
    }

    /// Broad-phase candidate pairs generated this step (summed across substeps).
    pub fn broad_phase_pair_count(&self) -> u32 {
        self.broad_phase_pair_count
    }

    /// Narrow-phase contacts generated this step (summed across substeps).
    pub fn contact_pair_count(&self) -> u32 {
        self.contact_pair_count
    }

    /// Contacts the solver actually resolved this step — approaching contacts
    /// that received a normal impulse (summed across substeps). Zero when no
    /// contact was solved, regardless of `solver_iteration_count`.
    pub fn solved_contact_count(&self) -> u32 {
        self.solved_contact_count
    }

    /// The **configured** sequential-impulse iteration budget — diagnostic
    /// metadata, not a measure of work performed. A step with zero contacts still
    /// reports this configured value; see `solved_contact_count` for real work.
    pub fn solver_iteration_count(&self) -> u32 {
        self.solver_iteration_count
    }

    /// The number of substeps this step was split into (`>= 1`).
    pub fn substep_count(&self) -> u32 {
        self.substep_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_all_zero() {
        let r = PhysicsStepRecord::empty();
        assert_eq!(r.step_index(), 0);
        assert_eq!(r.body_count(), 0);
        assert_eq!(r.collider_count(), 0);
        assert_eq!(r.dynamic_body_count(), 0);
        assert_eq!(r.command_count(), 0);
        assert_eq!(r.event_count(), 0);
        assert_eq!(r.integration_count(), 0);
        assert_eq!(r.broad_phase_pair_count(), 0);
        assert_eq!(r.contact_pair_count(), 0);
        assert_eq!(r.solved_contact_count(), 0);
        assert_eq!(r.solver_iteration_count(), 0);
        assert_eq!(r.substep_count(), 0);
    }

    #[test]
    fn new_exposes_every_count() {
        let result = PhysicsStepResult::new(7, 2, 1, 1, 8, 3);
        let r = PhysicsStepRecord::new(1, 2, 3, 4, 5, 6, &result);
        assert_eq!(r.step_index(), 1);
        assert_eq!(r.body_count(), 2);
        assert_eq!(r.collider_count(), 3);
        assert_eq!(r.dynamic_body_count(), 4);
        assert_eq!(r.command_count(), 5);
        assert_eq!(r.event_count(), 6);
        assert_eq!(r.integration_count(), 7);
        assert_eq!(r.broad_phase_pair_count(), 2);
        assert_eq!(r.contact_pair_count(), 1);
        assert_eq!(r.solved_contact_count(), 1);
        assert_eq!(r.solver_iteration_count(), 8);
        assert_eq!(r.substep_count(), 3);
    }

    #[test]
    fn derives_are_exercised() {
        let result = PhysicsStepResult::new(7, 0, 0, 0, 8, 1);
        let r = PhysicsStepRecord::new(1, 2, 3, 4, 5, 6, &result);
        let c = r;
        assert_eq!(r, c);
        assert_ne!(r, PhysicsStepRecord::empty());
        assert!(format!("{r:?}").contains("PhysicsStepRecord"));
    }
}
