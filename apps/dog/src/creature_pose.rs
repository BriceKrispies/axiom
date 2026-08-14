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
//! ## Why the body barely drops any more
//!
//! A creature authored with near-straight legs has no reach left to swing a paw
//! forward with, so every stride clamps — and the only cure available to a pose
//! pass is to fold the animal down until the legs have somewhere to go. That is
//! what [`CreaturePose::crouch`] is for, and on the straight-legged dog this
//! scene used to carry it was worth 47% of a leg.
//!
//! The dachshund does not need it, because the *geometry* is already bent: its
//! foreleg stands at 73% of its own reach with the rest in hand (see
//! `creature_dog.rs`). The crouch is therefore down to a tenth of what it was,
//! and the dial has gone back to meaning what it says — a small settle into each
//! footfall — rather than propping up an under-articulated rig. The stride is
//! still sized against what is left (`√(reach² − hip_height²)` is the furthest a
//! foot can be from under its own hip, and each stride's half-excursion sits
//! comfortably inside it); there is simply much more of it.

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
    /// How far above or below **its own body's line** a foot may be set down, in
    /// world units — the relief this creature's legs can actually absorb, which
    /// is a property of leg length against wheelbase. See [`CreaturePose::plant`].
    pub relief: f32,
    /// Steady forward pitch, in radians. Negative drops the nose.
    pub lean: f32,
    /// How far the pitch oscillates with the gait, in radians.
    pub pitch_swing: f32,
    /// How much of the ground's slope under its own wheelbase the body lies
    /// along, as a fraction: `1.0` is a body parallel to the line between the
    /// ground under its front feet and the ground under its back ones, `0.0` a
    /// body held dead level whatever it is standing on.
    ///
    /// **This is not styling — it is what makes a short-legged animal possible at
    /// all.** A creature's feet stand on the ground at their own arcs; a level
    /// body holds every hip at one height, so the vertical distance a leg must
    /// span is the *whole* of the terrain's roll across the wheelbase. That was
    /// affordable for a 7-unit wheelbase on a 5.5-unit leg and is not for a
    /// 10-unit wheelbase on a 3.7-unit one: it clamped every front leg at 108% of
    /// its length, on the widest rings as badly as the tightest, which is what
    /// identified it as a body problem rather than a ring or a stride one.
    ///
    /// Pitching closes it almost exactly, and for a reason that is not a
    /// coincidence: the front hip sits at `wheelbase/2` ahead of the pivot, so a
    /// pitch taken from the ground at `±wheelbase/2` lifts that hip by very
    /// nearly the amount the ground under its own foot has risen. What is left
    /// for the leg to absorb is the terrain's *curvature* over the wheelbase
    /// rather than its slope — a far smaller number.
    ///
    /// It is a pure function of the travel distance, exactly as `lean` and
    /// `pitch_swing` are, so it costs the pose nothing in replayability.
    pub terrain_pitch: f32,
    /// Bones given an extra gait-driven rotation on top of their rest pose.
    pub flex: &'static [Flex],
}

/// The dachshund's trot — short, quick and busy, because it could not be
/// anything else.
///
/// Every dial below is the same dial it was before the dog was re-proportioned;
/// what changed is the leg it is sized against. The front chain is **3.680
/// units** long at presentation scale and its shoulder stands **2.708** above
/// the paw — the leg is authored *bent* (see `creature_dog.rs`), so it starts at
/// 74% of its own reach with 26% in hand. That is the entire stride budget, and
/// three things are spent out of it: the stride's own half-excursion
/// (`stride · lead` = 1.35), the terrain the feet are allowed to follow
/// (`relief`), and the tight-ring correction — a 24-unit rigid body on a 26-unit
/// radius puts its shoulder 0.39 units outside the circle its paw is planted on.
///
/// Measured over 900 ticks, every dog on every ring, the worst hip-to-paw
/// distance is **86% of the front leg and 85% of the hind** — inside the 97% bar
/// `tests/locomotion.rs` holds them to, with the same margin the straight-legged
/// dog used to have.
///
/// Three consequences worth naming, because they are the breed rather than a
/// tuning accident:
///
/// * **The crouch collapsed from 2.6 to 0.40.** A near-straight leg has to be
///   folded to swing at all; a bent one is already folded. The body now rides at
///   its authored height, which is what keeps a 0.9-unit belly clearance under an
///   animal whose legs are 3.7 units long.
/// * **The stride collapsed from 8.2 to 5.2** while the travel per tick did
///   not — so the cycle runs 1.6× faster over the ground: a step every 8.4 ticks
///   where the old dog took one every 13.2. Short legs taking short steps at an
///   unchanged trot *is* the busy dachshund gait; it did not have to be dialled
///   in separately.
/// * **The relief cap fell from 1.1 to 0.70**, and it stopped being the thing
///   holding the animal together — see [`CreaturePose::terrain_pitch`], which is
///   the dial that actually pays for a 10-unit wheelbase on a 3.7-unit leg.
///
/// `RING_MIN_RADIUS` and these dials were therefore re-derived **together**: the
/// curve correction grew with the body at the same moment the leg absorbing it
/// halved, so neither number is meaningful without the other.
pub const DOG_GAIT: CreaturePose = CreaturePose {
    scale: 10.0,
    stride: 5.2,
    duty: 0.52,
    lead: 0.26,
    lift: 0.45,
    crouch: 0.40,
    bob: 0.09,
    relief: 0.70,
    lean: -0.04,
    pitch_swing: 0.016,
    terrain_pitch: 1.0,
    flex: &[
        ("dog-tail-base", Vec3::UNIT_Y, 0.22, 1.0),
        ("dog-tail-tip", Vec3::UNIT_Y, 0.30, 1.0),
        ("dog-ear-l", Vec3::UNIT_X, 0.16, 2.0),
        ("dog-ear-r", Vec3::UNIT_X, 0.16, 2.0),
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
        let root = self.body(path, travel, support(&plants), wheelbase(limbs, self.scale));
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
    /// bobbing, and pitched by both the gait and the ground it is standing on.
    fn body(&self, path: &LoopPath, travel: f32, support: f32, wheelbase: f32) -> Transform {
        let here = path.at(travel);
        let cycle = travel / self.stride;
        // Twice per stride, because there are two ground contacts in one: the
        // body sinks into each foot strike and rises between them.
        let bob = -self.bob * (TAU * 2.0 * cycle).cos();
        // The ground's own slope, measured over exactly the span the feet stand
        // on — so the body lies along the line its own front and back feet are
        // standing on rather than level across it. See `terrain_pitch`.
        let half = wheelbase * 0.5;
        let rise = path.at(travel + half).position.y - path.at(travel - half).position.y;
        let pitch = self.lean
            + self.pitch_swing * (TAU * 2.0 * cycle).sin()
            + self.terrain_pitch * (rise / wheelbase).atan();
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
    /// A leg has a fixed length, and the fore and hind paws of a trotting
    /// dachshund are **ten units apart** on terrain that rolls by three units
    /// over that span. Letting each paw follow its own ground without bound is
    /// what puts a plant outside the leg that has to reach it — and a leg that
    /// cannot reach its plant is a leg that slides off it. Capping the relief is
    /// not a fudge: it is the statement that an animal steps *short of* a hole
    /// rather than dislocating a hip, and the cap is sized from the reach the
    /// legs have.
    ///
    /// The reference it is capped *toward* has to be constant across a stance,
    /// or the cap itself becomes a skate. Capping toward the body's own support
    /// height — the mean ground under all four feet — fails exactly that test:
    /// the other three feet move every tick, so the mean moves, so a planted paw
    /// held at the cap creeps vertically while it is supposed to be nailed down.
    /// The ground under the path at the plant's own arc is a function of the arc
    /// alone, which makes the cap as constant as the plant it is capping.
    ///
    /// **This is the correct reference only because the body pitches.** A foot
    /// tracking the ground at its own arc is unreachable by a level body — that is
    /// what re-proportioning the dog to a dachshund exposed, clamping every front
    /// leg at 108% of its length on every ring at once. The fix is not to move the
    /// cap's reference (which merely trades a leg that cannot reach for a paw
    /// hanging two units over its own ground); it is that the body must lie along
    /// the slope its feet are standing on. See [`CreaturePose::terrain_pitch`] —
    /// with the body pitched over its own wheelbase, each hip rises with the
    /// ground its own foot is on, and this cap is left bounding what it was always
    /// meant to bound: the *lateral* excursion a foot makes off the ring line.
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

/// The fore-aft span between a creature's furthest-apart contacts, in world
/// units — the baseline its terrain pitch is measured over.
///
/// Taking it from the limbs rather than authoring it again is the point: the
/// body pitches over exactly the span its feet are planted on, so moving a paw
/// moves the measurement with it. The floor of one unit is for a creature whose
/// contacts are all at the same fore-aft station — nothing here has one, but a
/// zero baseline would divide the pitch by zero rather than simply hold the body
/// level, which is the only sensible answer for a creature with no wheelbase.
fn wheelbase(limbs: &[LimbChain], scale: f32) -> f32 {
    let fore = |limb: &LimbChain| -limb.contact.z * scale;
    let front = limbs.iter().map(fore).fold(f32::NEG_INFINITY, f32::max);
    let back = limbs.iter().map(fore).fold(f32::INFINITY, f32::min);
    (front - back).max(1.0)
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
