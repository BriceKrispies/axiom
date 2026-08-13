//! Posing one creature: the frame where the path, the gait and the inverse
//! kinematics meet.
//!
//! Given a rig, its limb chains, a ring and how far along it the dog is, this
//! produces one **world transform per bone**. Nothing here is stateful and
//! nothing here reads a clock — the same travel distance always yields the same
//! pose, which is what makes `tick → pose` a pure function all the way down.
//!
//! ## The three passes
//!
//! 1. **The body.** The root sits on the terrain at the path point, turned to
//!    face the tangent, dropped by a crouch, and bobbed and pitched by the gait
//!    cycle. Every non-limb bone then resolves from it by plain forward
//!    kinematics, with a handful of named bones (the tail, the ears, the spine)
//!    given a small extra rotation from the same cycle.
//! 2. **The feet.** Each limb's contact is *planted*: during stance its
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
//! The dog is authored standing, with its legs very nearly straight — which is
//! anatomically right and animationally useless: a straight leg has no reach
//! left to swing a paw forward with, so every stride would clamp. The crouch
//! buys that reach back. It is not a fudge; it is what a trotting animal
//! actually does, and the stride below is sized against the reach it leaves (`√(reach² − hip_height²)` is the furthest a foot can be from under
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
    /// How far above or below the ground under its own path a foot may follow
    /// the terrain, in world units — the relief this creature's legs can
    /// actually absorb. See [`CreaturePose::plant`].
    pub relief: f32,
    /// Steady forward pitch, in radians. Negative drops the nose.
    pub lean: f32,
    /// How far the pitch oscillates with the gait, in radians.
    pub pitch_swing: f32,
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
/// the leg's length (`tests/locomotion.rs` measures exactly that, over every dog
/// on both rings, so a tuning change that over-reaches a leg fails the suite
/// rather than quietly skating).
///
/// The stride and crouch are sized for the **tighter** of the two rings. A
/// 21-unit rigid body walking a 26-unit radius plants its outside paws a fifth
/// of a unit wide of where a straight-line body would put them — small, and
/// exactly the margin a leg tuned on a near-straight path did not have. Shorter
/// steps and a deeper crouch are also what a real animal does on a tight turn,
/// so the fix and the anatomy agree.
pub const DOG_GAIT: CreaturePose = CreaturePose {
    scale: 10.0,
    stride: 8.2,
    duty: 0.52,
    lead: 0.26,
    lift: 0.9,
    crouch: 2.6,
    bob: 0.22,
    relief: 1.1,
    lean: -0.05,
    pitch_swing: 0.020,
    flex: &[
        ("dog-tail-base", Vec3::UNIT_Y, 0.26, 1.0),
        ("dog-tail-tip", Vec3::UNIT_Y, 0.34, 1.0),
        ("dog-ear-l", Vec3::UNIT_X, 0.20, 2.0),
        ("dog-ear-r", Vec3::UNIT_X, 0.20, 2.0),
        ("dog-spine", Vec3::UNIT_X, 0.03, 2.0),
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
        let plants: Vec<Plant> = limbs
            .iter()
            .map(|limb| self.plant(limb, path, travel))
            .collect();
        let root = self.body(path, travel, support(&plants));
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
            .for_each(|(limb, plant)| self.solve_limb(rig, limb, plant, root, &mut world));
        world
    }

    /// The body root: standing on `support`, facing down the path, crouched,
    /// bobbing and pitching with the gait.
    fn body(&self, path: &LoopPath, travel: f32, support: f32) -> Transform {
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
                support - self.crouch + bob,
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
        plant: Plant,
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
        let contact = plant.contact;
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

    /// Where a limb's contact is this instant.
    ///
    /// **During stance the answer depends only on the step number** — not on the
    /// phase within the step, not on the travel distance, not on the speed, and
    /// not on anything else the frame is doing. That is the whole anti-skate
    /// guarantee, and every term below obeys it: the arc is `arc_of(step)`, and
    /// the relief cap is measured against the ground under the *path* at that
    /// same arc.
    ///
    /// ## Why the relief cap is measured against the path and not against the body
    ///
    /// A leg has a fixed length, and the fore and hind paws of a trotting dog
    /// are seven units apart on terrain that rolls by more than that in places.
    /// Letting each paw follow its own ground without bound is what puts a plant
    /// outside the leg that has to reach it — and a leg that cannot reach its
    /// plant is a leg that slides off it. Capping the relief is not a fudge: it
    /// is the statement that an animal steps *short of* a hole rather than
    /// dislocating a hip, and the cap is sized from the reach the legs have.
    ///
    /// The reference it is capped *toward* has to be constant across a stance,
    /// or the cap itself becomes a skate. Capping toward the body's own support
    /// height — the mean ground under all four feet — fails exactly that test:
    /// the other three feet move every tick, so the mean moves, so a planted paw
    /// held at the cap creeps vertically while it is supposed to be nailed down.
    /// The ground under the path at the plant's own arc is a function of the arc
    /// alone, which makes the cap as constant as the plant it is capping.
    ///
    /// The cap applies to the *ground* term only. The swing arc rides on top of
    /// it untouched, so a lifted paw still clears the bump it is stepping over.
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
        // `path.at` already stands its point on the terrain, so this is the
        // ground under the animal's own line at this plant's arc.
        let line = there.position.y;
        let ground = line + (ground_y(flat.x, flat.z) - line).clamp(-self.relief, self.relief);
        Plant {
            contact: Vec3::new(flat.x, ground + limb.contact.y * self.scale + lift, flat.z),
            ground,
        }
    }
}

/// Where one foot is this instant, and the height of the ground it stands on.
#[derive(Debug, Clone, Copy)]
struct Plant {
    /// The foot's world contact point, arced up if it is mid-swing.
    contact: Vec3,
    /// The height the foot stands at, relief-capped and with no swing arc in it.
    ground: f32,
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
///
/// Every one of the dog's four limbs is a leg, so there is always at least one
/// foot to average and the answer is always a height — an empty chain would be a
/// creature with no legs, which is not a thing this app can build.
fn support(plants: &[Plant]) -> f32 {
    let total: f32 = plants.iter().map(|plant| plant.ground).sum();
    total / (plants.len().max(1)) as f32
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
