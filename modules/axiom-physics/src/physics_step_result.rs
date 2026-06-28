//! The dynamics outcome of a single world step.

/// The result of running one step's dynamics phases (command application,
/// broad/narrow phase, solver, integration), summarized as deterministic
/// counts. The world turns this, together with its own body/collider/event
/// counts, into the full [`crate::physics_step_record`] diagnostic record.
///
/// The counts report **real per-step work**, summed across the step's substeps:
/// `integration_count`, `broad_phase_pair_count`, `contact_pair_count`, and
/// `solved_contact_count` are actual totals the pipeline produced this step.
/// `solver_iteration_count` is the **configured** sequential-impulse iteration
/// count — diagnostic metadata, not a measure of work performed — and
/// `substep_count` is the number of substeps the step was split into.
pub(crate) struct PhysicsStepResult {
    integration_count: u32,
    broad_phase_pair_count: u32,
    contact_pair_count: u32,
    solved_contact_count: u32,
    solver_iteration_count: u32,
    substep_count: u32,
}

impl PhysicsStepResult {
    /// Aggregate the dynamics counts for one step.
    pub(crate) fn new(
        integration_count: u32,
        broad_phase_pair_count: u32,
        contact_pair_count: u32,
        solved_contact_count: u32,
        solver_iteration_count: u32,
        substep_count: u32,
    ) -> Self {
        PhysicsStepResult {
            integration_count,
            broad_phase_pair_count,
            contact_pair_count,
            solved_contact_count,
            solver_iteration_count,
            substep_count,
        }
    }

    pub(crate) fn integration_count(&self) -> u32 {
        self.integration_count
    }

    pub(crate) fn broad_phase_pair_count(&self) -> u32 {
        self.broad_phase_pair_count
    }

    pub(crate) fn contact_pair_count(&self) -> u32 {
        self.contact_pair_count
    }

    pub(crate) fn solved_contact_count(&self) -> u32 {
        self.solved_contact_count
    }

    pub(crate) fn solver_iteration_count(&self) -> u32 {
        self.solver_iteration_count
    }

    pub(crate) fn substep_count(&self) -> u32 {
        self.substep_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_and_exposes_counts() {
        let r = PhysicsStepResult::new(3, 2, 1, 1, 8, 4);
        assert_eq!(r.integration_count(), 3);
        assert_eq!(r.broad_phase_pair_count(), 2);
        assert_eq!(r.contact_pair_count(), 1);
        assert_eq!(r.solved_contact_count(), 1);
        assert_eq!(r.solver_iteration_count(), 8);
        assert_eq!(r.substep_count(), 4);
    }
}
