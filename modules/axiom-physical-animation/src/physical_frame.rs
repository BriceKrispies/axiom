//! [`PhysicalAnimationFrame`] — the deterministic result of advancing the
//! physics-backed animation by one tick: the physics body transforms, key
//! effector world transforms, the active authored phase, the physical objectives
//! applied, the physics step index, and the ball's state.
//!
//! Pure data derived by the controller from the authored objectives and the
//! post-step physics snapshot. Same initial state + same inputs → identical
//! frame (the physics engine is same-binary deterministic).

use axiom_animation_authoring::EffectorId;
use axiom_math::{Transform, Vec3};

use crate::virtual_muscle::VirtualMuscleCommand;

/// A physics-backed animation frame at one tick. Held opaquely by callers and
/// read through the [`crate::PhysicalAnimationApi`] `frame_*` accessors.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalAnimationFrame {
    tick: u64,
    phase_name: Option<String>,
    bodies: Vec<(&'static str, Transform)>,
    effectors: Vec<(&'static str, Transform)>,
    root_velocity: Option<Vec3>,
    foot_plant: Option<(EffectorId, Vec3)>,
    motor_count: usize,
    motor_max_drive: f32,
    ball_impulse: Option<(Vec3, f32)>,
    gaze: Option<Vec3>,
    contact_count: usize,
    events: Vec<String>,
    step_index: u64,
    ball_transform: Option<Transform>,
    ball_velocity: Option<Vec3>,
    /// The virtual-muscle command applied this tick (`None` on the muscle-free
    /// `advance` path).
    muscle: Option<VirtualMuscleCommand>,
}

/// The parts of a frame the controller assembles (bundled to keep the
/// constructor's argument list small).
pub(crate) struct FrameParts {
    pub(crate) tick: u64,
    pub(crate) phase_name: Option<String>,
    pub(crate) bodies: Vec<(&'static str, Transform)>,
    pub(crate) effectors: Vec<(&'static str, Transform)>,
    pub(crate) root_velocity: Option<Vec3>,
    pub(crate) foot_plant: Option<(EffectorId, Vec3)>,
    pub(crate) motor_count: usize,
    pub(crate) motor_max_drive: f32,
    pub(crate) ball_impulse: Option<(Vec3, f32)>,
    pub(crate) gaze: Option<Vec3>,
    pub(crate) contact_count: usize,
    pub(crate) events: Vec<String>,
    pub(crate) step_index: u64,
    pub(crate) ball_transform: Option<Transform>,
    pub(crate) ball_velocity: Option<Vec3>,
    pub(crate) muscle: Option<VirtualMuscleCommand>,
}

impl PhysicalAnimationFrame {
    /// Assemble a frame from its parts.
    pub(crate) fn new(parts: FrameParts) -> Self {
        PhysicalAnimationFrame {
            tick: parts.tick,
            phase_name: parts.phase_name,
            bodies: parts.bodies,
            effectors: parts.effectors,
            root_velocity: parts.root_velocity,
            foot_plant: parts.foot_plant,
            motor_count: parts.motor_count,
            motor_max_drive: parts.motor_max_drive,
            ball_impulse: parts.ball_impulse,
            gaze: parts.gaze,
            contact_count: parts.contact_count,
            events: parts.events,
            step_index: parts.step_index,
            ball_transform: parts.ball_transform,
            ball_velocity: parts.ball_velocity,
            muscle: parts.muscle,
        }
    }

    /// The virtual-muscle command applied this tick, if the muscled path was used.
    pub(crate) fn muscle(&self) -> Option<&VirtualMuscleCommand> {
        self.muscle.as_ref()
    }

    /// The tick this frame was sampled at.
    pub(crate) fn tick(&self) -> u64 {
        self.tick
    }

    /// The active authored phase name, if any.
    pub(crate) fn phase_name(&self) -> Option<&str> {
        self.phase_name.as_deref()
    }

    /// The world transform of the bound body named `name`, if bound.
    pub(crate) fn body_transform(&self, name: &str) -> Option<Transform> {
        self.bodies
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| *t)
    }

    /// The world transform of the key effector named `name`, if present.
    pub(crate) fn effector_transform(&self, name: &str) -> Option<Transform> {
        self.effectors
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| *t)
    }

    /// The applied root-velocity objective, if any.
    pub(crate) fn root_velocity(&self) -> Option<Vec3> {
        self.root_velocity
    }

    /// The applied foot-plant objective, if any.
    pub(crate) fn foot_plant(&self) -> Option<(EffectorId, Vec3)> {
        self.foot_plant
    }

    /// The number of active joint-motor objectives.
    pub(crate) fn motor_count(&self) -> usize {
        self.motor_count
    }

    /// The maximum motor drive across the active joint-motor objectives.
    pub(crate) fn motor_max_drive(&self) -> f32 {
        self.motor_max_drive
    }

    /// The applied ball-impulse `(direction, magnitude)`, if any.
    pub(crate) fn ball_impulse(&self) -> Option<(Vec3, f32)> {
        self.ball_impulse
    }

    /// The active gaze objective, if any.
    pub(crate) fn gaze(&self) -> Option<Vec3> {
        self.gaze
    }

    /// The number of contacts the physics engine resolved this step.
    pub(crate) fn contact_count(&self) -> usize {
        self.contact_count
    }

    /// The names of the events emitted this tick.
    pub(crate) fn events(&self) -> &[String] {
        &self.events
    }

    /// The physics step index of this frame.
    pub(crate) fn step_index(&self) -> u64 {
        self.step_index
    }

    /// The ball's world transform after the step, if a ball is attached.
    pub(crate) fn ball_transform(&self) -> Option<Transform> {
        self.ball_transform
    }

    /// The ball's linear velocity after the step, if a ball is attached.
    pub(crate) fn ball_velocity(&self) -> Option<Vec3> {
        self.ball_velocity
    }
}
