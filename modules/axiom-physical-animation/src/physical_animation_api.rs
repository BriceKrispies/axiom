//! The single public facade for the physical-animation bridge — the controller
//! that owns an `axiom-physics` world, binds a humanoid, and advances the
//! physics-backed animation one deterministic tick at a time.

use axiom_animation_authoring::{AnimationAuthoringApi, EffectorId, PlanId};
use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

use crate::humanoid_binding::{create_ball, BoundBody, HumanoidPhysicsBinding};
use crate::ids::HumanoidHandle;
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
    /// The colliding-humanoid crowd sharing the same world — the multi-humanoid
    /// surface, indexed by [`HumanoidHandle`]. Orthogonal to the single kick
    /// `binding`/`ball` above; both live in the one `physics` world.
    crowd: Vec<HumanoidPhysicsBinding>,
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
            crowd: Vec::new(),
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

    /// Bind a *colliding* humanoid into the shared world at `origin` and return its
    /// [`HumanoidHandle`]. Unlike the single kick humanoid, its dynamic pelvis
    /// carries a solid collision sphere, so multiple crowd members resolve against
    /// one another when [`Self::advance_crowd`] steps the world.
    pub fn bind_colliding_humanoid(
        &mut self,
        authoring: &AnimationAuthoringApi,
        plan: PlanId,
        origin: Vec3,
    ) -> PhysicalResult<HumanoidHandle> {
        HumanoidPhysicsBinding::build_colliding(&mut self.physics, authoring, plan, origin).map(
            |binding| {
                let handle = HumanoidHandle::new(self.crowd.len());
                self.crowd.push(binding);
                handle
            },
        )
    }

    /// Bind a *bare* colliding body — a single dynamic collision sphere of `radius`
    /// at `origin`, no rig and no authored plan — into the shared world, returning
    /// its [`HumanoidHandle`]. The right-sized crowd member when an app only needs
    /// body-to-body collision for many agents (drive with [`Self::advance_crowd`],
    /// read back with [`Self::crowd_pelvis_transform`]).
    pub fn bind_colliding_body(
        &mut self,
        origin: Vec3,
        radius: Meters,
    ) -> PhysicalResult<HumanoidHandle> {
        HumanoidPhysicsBinding::build_bare(&mut self.physics, origin, radius.get()).map(|binding| {
            let handle = HumanoidHandle::new(self.crowd.len());
            self.crowd.push(binding);
            handle
        })
    }

    /// Advance the crowd one deterministic tick: force each listed humanoid's
    /// dynamic pelvis toward its desired ground velocity (an anti-gravity hold plus
    /// the approach drive), then step the shared world once so their pelvis spheres
    /// collide and resolve. An unknown handle fails with `NotBound` before the step.
    pub fn advance_crowd(
        &mut self,
        drives: &[(HumanoidHandle, Vec3)],
        tick: Tick,
    ) -> PhysicalResult<()> {
        let PhysicalAnimationApi { physics, crowd, .. } = self;
        drives
            .iter()
            .try_for_each(|&(handle, velocity)| {
                crowd
                    .get(handle.index())
                    .ok_or_else(|| PhysicalError::not_bound("unknown crowd humanoid handle"))
                    .and_then(|binding| {
                        binding
                            .bodies()
                            .iter()
                            .filter(|b| b.dynamic())
                            .try_for_each(|b| drive_dynamic(physics, b, Some(velocity), Vec3::ZERO))
                    })
            })
            .and_then(|()| phys(physics.step(runtime_step(tick.raw()))))
    }

    /// The world transform of crowd member `handle`'s pelvis after the last step —
    /// the resolved position an app reads back. `NotBound` for an unknown handle.
    pub fn crowd_pelvis_transform(&self, handle: HumanoidHandle) -> PhysicalResult<Transform> {
        let snapshot = self.physics.snapshot();
        self.crowd
            .get(handle.index())
            .and_then(HumanoidPhysicsBinding::dynamic_body)
            .and_then(|body| {
                snapshot
                    .bodies()
                    .iter()
                    .find(|sb| sb.handle() == body)
                    .map(|sb| sb.transform())
            })
            .ok_or_else(|| PhysicalError::not_bound("unknown crowd humanoid handle"))
    }

    /// How many contacts the shared world resolved on the last step — non-zero
    /// when crowd pelvises are colliding.
    pub fn crowd_contact_count(&self) -> usize {
        self.physics.latest_contacts().len()
    }

    /// Whether crowd members `a` and `b` had their bodies in solver contact on
    /// the last step — the authoritative "these two bodies are actually
    /// touching" query. Unlike a caller-side distance test, this reads the real
    /// contact manifolds the de-penetration step already produced, so no extra
    /// simulation is paid for the answer. `false` for an unknown handle or when
    /// the pair never met.
    pub fn crowd_bodies_in_contact(&self, a: HumanoidHandle, b: HumanoidHandle) -> bool {
        let body = |handle: HumanoidHandle| {
            self.crowd
                .get(handle.index())
                .and_then(HumanoidPhysicsBinding::dynamic_body)
        };
        body(a)
            .zip(body(b))
            .map(|(ba, bb)| {
                self.physics.latest_contacts().iter().any(|contact| {
                    ((contact.body_a() == ba) & (contact.body_b() == bb))
                        | ((contact.body_a() == bb) & (contact.body_b() == ba))
                })
            })
            .unwrap_or(false)
    }

    /// Resolve the crowd one deterministic tick as a *de-penetration* pass: snap
    /// each listed body to the caller's authoritative position and velocity, then
    /// step the shared world once so overlapping bodies push apart and exchange
    /// momentum. The caller reads the resolved position/velocity back as the new
    /// authoritative state. This is the sim-authoritative counterpart to
    /// [`Self::advance_crowd`] (which force-drives instead of snapping). An unknown
    /// handle fails `NotBound` before the step.
    pub fn resolve_crowd(
        &mut self,
        placements: &[(HumanoidHandle, Vec3, Vec3)],
        tick: Tick,
    ) -> PhysicalResult<()> {
        let PhysicalAnimationApi { physics, crowd, .. } = self;
        placements
            .iter()
            .try_for_each(|&(handle, position, velocity)| {
                crowd
                    .get(handle.index())
                    .and_then(HumanoidPhysicsBinding::dynamic_body)
                    .ok_or_else(|| PhysicalError::not_bound("unknown crowd humanoid handle"))
                    .and_then(|body| {
                        phys(physics
                            .set_body_transform(body, Transform::from_translation(position)))
                        .and_then(|()| phys(physics.set_body_velocity(body, velocity, Vec3::ZERO)))
                    })
            })
            .and_then(|()| phys(physics.step(runtime_step(tick.raw()))))
    }

    /// The linear velocity of crowd member `handle`'s body after the last step —
    /// the resolved velocity (post-collision) an app reads back alongside
    /// [`Self::crowd_pelvis_transform`]. `NotBound` for an unknown handle.
    pub fn crowd_pelvis_velocity(&self, handle: HumanoidHandle) -> PhysicalResult<Vec3> {
        let snapshot = self.physics.snapshot();
        self.crowd
            .get(handle.index())
            .and_then(HumanoidPhysicsBinding::dynamic_body)
            .and_then(|body| {
                snapshot
                    .bodies()
                    .iter()
                    .find(|sb| sb.handle() == body)
                    .map(|sb| sb.linear_velocity())
            })
            .ok_or_else(|| PhysicalError::not_bound("unknown crowd humanoid handle"))
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
            crowd: _,
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
mod tests;
