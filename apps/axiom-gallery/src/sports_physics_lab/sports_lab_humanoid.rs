//! The procedural humanoid: a 15-part low-poly box figure (head, torso, pelvis,
//! upper/lower arms, hands, upper/lower legs, feet) authored **fully in code**
//! through the engine's `axiom-figure` vocabulary. One generator serves both
//! characters: the T-pose practice dummy (arms out) and the player's own body
//! (arms lowered, walk-bobbed) — same primitive family, different palette.
//!
//! The figure's local origin sits at its volumetric center
//! ([`FIGURE_CENTER_Y`] above the feet) so a physics body placed at that origin
//! carries the figure exactly: posing is `body transform ∘ rest chain` per part,
//! resolved through [`FigureApi::posed_parts`].

use axiom::prelude::Vec3;
use axiom_figure::{FigureApi, FigureDefinition, FigurePart};
use axiom_math::{Quat, Transform};

/// Number of parts in the humanoid.
pub const PART_COUNT: usize = 15;

/// Height of the figure's local origin (and physics body center) above the feet.
pub const FIGURE_CENTER_Y: f32 = 0.95;

/// Bounding-sphere radius used for pickup targeting (covers the T-pose arm span).
pub const DUMMY_GRAB_RADIUS: f32 = 1.15;

/// The dummy's physics box collider half extents (torso-sized: the visual arms
/// overhang it — a compound collider per limb needs joint support the physics
/// module does not have, so the whole dummy is one rigid box).
pub const DUMMY_BOX_HALF_EXTENTS: Vec3 = Vec3::new(0.24, 0.95, 0.16);

// Opaque part tags — the app maps them to palette materials.
pub const TAG_SHIRT: u32 = 0;
pub const TAG_SHORTS: u32 = 1;
pub const TAG_SKIN: u32 = 2;
pub const TAG_LEGS: u32 = 3;
pub const TAG_SHOES: u32 = 4;
/// Number of distinct tags (palette size).
pub const TAG_COUNT: usize = 5;

/// Arm rest pose: the dummy holds a T-pose; the player's body hangs its arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmPose {
    TPose,
    Lowered,
}

/// `(parent, joint offset, box size, box offset, tag)` — parent-before-child.
/// Y up, toes point +Z. Offsets are from the parent's joint pivot.
struct PartSpec {
    parent: Option<u32>,
    offset: Vec3,
    box_size: Vec3,
    box_offset: Vec3,
    tag: u32,
}

const fn p(parent: Option<u32>, offset: Vec3, box_size: Vec3, box_offset: Vec3, tag: u32) -> PartSpec {
    PartSpec { parent, offset, box_size, box_offset, tag }
}

/// Feet on y=0, head top ≈ 1.89. The pelvis root is expressed relative to the
/// figure center ([`FIGURE_CENTER_Y`]), everything else relative to its parent.
const PARTS: [PartSpec; PART_COUNT] = [
    // 0 pelvis (root; pivot at y 1.02 absolute = 0.07 above the figure center)
    p(None, Vec3::new(0.0, 1.02 - FIGURE_CENTER_Y, 0.0), Vec3::new(0.30, 0.26, 0.20), Vec3::ZERO, TAG_SHORTS),
    // 1 chest
    p(Some(0), Vec3::new(0.0, 0.33, 0.0), Vec3::new(0.44, 0.42, 0.24), Vec3::new(0.0, 0.04, 0.0), TAG_SHIRT),
    // 2 head
    p(Some(1), Vec3::new(0.0, 0.34, 0.0), Vec3::new(0.20, 0.22, 0.20), Vec3::new(0.0, 0.09, 0.0), TAG_SKIN),
    // 3 left thigh / 4 left shin / 5 left foot
    p(Some(0), Vec3::new(-0.10, -0.14, 0.0), Vec3::new(0.15, 0.48, 0.16), Vec3::new(0.0, -0.24, 0.0), TAG_LEGS),
    p(Some(3), Vec3::new(0.0, -0.42, 0.0), Vec3::new(0.13, 0.40, 0.14), Vec3::new(0.0, -0.20, 0.0), TAG_LEGS),
    p(Some(4), Vec3::new(0.0, -0.40, 0.0), Vec3::new(0.14, 0.11, 0.30), Vec3::new(0.0, -0.005, 0.07), TAG_SHOES),
    // 6 right thigh / 7 right shin / 8 right foot
    p(Some(0), Vec3::new(0.10, -0.14, 0.0), Vec3::new(0.15, 0.48, 0.16), Vec3::new(0.0, -0.24, 0.0), TAG_LEGS),
    p(Some(6), Vec3::new(0.0, -0.42, 0.0), Vec3::new(0.13, 0.40, 0.14), Vec3::new(0.0, -0.20, 0.0), TAG_LEGS),
    p(Some(7), Vec3::new(0.0, -0.40, 0.0), Vec3::new(0.14, 0.11, 0.30), Vec3::new(0.0, -0.005, 0.07), TAG_SHOES),
    // 9 left upper arm / 10 left forearm / 11 left hand
    p(Some(1), Vec3::new(-0.28, 0.12, 0.0), Vec3::new(0.11, 0.36, 0.11), Vec3::new(0.0, -0.18, 0.0), TAG_SHIRT),
    p(Some(9), Vec3::new(0.0, -0.36, 0.0), Vec3::new(0.10, 0.32, 0.10), Vec3::new(0.0, -0.16, 0.0), TAG_SKIN),
    p(Some(10), Vec3::new(0.0, -0.32, 0.0), Vec3::new(0.09, 0.12, 0.10), Vec3::new(0.0, -0.06, 0.0), TAG_SKIN),
    // 12 right upper arm / 13 right forearm / 14 right hand
    p(Some(1), Vec3::new(0.28, 0.12, 0.0), Vec3::new(0.11, 0.36, 0.11), Vec3::new(0.0, -0.18, 0.0), TAG_SHIRT),
    p(Some(12), Vec3::new(0.0, -0.36, 0.0), Vec3::new(0.10, 0.32, 0.10), Vec3::new(0.0, -0.16, 0.0), TAG_SKIN),
    p(Some(13), Vec3::new(0.0, -0.32, 0.0), Vec3::new(0.09, 0.12, 0.10), Vec3::new(0.0, -0.06, 0.0), TAG_SKIN),
];

/// Indices of the upper-arm parts (the joints an [`ArmPose`] rotates).
const LEFT_UPPER_ARM: usize = 9;
const RIGHT_UPPER_ARM: usize = 12;

/// Build the 15-part humanoid. `ArmPose::TPose` rotates the shoulder joints ±90°
/// about Z so the arm chains (which hang along local −Y) extend along ±X;
/// `ArmPose::Lowered` leaves them hanging.
pub fn humanoid_figure(arms: ArmPose) -> FigureDefinition {
    let arm_roll = |i: usize| -> Quat {
        let is_arm_root = i == LEFT_UPPER_ARM || i == RIGHT_UPPER_ARM;
        if arms == ArmPose::TPose && is_arm_root {
            // Rz(+90°) maps local −Y → +X (right arm out); Rz(−90°) → −X (left).
            let sign = if i == RIGHT_UPPER_ARM { 1.0 } else { -1.0 };
            Quat::from_euler_xyz(0.0, 0.0, sign * core::f32::consts::FRAC_PI_2)
        } else {
            Quat::IDENTITY
        }
    };
    let parts = PARTS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let rest = Transform::new(s.offset, arm_roll(i), Vec3::ONE);
            match s.parent {
                None => FigurePart::root(rest, s.box_size, s.box_offset, s.tag),
                Some(parent) => FigurePart::child(parent, rest, s.box_size, s.box_offset, s.tag),
            }
        })
        .collect();
    FigureDefinition::new(parts)
}

/// Resolve every part of `figure` to world space under `body` (the physics body
/// transform whose origin is the figure center): accumulate each part's rest
/// chain, then place the chain under the body and bake the box offsets through
/// [`FigureApi::posed_parts`].
pub fn posed_boxes(figure: &FigureDefinition, body: Transform) -> Vec<axiom_figure::PosedPart> {
    let mut locals: Vec<Transform> = Vec::with_capacity(figure.part_count());
    for part in figure.parts() {
        let local = match part.parent {
            None => part.rest,
            Some(parent) => Transform::combine(locals[parent as usize], part.rest),
        };
        locals.push(local);
    }
    let worlds: Vec<Transform> = locals.iter().map(|l| Transform::combine(body, *l)).collect();
    FigureApi::new()
        .posed_parts(figure, &worlds)
        .expect("humanoid part/world counts match")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_figure_validates_and_has_fifteen_parts() {
        let figure = humanoid_figure(ArmPose::TPose);
        assert_eq!(figure.part_count(), PART_COUNT);
        assert!(figure.validate().is_ok());
        assert!(humanoid_figure(ArmPose::Lowered).validate().is_ok());
    }

    #[test]
    fn a_standing_figure_keeps_its_feet_on_the_ground() {
        let figure = humanoid_figure(ArmPose::TPose);
        let body = Transform::from_translation(Vec3::new(0.0, FIGURE_CENTER_Y, 0.0));
        let parts = posed_boxes(&figure, body);
        let feet: Vec<_> = parts.iter().filter(|p| p.tag == TAG_SHOES).collect();
        assert_eq!(feet.len(), 2);
        for foot in feet {
            let bottom = foot.transform.translation.y - foot.box_size.y * 0.5;
            assert!(bottom.abs() < 0.02, "foot bottom sits on y=0, got {bottom}");
        }
        // Head sits near the top.
        let head = parts[2];
        assert!(head.transform.translation.y > 1.6);
    }

    #[test]
    fn the_t_pose_extends_the_arms_horizontally() {
        let body = Transform::from_translation(Vec3::new(0.0, FIGURE_CENTER_Y, 0.0));
        let t_pose = posed_boxes(&humanoid_figure(ArmPose::TPose), body);
        let lowered = posed_boxes(&humanoid_figure(ArmPose::Lowered), body);
        // Hands: far out in ±X in the T-pose, near the hips lowered.
        let (lh_t, rh_t) = (t_pose[11].transform.translation, t_pose[14].transform.translation);
        assert!(lh_t.x < -0.85 && rh_t.x > 0.85, "T-pose hands reach out: {lh_t:?} {rh_t:?}");
        assert!((lh_t.y - t_pose[9].transform.translation.y).abs() < 0.25, "T-pose arms are level");
        let rh_low = lowered[14].transform.translation;
        assert!(rh_low.x < 0.5 && rh_low.y < 1.0, "lowered hands hang by the hips: {rh_low:?}");
        // Arm span stays inside the grab radius.
        assert!(rh_t.x + 0.1 < DUMMY_GRAB_RADIUS);
    }

    #[test]
    fn posing_follows_the_body_transform() {
        let figure = humanoid_figure(ArmPose::TPose);
        let moved = Transform::from_translation(Vec3::new(5.0, FIGURE_CENTER_Y, -3.0));
        let parts = posed_boxes(&figure, moved);
        let head = parts[2].transform.translation;
        assert!((head.x - 5.0).abs() < 1e-4 && (head.z - -3.0).abs() < 1e-4);
    }
}
