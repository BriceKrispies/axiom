//! The single public facade for the animation-authoring module.

use axiom_kernel::{Ratio, Tick};
use axiom_math::{Transform, Vec3};

use crate::authoring_error::AuthoringError;
use crate::authoring_result::AuthoringResult;
use crate::constraint::Constraint;
use crate::contact::ContactDeclaration;
use crate::ease::EaseCurve;
use crate::humanoid_rig::HumanoidRigSpec;
use crate::ids::{EffectorId, JointId, MotionId, PhaseId, PlanId, RigId, TargetId};
use crate::motion_compiler::MotionCompiler;
use crate::motion_event::MotionEvent;
use crate::motion_phase::MotionPhase;
use crate::motion_plan::MotionPlan;
use crate::motion_sampler::MotionSampler;
use crate::motion_spec::MotionSpec;
use crate::physical_objective;
use crate::pose_frame::PoseFrame;
use crate::pose_goal::PoseGoal;
use crate::root_motion::RootMotion;

/// The deterministic procedural-animation authoring facade — the only behavioral
/// type in the module. Rigs, motions and compiled plans are registered here and
/// referred to by [`RigId`] / [`MotionId`] / [`PlanId`]; motions are authored
/// phase-by-phase, compiled with [`AnimationAuthoringApi::compile`], and sampled
/// with [`AnimationAuthoringApi::sample`] into a [`PoseFrame`] read back through
/// the `frame_*` accessors. Every scalar crosses the boundary as a value type
/// ([`Tick`], [`Ratio`], [`Vec3`], [`Transform`]) — never a naked float — and
/// every fallible call returns an [`AuthoringError`] rather than panicking.
#[derive(Debug, Default)]
pub struct AnimationAuthoringApi {
    rigs: Vec<HumanoidRigSpec>,
    motions: Vec<MotionSpec>,
    plans: Vec<MotionPlan>,
}

impl AnimationAuthoringApi {
    /// An empty registry.
    pub fn new() -> Self {
        AnimationAuthoringApi {
            rigs: Vec::new(),
            motions: Vec::new(),
            plans: Vec::new(),
        }
    }

    // --- rigs ----------------------------------------------------------------

    /// Register the standard PS1/low-poly humanoid rig and return its handle.
    pub fn standard_humanoid(&mut self) -> RigId {
        let id = RigId::from_raw(self.rigs.len() as u64);
        self.rigs.push(HumanoidRigSpec::standard_humanoid());
        id
    }

    /// The joint names of `rig`, in order.
    pub fn joint_names(&self, rig: RigId) -> AuthoringResult<Vec<&'static str>> {
        self.rig(rig).map(HumanoidRigSpec::joint_names)
    }

    /// The effector names of `rig`, in order.
    pub fn effector_names(&self, rig: RigId) -> AuthoringResult<Vec<&'static str>> {
        self.rig(rig).map(HumanoidRigSpec::effector_names)
    }

    /// Whether `rig`'s joint hierarchy is valid (every parent precedes its child).
    pub fn rig_is_valid(&self, rig: RigId) -> AuthoringResult<bool> {
        self.rig(rig).map(HumanoidRigSpec::is_valid)
    }

    /// The id of the joint named `name` in `rig` (or `None` if absent), so a
    /// caller can translate a name into the [`JointId`] the `frame_*` readers take.
    pub fn joint_id(&self, rig: RigId, name: &str) -> AuthoringResult<Option<JointId>> {
        self.rig(rig).map(|r| r.joint_id(name))
    }

    /// The id of the effector named `name` in `rig` (or `None` if absent).
    pub fn effector_id(&self, rig: RigId, name: &str) -> AuthoringResult<Option<EffectorId>> {
        self.rig(rig).map(|r| r.effector_id(name))
    }

    // --- motion authoring ----------------------------------------------------

    /// Create a motion `name` of `duration` ticks driving `rig`, returning its id.
    pub fn create_motion(&mut self, name: &str, duration: Tick, rig: RigId) -> AuthoringResult<MotionId> {
        // Confirm the rig exists (releasing the shared borrow) before registering.
        let exists = self.rig(rig).map(|_| ());
        exists.map(|()| {
            let id = MotionId::from_raw(self.motions.len() as u64);
            self.motions.push(MotionSpec::new(name, duration, rig));
            id
        })
    }

    /// Declare a target `name` at `position` on `motion`.
    pub fn add_target(&mut self, motion: MotionId, name: &str, position: Vec3) -> AuthoringResult<TargetId> {
        self.motion_mut(motion).map(|m| m.add_target(name, position))
    }

    /// Set style scalar `name` on `motion` to `value`.
    pub fn set_style(&mut self, motion: MotionId, name: &str, value: Ratio) -> AuthoringResult<()> {
        self.motion_mut(motion).map(|m| m.set_style(name, value.get()))
    }

    /// Append a phase `name` spanning `[start, end)` to `motion`, returning its id.
    pub fn add_phase(&mut self, motion: MotionId, name: &str, start: Tick, end: Tick) -> AuthoringResult<PhaseId> {
        self.motion_mut(motion)
            .map(|m| PhaseId::new(motion, m.add_phase(MotionPhase::new(name, start, end))))
    }

    /// Set `phase`'s root motion to move from target `from` to target `to`.
    pub fn set_phase_root_motion_move_toward(&mut self, phase: PhaseId, from: &str, to: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.set_root(RootMotion::move_toward(from, to)))
    }

    /// Set `phase`'s root motion to hold in place.
    pub fn set_phase_root_motion_hold(&mut self, phase: PhaseId) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.set_root(RootMotion::hold()))
    }

    /// Set `phase`'s root motion to settle in place.
    pub fn set_phase_root_motion_settle(&mut self, phase: PhaseId) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.set_root(RootMotion::settle()))
    }

    /// Give `phase` a linear ease.
    pub fn set_phase_ease_linear(&mut self, phase: PhaseId) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.set_ease(EaseCurve::Linear))
    }

    /// Give `phase` a smoothstep ease.
    pub fn set_phase_ease_smoothstep(&mut self, phase: PhaseId) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.set_ease(EaseCurve::SmoothStep))
    }

    /// Give `phase` an ease-in curve.
    pub fn set_phase_ease_in(&mut self, phase: PhaseId) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.set_ease(EaseCurve::EaseIn))
    }

    /// Give `phase` an ease-out curve.
    pub fn set_phase_ease_out(&mut self, phase: PhaseId) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.set_ease(EaseCurve::EaseOut))
    }

    /// Set `phase`'s layer weight.
    pub fn set_phase_layer_weight(&mut self, phase: PhaseId, weight: Ratio) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.set_layer_weight(weight.get()))
    }

}

/// Pose goals, constraints, contacts, and events.
impl AnimationAuthoringApi {
    // --- pose goals ----------------------------------------------------------

    /// Rotate `joint` toward `euler` (radians, XYZ) during `phase`.
    pub fn add_set_joint_rotation(&mut self, phase: PhaseId, joint: &str, euler: Vec3) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_goal(PoseGoal::set_joint_rotation(joint, euler)))
    }

    /// Aim `effector` at `target` during `phase`.
    pub fn add_aim_effector_at_target(&mut self, phase: PhaseId, effector: &str, target: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_goal(PoseGoal::aim_effector_at_target(effector, target)))
    }

    /// Move `effector` a fraction `amount` toward `target` during `phase`.
    pub fn add_move_effector_toward_target(&mut self, phase: PhaseId, effector: &str, target: &str, amount: Ratio) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_goal(PoseGoal::move_effector_toward_target(effector, target, amount.get())))
    }

    /// Raise the right/left arm for balance during `phase`.
    pub fn add_raise_arm_for_balance(&mut self, phase: PhaseId, right: bool) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_goal(PoseGoal::raise_arm_for_balance(right)))
    }

    /// Twist the torso toward `target` by `amount` during `phase`.
    pub fn add_torso_twist_toward_target(&mut self, phase: PhaseId, target: &str, amount: Ratio) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_goal(PoseGoal::torso_twist_toward_target(target, amount.get())))
    }

    /// Draw the right/left leg back by `amount` during `phase`.
    pub fn add_leg_backswing(&mut self, phase: PhaseId, right: bool, amount: Ratio) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_goal(PoseGoal::leg_backswing(right, amount.get())))
    }

    /// Strike with the right/left leg toward `target` during `phase`.
    pub fn add_leg_strike(&mut self, phase: PhaseId, right: bool, target: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_goal(PoseGoal::leg_strike(right, target)))
    }

    /// Follow through with the right/left leg toward `target` during `phase`.
    pub fn add_follow_through(&mut self, phase: PhaseId, right: bool, target: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_goal(PoseGoal::follow_through(right, target)))
    }

    /// Author a **locomotion cycle** (a walk/run) over `phase` for the standard
    /// humanoid: the legs step, the knees lift, and the arms pump for `steps`
    /// strides across the phase's progress. `stride` scales the thigh fore/aft swing,
    /// `knee_bend` the knee lift, and `arm_swing` the arm pump (each a `[0, 1]`
    /// fraction of a sensible maximum). The two legs run in antiphase, each shin
    /// leads its thigh by a quarter cycle to lift on the forward swing, and each arm
    /// opposes the same-side leg — the standard contralateral gait. Composes the
    /// per-joint [`PoseGoal::run_cycle`] oscillator; pair it with a forward
    /// `set_phase_root_motion_move_toward` so the figure travels as it steps.
    pub fn add_run_cycle(
        &mut self,
        phase: PhaseId,
        steps: u32,
        stride: Ratio,
        knee_bend: Ratio,
        arm_swing: Ratio,
    ) -> AuthoringResult<()> {
        use core::f32::consts::{FRAC_PI_2, PI};
        // Maximum radians each cycle component reaches at full strength.
        const STRIDE_MAX: f32 = 0.8;
        const KNEE_MAX: f32 = 1.0;
        const ARM_MAX: f32 = 0.7;
        let n = steps as f32;
        let thigh = stride.get() * STRIDE_MAX;
        let knee = knee_bend.get() * KNEE_MAX;
        let arm = arm_swing.get() * ARM_MAX;
        // `(joint, amplitude, phase_offset, bias)` per driven joint.
        let cycle: [(&str, f32, f32, f32); 6] = [
            ("left_thigh", thigh, 0.0, 0.0),
            ("right_thigh", thigh, PI, 0.0),
            // Shins lead their thigh by a quarter cycle and stay flexed (bias) so the
            // knee lifts on the forward swing and never hyperextends.
            ("left_shin", knee * 0.5, FRAC_PI_2, knee * 0.5),
            ("right_shin", knee * 0.5, PI + FRAC_PI_2, knee * 0.5),
            // Arms oppose the same-side leg (contralateral swing).
            ("left_upper_arm", arm, PI, 0.0),
            ("right_upper_arm", arm, 0.0, 0.0),
        ];
        self.phase_mut(phase).map(|p| {
            cycle.iter().for_each(|&(joint, amplitude, offset, bias)| {
                p.push_goal(PoseGoal::run_cycle(joint, amplitude, offset, n, bias));
            });
        })
    }

    // --- constraints ---------------------------------------------------------

    /// Pin `effector` to `target` during `phase`.
    pub fn add_pin_effector_to_target(&mut self, phase: PhaseId, effector: &str, target: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_constraint(Constraint::pin_effector_to_target(effector, target)))
    }

    /// Keep the gaze on `target` during `phase`.
    pub fn add_keep_gaze_on_target(&mut self, phase: PhaseId, target: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_constraint(Constraint::keep_gaze_on_target(target)))
    }

    /// Keep the center of mass over `support` during `phase`.
    pub fn add_keep_center_of_mass_over_support(&mut self, phase: PhaseId, support: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_constraint(Constraint::keep_center_of_mass_over_support(support)))
    }

    /// Orient `effector`'s surface toward `target` during `phase`.
    pub fn add_orient_surface_toward_target(&mut self, phase: PhaseId, effector: &str, target: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_constraint(Constraint::orient_surface_toward_target(effector, target)))
    }

    /// Preserve `effector`'s contact with `target` during `phase`.
    pub fn add_preserve_foot_contact(&mut self, phase: PhaseId, effector: &str, target: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_constraint(Constraint::preserve_foot_contact(effector, target)))
    }

    /// Declare `effector` in contact with `target` during `phase`.
    pub fn add_contact(&mut self, phase: PhaseId, effector: &str, target: &str) -> AuthoringResult<()> {
        self.phase_mut(phase).map(|p| p.push_contact(ContactDeclaration::new(effector, target)))
    }

    // --- events --------------------------------------------------------------

    /// Emit a named cue `name` at `tick` on `motion`.
    pub fn add_named_event(&mut self, motion: MotionId, tick: Tick, name: &str) -> AuthoringResult<()> {
        self.motion_mut(motion).map(|m| m.add_event(MotionEvent::named(tick, name)))
    }

    /// Emit a ball-contact at `tick`: `contact_surface` strikes `target` in the
    /// direction of `direction_target` with `power`.
    pub fn add_ball_contact(
        &mut self,
        motion: MotionId,
        tick: Tick,
        contact_surface: &str,
        target: &str,
        direction_target: &str,
        power: Ratio,
    ) -> AuthoringResult<()> {
        self.motion_mut(motion).map(|m| {
            m.add_event(MotionEvent::ball_contact(tick, contact_surface, target, direction_target, power.get()))
        })
    }

}

/// Compilation, sampling, inspection, pose-frame readers, and internal lookups.
impl AnimationAuthoringApi {
    // --- compile / sample ----------------------------------------------------

    /// Validate and compile `motion` into a plan, returning its id.
    pub fn compile(&mut self, motion: MotionId) -> AuthoringResult<PlanId> {
        // Build the plan under shared borrows, then register it under a mutable
        // borrow — the two never overlap.
        let built = self
            .motion(motion)
            .and_then(|spec| self.rig(spec.rig()).and_then(|rig| MotionCompiler::compile(spec, rig)));
        built.map(|plan| {
            let id = PlanId::from_raw(self.plans.len() as u64);
            self.plans.push(plan);
            id
        })
    }

    /// Sample compiled `plan` at `tick`.
    pub fn sample(&self, plan: PlanId, tick: Tick) -> AuthoringResult<PoseFrame> {
        self.plan(plan).map(|p| MotionSampler::sample(p, tick))
    }

    // --- inspection ----------------------------------------------------------

    /// The name of `motion`.
    pub fn motion_name(&self, motion: MotionId) -> AuthoringResult<String> {
        self.motion(motion).map(|m| m.name().to_string())
    }

    /// The names of `motion`'s phases, in order.
    pub fn motion_phase_names(&self, motion: MotionId) -> AuthoringResult<Vec<String>> {
        self.motion(motion)
            .map(|m| m.phases().iter().map(|p| p.name().to_string()).collect())
    }

    /// The value of style scalar `name` on `motion`, if set.
    pub fn motion_style(&self, motion: MotionId, name: &str) -> AuthoringResult<Option<Ratio>> {
        self.motion(motion).map(|m| m.style_value(name).map(Ratio::finite_or_zero))
    }

    /// The duration in ticks of compiled `plan`.
    pub fn plan_duration(&self, plan: PlanId) -> AuthoringResult<Tick> {
        self.plan(plan).map(|p| Tick::new(p.duration()))
    }

    /// The number of events in compiled `plan`.
    pub fn plan_event_count(&self, plan: PlanId) -> AuthoringResult<usize> {
        self.plan(plan).map(|p| p.events().len())
    }

    // --- pose-frame readers --------------------------------------------------

    /// The world root transform of `frame`.
    pub fn frame_root(&self, frame: &PoseFrame) -> Transform {
        frame.root()
    }

    /// The local transform of `joint` in `frame`, if in range.
    pub fn frame_joint_local(&self, frame: &PoseFrame, joint: JointId) -> Option<Transform> {
        frame.joint_local(joint)
    }

    /// The world transform of `joint` in `frame`, if in range — the composed FK
    /// result, used to place/drive a kinematic physics body at that joint.
    pub fn frame_joint_world(&self, frame: &PoseFrame, joint: JointId) -> Option<Transform> {
        frame.joint_world(joint)
    }

    /// The world transform of `effector` in `frame`, if in range.
    pub fn frame_effector_world(&self, frame: &PoseFrame, effector: EffectorId) -> Option<Transform> {
        frame.effector_world(effector)
    }

    /// The names of the events emitted in `frame`.
    pub fn frame_event_names(&self, frame: &PoseFrame) -> Vec<String> {
        frame.event_names().iter().map(|s| s.to_string()).collect()
    }

    /// The `(contact_surface, target, direction_target, power)` of the ball-contact
    /// emitted in `frame`, if any.
    pub fn frame_ball_contact(&self, frame: &PoseFrame) -> Option<(EffectorId, TargetId, TargetId, Ratio)> {
        frame.ball_contact().map(|e| {
            (
                e.contact_surface(),
                e.target(),
                e.direction_target(),
                Ratio::finite_or_zero(e.power()),
            )
        })
    }

    /// The number of constraints active in `frame`.
    pub fn frame_active_constraint_count(&self, frame: &PoseFrame) -> usize {
        frame.active_constraints().len()
    }

    /// The number of contacts active in `frame`.
    pub fn frame_active_contact_count(&self, frame: &PoseFrame) -> usize {
        frame.active_contacts().len()
    }

    /// Register a pre-built motion spec (used by the built-in
    /// `soccer_penalty_kick_v0`, which authors its spec directly with the internal
    /// infallible builders rather than the fallible per-call facade methods).
    pub(crate) fn push_motion(&mut self, spec: MotionSpec) -> MotionId {
        let id = MotionId::from_raw(self.motions.len() as u64);
        self.motions.push(spec);
        id
    }

    // --- internal lookups ----------------------------------------------------

    fn rig(&self, id: RigId) -> AuthoringResult<&HumanoidRigSpec> {
        self.rigs
            .get(id.raw() as usize)
            .ok_or_else(|| AuthoringError::rig_not_found("no rig with that id"))
    }

    fn motion(&self, id: MotionId) -> AuthoringResult<&MotionSpec> {
        self.motions
            .get(id.raw() as usize)
            .ok_or_else(|| AuthoringError::motion_not_found("no motion with that id"))
    }

    fn motion_mut(&mut self, id: MotionId) -> AuthoringResult<&mut MotionSpec> {
        self.motions
            .get_mut(id.raw() as usize)
            .ok_or_else(|| AuthoringError::motion_not_found("no motion with that id"))
    }

    fn phase_mut(&mut self, phase: PhaseId) -> AuthoringResult<&mut MotionPhase> {
        self.motion_mut(phase.motion()).and_then(|m| {
            m.phase_mut(phase.index())
                .ok_or_else(|| AuthoringError::phase_not_found("no phase with that index"))
        })
    }

    fn plan(&self, id: PlanId) -> AuthoringResult<&MotionPlan> {
        self.plans
            .get(id.raw() as usize)
            .ok_or_else(|| AuthoringError::plan_not_found("no plan with that id"))
    }
}

/// Physical-control objectives — the neutral, deterministic control data a physics
/// bridge consumes to drive a real physics engine. Each is a pure function of the
/// compiled plan and the tick; none mutate anything. These sit *beneath* the
/// authoring vocabulary — the pose path (`sample` / `frame_*`) is unchanged.
impl AnimationAuthoringApi {
    /// The name of the phase active in `plan` at `tick`, if any.
    pub fn active_phase_name(&self, plan: PlanId, tick: Tick) -> AuthoringResult<Option<String>> {
        self.plan(plan).map(|p| p.active_phase_name(tick.raw()).map(str::to_string))
    }

    /// The world position of the target named `name` in `plan` (e.g. `"ball"` or
    /// `"net_center"`), for a bridge that must place a physics body at it.
    pub fn plan_target_position(&self, plan: PlanId, name: &str) -> AuthoringResult<Option<Vec3>> {
        self.plan(plan).map(|p| p.target_position_by_name(name))
    }

    /// The id of the joint named `name` in `plan`'s rig (for a bridge binding a
    /// physics body to it) — resolves without the caller holding the `RigId`.
    pub fn plan_joint_id(&self, plan: PlanId, name: &str) -> AuthoringResult<Option<JointId>> {
        self.plan(plan).map(|p| p.rig().joint_id(name))
    }

    /// The id of the effector named `name` in `plan`'s rig.
    pub fn plan_effector_id(&self, plan: PlanId, name: &str) -> AuthoringResult<Option<EffectorId>> {
        self.plan(plan).map(|p| p.rig().effector_id(name))
    }

    /// The root's per-tick velocity target at `tick` (a `MoveToward` phase advances
    /// the root toward its destination; `Hold`/`Settle` yield `None`).
    pub fn objective_root_velocity(&self, plan: PlanId, tick: Tick) -> AuthoringResult<Option<Vec3>> {
        self.plan(plan).map(|p| physical_objective::root_velocity(p, tick.raw()))
    }

    /// The active foot-plant objective at `tick`: the pinned effector and its world
    /// target (a contact declaration, else a pinning constraint).
    pub fn objective_foot_plant(&self, plan: PlanId, tick: Tick) -> AuthoringResult<Option<(EffectorId, Vec3)>> {
        self.plan(plan).map(|p| physical_objective::foot_plant(p, tick.raw()))
    }

    /// The active joint-motor objectives at `tick`: `(joint, authored Euler target,
    /// drive)` per joint the active phase drives — `drive` is the phase's layer
    /// weight, so a stronger phase outdrives a weaker one.
    pub fn objective_joint_motors(&self, plan: PlanId, tick: Tick) -> AuthoringResult<Vec<(JointId, Vec3, Ratio)>> {
        self.plan(plan).map(|p| physical_objective::joint_motors(p, tick.raw()))
    }

    /// The ball-impulse objective at `tick`: `(contact_surface, unit direction,
    /// magnitude)` at a `ball_contact`, direction pointing from the aim target
    /// toward the direction target, magnitude scaled by the event power.
    pub fn objective_ball_impulse(&self, plan: PlanId, tick: Tick) -> AuthoringResult<Option<(EffectorId, Vec3, Ratio)>> {
        self.plan(plan).map(|p| physical_objective::ball_impulse(p, tick.raw()))
    }

    /// The active gaze objective at `tick`: the world position a `keep_gaze` holds
    /// the gaze on, if any.
    pub fn objective_gaze(&self, plan: PlanId, tick: Tick) -> AuthoringResult<Option<Vec3>> {
        self.plan(plan).map(|p| physical_objective::gaze(p, tick.raw()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring_error_code::AuthoringErrorCode;
    use axiom_math::Quat;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    #[test]
    fn new_and_default_are_equivalent_empty_registries() {
        let a = AnimationAuthoringApi::new();
        let b = AnimationAuthoringApi::default();
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }

    #[test]
    fn a_rig_reports_its_joint_and_effector_names_and_validity() {
        let mut api = AnimationAuthoringApi::new();
        let rig = api.standard_humanoid();
        assert_eq!(rig, RigId::from_raw(0));
        assert_eq!(api.joint_names(rig).unwrap().len(), 25);
        assert_eq!(api.effector_names(rig).unwrap().len(), 8);
        assert!(api.rig_is_valid(rig).unwrap());
        assert_eq!(api.joint_id(rig, "root").unwrap(), Some(JointId::from_raw(0)));
        assert_eq!(api.joint_id(rig, "nope").unwrap(), None);
        assert_eq!(api.effector_id(rig, "left_foot_sole").unwrap(), Some(EffectorId::from_raw(0)));
        assert_eq!(api.effector_id(rig, "nope").unwrap(), None);
        assert_eq!(api.joint_id(RigId::from_raw(9), "root").unwrap_err().code(), AuthoringErrorCode::RigNotFound);
        assert_eq!(api.effector_id(RigId::from_raw(9), "x").unwrap_err().code(), AuthoringErrorCode::RigNotFound);
    }

    #[test]
    fn a_full_motion_authors_compiles_and_samples() {
        let mut api = AnimationAuthoringApi::new();
        let rig = api.standard_humanoid();
        let m = api.create_motion("kick", Tick::new(20), rig).unwrap();
        api.add_target(m, "approach_start", Vec3::new(0.0, 0.0, -3.0)).unwrap();
        api.add_target(m, "ball", Vec3::new(0.0, 0.0, 0.0)).unwrap();
        api.add_target(m, "net_center", Vec3::new(0.0, 0.8, 8.0)).unwrap();
        api.add_target(m, "left_plant_spot", Vec3::new(0.25, 0.0, -0.1)).unwrap();
        api.set_style(m, "power", ratio(0.7)).unwrap();

        let approach = api.add_phase(m, "approach", Tick::new(0), Tick::new(10)).unwrap();
        api.set_phase_root_motion_move_toward(approach, "approach_start", "ball").unwrap();
        api.set_phase_ease_smoothstep(approach).unwrap();
        api.set_phase_layer_weight(approach, ratio(1.0)).unwrap();

        let strike = api.add_phase(m, "strike", Tick::new(10), Tick::new(20)).unwrap();
        api.set_phase_root_motion_hold(strike).unwrap();
        api.set_phase_ease_linear(strike).unwrap();
        api.set_phase_ease_in(strike).unwrap();
        api.set_phase_ease_out(strike).unwrap();
        api.set_phase_root_motion_settle(strike).unwrap();
        api.add_set_joint_rotation(strike, "chest", Vec3::new(0.0, 0.2, 0.0)).unwrap();
        api.add_aim_effector_at_target(strike, "right_foot_instep", "ball").unwrap();
        api.add_move_effector_toward_target(strike, "left_hand", "ball", ratio(0.5)).unwrap();
        api.add_raise_arm_for_balance(strike, true).unwrap();
        api.add_torso_twist_toward_target(strike, "net_center", ratio(0.5)).unwrap();
        api.add_leg_backswing(strike, true, ratio(0.8)).unwrap();
        api.add_leg_strike(strike, true, "ball").unwrap();
        api.add_follow_through(strike, true, "net_center").unwrap();
        api.add_pin_effector_to_target(strike, "left_foot_sole", "left_plant_spot").unwrap();
        api.add_keep_gaze_on_target(strike, "ball").unwrap();
        api.add_keep_center_of_mass_over_support(strike, "left_foot_sole").unwrap();
        api.add_orient_surface_toward_target(strike, "right_foot_instep", "net_center").unwrap();
        api.add_preserve_foot_contact(strike, "left_foot_sole", "left_plant_spot").unwrap();
        api.add_contact(strike, "right_foot_sole", "ball").unwrap();
        api.add_named_event(m, Tick::new(3), "whistle").unwrap();
        api.add_ball_contact(m, Tick::new(15), "right_foot_instep", "ball", "net_center", ratio(0.7)).unwrap();

        let plan = api.compile(m).unwrap();
        assert_eq!(plan, PlanId::from_raw(0));

        let frame = api.sample(plan, Tick::new(15)).unwrap();
        assert_eq!(api.frame_root(&frame).translation, Vec3::new(0.0, 0.0, 0.0)); // held at ball
        assert!(api.frame_joint_local(&frame, JointId::from_raw(0)).is_some());
        assert!(api.frame_joint_local(&frame, JointId::from_raw(99)).is_none());
        assert!(api.frame_joint_world(&frame, JointId::from_raw(0)).is_some());
        assert!(api.frame_joint_world(&frame, JointId::from_raw(99)).is_none());
        assert!(api.frame_effector_world(&frame, EffectorId::from_raw(0)).is_some());
        assert!(api.frame_effector_world(&frame, EffectorId::from_raw(99)).is_none());
        assert_eq!(api.frame_event_names(&frame), vec!["ball_contact"]);
        assert_eq!(api.frame_active_constraint_count(&frame), 5);
        assert_eq!(api.frame_active_contact_count(&frame), 1);
        let (surface, target, direction, power) = api.frame_ball_contact(&frame).unwrap();
        assert_eq!(surface, EffectorId::from_raw(2)); // right_foot_instep is effector 2
        assert_eq!(target, TargetId::from_raw(1)); // ball
        assert_eq!(direction, TargetId::from_raw(2)); // net_center
        assert!((power.get() - 0.7).abs() < 1.0e-6);
    }

    #[test]
    fn a_run_cycle_authors_a_stepping_gait_that_oscillates_the_legs() {
        let mut api = AnimationAuthoringApi::new();
        let rig = api.standard_humanoid();
        let m = api.create_motion("run", Tick::new(12), rig).unwrap();
        api.add_target(m, "start", Vec3::new(0.0, 0.0, -3.0)).unwrap();
        api.add_target(m, "ball", Vec3::new(0.0, 0.0, 0.0)).unwrap();
        let run = api.add_phase(m, "run", Tick::new(0), Tick::new(12)).unwrap();
        api.set_phase_root_motion_move_toward(run, "start", "ball").unwrap();
        api.add_run_cycle(run, 3, ratio(0.6), ratio(0.5), ratio(0.4)).unwrap();
        let plan = api.compile(m).unwrap();

        let left = api.plan_joint_id(plan, "left_thigh").unwrap().unwrap();
        let right = api.plan_joint_id(plan, "right_thigh").unwrap().unwrap();
        let rot = |joint, t| api.sample(plan, Tick::new(t)).unwrap().joint_local(joint).unwrap().rotation;
        // The gait cycles the thigh over raw progress: tick 1 (sin≈+1) and tick 3
        // (sin≈-1) read distinct, non-identity rotations — the leg steps, not slides.
        assert_ne!(rot(left, 1), rot(left, 3));
        assert_ne!(rot(left, 1), Quat::IDENTITY);
        // The two legs run in antiphase (offset π), so they differ at the same tick.
        assert_ne!(rot(left, 1), rot(right, 1));
    }

    #[test]
    fn inspection_readers_expose_names_style_and_plan_stats() {
        let mut api = AnimationAuthoringApi::new();
        let rig = api.standard_humanoid();
        let m = api.create_motion("kick", Tick::new(10), rig).unwrap();
        api.set_style(m, "power", ratio(0.4)).unwrap();
        api.add_phase(m, "approach", Tick::new(0), Tick::new(5)).unwrap();
        api.add_phase(m, "strike", Tick::new(5), Tick::new(10)).unwrap();
        api.add_named_event(m, Tick::new(3), "cue").unwrap();

        assert_eq!(api.motion_name(m).unwrap(), "kick");
        assert_eq!(api.motion_phase_names(m).unwrap(), vec!["approach", "strike"]);
        assert!((api.motion_style(m, "power").unwrap().unwrap().get() - 0.4).abs() < 1.0e-6);
        assert_eq!(api.motion_style(m, "missing").unwrap(), None);

        let plan = api.compile(m).unwrap();
        assert_eq!(api.plan_duration(plan).unwrap(), Tick::new(10));
        assert_eq!(api.plan_event_count(plan).unwrap(), 1);

        // Missing ids fail with their codes.
        let ghost_motion = MotionId::from_raw(9);
        let ghost_plan = PlanId::from_raw(9);
        assert_eq!(api.motion_name(ghost_motion).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.motion_phase_names(ghost_motion).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.motion_style(ghost_motion, "power").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.plan_duration(ghost_plan).unwrap_err().code(), AuthoringErrorCode::PlanNotFound);
        assert_eq!(api.plan_event_count(ghost_plan).unwrap_err().code(), AuthoringErrorCode::PlanNotFound);
    }

    #[test]
    fn a_frame_without_a_ball_contact_reports_no_ball_contact() {
        let mut api = AnimationAuthoringApi::new();
        let rig = api.standard_humanoid();
        let m = api.create_motion("k", Tick::new(10), rig).unwrap();
        api.add_phase(m, "p", Tick::new(0), Tick::new(10)).unwrap();
        let plan = api.compile(m).unwrap();
        let frame = api.sample(plan, Tick::new(5)).unwrap();
        assert_eq!(api.frame_ball_contact(&frame), None);
        assert!(api.frame_event_names(&frame).is_empty());
    }

    #[test]
    fn every_missing_id_fails_with_its_code() {
        let mut api = AnimationAuthoringApi::new();
        let ghost_rig = RigId::from_raw(9);
        let ghost_motion = MotionId::from_raw(9);
        let ghost_phase = PhaseId::new(MotionId::from_raw(9), 0);
        let ghost_plan = PlanId::from_raw(9);

        assert_eq!(api.joint_names(ghost_rig).unwrap_err().code(), AuthoringErrorCode::RigNotFound);
        assert_eq!(api.effector_names(ghost_rig).unwrap_err().code(), AuthoringErrorCode::RigNotFound);
        assert_eq!(api.rig_is_valid(ghost_rig).unwrap_err().code(), AuthoringErrorCode::RigNotFound);
        assert_eq!(api.create_motion("k", Tick::new(1), ghost_rig).unwrap_err().code(), AuthoringErrorCode::RigNotFound);

        assert_eq!(api.add_target(ghost_motion, "t", Vec3::ZERO).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.set_style(ghost_motion, "s", ratio(0.1)).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_phase(ghost_motion, "p", Tick::new(0), Tick::new(1)).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_named_event(ghost_motion, Tick::new(0), "n").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(
            api.add_ball_contact(ghost_motion, Tick::new(0), "e", "t", "d", ratio(0.1)).unwrap_err().code(),
            AuthoringErrorCode::MotionNotFound
        );
        assert_eq!(api.compile(ghost_motion).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);

        // Phase-scoped setters and every goal/constraint/contact adder.
        assert_eq!(api.set_phase_root_motion_move_toward(ghost_phase, "a", "b").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.set_phase_root_motion_hold(ghost_phase).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.set_phase_root_motion_settle(ghost_phase).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.set_phase_ease_linear(ghost_phase).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.set_phase_ease_smoothstep(ghost_phase).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.set_phase_ease_in(ghost_phase).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.set_phase_ease_out(ghost_phase).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.set_phase_layer_weight(ghost_phase, ratio(1.0)).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_set_joint_rotation(ghost_phase, "chest", Vec3::ZERO).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_aim_effector_at_target(ghost_phase, "e", "t").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_move_effector_toward_target(ghost_phase, "e", "t", ratio(0.5)).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_raise_arm_for_balance(ghost_phase, true).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_torso_twist_toward_target(ghost_phase, "t", ratio(0.5)).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_leg_backswing(ghost_phase, true, ratio(0.5)).unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_leg_strike(ghost_phase, true, "t").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_follow_through(ghost_phase, true, "t").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_pin_effector_to_target(ghost_phase, "e", "t").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_keep_gaze_on_target(ghost_phase, "t").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_keep_center_of_mass_over_support(ghost_phase, "e").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_orient_surface_toward_target(ghost_phase, "e", "t").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_preserve_foot_contact(ghost_phase, "e", "t").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(api.add_contact(ghost_phase, "e", "t").unwrap_err().code(), AuthoringErrorCode::MotionNotFound);

        assert_eq!(api.sample(ghost_plan, Tick::new(0)).unwrap_err().code(), AuthoringErrorCode::PlanNotFound);
    }

    #[test]
    fn a_missing_phase_index_fails_with_phase_not_found() {
        let mut api = AnimationAuthoringApi::new();
        let rig = api.standard_humanoid();
        let m = api.create_motion("k", Tick::new(10), rig).unwrap();
        let bad_phase = PhaseId::new(m, 7); // motion exists, phase index does not
        assert_eq!(
            api.set_phase_ease_linear(bad_phase).unwrap_err().code(),
            AuthoringErrorCode::PhaseNotFound
        );
    }
}
