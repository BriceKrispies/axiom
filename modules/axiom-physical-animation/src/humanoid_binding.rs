//! [`HumanoidPhysicsBinding`] — the mapping from the procedural humanoid rig to
//! `axiom-physics` bodies, and the deterministic builder that creates them.
//!
//! The smallest standard binding for the penalty-kick slice: one physics body per
//! bound joint (root/pelvis, chest, head, both arms, both legs) plus the three key
//! foot effectors (`left_foot_sole`, `right_foot_sole`, `right_foot_instep`) that
//! address a foot body. The **pelvis** is a *dynamic* body (force-driven during
//! the approach); every other limb is a *kinematic* body driven from the authored
//! pose. Humanoid colliders are triggers, so they never solver-collide with the
//! dynamic ball — the ball moves purely under its strike impulse and gravity.

use axiom_animation_authoring::{AnimationAuthoringApi, EffectorId, JointId, PlanId};
use axiom_kernel::{Meters, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};

use crate::physical_result::{auth, phys, PhysicalResult};

/// The dynamic-body mass (kilograms). Kinematic bodies ignore it.
const LIMB_MASS: f32 = 1.0;
/// The uniform collider half-extents (metres) for a humanoid body.
const HALF: f32 = 0.09;
/// The pelvis collision-sphere radius (metres) for a *colliding* humanoid — a
/// torso-width sphere so two crowd members meet body-to-body, not point-to-point.
/// A sphere (not the default box) because `axiom-physics` resolves sphere/sphere
/// contacts today but not box/box.
const PELVIS_RADIUS: f32 = 0.2;

/// The bound joints: `(joint name, is_dynamic)`. Only the pelvis is dynamic.
const BODY_SPECS: [(&str, bool); 13] = [
    ("pelvis", true),
    ("chest", false),
    ("head", false),
    ("left_upper_arm", false),
    ("left_forearm", false),
    ("right_upper_arm", false),
    ("right_forearm", false),
    ("left_thigh", false),
    ("left_shin", false),
    ("left_foot", false),
    ("right_thigh", false),
    ("right_shin", false),
    ("right_foot", false),
];

/// The key foot effectors: `(effector name, owning foot-body joint name)`.
const FOOT_SPECS: [(&str, &str); 3] = [
    ("left_foot_sole", "left_foot"),
    ("right_foot_sole", "right_foot"),
    ("right_foot_instep", "right_foot"),
];

/// A body maker: create a physics body at a transform (kinematic ignores the
/// mass). Returns a bridge result — the physics-private error is funneled inside.
type BodyMaker = fn(&mut PhysicsApi, Transform, Ratio) -> PhysicalResult<PhysicsBodyHandle>;

fn make_kinematic(
    physics: &mut PhysicsApi,
    at: Transform,
    _mass: Ratio,
) -> PhysicalResult<PhysicsBodyHandle> {
    phys(physics.create_kinematic_body(at))
}

fn make_dynamic(
    physics: &mut PhysicsApi,
    at: Transform,
    mass: Ratio,
) -> PhysicalResult<PhysicsBodyHandle> {
    phys(physics.create_dynamic_body(at, mass))
}

/// Body makers indexed by `is_dynamic as usize` — a table lookup, not a branch.
const BODY_MAKERS: [BodyMaker; 2] = [make_kinematic, make_dynamic];

/// A trigger surface material shared by the humanoid/ball bodies (obtained via
/// inference — `PhysicsMaterial` is not a nameable public type).
fn surface_material(
    physics: &mut PhysicsApi,
    body: PhysicsBodyHandle,
    half: Vec3,
    trigger: bool,
) -> PhysicalResult<PhysicsBodyHandle> {
    phys(PhysicsApi::material(
        Ratio::finite_or_zero(0.5),
        Ratio::finite_or_zero(0.0),
        Ratio::finite_or_zero(1.0),
    ))
    .and_then(|material| {
        phys(physics.attach_box_collider(body, half, material, trigger)).map(|_| body)
    })
}

/// A collider maker: attach a body's collider and return the body.
type ColliderMaker = fn(&mut PhysicsApi, PhysicsBodyHandle) -> PhysicalResult<PhysicsBodyHandle>;

/// The default collider: a trigger box that never solver-collides (the kick
/// path, and every kinematic limb of a colliding humanoid).
fn trigger_box(physics: &mut PhysicsApi, body: PhysicsBodyHandle) -> PhysicalResult<PhysicsBodyHandle> {
    surface_material(physics, body, Vec3::new(HALF, HALF, HALF), true)
}

/// Attach a solid (non-trigger) collision sphere of `radius` metres, so this body
/// resolves against the other crowd members' spheres in the shared world.
fn collision_sphere_of(
    physics: &mut PhysicsApi,
    body: PhysicsBodyHandle,
    radius: f32,
) -> PhysicalResult<PhysicsBodyHandle> {
    phys(PhysicsApi::material(
        Ratio::finite_or_zero(0.5),
        Ratio::finite_or_zero(0.0),
        Ratio::finite_or_zero(1.0),
    ))
    .and_then(|material| {
        phys(Meters::new(radius)).and_then(|r| {
            phys(physics.attach_sphere_collider(body, r, material, false)).map(|_| body)
        })
    })
}

/// A colliding humanoid's dynamic pelvis: the default torso-width collision sphere.
fn collision_sphere(
    physics: &mut PhysicsApi,
    body: PhysicsBodyHandle,
) -> PhysicalResult<PhysicsBodyHandle> {
    collision_sphere_of(physics, body, PELVIS_RADIUS)
}

/// Collider makers indexed by `(colliding & dynamic) as usize` — a table lookup,
/// not a branch: `[trigger box (default), collision sphere (a colliding pelvis)]`.
const COLLIDER_MAKERS: [ColliderMaker; 2] = [trigger_box, collision_sphere];

/// Translate a body's authored world transform by the humanoid's spawn origin.
fn offset_transform(world: Transform, origin: Vec3) -> Transform {
    Transform::new(world.translation.add(origin), world.rotation, world.scale)
}

/// One bound humanoid body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BoundBody {
    name: &'static str,
    joint: JointId,
    body: PhysicsBodyHandle,
    dynamic: bool,
}

impl BoundBody {
    /// The bound joint name.
    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    /// The authored joint this body tracks.
    pub(crate) fn joint(&self) -> JointId {
        self.joint
    }

    /// The physics body handle.
    pub(crate) fn body(&self) -> PhysicsBodyHandle {
        self.body
    }

    /// Whether this body is dynamic (else kinematic).
    pub(crate) fn dynamic(&self) -> bool {
        self.dynamic
    }
}

/// One bound foot effector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BoundFoot {
    effector: EffectorId,
    body: PhysicsBodyHandle,
}

impl BoundFoot {
    /// The authored effector.
    pub(crate) fn effector(&self) -> EffectorId {
        self.effector
    }

    /// The foot body the effector addresses.
    pub(crate) fn body(&self) -> PhysicsBodyHandle {
        self.body
    }
}

/// The humanoid → physics body mapping.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HumanoidPhysicsBinding {
    bodies: Vec<BoundBody>,
    feet: Vec<BoundFoot>,
}

impl HumanoidPhysicsBinding {
    /// The bound bodies in order.
    pub(crate) fn bodies(&self) -> &[BoundBody] {
        &self.bodies
    }

    /// The foot body that effector `effector` addresses, if bound.
    pub(crate) fn foot_body_for(&self, effector: EffectorId) -> Option<PhysicsBodyHandle> {
        self.feet
            .iter()
            .find(|f| f.effector() == effector)
            .map(BoundFoot::body)
    }

    /// The dynamic (pelvis) body handle, if any — the body a crowd drive forces
    /// and a crowd readback reports.
    pub(crate) fn dynamic_body(&self) -> Option<PhysicsBodyHandle> {
        self.bodies
            .iter()
            .find(|b| b.dynamic())
            .map(BoundBody::body)
    }

    /// Build the standard humanoid binding: a physics body per bound joint at its
    /// authored world transform (sampled at tick 0), then the foot effectors. The
    /// kick path — triggered colliders at the plan origin, no solver collision.
    pub(crate) fn build_standard(
        physics: &mut PhysicsApi,
        authoring: &AnimationAuthoringApi,
        plan: PlanId,
    ) -> PhysicalResult<Self> {
        Self::build(physics, authoring, plan, Vec3::ZERO, false)
    }

    /// Build a *colliding* humanoid at `origin`: identical rig, but its dynamic
    /// pelvis carries a solid collision sphere so it resolves against the other
    /// crowd members' pelvises in the shared world.
    pub(crate) fn build_colliding(
        physics: &mut PhysicsApi,
        authoring: &AnimationAuthoringApi,
        plan: PlanId,
        origin: Vec3,
    ) -> PhysicalResult<Self> {
        Self::build(physics, authoring, plan, origin, true)
    }

    /// Build a *bare* colliding body: one dynamic collision sphere of `radius` at
    /// `origin`, no authored rig or feet. The right-sized crowd member for an app
    /// (e.g. a field of game agents) that only needs body-to-body collision, not a
    /// full humanoid. Driven and read back through the same crowd path.
    pub(crate) fn build_bare(
        physics: &mut PhysicsApi,
        origin: Vec3,
        radius: f32,
    ) -> PhysicalResult<Self> {
        let at = Transform::from_translation(origin);
        make_dynamic(physics, at, Ratio::finite_or_zero(LIMB_MASS))
            .and_then(|body| collision_sphere_of(physics, body, radius))
            .map(|body| HumanoidPhysicsBinding {
                bodies: vec![BoundBody {
                    name: "body",
                    joint: JointId::from_raw(0),
                    body,
                    dynamic: true,
                }],
                feet: Vec::new(),
            })
    }

    /// The shared builder: `origin` offsets every body; `colliding` gives the
    /// dynamic pelvis a solid sphere (else the default trigger box). Deterministic
    /// for identical inputs.
    fn build(
        physics: &mut PhysicsApi,
        authoring: &AnimationAuthoringApi,
        plan: PlanId,
        origin: Vec3,
        colliding: bool,
    ) -> PhysicalResult<Self> {
        auth(authoring.sample(plan, Tick::new(0))).and_then(|frame| {
            BODY_SPECS
                .iter()
                .filter_map(|&(name, dynamic)| {
                    authoring
                        .plan_joint_id(plan, name)
                        .ok()
                        .flatten()
                        .and_then(|joint| {
                            authoring
                                .frame_joint_world(&frame, joint)
                                .map(|world| (name, dynamic, joint, offset_transform(world, origin)))
                        })
                })
                .map(|(name, dynamic, joint, world)| {
                    create_humanoid_body(physics, world, dynamic, colliding).map(|body| BoundBody {
                        name,
                        joint,
                        body,
                        dynamic,
                    })
                })
                .collect::<PhysicalResult<Vec<_>>>()
                .and_then(|bodies| {
                    build_feet(authoring, plan, &bodies)
                        .map(|feet| HumanoidPhysicsBinding { bodies, feet })
                })
        })
    }
}

/// Create one humanoid body (dynamic or kinematic) at `world`. Its collider is a
/// trigger box by default; a *colliding* humanoid's dynamic pelvis
/// (`colliding & dynamic`) instead gets a solid collision sphere.
fn create_humanoid_body(
    physics: &mut PhysicsApi,
    world: Transform,
    dynamic: bool,
    colliding: bool,
) -> PhysicalResult<PhysicsBodyHandle> {
    let mass = Ratio::finite_or_zero(LIMB_MASS);
    BODY_MAKERS[dynamic as usize](physics, world, mass)
        .and_then(|body| COLLIDER_MAKERS[(colliding & dynamic) as usize](physics, body))
}

/// Resolve the foot effectors to `(effector, foot body)` pairs.
fn build_feet(
    authoring: &AnimationAuthoringApi,
    plan: PlanId,
    bodies: &[BoundBody],
) -> PhysicalResult<Vec<BoundFoot>> {
    FOOT_SPECS
        .iter()
        .filter_map(|&(effector_name, body_name)| {
            authoring
                .plan_effector_id(plan, effector_name)
                .ok()
                .flatten()
                .and_then(|effector| {
                    bodies
                        .iter()
                        .find(|b| b.name() == body_name)
                        .map(|b| BoundFoot {
                            effector,
                            body: b.body(),
                        })
                })
        })
        .map(Ok)
        .collect()
}

/// The ball radius (metres), exposed for the controller that spawns the ball.
pub(crate) const BALL_RADIUS: f32 = 0.11;

/// Create the dynamic soccer-ball body: a solid sphere at `center`, so it moves
/// under gravity and its strike impulse. Returned handle is the ball body.
pub(crate) fn create_ball(
    physics: &mut PhysicsApi,
    center: Vec3,
    mass: f32,
) -> PhysicalResult<PhysicsBodyHandle> {
    let at = Transform::from_translation(center);
    phys(physics.create_dynamic_body(at, Ratio::finite_or_zero(mass))).and_then(|body| {
        phys(PhysicsApi::material(
            Ratio::finite_or_zero(0.5),
            Ratio::finite_or_zero(0.4),
            Ratio::finite_or_zero(1.0),
        ))
        .and_then(|material| {
            phys(Meters::new(BALL_RADIUS)).and_then(|radius| {
                phys(physics.attach_sphere_collider(body, radius, material, false)).map(|_| body)
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_of(binding: &HumanoidPhysicsBinding, name: &str) -> Option<PhysicsBodyHandle> {
        binding
            .bodies()
            .iter()
            .find(|b| b.name() == name)
            .map(BoundBody::body)
    }

    fn penalty_plan() -> (AnimationAuthoringApi, PlanId) {
        let mut api = AnimationAuthoringApi::new();
        let m = api.soccer_penalty_kick_v0(Ratio::new(0.7).unwrap());
        let plan = api.compile(m).unwrap();
        (api, plan)
    }

    #[test]
    fn building_the_standard_binding_is_deterministic_and_complete() {
        let (authoring, plan) = penalty_plan();
        let mut pa = PhysicsApi::new();
        let mut pb = PhysicsApi::new();
        let a = HumanoidPhysicsBinding::build_standard(&mut pa, &authoring, plan).unwrap();
        let b = HumanoidPhysicsBinding::build_standard(&mut pb, &authoring, plan).unwrap();
        // Same handles + names in the same order — deterministic.
        assert_eq!(a, b);
        assert_eq!(a.bodies().len(), 13);
        // The pelvis is dynamic; a limb is kinematic.
        assert!(a
            .bodies()
            .iter()
            .find(|x| x.name() == "pelvis")
            .unwrap()
            .dynamic());
        assert!(!a
            .bodies()
            .iter()
            .find(|x| x.name() == "left_foot")
            .unwrap()
            .dynamic());
        assert!(body_of(&a, "head").is_some());
        assert_eq!(body_of(&a, "no_such_body"), None);
    }

    #[test]
    fn foot_effectors_address_their_foot_bodies() {
        let (authoring, plan) = penalty_plan();
        let mut physics = PhysicsApi::new();
        let binding =
            HumanoidPhysicsBinding::build_standard(&mut physics, &authoring, plan).unwrap();
        let left_sole = authoring
            .plan_effector_id(plan, "left_foot_sole")
            .unwrap()
            .unwrap();
        let left_foot = body_of(&binding, "left_foot").unwrap();
        assert_eq!(binding.foot_body_for(left_sole), Some(left_foot));
        let missing = authoring
            .plan_effector_id(plan, "head_gaze")
            .unwrap()
            .unwrap();
        assert_eq!(binding.foot_body_for(missing), None); // head_gaze is not a bound foot
    }

    #[test]
    fn a_too_small_world_fails_the_build_through_physics() {
        // A one-body world cannot hold the 13 humanoid bodies: the build funnels
        // the physics capacity failure into a bridge error.
        let (authoring, plan) = penalty_plan();
        let mut physics = PhysicsApi::with_config(
            Vec3::new(0.0, -9.8, 0.0),
            8,
            1,
            8,
            1,
            true,
            Ratio::new(0.0).unwrap(),
            Ratio::new(0.0).unwrap(),
        )
        .unwrap();
        let err =
            HumanoidPhysicsBinding::build_standard(&mut physics, &authoring, plan).unwrap_err();
        assert_eq!(
            err.code(),
            crate::physical_error_code::PhysicalErrorCode::PhysicsFailed
        );
    }

    #[test]
    fn a_missing_plan_fails_the_build_through_authoring() {
        let authoring = AnimationAuthoringApi::new();
        let mut physics = PhysicsApi::new();
        let err =
            HumanoidPhysicsBinding::build_standard(&mut physics, &authoring, PlanId::from_raw(9))
                .unwrap_err();
        assert_eq!(
            err.code(),
            crate::physical_error_code::PhysicalErrorCode::AuthoringFailed
        );
    }

    #[test]
    fn the_ball_body_is_created_as_a_dynamic_sphere() {
        let mut physics = PhysicsApi::new();
        let ball = create_ball(&mut physics, Vec3::new(0.0, BALL_RADIUS, 0.0), 0.4).unwrap();
        let snap = physics.snapshot();
        assert!(snap.bodies().iter().any(|b| b.handle() == ball));
    }
}
