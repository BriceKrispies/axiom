//! The single public facade for the physical-animation bridge — the controller
//! that owns an `axiom-physics` world, binds a humanoid, and advances the
//! physics-backed animation one deterministic tick at a time.

use axiom_animation_authoring::{AnimationAuthoringApi, EffectorId, PlanId};
use axiom_kernel::{FrameIndex, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

use crate::humanoid_binding::{create_ball, BoundBody, HumanoidPhysicsBinding};
use crate::muscle_group::{MuscleGroup, MuscleGroupParams, MUSCLE_GROUP_COUNT};
use crate::muscle_profile::{MusclePhaseProfile, MuscleStyle, SupportMode, VirtualMuscleProfile};
use crate::physical_error::PhysicalError;
use crate::physical_frame::{FrameParts, PhysicalAnimationFrame};
use crate::physical_result::{auth, phys, PhysicalResult};
use crate::virtual_muscle::{
    MuscleBodyState, MuscleObjectives, VirtualMuscleCommand, VirtualMuscleController,
};

/// The fixed physics timestep (60 Hz), in nanoseconds.
const FIXED_DELTA_NANOS: u64 = 16_666_667;
/// Gravity magnitude (m/s²) the pelvis anti-gravity hold cancels.
const GRAVITY_Y: f32 = 9.8;
/// The dynamic-body mass (kg) — matches the binding's.
const BODY_MASS: f32 = 1.0;
/// Force gain applied to the pelvis per unit of approach velocity.
const APPROACH_DRIVE: f32 = 40.0;
/// The ball mass (kg).
const BALL_MASS: f32 = 0.4;
/// Impulse speed (m/s) imparted to the ball per unit of strike power.
const IMPULSE_SPEED: f32 = 12.0;
/// The key effectors reported in every frame.
const KEY_EFFECTORS: [&str; 3] = ["left_foot_sole", "right_foot_sole", "right_foot_instep"];

/// The physics-backed procedural-animation controller and facade. Owns a real
/// `axiom-physics` world; binds a humanoid rig to physics bodies; per tick reads
/// the authored physical objectives, applies them to the world (force on the
/// dynamic pelvis, kinematic drive on the limbs, a real impulse on the ball),
/// steps deterministically, and reads back a [`PhysicalAnimationFrame`]. Renders
/// nothing. Authoring is passed in by reference — the app authors, the bridge
/// simulates.
#[derive(Debug)]
pub struct PhysicalAnimationApi {
    physics: PhysicsApi,
    binding: Option<HumanoidPhysicsBinding>,
    ball: Option<PhysicsBodyHandle>,
    muscle_profile: VirtualMuscleProfile,
    muscle_style: MuscleStyle,
}

impl PhysicalAnimationApi {
    /// A new controller with a fresh physics world (gravity `(0, -9.8, 0)`) and a
    /// balanced default virtual-muscle profile + style.
    pub fn new() -> Self {
        PhysicalAnimationApi {
            physics: PhysicsApi::new(),
            binding: None,
            ball: None,
            muscle_profile: VirtualMuscleProfile::default_profile(),
            muscle_style: MuscleStyle::default_style(),
        }
    }

    /// Bind the standard humanoid of `plan`'s rig to physics bodies, placed at the
    /// pose sampled at tick 0.
    pub fn bind_standard_humanoid(
        &mut self,
        authoring: &AnimationAuthoringApi,
        plan: PlanId,
    ) -> PhysicalResult<()> {
        HumanoidPhysicsBinding::build_standard(&mut self.physics, authoring, plan).map(|binding| {
            self.binding = Some(binding);
        })
    }

    /// Attach the dynamic soccer ball at `plan`'s `"ball"` target, so a strike
    /// impulse sends it flying under real physics.
    pub fn attach_ball(
        &mut self,
        authoring: &AnimationAuthoringApi,
        plan: PlanId,
    ) -> PhysicalResult<()> {
        let PhysicalAnimationApi { physics, ball, .. } = self;
        auth(authoring.plan_target_position(plan, "ball"))
            .map(|opt| opt.unwrap_or(Vec3::ZERO))
            .and_then(|center| {
                // Rest the ball on its radius above the target ground level.
                create_ball(
                    physics,
                    center.add(Vec3::new(0.0, crate::humanoid_binding::BALL_RADIUS, 0.0)),
                    BALL_MASS,
                )
                .map(|handle| {
                    *ball = Some(handle);
                })
            })
    }

    /// Set the per-group virtual-muscle base parameters, in group-code order
    /// (`core, pelvis, spine, neck_head, left_leg, right_leg, left_ankle,
    /// right_ankle, left_arm, right_arm`), each `(stiffness, damping, max_torque,
    /// rest_weight)`.
    pub fn set_muscle_profile(
        &mut self,
        groups: [(Ratio, Ratio, Ratio, Ratio); MUSCLE_GROUP_COUNT],
    ) {
        self.muscle_profile = VirtualMuscleProfile::new(
            groups.map(|(s, d, t, w)| MuscleGroupParams::new(s.get(), d.get(), t.get(), w.get())),
        );
    }

    /// Set the global muscle-style scalars.
    pub fn set_muscle_style(
        &mut self,
        muscle_strength: Ratio,
        muscle_damping: Ratio,
        balance_strength: Ratio,
    ) {
        self.muscle_style = MuscleStyle::new(
            muscle_strength.get(),
            muscle_damping.get(),
            balance_strength.get(),
        );
    }

    /// Advance the physics-backed animation to `tick`, returning the frame. Reads
    /// the authored objectives, applies them, steps the world, and reads it back.
    /// This is the muscle-free path (the pelvis balance/upright control is off).
    pub fn advance(
        &mut self,
        authoring: &AnimationAuthoringApi,
        plan: PlanId,
        tick: Tick,
    ) -> PhysicalResult<PhysicalAnimationFrame> {
        let PhysicalAnimationApi {
            physics,
            binding,
            ball,
            ..
        } = self;
        binding
            .as_ref()
            .ok_or_else(|| PhysicalError::not_bound("bind a humanoid before advancing"))
            .and_then(|binding| {
                ball.ok_or_else(|| PhysicalError::no_ball("attach a ball before advancing"))
                    .and_then(|ball| step_once(physics, binding, ball, authoring, plan, tick, None))
            })
    }

    /// Advance with the **virtual-muscle controller active** for this tick: the
    /// caller supplies the phase's `support_mode` (`0` both feet, `1` left foot,
    /// `2` right foot, `3` airborne) and per-group phase weights (group-code order).
    /// The controller computes balance/upright/plant/recovery commands from the
    /// current physics state + authored objectives and applies the balance force +
    /// upright torque to the dynamic pelvis before stepping. The command is carried
    /// in the frame (read via the `frame_muscle_*` accessors).
    pub fn advance_muscled(
        &mut self,
        authoring: &AnimationAuthoringApi,
        plan: PlanId,
        tick: Tick,
        support_mode: u8,
        group_phase_weights: [Ratio; MUSCLE_GROUP_COUNT],
    ) -> PhysicalResult<PhysicalAnimationFrame> {
        let PhysicalAnimationApi {
            physics,
            binding,
            ball,
            muscle_profile,
            muscle_style,
        } = self;
        let phase = MusclePhaseProfile::new(
            SupportMode::from_code(support_mode),
            group_phase_weights.map(|r| r.get()),
        );
        let cfg = MuscleConfig {
            profile: muscle_profile,
            style: *muscle_style,
            phase,
        };
        binding
            .as_ref()
            .ok_or_else(|| PhysicalError::not_bound("bind a humanoid before advancing"))
            .and_then(|binding| {
                ball.ok_or_else(|| PhysicalError::no_ball("attach a ball before advancing"))
                    .and_then(|ball| {
                        step_once(physics, binding, ball, authoring, plan, tick, Some(cfg))
                    })
            })
    }

    // --- frame readers -------------------------------------------------------

    /// The tick a frame was sampled at.
    pub fn frame_tick(&self, frame: &PhysicalAnimationFrame) -> Tick {
        Tick::new(frame.tick())
    }

    /// The active authored phase name in a frame, if any.
    pub fn frame_phase_name(&self, frame: &PhysicalAnimationFrame) -> Option<String> {
        frame.phase_name().map(str::to_string)
    }

    /// The world transform of the bound body named `name` in a frame.
    pub fn frame_body_transform(
        &self,
        frame: &PhysicalAnimationFrame,
        name: &str,
    ) -> Option<Transform> {
        frame.body_transform(name)
    }

    /// The world transform of the key effector named `name` in a frame.
    pub fn frame_effector_transform(
        &self,
        frame: &PhysicalAnimationFrame,
        name: &str,
    ) -> Option<Transform> {
        frame.effector_transform(name)
    }

    /// The ball's world transform after the step.
    pub fn frame_ball_transform(&self, frame: &PhysicalAnimationFrame) -> Option<Transform> {
        frame.ball_transform()
    }

    /// The ball's linear velocity after the step.
    pub fn frame_ball_velocity(&self, frame: &PhysicalAnimationFrame) -> Option<Vec3> {
        frame.ball_velocity()
    }

    /// The applied root-velocity objective in a frame, if any.
    pub fn frame_root_velocity(&self, frame: &PhysicalAnimationFrame) -> Option<Vec3> {
        frame.root_velocity()
    }

    /// The applied foot-plant objective `(effector, target)` in a frame, if any.
    pub fn frame_foot_plant(&self, frame: &PhysicalAnimationFrame) -> Option<(EffectorId, Vec3)> {
        frame.foot_plant()
    }

    /// The number of active joint-motor objectives in a frame.
    pub fn frame_motor_count(&self, frame: &PhysicalAnimationFrame) -> usize {
        frame.motor_count()
    }

    /// The maximum motor drive across a frame's joint-motor objectives.
    pub fn frame_motor_drive(&self, frame: &PhysicalAnimationFrame) -> Ratio {
        Ratio::finite_or_zero(frame.motor_max_drive())
    }

    /// The applied ball-impulse `(direction, magnitude)` in a frame, if any.
    pub fn frame_ball_impulse(&self, frame: &PhysicalAnimationFrame) -> Option<(Vec3, Ratio)> {
        frame
            .ball_impulse()
            .map(|(dir, mag)| (dir, Ratio::finite_or_zero(mag)))
    }

    /// The active gaze objective in a frame, if any.
    pub fn frame_gaze(&self, frame: &PhysicalAnimationFrame) -> Option<Vec3> {
        frame.gaze()
    }

    /// The number of contacts the physics engine resolved this step.
    pub fn frame_contact_count(&self, frame: &PhysicalAnimationFrame) -> usize {
        frame.contact_count()
    }

    /// The names of the events emitted this tick.
    pub fn frame_event_names(&self, frame: &PhysicalAnimationFrame) -> Vec<String> {
        frame.events().to_vec()
    }

    /// The physics step index of a frame.
    pub fn frame_step_index(&self, frame: &PhysicalAnimationFrame) -> u64 {
        frame.step_index()
    }
}

/// Virtual-muscle frame readers — all `None` on a muscle-free `advance` frame.
impl PhysicalAnimationApi {
    /// The active support mode code applied this tick.
    pub fn frame_support_mode(&self, frame: &PhysicalAnimationFrame) -> Option<u8> {
        frame.muscle().map(|c| c.support_mode().code())
    }

    /// The deterministic centre-of-mass estimate this tick.
    pub fn frame_center_of_mass(&self, frame: &PhysicalAnimationFrame) -> Option<Vec3> {
        frame.muscle().map(VirtualMuscleCommand::center_of_mass)
    }

    /// The support target the balance controller pulled the CoM toward.
    pub fn frame_support_target(&self, frame: &PhysicalAnimationFrame) -> Option<Vec3> {
        frame.muscle().map(VirtualMuscleCommand::support_target)
    }

    /// The final actuation weight for muscle group `group` (code `0..=9`).
    pub fn frame_muscle_group_weight(
        &self,
        frame: &PhysicalAnimationFrame,
        group: u8,
    ) -> Option<Ratio> {
        frame
            .muscle()
            .map(|c| Ratio::finite_or_zero(c.group_weight(MuscleGroup::from_code(group))))
    }

    /// The peak actuation for muscle group `group` (scaled by muscle strength).
    pub fn frame_muscle_group_max_torque(
        &self,
        frame: &PhysicalAnimationFrame,
        group: u8,
    ) -> Option<Ratio> {
        frame
            .muscle()
            .map(|c| Ratio::finite_or_zero(c.group_max_torque(MuscleGroup::from_code(group))))
    }

    /// The foot-plant hold strength this tick (`0` once the plant releases).
    pub fn frame_plant_strength(&self, frame: &PhysicalAnimationFrame) -> Option<Ratio> {
        frame
            .muscle()
            .map(|c| Ratio::finite_or_zero(c.plant_strength()))
    }

    /// The horizontal balance-correction force applied to the pelvis.
    pub fn frame_balance_correction(&self, frame: &PhysicalAnimationFrame) -> Option<Vec3> {
        frame.muscle().map(VirtualMuscleCommand::balance_correction)
    }

    /// The recovery / settling damping factor this tick.
    pub fn frame_recovery_damping(&self, frame: &PhysicalAnimationFrame) -> Option<Ratio> {
        frame
            .muscle()
            .map(|c| Ratio::finite_or_zero(c.recovery_damping()))
    }

    /// A deterministic one-line muscle debug report for this tick.
    pub fn frame_muscle_report(&self, frame: &PhysicalAnimationFrame) -> Option<String> {
        frame.muscle().map(VirtualMuscleCommand::report)
    }
}

impl Default for PhysicalAnimationApi {
    fn default() -> Self {
        PhysicalAnimationApi::new()
    }
}

/// The deterministic fixed step for `tick`.
fn runtime_step(tick: u64) -> RuntimeStep {
    RuntimeStep::new(
        FrameIndex::new(tick),
        Tick::new(tick),
        FIXED_DELTA_NANOS,
        tick,
    )
}

/// The plant-hold transform for `b` if it is the currently-planted foot body.
fn plant_hold(
    binding: &HumanoidPhysicsBinding,
    b: &BoundBody,
    foot_plant: Option<(EffectorId, Vec3)>,
) -> Option<Transform> {
    foot_plant.and_then(|(effector, target)| {
        binding
            .foot_body_for(effector)
            .filter(|foot| *foot == b.body())
            .map(|_| Transform::from_translation(target))
    })
}

/// Force the dynamic pelvis: an anti-gravity hold, the approach drive, plus the
/// muscle balance correction (`ZERO` on the muscle-free path).
fn drive_dynamic(
    physics: &mut PhysicsApi,
    b: &BoundBody,
    root_velocity: Option<Vec3>,
    balance: Vec3,
) -> PhysicalResult<()> {
    let hold = Vec3::new(0.0, GRAVITY_Y * BODY_MASS, 0.0);
    let drive = root_velocity
        .map(|v| v.mul_scalar(APPROACH_DRIVE))
        .unwrap_or(Vec3::ZERO);
    phys(physics.apply_force(b.body(), hold.add(drive).add(balance)))
}

/// The per-tick virtual-muscle configuration the muscled step applies.
struct MuscleConfig<'a> {
    profile: &'a VirtualMuscleProfile,
    style: MuscleStyle,
    phase: MusclePhaseProfile,
}

/// The world transform of the body named `name` in `bodies` (`IDENTITY` if absent).
fn named_transform(bodies: &[(&'static str, Transform)], name: &str) -> Transform {
    bodies
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
        .unwrap_or(Transform::IDENTITY)
}

/// Run the virtual-muscle controller against the current (pre-step) physics state.
fn muscle_command(
    physics: &PhysicsApi,
    binding: &HumanoidPhysicsBinding,
    cfg: MuscleConfig<'_>,
    foot_plant: Option<Vec3>,
    motor_drive: f32,
    ball_impulse: Option<Vec3>,
) -> VirtualMuscleCommand {
    let snap = physics.snapshot();
    let bodies: Vec<(&'static str, Transform)> = binding
        .bodies()
        .iter()
        .filter_map(|b| {
            snap.bodies()
                .iter()
                .find(|sb| sb.handle() == b.body())
                .map(|sb| (b.name(), sb.transform()))
        })
        .collect();
    let com: Vec<Vec3> = bodies.iter().map(|(_, t)| t.translation).collect();
    let body = MuscleBodyState {
        com_samples: &com,
        left_foot: named_transform(&bodies, "left_foot").translation,
        right_foot: named_transform(&bodies, "right_foot").translation,
        pelvis: named_transform(&bodies, "pelvis"),
    };
    VirtualMuscleController::command(
        cfg.profile,
        cfg.style,
        cfg.phase,
        MuscleObjectives {
            foot_plant,
            motor_drive,
            ball_impulse,
        },
        body,
    )
}

/// Steps 2–4 of the tick (nothing here touches the un-nameable pose frame):
/// run the muscle controller when configured, drive the dynamic bodies with the
/// root-velocity approach + balance correction + upright torque, apply the
/// strike-tick ball impulse, and step the world deterministically. Returns the
/// muscle command for the frame read-back.
fn drive_and_step(
    physics: &mut PhysicsApi,
    binding: &HumanoidPhysicsBinding,
    ball: PhysicsBodyHandle,
    tick: Tick,
    muscle: Option<MuscleConfig<'_>>,
    foot_plant: Option<(EffectorId, Vec3)>,
    root_velocity: Option<Vec3>,
    motor_drive: f32,
    impulse_vec: Option<Vec3>,
) -> PhysicalResult<Option<VirtualMuscleCommand>> {
    let command = muscle.map(|cfg| {
        muscle_command(
            physics,
            binding,
            cfg,
            foot_plant.map(|(_, t)| t),
            motor_drive,
            impulse_vec,
        )
    });
    let balance = command
        .map(|c| c.balance_correction())
        .unwrap_or(Vec3::ZERO);
    let torque = command.map(|c| c.upright_torque()).unwrap_or(Vec3::ZERO);
    binding
        .bodies()
        .iter()
        .filter(|b| b.dynamic())
        .try_for_each(|b| {
            drive_dynamic(physics, b, root_velocity, balance)
                .and_then(|()| phys(physics.apply_torque(b.body(), torque)))
        })
        // The ball: a real impulse at the strike tick — never a teleport.
        .and_then(|()| {
            impulse_vec
                .into_iter()
                .try_for_each(|imp| phys(physics.apply_impulse(ball, imp)))
        })
        // Step the world deterministically.
        .and_then(|()| phys(physics.step(runtime_step(tick.raw()))))
        .map(|()| command)
}

/// The one-tick step: read authored objectives, run the muscle controller (when
/// configured), apply everything to physics, step, and read it back. The pose
/// frame is only nameable by inference, so all pose-dependent work is inlined.
fn step_once(
    physics: &mut PhysicsApi,
    binding: &HumanoidPhysicsBinding,
    ball: PhysicsBodyHandle,
    authoring: &AnimationAuthoringApi,
    plan: PlanId,
    tick: Tick,
    muscle: Option<MuscleConfig<'_>>,
) -> PhysicalResult<PhysicalAnimationFrame> {
    auth(authoring.sample(plan, tick)).and_then(|pose| {
        auth(authoring.objective_root_velocity(plan, tick)).and_then(|root_velocity| {
            auth(authoring.objective_foot_plant(plan, tick)).and_then(|foot_plant| {
                auth(authoring.objective_joint_motors(plan, tick)).and_then(|motors| {
                    auth(authoring.objective_ball_impulse(plan, tick)).and_then(|ball_impulse| {
                        auth(authoring.objective_gaze(plan, tick)).and_then(|gaze| {
                            auth(authoring.active_phase_name(plan, tick)).and_then(|phase_name| {
                                let motor_drive =
                                    motors.iter().map(|(_, _, d)| d.get()).fold(0.0, f32::max);
                                let impulse_vec = ball_impulse.map(|(_, dir, mag)| {
                                    dir.mul_scalar(mag.get() * IMPULSE_SPEED * BALL_MASS)
                                });
                                // 1. Kinematic limbs: hold the planted foot, else track the authored pose.
                                binding
                                    .bodies()
                                    .iter()
                                    .filter(|b| !b.dynamic())
                                    .try_for_each(|b| {
                                        let target =
                                            plant_hold(binding, b, foot_plant).or_else(|| {
                                                authoring.frame_joint_world(&pose, b.joint())
                                            });
                                        target.into_iter().try_for_each(|t| {
                                            phys(physics.set_body_transform(b.body(), t))
                                        })
                                    })
                                    // 2–4. Muscle control, dynamic drive, ball impulse, world step
                                    //      (pose-free, so it lives in `drive_and_step`).
                                    .and_then(|()| {
                                        drive_and_step(
                                            physics,
                                            binding,
                                            ball,
                                            tick,
                                            muscle,
                                            foot_plant,
                                            root_velocity,
                                            motor_drive,
                                            impulse_vec,
                                        )
                                        // 5. Read the world back into a frame.
                                        .map(|command| {
                                            let snap = physics.snapshot();
                                            let bodies = binding
                                                .bodies()
                                                .iter()
                                                .filter_map(|b| {
                                                    snap.bodies()
                                                        .iter()
                                                        .find(|sb| sb.handle() == b.body())
                                                        .map(|sb| (b.name(), sb.transform()))
                                                })
                                                .collect();
                                            let effectors = KEY_EFFECTORS
                                                .iter()
                                                .filter_map(|&name| {
                                                    authoring
                                                        .plan_effector_id(plan, name)
                                                        .ok()
                                                        .flatten()
                                                        .and_then(|eid| {
                                                            authoring
                                                                .frame_effector_world(&pose, eid)
                                                                .map(|t| (name, t))
                                                        })
                                                })
                                                .collect();
                                            let ball_state = snap
                                                .bodies()
                                                .iter()
                                                .find(|sb| sb.handle() == ball)
                                                .map(|sb| (sb.transform(), sb.linear_velocity()));
                                            PhysicalAnimationFrame::new(FrameParts {
                                                tick: tick.raw(),
                                                phase_name,
                                                bodies,
                                                effectors,
                                                root_velocity,
                                                foot_plant,
                                                motor_count: motors.len(),
                                                motor_max_drive: motor_drive,
                                                ball_impulse: ball_impulse
                                                    .map(|(_, dir, mag)| (dir, mag.get())),
                                                gaze,
                                                contact_count: physics.latest_contacts().len(),
                                                events: authoring.frame_event_names(&pose),
                                                step_index: physics
                                                    .latest_step_record()
                                                    .step_index(),
                                                ball_transform: ball_state.map(|(t, _)| t),
                                                ball_velocity: ball_state.map(|(_, v)| v),
                                                muscle: command,
                                            })
                                        })
                                    })
                            })
                        })
                    })
                })
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical_error_code::PhysicalErrorCode;

    /// A ready-to-simulate `(authoring, plan)` for the built-in penalty kick.
    fn penalty(power: f32) -> (AnimationAuthoringApi, PlanId) {
        let mut api = AnimationAuthoringApi::new();
        let m = api.soccer_penalty_kick_v0(Ratio::new(power).unwrap());
        let plan = api.compile(m).unwrap();
        (api, plan)
    }

    /// A bound + ball-attached controller for the penalty kick.
    fn ready(authoring: &AnimationAuthoringApi, plan: PlanId) -> PhysicalAnimationApi {
        let mut sim = PhysicalAnimationApi::new();
        sim.bind_standard_humanoid(authoring, plan).unwrap();
        sim.attach_ball(authoring, plan).unwrap();
        sim
    }

    #[test]
    fn new_and_default_agree_and_advancing_unbound_or_ballless_fails() {
        let a = PhysicalAnimationApi::new();
        let b = PhysicalAnimationApi::default();
        assert!(format!("{a:?}").contains("PhysicalAnimationApi"));
        assert!(format!("{b:?}").contains("PhysicalAnimationApi"));
        let (authoring, plan) = penalty(0.7);

        // Advancing before binding fails NotBound.
        let mut unbound = PhysicalAnimationApi::new();
        assert_eq!(
            unbound
                .advance(&authoring, plan, Tick::new(0))
                .unwrap_err()
                .code(),
            PhysicalErrorCode::NotBound
        );
        // Bound but no ball fails NoBall.
        let mut no_ball = PhysicalAnimationApi::new();
        no_ball.bind_standard_humanoid(&authoring, plan).unwrap();
        assert_eq!(
            no_ball
                .advance(&authoring, plan, Tick::new(0))
                .unwrap_err()
                .code(),
            PhysicalErrorCode::NoBall
        );
    }

    #[test]
    fn advancing_the_same_inputs_twice_produces_identical_frames() {
        let (authoring, plan) = penalty(0.7);
        let mut a = ready(&authoring, plan);
        let mut b = ready(&authoring, plan);
        // Run both simulations through the strike and compare the final frames.
        let last_a = (0..40)
            .map(|t| a.advance(&authoring, plan, Tick::new(t)).unwrap())
            .last()
            .unwrap();
        let last_b = (0..40)
            .map(|t| b.advance(&authoring, plan, Tick::new(t)).unwrap())
            .last()
            .unwrap();
        assert_eq!(format!("{last_a:?}"), format!("{last_b:?}"));
    }

    #[test]
    fn approach_drives_the_pelvis_toward_the_ball_and_reports_the_objective() {
        let (authoring, plan) = penalty(0.7);
        let mut sim = ready(&authoring, plan);
        let ball_z = 0.0;
        let early = sim.advance(&authoring, plan, Tick::new(2)).unwrap();
        let early_z = sim
            .frame_body_transform(&early, "pelvis")
            .unwrap()
            .translation
            .z;
        let late = (3..11)
            .map(|t| sim.advance(&authoring, plan, Tick::new(t)).unwrap())
            .last()
            .unwrap();
        let late_z = sim
            .frame_body_transform(&late, "pelvis")
            .unwrap()
            .translation
            .z;
        assert!(
            late_z > early_z,
            "pelvis moved toward the ball under physics"
        );
        assert!(early_z <= ball_z + 1.0);
        // The approach frame reports the root-velocity objective (+Z).
        assert!(sim.frame_root_velocity(&early).unwrap().z > 0.0);
        assert_eq!(sim.frame_phase_name(&early).as_deref(), Some("approach"));
        assert_eq!(sim.frame_tick(&early), Tick::new(2));
    }

    #[test]
    fn plant_holds_the_left_foot_body_at_the_plant_spot() {
        let (authoring, plan) = penalty(0.7);
        let mut sim = ready(&authoring, plan);
        let frame = (0..17)
            .map(|t| sim.advance(&authoring, plan, Tick::new(t)).unwrap())
            .last()
            .unwrap();
        // At tick 16 (plant phase), the left-foot body is held at the plant spot.
        let foot = sim
            .frame_body_transform(&frame, "left_foot")
            .unwrap()
            .translation;
        assert!(foot.distance(Vec3::new(0.25, 0.0, -0.1)) < 1.0e-4);
        assert!(sim.frame_foot_plant(&frame).is_some());
    }

    #[test]
    fn strike_applies_a_real_ball_impulse_toward_the_net_and_drives_harder_than_backswing() {
        let (authoring, plan) = penalty(0.7);
        let mut sim = ready(&authoring, plan);
        // Capture the backswing drive on the way to the strike.
        let mut backswing_drive = 0.0;
        let mut strike_frame = None;
        (0..39).for_each(|t| {
            let f = sim.advance(&authoring, plan, Tick::new(t)).unwrap();
            (t == 26).then(|| backswing_drive = sim.frame_motor_drive(&f).get());
            (t == 38).then(|| strike_frame = Some(f));
        });
        let strike = strike_frame.unwrap();
        // A real impulse was applied to the ball this tick.
        assert!(sim.frame_ball_impulse(&strike).is_some());
        // The ball gained velocity pointing toward the net (+Z dominant).
        let vel = sim.frame_ball_velocity(&strike).unwrap();
        assert!(vel.z > 0.0, "ball flies toward the net");
        assert!(vel.length() > 1.0, "the strike imparted real speed");
        // Strike drive exceeds backswing drive.
        assert!(sim.frame_motor_drive(&strike).get() > backswing_drive);
        assert!(sim.frame_motor_count(&strike) > 0);
    }

    #[test]
    fn frame_exposes_gaze_effectors_contacts_events_and_step_index() {
        let (authoring, plan) = penalty(0.7);
        let mut sim = ready(&authoring, plan);
        let strike = (0..39)
            .map(|t| sim.advance(&authoring, plan, Tick::new(t)).unwrap())
            .last()
            .unwrap();
        assert_eq!(sim.frame_gaze(&strike), Some(Vec3::new(0.0, 0.0, 0.0))); // gaze on the ball
        assert!(sim
            .frame_effector_transform(&strike, "right_foot_instep")
            .is_some());
        assert_eq!(
            sim.frame_effector_transform(&strike, "no_such_effector"),
            None
        );
        assert_eq!(
            sim.frame_event_names(&strike),
            vec!["ball_contact".to_string()]
        );
        assert_eq!(sim.frame_step_index(&strike), 39); // 39 steps taken (ticks 0..39)
        assert_eq!(
            sim.frame_contact_count(&strike),
            sim.frame_contact_count(&strike)
        );
        assert!(sim.frame_ball_transform(&strike).is_some());
        assert_eq!(sim.frame_body_transform(&strike, "no_such_body"), None);
    }

    #[test]
    fn recover_drive_is_weaker_than_the_strike() {
        let (authoring, plan) = penalty(0.7);
        let mut sim = ready(&authoring, plan);
        let mut strike_drive = 0.0;
        let mut recover = None;
        (0..57).for_each(|t| {
            let f = sim.advance(&authoring, plan, Tick::new(t)).unwrap();
            (t == 38).then(|| strike_drive = sim.frame_motor_drive(&f).get());
            (t == 56).then(|| recover = Some(f));
        });
        // The recover phase (layer weight 0.3) drives less than the strike (1.0).
        let recover = recover.unwrap();
        assert_eq!(sim.frame_phase_name(&recover).as_deref(), Some("recover"));
        assert!(sim.frame_motor_drive(&recover).get() < strike_drive);
    }

    #[test]
    fn attach_ball_missing_plan_fails_through_authoring() {
        let mut sim = PhysicalAnimationApi::new();
        let authoring = AnimationAuthoringApi::new();
        assert_eq!(
            sim.attach_ball(&authoring, PlanId::from_raw(9))
                .unwrap_err()
                .code(),
            PhysicalErrorCode::AuthoringFailed
        );
    }

    /// Ten identical per-group phase weights.
    fn weights(w: f32) -> [Ratio; MUSCLE_GROUP_COUNT] {
        [Ratio::new(w).unwrap(); MUSCLE_GROUP_COUNT]
    }

    /// Ten identical per-group base params.
    fn profile(
        s: f32,
        d: f32,
        t: f32,
        rw: f32,
    ) -> [(Ratio, Ratio, Ratio, Ratio); MUSCLE_GROUP_COUNT] {
        [(
            Ratio::new(s).unwrap(),
            Ratio::new(d).unwrap(),
            Ratio::new(t).unwrap(),
            Ratio::new(rw).unwrap(),
        ); MUSCLE_GROUP_COUNT]
    }

    #[test]
    fn muscled_advance_records_the_command_and_muscle_free_does_not() {
        let (authoring, plan) = penalty(0.7);
        let mut sim = ready(&authoring, plan);
        // Run into the plant phase with left-foot support.
        let frame = (0..17)
            .map(|t| {
                sim.advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.6))
                    .unwrap()
            })
            .last()
            .unwrap();
        assert_eq!(sim.frame_support_mode(&frame), Some(1));
        assert!(sim.frame_center_of_mass(&frame).is_some());
        assert!(sim.frame_support_target(&frame).is_some());
        assert!(sim.frame_balance_correction(&frame).is_some());
        assert!(sim.frame_plant_strength(&frame).unwrap().get() > 0.0);
        assert!(sim.frame_recovery_damping(&frame).is_some());
        assert!(sim.frame_muscle_group_weight(&frame, 5).is_some());
        assert!(sim.frame_muscle_group_max_torque(&frame, 5).unwrap().get() > 0.0);
        assert!(sim
            .frame_muscle_report(&frame)
            .unwrap()
            .contains("support=1"));

        // The muscle-free path carries no muscle readouts.
        let plain = sim.advance(&authoring, plan, Tick::new(17)).unwrap();
        assert_eq!(sim.frame_support_mode(&plain), None);
        assert_eq!(sim.frame_center_of_mass(&plain), None);
        assert_eq!(sim.frame_muscle_report(&plain), None);
        assert_eq!(sim.frame_balance_correction(&plain), None);
        assert_eq!(sim.frame_muscle_group_weight(&plain, 0), None);
        assert_eq!(sim.frame_muscle_group_max_torque(&plain, 0), None);
        assert_eq!(sim.frame_plant_strength(&plain), None);
        assert_eq!(sim.frame_recovery_damping(&plain), None);
        assert_eq!(sim.frame_support_target(&plain), None);
    }

    #[test]
    fn muscle_strength_and_balance_strength_scale_the_command() {
        let (authoring, plan) = penalty(0.7);
        let torque_at = |strength: f32| {
            let mut sim = ready(&authoring, plan);
            sim.set_muscle_profile(profile(1.0, 0.5, 1.0, 0.6));
            sim.set_muscle_style(
                Ratio::new(strength).unwrap(),
                Ratio::new(1.0).unwrap(),
                Ratio::new(1.0).unwrap(),
            );
            let f = (0..17)
                .map(|t| {
                    sim.advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.8))
                        .unwrap()
                })
                .last()
                .unwrap();
            sim.frame_muscle_group_max_torque(&f, 5).unwrap().get()
        };
        assert!(
            torque_at(2.0) > torque_at(1.0),
            "muscle_strength scales max_torque"
        );

        let corr_at = |bal: f32| {
            let mut sim = ready(&authoring, plan);
            sim.set_muscle_style(
                Ratio::new(1.0).unwrap(),
                Ratio::new(1.0).unwrap(),
                Ratio::new(bal).unwrap(),
            );
            let f = (0..17)
                .map(|t| {
                    sim.advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.6))
                        .unwrap()
                })
                .last()
                .unwrap();
            sim.frame_balance_correction(&f).unwrap().length()
        };
        assert!(
            corr_at(2.0) > corr_at(1.0),
            "balance_strength scales the balance force"
        );
    }

    #[test]
    fn the_balance_force_pulls_the_pelvis_toward_its_support() {
        // A strongly-balanced muscled run keeps the pelvis nearer the left-foot
        // support than the muscle-free run — proof the balance force is real.
        let (authoring, plan) = penalty(0.7);
        let mut muscled = ready(&authoring, plan);
        muscled.set_muscle_style(
            Ratio::new(1.0).unwrap(),
            Ratio::new(1.0).unwrap(),
            Ratio::new(3.0).unwrap(),
        );
        let mut plain = ready(&authoring, plan);
        let (mut mf, mut pf) = (None, None);
        (0..20).for_each(|t| {
            mf = Some(
                muscled
                    .advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.7))
                    .unwrap(),
            );
            pf = Some(plain.advance(&authoring, plan, Tick::new(t)).unwrap());
        });
        let mframe = mf.unwrap();
        let support = muscled.frame_support_target(&mframe).unwrap();
        let m_pelvis = muscled
            .frame_body_transform(&mframe, "pelvis")
            .unwrap()
            .translation;
        let p_pelvis = plain
            .frame_body_transform(&pf.unwrap(), "pelvis")
            .unwrap()
            .translation;
        let horiz = |a: Vec3, b: Vec3| ((a.x - b.x).powi(2) + (a.z - b.z).powi(2)).sqrt();
        let muscled_gap = horiz(m_pelvis, support);
        let plain_gap = horiz(p_pelvis, support);
        // The muscled pelvis tracks its support at least as well as the muscle-free one.
        assert!(muscled_gap <= plain_gap + 1.0e-4);
    }

    #[test]
    fn two_identical_muscled_runs_produce_identical_frames() {
        let (authoring, plan) = penalty(0.7);
        let run = || {
            let mut sim = ready(&authoring, plan);
            (0..40)
                .map(|t| {
                    sim.advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.6))
                        .unwrap()
                })
                .last()
                .unwrap()
        };
        assert_eq!(format!("{:?}", run()), format!("{:?}", run()));
    }

    #[test]
    fn muscled_advance_fails_before_binding_and_before_a_ball() {
        let (authoring, plan) = penalty(0.7);
        let mut unbound = PhysicalAnimationApi::new();
        assert_eq!(
            unbound
                .advance_muscled(&authoring, plan, Tick::new(0), 1, weights(0.5))
                .unwrap_err()
                .code(),
            PhysicalErrorCode::NotBound
        );
        let mut no_ball = PhysicalAnimationApi::new();
        no_ball.bind_standard_humanoid(&authoring, plan).unwrap();
        assert_eq!(
            no_ball
                .advance_muscled(&authoring, plan, Tick::new(0), 1, weights(0.5))
                .unwrap_err()
                .code(),
            PhysicalErrorCode::NoBall
        );
    }

    #[test]
    fn named_transform_finds_bodies_and_defaults_when_absent() {
        let bodies = [(
            "pelvis",
            Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        )];
        assert_eq!(
            named_transform(&bodies, "pelvis").translation,
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(named_transform(&bodies, "left_foot"), Transform::IDENTITY);
    }
}
