//! App-local **rigid low-poly humanoid character kit** for the penalty athletes.
//!
//! A soccer athlete is a fixed hierarchy of angular box parts (pelvis, torso,
//! head, hair, arms, hands, legs, feet) with **parented local transforms** — the
//! same rig shape the goalie already uses (`penalty_goalie_pose`), generalized so
//! the kicker is built the same way instead of as a stack of fake-offset boxes.
//! Each part carries an angular box mesh (scaled per part), a material **slot**
//! (skin / hair / jersey / shorts / socks / shoes / gloves), and a rest offset
//! from its parent; a [`HumanoidPose`] adds per-joint rotations for a static
//! athletic stance. Two instances consume the kit: the kicker (a back-facing
//! stride) and — via its own rig — the goalie.
//!
//! TEMPORARY APP GLUE, like the other `penalty_*` builders: Axiom has no
//! character-rig module, so this lives in the app. It is a pure function of
//! constants (deterministic) and, being app code, uses ordinary loops.

use axiom_math::{Quat, Transform, Vec3};

use crate::soccer_penalty::penalty_materials::PenaltyMaterialId;

/// The named parts of the rigid humanoid, in a parents-before-children order so a
/// single forward pass resolves world transforms. The discriminant is the array
/// index used by [`HumanoidPose`] and the canonical skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Part {
    Pelvis,
    Torso,
    Head,
    Hair,
    UpperArmL,
    UpperArmR,
    ForearmL,
    ForearmR,
    HandL,
    HandR,
    ThighL,
    ThighR,
    ShinL,
    ShinR,
    FootL,
    FootR,
}

/// The number of parts in the humanoid.
pub const PART_COUNT: usize = 16;

/// Every part, in resolve order.
pub const PARTS: [Part; PART_COUNT] = [
    Part::Pelvis,
    Part::Torso,
    Part::Head,
    Part::Hair,
    Part::UpperArmL,
    Part::UpperArmR,
    Part::ForearmL,
    Part::ForearmR,
    Part::HandL,
    Part::HandR,
    Part::ThighL,
    Part::ThighR,
    Part::ShinL,
    Part::ShinR,
    Part::FootL,
    Part::FootR,
];

/// A material slot on the humanoid; an [`Outfit`] maps each to a palette id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Skin,
    Hair,
    Jersey,
    Shorts,
    Socks,
    Shoes,
    Gloves,
}

/// The palette ids an outfit paints each slot with.
#[derive(Debug, Clone, Copy)]
pub struct Outfit {
    pub skin: PenaltyMaterialId,
    pub hair: PenaltyMaterialId,
    pub jersey: PenaltyMaterialId,
    pub shorts: PenaltyMaterialId,
    pub socks: PenaltyMaterialId,
    pub shoes: PenaltyMaterialId,
    pub gloves: PenaltyMaterialId,
}

impl Outfit {
    fn material(&self, slot: Slot) -> PenaltyMaterialId {
        match slot {
            Slot::Skin => self.skin,
            Slot::Hair => self.hair,
            Slot::Jersey => self.jersey,
            Slot::Shorts => self.shorts,
            Slot::Socks => self.socks,
            Slot::Shoes => self.shoes,
            Slot::Gloves => self.gloves,
        }
    }
}

/// One part's fixed definition. The part has a **joint** (the pivot it rotates
/// about, e.g. a shoulder or knee) placed at `joint` relative to its parent's
/// joint; its box is drawn `pivot` away from that joint (e.g. a limb hangs
/// `-half-length` below its top joint) with extents `size`. Rotating a joint
/// therefore swings the whole box and every child about the joint — a real
/// hinge, so elbows/knees never bend backward through their own middle.
#[derive(Debug, Clone, Copy)]
struct PartSpec {
    parent: Option<usize>,
    joint: Vec3,
    pivot: Vec3,
    size: Vec3,
    slot: Slot,
}

const fn spec(parent: Option<usize>, joint: Vec3, pivot: Vec3, size: Vec3, slot: Slot) -> PartSpec {
    PartSpec { parent, joint, pivot, size, slot }
}

/// The canonical rest skeleton (facing +Z, feet ≈ ground, ~1.9 m tall), indexed
/// by [`Part`] ordinal. Limbs hang from their top joints (shoulder/hip → elbow/
/// knee → wrist/ankle); the torso pivots at the waist and the head at the neck.
fn skeleton() -> [PartSpec; PART_COUNT] {
    let up = |h: f32| Vec3::new(0.0, h, 0.0); // box centered above the joint
    let down = |h: f32| Vec3::new(0.0, -h, 0.0); // box hangs below the joint
    [
        // Pelvis (shorts) — root; joint at the hips, box centered on it.
        spec(None, Vec3::new(0.0, 0.92, 0.0), Vec3::ZERO, Vec3::new(0.44, 0.30, 0.28), Slot::Shorts),
        // Torso pivots at the waist, box rising to the shoulders.
        spec(Some(0), up(0.13), up(0.28), Vec3::new(0.52, 0.56, 0.30), Slot::Jersey),
        // Head at the neck (top of torso), box above; hair caps the head.
        spec(Some(1), Vec3::new(0.0, 0.56, 0.02), up(0.14), Vec3::new(0.24, 0.26, 0.24), Slot::Skin),
        spec(Some(2), up(0.15), up(0.06), Vec3::new(0.27, 0.14, 0.28), Slot::Hair),
        // Upper arms: joint at the shoulder, box hangs down.
        spec(Some(1), Vec3::new(-0.34, 0.50, 0.0), down(0.22), Vec3::new(0.15, 0.44, 0.16), Slot::Jersey),
        spec(Some(1), Vec3::new(0.34, 0.50, 0.0), down(0.22), Vec3::new(0.15, 0.44, 0.16), Slot::Jersey),
        // Forearms: joint at the elbow (bottom of the upper arm), box hangs down.
        spec(Some(4), down(0.44), down(0.20), Vec3::new(0.13, 0.40, 0.14), Slot::Skin),
        spec(Some(5), down(0.44), down(0.20), Vec3::new(0.13, 0.40, 0.14), Slot::Skin),
        // Hands at the wrist.
        spec(Some(6), down(0.40), down(0.08), Vec3::new(0.15, 0.16, 0.15), Slot::Skin),
        spec(Some(7), down(0.40), down(0.08), Vec3::new(0.15, 0.16, 0.15), Slot::Skin),
        // Thighs (bare skin): joint at the hip socket, box hangs down.
        spec(Some(0), Vec3::new(-0.13, -0.12, 0.0), down(0.20), Vec3::new(0.20, 0.40, 0.22), Slot::Skin),
        spec(Some(0), Vec3::new(0.13, -0.12, 0.0), down(0.20), Vec3::new(0.20, 0.40, 0.22), Slot::Skin),
        // Shins (socks): joint at the knee.
        spec(Some(10), down(0.40), down(0.18), Vec3::new(0.17, 0.36, 0.18), Slot::Socks),
        spec(Some(11), down(0.40), down(0.18), Vec3::new(0.17, 0.36, 0.18), Slot::Socks),
        // Feet (shoes): joint at the ankle, box low and extended forward (+Z).
        spec(Some(12), down(0.36), Vec3::new(0.0, -0.04, 0.10), Vec3::new(0.18, 0.14, 0.34), Slot::Shoes),
        spec(Some(13), down(0.36), Vec3::new(0.0, -0.04, 0.10), Vec3::new(0.18, 0.14, 0.34), Slot::Shoes),
    ]
}

/// A static athletic pose: a per-joint local rotation applied on top of the rest
/// skeleton (identity = rest).
#[derive(Debug, Clone, Copy)]
pub struct HumanoidPose {
    rot: [Quat; PART_COUNT],
}

/// A degrees→radians rotation about an axis (fixed constants; the fallible
/// conversion never fails for a finite angle).
fn axis_deg(axis: Vec3, degrees: f32) -> Quat {
    Quat::from_axis_angle(axis, degrees.to_radians()).expect("finite pose angle")
}

fn rot_x(d: f32) -> Quat {
    axis_deg(Vec3::new(1.0, 0.0, 0.0), d)
}

impl HumanoidPose {
    fn rest() -> Self {
        HumanoidPose { rot: [Quat::IDENTITY; PART_COUNT] }
    }

    fn set(mut self, part: Part, q: Quat) -> Self {
        self.rot[part as usize] = q;
        self
    }

    /// A back-facing athletic stride mid-run-up: torso leaning in, planted lead
    /// leg, trailing kick leg cocked back with a bent knee, arms counter-swinging
    /// with the elbows bent forward (real hinges — nothing bends backward).
    pub fn kicker_stride() -> Self {
        Self::rest()
            .set(Part::Torso, rot_x(10.0)) // lean forward toward the ball
            // Lead (left) leg planted slightly forward, near-straight knee.
            .set(Part::ThighL, rot_x(-16.0))
            .set(Part::ShinL, rot_x(10.0))
            // Trailing (right) leg cocked back, knee bent so the heel lifts.
            .set(Part::ThighR, rot_x(30.0))
            .set(Part::ShinR, rot_x(-52.0))
            .set(Part::FootR, rot_x(28.0))
            // Arms counter-swing (left back, right forward), elbows bent forward.
            .set(Part::UpperArmL, rot_x(32.0))
            .set(Part::UpperArmR, rot_x(-38.0))
            .set(Part::ForearmL, rot_x(-66.0))
            .set(Part::ForearmR, rot_x(-72.0))
    }

    /// The wind-up: planted lead (left) leg, the right kicking leg cocked well back
    /// with a sharply bent knee, torso upright and loading, arms out to balance.
    /// Held while the shot charges.
    pub fn kicker_windup() -> Self {
        Self::rest()
            .set(Part::Torso, rot_x(-2.0))
            .set(Part::ThighL, rot_x(-14.0))
            .set(Part::ShinL, rot_x(8.0))
            .set(Part::ThighR, rot_x(50.0))
            .set(Part::ShinR, rot_x(-78.0))
            .set(Part::FootR, rot_x(34.0))
            // Left arm swings forward, right arm back — balancing the cocked leg.
            .set(Part::UpperArmL, rot_x(-34.0))
            .set(Part::UpperArmR, rot_x(40.0))
            .set(Part::ForearmL, rot_x(-58.0))
            .set(Part::ForearmR, rot_x(-70.0))
    }

    /// Ball contact: the right leg has swung down and through to the ball (thigh
    /// forward, shin extending), torso leaning in over the strike, arms reversing.
    pub fn kicker_contact() -> Self {
        Self::rest()
            .set(Part::Torso, rot_x(14.0))
            .set(Part::ThighL, rot_x(-6.0))
            .set(Part::ShinL, rot_x(4.0))
            .set(Part::ThighR, rot_x(-30.0))
            .set(Part::ShinR, rot_x(18.0))
            .set(Part::FootR, rot_x(-12.0))
            .set(Part::UpperArmL, rot_x(36.0))
            .set(Part::UpperArmR, rot_x(-32.0))
            .set(Part::ForearmL, rot_x(-64.0))
            .set(Part::ForearmR, rot_x(-58.0))
    }

    /// Follow-through: the kicking leg carries high and forward, the torso leans
    /// well over, the support leg pushes up. Held after the ball is struck.
    pub fn kicker_follow_through() -> Self {
        Self::rest()
            .set(Part::Torso, rot_x(20.0))
            .set(Part::ThighL, rot_x(6.0))
            .set(Part::ShinL, rot_x(-8.0))
            .set(Part::ThighR, rot_x(-66.0))
            .set(Part::ShinR, rot_x(10.0))
            .set(Part::FootR, rot_x(-6.0))
            .set(Part::UpperArmL, rot_x(44.0))
            .set(Part::UpperArmR, rot_x(-40.0))
            .set(Part::ForearmL, rot_x(-70.0))
            .set(Part::ForearmR, rot_x(-52.0))
    }
}

/// One resolved part ready to emit: its world transform (translation + rotation),
/// box extents, and resolved palette material.
#[derive(Debug, Clone, Copy)]
pub struct KitPart {
    pub part: Part,
    pub world: Transform,
    pub size: Vec3,
    pub material: PenaltyMaterialId,
}

/// Resolve the humanoid at `base` (world position of the feet) facing `base_yaw`,
/// in `pose`, wearing `outfit`. Parents precede children, so one forward pass
/// composes every world transform via `Transform::combine` (the same rig math the
/// goalie uses). Deterministic: a pure function of the constants above.
pub fn build_character(base: Vec3, base_yaw: Quat, pose: &HumanoidPose, outfit: &Outfit) -> Vec<KitPart> {
    let skel = skeleton();
    let root = Transform::combine(Transform::from_translation(base), Transform::from_rotation(base_yaw));
    // `joint[i]` is the world frame of part i's pivot (shoulder / knee / …).
    let mut joint = [Transform::IDENTITY; PART_COUNT];
    PARTS.iter().for_each(|&part| {
        let i = part as usize;
        let s = &skel[i];
        // The joint sits at `joint` from the parent's joint, then rotates by the
        // pose — so the box and every child swing about this hinge.
        let local = Transform::combine(
            Transform::from_translation(s.joint),
            Transform::from_rotation(pose.rot[i]),
        );
        let parent = s.parent.map(|p| joint[p]).unwrap_or(root);
        joint[i] = Transform::combine(parent, local);
    });
    PARTS
        .iter()
        .map(|&part| {
            let i = part as usize;
            let s = &skel[i];
            // The box center is `pivot` away from the joint, carried into the joint's
            // world frame; the box keeps the joint's orientation.
            let world = Transform::combine(joint[i], Transform::from_translation(s.pivot));
            KitPart { part, world, size: s.size, material: outfit.material(s.slot) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soccer_penalty::penalty_materials::PenaltyMaterialId as M;

    fn outfit() -> Outfit {
        Outfit {
            skin: M::KickerSkin,
            hair: M::GoalieHair,
            jersey: M::KickerJerseyBlue,
            shorts: M::KickerShortsWhite,
            socks: M::KickerSocksDark,
            shoes: M::KickerShoes,
            gloves: M::GoalieGloves,
        }
    }

    #[test]
    fn build_is_deterministic_and_complete() {
        let a = build_character(Vec3::ZERO, Quat::IDENTITY, &HumanoidPose::rest(), &outfit());
        let b = build_character(Vec3::ZERO, Quat::IDENTITY, &HumanoidPose::rest(), &outfit());
        assert_eq!(a.len(), PART_COUNT);
        a.iter().zip(b.iter()).for_each(|(x, y)| {
            assert_eq!(x.world.translation, y.world.translation, "deterministic transforms");
        });
    }

    #[test]
    fn rest_pose_stands_upright_with_feet_near_ground() {
        let parts = build_character(Vec3::ZERO, Quat::IDENTITY, &HumanoidPose::rest(), &outfit());
        let y = |p: Part| parts[p as usize].world.translation.y;
        assert!(y(Part::Head) > y(Part::Torso), "head above torso");
        assert!(y(Part::Torso) > y(Part::Pelvis), "torso above pelvis");
        assert!(y(Part::FootL) < 0.25, "left foot near the ground");
        assert!(y(Part::FootR) < 0.25, "right foot near the ground");
    }

    #[test]
    fn kick_poses_are_distinct_stances() {
        // Each phase of the kick is a genuinely different pose (guards against a
        // copy-paste leaving two phases identical, which would freeze the animation).
        let world = |p: &HumanoidPose, part: Part| {
            build_character(Vec3::ZERO, Quat::IDENTITY, p, &outfit())[part as usize].world.translation
        };
        let stride = HumanoidPose::kicker_stride();
        let windup = HumanoidPose::kicker_windup();
        let contact = HumanoidPose::kicker_contact();
        let follow = HumanoidPose::kicker_follow_through();
        // The right (kicking) foot travels through the four phases.
        let foot = |p: &HumanoidPose| world(p, Part::FootR);
        assert_ne!(foot(&stride), foot(&windup));
        assert_ne!(foot(&windup), foot(&contact));
        assert_ne!(foot(&contact), foot(&follow));
    }

    #[test]
    fn outfit_paints_slots() {
        let parts = build_character(Vec3::ZERO, Quat::IDENTITY, &HumanoidPose::kicker_stride(), &outfit());
        assert_eq!(parts[Part::Torso as usize].material, M::KickerJerseyBlue);
        assert_eq!(parts[Part::Pelvis as usize].material, M::KickerShortsWhite);
        assert_eq!(parts[Part::FootL as usize].material, M::KickerShoes);
        assert_eq!(parts[Part::ShinL as usize].material, M::KickerSocksDark);
    }
}
