//! [`MotionPhase`] — one authored span of a motion, and [`ResolvedPhase`], its
//! name-resolved, sample-ready form.
//!
//! A phase owns a tick span `[start, end)`, a root-motion command, an ordered set
//! of pose goals, constraints, and contact declarations, an ease curve shaping
//! its progress, and a layer weight scaling how strongly its goals apply.

use axiom_kernel::Tick;

use crate::constraint::{Constraint, ResolvedConstraint};
use crate::contact::{ContactDeclaration, ResolvedContact};
use crate::ease::EaseCurve;
use crate::pose_goal::{PoseGoal, ResolvedGoal};
use crate::root_motion::{ResolvedRootMotion, RootMotion};

/// One authored phase of a motion.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionPhase {
    name: String,
    start: Tick,
    end: Tick,
    root: RootMotion,
    goals: Vec<PoseGoal>,
    constraints: Vec<Constraint>,
    contacts: Vec<ContactDeclaration>,
    ease: EaseCurve,
    layer_weight: f32,
}

impl MotionPhase {
    /// A phase named `name` spanning `[start, end)`, holding root position, with a
    /// linear ease and full layer weight.
    pub(crate) fn new(name: &str, start: Tick, end: Tick) -> Self {
        MotionPhase {
            name: name.to_string(),
            start,
            end,
            root: RootMotion::hold(),
            goals: Vec::new(),
            constraints: Vec::new(),
            contacts: Vec::new(),
            ease: EaseCurve::Linear,
            layer_weight: 1.0,
        }
    }

    /// Set the root-motion command.
    pub(crate) fn set_root(&mut self, root: RootMotion) {
        self.root = root;
    }

    /// Set the ease curve.
    pub(crate) fn set_ease(&mut self, ease: EaseCurve) {
        self.ease = ease;
    }

    /// Set the layer weight.
    pub(crate) fn set_layer_weight(&mut self, weight: f32) {
        self.layer_weight = weight;
    }

    /// Append a pose goal.
    pub(crate) fn push_goal(&mut self, goal: PoseGoal) {
        self.goals.push(goal);
    }

    /// Append a constraint.
    pub(crate) fn push_constraint(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }

    /// Append a contact declaration.
    pub(crate) fn push_contact(&mut self, contact: ContactDeclaration) {
        self.contacts.push(contact);
    }

    /// The phase name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The start tick (inclusive).
    pub(crate) fn start(&self) -> Tick {
        self.start
    }

    /// The end tick (exclusive).
    pub(crate) fn end(&self) -> Tick {
        self.end
    }

    /// The root-motion command.
    pub(crate) fn root(&self) -> &RootMotion {
        &self.root
    }

    /// The pose goals in order.
    pub(crate) fn goals(&self) -> &[PoseGoal] {
        &self.goals
    }

    /// The constraints in order.
    pub(crate) fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// The contact declarations in order.
    pub(crate) fn contacts(&self) -> &[ContactDeclaration] {
        &self.contacts
    }

    /// The ease curve.
    pub(crate) fn ease(&self) -> EaseCurve {
        self.ease
    }

    /// The layer weight.
    pub(crate) fn layer_weight(&self) -> f32 {
        self.layer_weight
    }
}

/// A resolved phase: names replaced by ids/positions, ready to sample.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPhase {
    name: String,
    start: u64,
    end: u64,
    root: ResolvedRootMotion,
    goals: Vec<ResolvedGoal>,
    constraints: Vec<ResolvedConstraint>,
    contacts: Vec<ResolvedContact>,
    ease: EaseCurve,
    layer_weight: f32,
}

impl ResolvedPhase {
    /// Construct a resolved phase. `header` is `(name, start, end)` — the half-open
    /// tick span plus the phase name (bundled to keep the argument list small).
    pub(crate) fn new(
        header: (String, u64, u64),
        root: ResolvedRootMotion,
        goals: Vec<ResolvedGoal>,
        constraints: Vec<ResolvedConstraint>,
        contacts: Vec<ResolvedContact>,
        ease: EaseCurve,
        layer_weight: f32,
    ) -> Self {
        ResolvedPhase {
            name: header.0,
            start: header.1,
            end: header.2,
            root,
            goals,
            constraints,
            contacts,
            ease,
            layer_weight,
        }
    }

    /// The phase name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The start tick (inclusive).
    pub(crate) fn start(&self) -> u64 {
        self.start
    }

    /// The end tick (exclusive). Used to carry the running root past completed
    /// phases during sampling.
    pub(crate) fn end(&self) -> u64 {
        self.end
    }

    /// The root-motion command.
    pub(crate) fn root(&self) -> ResolvedRootMotion {
        self.root
    }

    /// The resolved pose goals.
    pub(crate) fn goals(&self) -> &[ResolvedGoal] {
        &self.goals
    }

    /// The resolved constraints.
    pub(crate) fn constraints(&self) -> &[ResolvedConstraint] {
        &self.constraints
    }

    /// The resolved contacts.
    pub(crate) fn contacts(&self) -> &[ResolvedContact] {
        &self.contacts
    }

    /// The layer weight, clamped to `[0, 1]`.
    pub(crate) fn layer_weight(&self) -> f32 {
        self.layer_weight.clamp(0.0, 1.0)
    }

    /// Whether `tick` falls in this phase's `[start, end)` span.
    pub(crate) fn covers(&self, tick: u64) -> bool {
        (tick >= self.start) & (tick < self.end)
    }

    /// The phase's **raw** linear progress at `tick`: normalized `[0, 1]`
    /// `(tick - start) / span`, *without* the ease curve or the layer weight. This
    /// is the even clock a [`crate::pose_goal::GoalKind::RunCycle`] oscillates on, so
    /// a gait's steps stay uniform (an eased/weighted clock would bunch or shrink
    /// them). The span is floored at one tick so a zero-length span never divides by
    /// zero (it reads progress `0` at its start tick).
    pub(crate) fn progress(&self, tick: u64) -> f32 {
        let span = self.end.saturating_sub(self.start).max(1) as f32;
        tick.saturating_sub(self.start) as f32 / span
    }

    /// The phase's eased progress at `tick`: its normalized `[0, 1]` progress
    /// reshaped by its ease curve, *without* the layer weight. Root motion
    /// interpolates on this. The span is floored at one tick so a zero-length span
    /// never divides by zero (it reads progress `0` at its start tick).
    pub(crate) fn eased_progress(&self, tick: u64) -> f32 {
        self.ease.apply(self.progress(tick))
    }

    /// The eased, weight-scaled application strength at `tick`: the eased progress
    /// scaled by the (clamped) layer weight. Pose goals apply at this strength.
    pub(crate) fn strength(&self, tick: u64) -> f32 {
        self.eased_progress(tick) * self.layer_weight()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root_motion::RootMotionKind;
    use axiom_math::Vec3;

    #[test]
    fn authored_phase_defaults_then_accepts_mutations() {
        let mut p = MotionPhase::new("backswing", Tick::new(10), Tick::new(20));
        assert_eq!(p.name(), "backswing");
        assert_eq!(p.start(), Tick::new(10));
        assert_eq!(p.end(), Tick::new(20));
        assert_eq!(p.ease(), EaseCurve::Linear);
        assert_eq!(p.layer_weight(), 1.0);
        assert_eq!(p.root().kind(), RootMotionKind::Hold);
        assert!(p.goals().is_empty());
        assert!(p.constraints().is_empty());
        assert!(p.contacts().is_empty());

        p.set_root(RootMotion::move_toward("a", "b"));
        p.set_ease(EaseCurve::SmoothStep);
        p.set_layer_weight(0.5);
        p.push_goal(PoseGoal::leg_backswing(true, 0.8));
        p.push_constraint(Constraint::keep_gaze_on_target("ball"));
        p.push_contact(ContactDeclaration::new("left_foot_sole", "left_plant_spot"));
        assert_eq!(p.root().kind(), RootMotionKind::MoveToward);
        assert_eq!(p.ease(), EaseCurve::SmoothStep);
        assert_eq!(p.layer_weight(), 0.5);
        assert_eq!(p.goals().len(), 1);
        assert_eq!(p.constraints().len(), 1);
        assert_eq!(p.contacts().len(), 1);
    }

    fn resolved(start: u64, end: u64, ease: EaseCurve, weight: f32) -> ResolvedPhase {
        ResolvedPhase::new(
            ("test".to_string(), start, end),
            ResolvedRootMotion::new(RootMotionKind::Hold, Vec3::ZERO, Vec3::ZERO),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ease,
            weight,
        )
    }

    #[test]
    fn resolved_phase_covers_its_half_open_span() {
        let p = resolved(10, 20, EaseCurve::Linear, 1.0);
        assert!(!p.covers(9));
        assert!(p.covers(10));
        assert!(p.covers(19));
        assert!(!p.covers(20));
        assert_eq!(p.name(), "test");
        assert_eq!(p.start(), 10);
        assert_eq!(p.end(), 20);
        assert!(p.goals().is_empty());
        assert!(p.constraints().is_empty());
        assert!(p.contacts().is_empty());
    }

    #[test]
    fn strength_is_eased_progress_times_clamped_weight() {
        // Linear, full weight: strength == progress.
        let p = resolved(0, 10, EaseCurve::Linear, 1.0);
        assert!((p.strength(0) - 0.0).abs() < 1.0e-6);
        assert!((p.strength(5) - 0.5).abs() < 1.0e-6);
        // Half weight halves the strength but not the eased progress.
        let half = resolved(0, 10, EaseCurve::Linear, 0.5);
        assert!((half.strength(10) - 0.5).abs() < 1.0e-6);
        assert!((half.eased_progress(10) - 1.0).abs() < 1.0e-6);
        // Weight clamps into [0, 1].
        let over = resolved(0, 10, EaseCurve::Linear, 4.0);
        assert!((over.layer_weight() - 1.0).abs() < 1.0e-6);
        // A zero-length span floors its span at 1 (no divide-by-zero) and reads
        // progress 0 at its start tick.
        let degenerate = resolved(5, 5, EaseCurve::Linear, 1.0);
        assert!((degenerate.strength(5) - 0.0).abs() < 1.0e-6);
    }

    #[test]
    fn raw_progress_is_linear_and_independent_of_the_ease_curve() {
        // Raw progress is (tick-start)/span regardless of the ease; a SmoothStep
        // phase's eased progress differs from its raw progress at the midpoint.
        let p = resolved(0, 10, EaseCurve::SmoothStep, 1.0);
        assert!((p.progress(0) - 0.0).abs() < 1.0e-6);
        assert!((p.progress(5) - 0.5).abs() < 1.0e-6);
        assert!((p.progress(10) - 1.0).abs() < 1.0e-6);
        // SmoothStep(0.5) == 0.5, so pick a quarter point where they diverge.
        assert!((p.progress(2) - 0.2).abs() < 1.0e-6);
        assert!(p.eased_progress(2) < p.progress(2), "smoothstep eases in below linear early");
        // A zero-length span floors at 1 and reads 0 at the start.
        assert!((resolved(5, 5, EaseCurve::Linear, 1.0).progress(5) - 0.0).abs() < 1.0e-6);
    }
}
