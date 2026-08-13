//! Posing one creature: the frame where the path, the gait and the inverse
//! kinematics meet.
//!
//! Given a rig, its limb chains, the loop and how far along it the creature is,
//! this produces one **world transform per bone**. Nothing here is stateful and
//! nothing here reads a clock — the same travel distance always yields the same
//! pose, which is what makes `tick → pose` a pure function all the way down.
//!
//! ## The three passes
//!
//! 1. **The body.** The root sits on the terrain at the path point, turned to
//!    face the tangent, dropped by a crouch, and bobbed and pitched by the gait
//!    cycle. Every non-limb bone then resolves from it by plain forward
//!    kinematics, with a handful of named bones (the dog's tail and ears, the
//!    human's waist) given a small extra rotation from the same cycle.
//! 2. **The feet.** Each grounded limb's contact is *planted*: during stance its
//!    world position is a function of the **step number** alone, so it does not
//!    move by so much as a float while the body travels over it. During swing it
//!    eases forward exactly one stride and arcs up on the way. This is the
//!    difference between a run and a slide, and it is the reason
//!    [`crate::leg_ik::stride_phase`] reports the step number rather than just a
//!    fraction.
//! 3. **The legs.** Two-bone inverse kinematics from the hip the forward pass
//!    resolved to the planted contact, bending toward a pole taken from the
//!    creature's own facing. The solved bones then *overwrite* the forward pass's
//!    world transforms for that limb.
//!
//! ## Why the body drops
//!
//! Both creatures are authored standing, with their legs very nearly straight —
//! which is anatomically right and animationally useless: a straight leg has no
//! reach left to swing a foot forward with, so every stride would clamp. The
//! crouch buys that reach back. It is not a fudge; it is what a running animal
//! actually does, and the stride lengths below are sized against the reach it
//! leaves (`√(reach² − hip_height²)` is the furthest a foot can be from under
//! its own hip, and each stride's half-excursion sits comfortably inside it).

use axiom_math::{Quat, Transform, Vec3};

use crate::creature_rig::{aim, CreatureRig, LimbChain};
use crate::leg_ik::{ease, solve_two_bone, stride_phase, swing_lift};
use crate::locomotion::LoopPath;
use crate::terrain::ground_y;

const TAU: f32 = core::f32::consts::TAU;

/// One extra rotation layered onto a named bone's rest pose: which bone, about
/// which local axis, how far, and how many times per stride.
pub type Flex = (&'static str, Vec3, f32, f32);

/// Everything about how one creature runs. Every field is a dial with a
/// physical meaning; none of them is a magic number standing in for another.
#[derive(Debug, Clone, Copy)]
pub struct CreaturePose {
    /// The uniform world scale the creature is presented at.
    pub scale: f32,
    /// One full step, in world units. Sized against the leg's spare reach.
    pub stride: f32,
    /// The fraction of a step a foot spends on the ground.
    pub duty: f32,
    /// How far ahead of the hip a foot plants, as a fraction of a stride. At
    /// exactly `duty / 2` the stance is symmetric about the hip.
    pub lead: f32,
    /// Peak height of the swinging foot's arc, in world units.
    pub lift: f32,
    /// How far the body is carried below its standing height, in world units.
    pub crouch: f32,
    /// Peak vertical bob, in world units. Twice per stride: once per foot.
    pub bob: f32,
    /// How far above or below the body's own support height a foot may follow
    /// the terrain, in world units — the relief this creature's legs can
    /// actually absorb. See [`Plant::levelled`].
    pub relief: f32,
    /// Steady forward pitch, in radians. Negative drops the nose.
    pub lean: f32,
    /// How far the pitch oscillates with the gait, in radians.
    pub pitch_swing: f32,
    /// Where a carried hand rests, in creature-local units, `+x` side.
    pub arm_carry: Vec3,
    /// Fore/aft hand swing, in creature-local units.
    pub arm_swing: f32,
    /// Vertical hand swing, in creature-local units.
    pub arm_rise: f32,
    /// Bones given an extra gait-driven rotation on top of their rest pose.
    pub flex: &'static [Flex],
}

/// The dog's trot.
///
/// A dog's legs are short against its body and nearly straight when it stands:
/// the front leg is 5.52 units long at presentation scale and its shoulder sits
/// 6.2 above the ground, so a *standing* dog has no spare reach at all. The
/// crouch is what buys it — and the stride is then sized against what is left,
/// with the worst hip-to-paw distance over a full lap staying comfortably under
/// the leg's length (`tests/locomotion.rs` measures exactly that, so a tuning
/// change that over-reaches a leg fails the suite rather than quietly skating).
pub const DOG_GAIT: CreaturePose = CreaturePose {
    scale: 10.0,
    stride: 9.0,
    duty: 0.52,
    lead: 0.26,
    lift: 0.9,
    crouch: 2.3,
    bob: 0.22,
    relief: 1.1,
    lean: -0.05,
    pitch_swing: 0.020,
    arm_carry: Vec3::ZERO,
    arm_swing: 0.0,
    arm_rise: 0.0,
    flex: &[
        ("dog-tail-base", Vec3::UNIT_Y, 0.26, 1.0),
        ("dog-tail-tip", Vec3::UNIT_Y, 0.34, 1.0),
        ("dog-ear-l", Vec3::UNIT_X, 0.20, 2.0),
        ("dog-ear-r", Vec3::UNIT_X, 0.20, 2.0),
        ("dog-spine", Vec3::UNIT_X, 0.03, 2.0),
    ],
};

/// The human's run: a longer stride off longer legs, arms carried bent and
/// swung counter to them (see the offsets in `creature_human`).
///
/// The legs are 8.61 long against a 9.4 hip, so — as with the dog — the crouch
/// is what makes a stride possible at all rather than a stylistic choice.
pub const HUMAN_GAIT: CreaturePose = CreaturePose {
    scale: 10.0,
    stride: 15.0,
    duty: 0.45,
    lead: 0.225,
    lift: 1.4,
    crouch: 2.7,
    bob: 0.30,
    relief: 1.0,
    lean: -0.10,
    pitch_swing: 0.020,
    arm_carry: Vec3::new(0.24, 1.02, -0.22),
    arm_swing: 0.20,
    arm_rise: 0.05,
    flex: &[
        // Shoulders counter-rotate against the hips — the thing that stops a
        // running figure reading as a marching one.
        ("human-pelvis", Vec3::UNIT_Y, -0.08, 1.0),
        ("human-torso", Vec3::UNIT_Y, 0.14, 1.0),
    ],
};

impl CreaturePose {
    /// Every bone's world transform, in rig order, for a creature `travel`
    /// units along `path`.
    pub fn pose(
        &self,
        rig: &CreatureRig,
        limbs: &[LimbChain],
        path: &LoopPath,
        travel: f32,
    ) -> Vec<Transform> {
        // The feet first: their plant points depend only on the path and the
        // travel distance, never on the body — so they can be resolved before
        // it, and the body can then be stood on the ground they are actually on.
        let plants: Vec<Option<Plant>> = limbs
            .iter()
            .map(|limb| limb.grounded.then(|| self.plant(limb, path, travel)))
            .collect();
        let support = support(&plants);
        // Now that the body's own ground is known, hold each foot within the
        // relief its legs can absorb (see `Plant::levelled`).
        let plants: Vec<Option<Plant>> = plants
            .into_iter()
            .map(|plant| plant.map(|p| p.levelled(support, self.relief)))
            .collect();
        let root = self.body(path, travel, support);
        let cycle = travel / self.stride;
        let locals: Vec<Transform> = rig
            .parts()
            .iter()
            .map(|part| {
                self.flex
                    .iter()
                    .find(|(name, _, _, _)| *name == part.name)
                    .map(|(_, axis, amount, harmonic)| {
                        let angle = amount * (TAU * harmonic * cycle).sin();
                        let turn = Quat::from_axis_angle(*axis, angle).unwrap_or(Quat::IDENTITY);
                        Transform::combine(part.rest, Transform::from_rotation(turn))
                    })
                    .unwrap_or(part.rest)
            })
            .collect();

        let mut world = rig.resolve(root, &locals);
        limbs
            .iter()
            .zip(plants)
            .for_each(|(limb, plant)| self.solve_limb(rig, limb, plant, cycle, root, &mut world));
        world
    }

    /// The body root: standing on `support`, facing down the path, crouched,
    /// bobbing and pitching with the gait.
    fn body(&self, path: &LoopPath, travel: f32, support: Option<f32>) -> Transform {
        let here = path.at(travel);
        let cycle = travel / self.stride;
        // Twice per stride, because there are two ground contacts in one: the
        // body sinks into each foot strike and rises between them.
        let bob = -self.bob * (TAU * 2.0 * cycle).cos();
        let pitch = self.lean + self.pitch_swing * (TAU * 2.0 * cycle).sin();
        // Both creatures are authored facing `-Z`, which is exactly what `aim`
        // sends down the path tangent — no correction turn is needed.
        let facing = aim(here.forward, Vec3::UNIT_Y).multiply(
            Quat::from_axis_angle(Vec3::UNIT_X, pitch).unwrap_or(Quat::IDENTITY),
        );
        Transform::new(
            Vec3::new(
                here.position.x,
                support.unwrap_or(here.position.y) - self.crouch + bob,
                here.position.z,
            ),
            facing,
            Vec3::new(self.scale, self.scale, self.scale),
        )
    }

    /// Solve one limb and write its bones over the forward pass's answers.
    fn solve_limb(
        &self,
        rig: &CreatureRig,
        limb: &LimbChain,
        plant: Option<Plant>,
        cycle: f32,
        root: Transform,
        world: &mut [Transform],
    ) {
        let Some(upper) = rig.index_of(limb.upper) else {
            return;
        };
        // The forward pass already put the hip/shoulder pivot in the world; the
        // solve only replaces the rotations below it.
        let hip = world[upper].translation;
        let bones = Vec3::new(self.scale, self.scale, self.scale);
        let pole = root.rotation.rotate(limb.pole);
        let contact = plant.map_or_else(|| self.carried_hand(limb, root, cycle), |p| p.contact);
        let target = contact.add(root.rotation.rotate(limb.tip_offset.mul_scalar(self.scale)));
        let solved = solve_two_bone(
            hip,
            target,
            pole,
            limb.len_upper * self.scale,
            limb.len_lower * self.scale,
        );
        write(rig, world, limb.upper, solved.upper, bones);
        write(rig, world, limb.lower, solved.lower, bones);

        // The chain's own end, which is the solved end for a two-bone limb and
        // the extra bone's far end for a three-bone one. Placing the terminating
        // block against *that* rather than against the requested contact keeps a
        // clamped limb whole: a foot that could not quite be reached stays on
        // the end of its leg instead of detaching from it.
        let block = match limb.extra {
            None => solved.end.subtract(root.rotation.rotate(limb.tip_offset.mul_scalar(self.scale))),
            Some(extra) => {
                let toward = contact
                    .add(root.rotation.rotate(limb.ankle_offset.mul_scalar(self.scale)))
                    .subtract(solved.end);
                let direction = toward.normalize().unwrap_or(Vec3::new(0.0, -1.0, 0.0));
                write(
                    rig,
                    world,
                    extra,
                    Transform::new(solved.end, aim(direction, pole), Vec3::ONE),
                    bones,
                );
                solved
                    .end
                    .add(direction.mul_scalar(limb.len_extra * self.scale))
                    .subtract(root.rotation.rotate(limb.ankle_offset.mul_scalar(self.scale)))
            }
        };
        write(
            rig,
            world,
            limb.tip,
            Transform::new(block, root.rotation, Vec3::ONE),
            bones,
        );
    }

    /// Where a grounded limb's contact is this instant.
    ///
    /// **During stance the answer depends only on the step number** — not on the
    /// phase within the step, not on the travel distance, not on the speed. That
    /// is the whole anti-skate guarantee, and it is one line: `arc_of(step)`.
    fn plant(&self, limb: &LimbChain, path: &LoopPath, travel: f32) -> Plant {
        // Local `-Z` is forward, so a contact behind the origin in local `z` is
        // that far back along the path.
        let fore = -limb.contact.z * self.scale;
        let across = limb.contact.x * self.scale;
        let phase = stride_phase(travel + fore, self.stride, limb.offset, self.duty);
        let arc_of = |step: f32| (step - limb.offset + self.lead) * self.stride;
        let (arc, lift) = match phase.planted {
            true => (arc_of(phase.step), 0.0),
            false => {
                let from = arc_of(phase.step);
                (
                    from + self.stride * ease(phase.swing),
                    swing_lift(phase.swing, self.lift),
                )
            }
        };
        let there = path.at(arc);
        let flat = there.position.add(there.right.mul_scalar(across));
        let ground = ground_y(flat.x, flat.z);
        Plant {
            contact: Vec3::new(flat.x, ground + limb.contact.y * self.scale + lift, flat.z),
            ground,
        }
    }

    /// Where a carried limb's hand is this instant: held in front of the body
    /// and swung fore and aft on its own share of the cycle.
    fn carried_hand(&self, limb: &LimbChain, root: Transform, cycle: f32) -> Vec3 {
        let angle = (cycle + limb.offset) * TAU;
        let side = limb.contact.x.signum();
        root.transform_point(Vec3::new(
            side * self.arm_carry.x,
            self.arm_carry.y + self.arm_rise * angle.cos(),
            self.arm_carry.z + self.arm_swing * angle.sin(),
        ))
    }
}

/// Where one foot is this instant, and the height of the ground it is over.
#[derive(Debug, Clone, Copy)]
struct Plant {
    /// The foot's world contact point, arced up if it is mid-swing.
    contact: Vec3,
    /// The terrain height under it, with no swing arc in it.
    ground: f32,
}

impl Plant {
    /// Hold the foot within `relief` of the body's own support height.
    ///
    /// A leg has a fixed length, and the fore and hind feet of a trotting dog
    /// are seven units apart on terrain that rolls by more than that in places.
    /// Letting each foot follow its own ground without bound is what puts a
    /// plant outside the leg that has to reach it — and a leg that cannot reach
    /// its plant is a leg that slides off it, which is the skate this whole
    /// design exists to avoid. Capping the relief is not a fudge: it is the
    /// statement that a creature steps *short of* a hole rather than dislocating
    /// a hip, and the cap is sized from the reach each creature actually has.
    ///
    /// The cap applies to the *ground* term only. The swing arc rides on top of
    /// it untouched, so a lifted foot still clears the bump it is stepping over.
    fn levelled(self, support: Option<f32>, relief: f32) -> Plant {
        support.map_or(self, |support| {
            let held = support + (self.ground - support).clamp(-relief, relief);
            Plant {
                contact: Vec3::new(
                    self.contact.x,
                    self.contact.y + held - self.ground,
                    self.contact.z,
                ),
                ground: held,
            }
        })
    }
}

/// The ground the body rides: the mean terrain height under every one of the
/// creature's feet.
///
/// Standing the body on the ground under its own *origin* — the obvious thing —
/// is what breaks a runner on rolling terrain. The feet are planted metres away
/// from the origin, so on a slope the body ends up too high above the downhill
/// foot and the leg reaching for it runs out of length exactly when the stride
/// needs it most. Averaging the ground under the feet keeps every leg inside its
/// reach, and is what a real animal does with its own body.
fn support(plants: &[Option<Plant>]) -> Option<f32> {
    let feet: Vec<f32> = plants
        .iter()
        .filter_map(|plant| plant.map(|p| p.ground))
        .collect();
    (!feet.is_empty()).then(|| feet.iter().sum::<f32>() / feet.len() as f32)
}

/// Write one bone's world transform, at the creature's presentation scale.
fn write(
    rig: &CreatureRig,
    world: &mut [Transform],
    name: &str,
    placement: Transform,
    scale: Vec3,
) {
    if let Some(index) = rig.index_of(name) {
        world[index] = Transform::new(placement.translation, placement.rotation, scale);
    }
}
