//! The soccer player: a 17-box procedural figure authored through the engine's
//! `axiom-figure` vocabulary.
//!
//! This is the End Zone arcade footballer with the football taken off it. The
//! construction is the same and the proportions are the same — a sturdy torso, a
//! low centre of gravity, a parented limb chain whose boxes pivot at the joint
//! and are centred along the segment, tag-driven materials and zero branches in
//! the build. What is gone is every piece of football equipment: no helmet, no
//! facemask, no shoulder-pad slab. The pad girdle that made that figure read as
//! a lineman is replaced by an ordinary shoulder yoke, and the head is a head.
//!
//! What replaces the padding is a **kit**: shirt, shorts, socks, boots, and (for
//! the keeper, through the same tag) gloves. Bare arms and bare thighs are what
//! actually make a box figure read as a footballer rather than as a soldier, so
//! the shorts are a wide hip box and the thighs are skin.
//!
//! Units are metres (1 world unit = 1 m). Y up, toes point `+Z`,
//! parent-before-child.

use axiom::prelude::Vec3;
use axiom_figure::{FigureDefinition, FigurePart};
use axiom_math::{Quat, Transform};

/// Number of parts in the figure.
pub const PART_COUNT: usize = 17;

/// Height of the figure's local origin above the soles, metres. The rig hands
/// the figure a body transform at the ground position raised by this.
pub const FIGURE_CENTER_Y: f32 = 0.92;

/// Overall standing height, metres — used by the camera fit and the keeper's
/// reach so nothing has to re-measure the model by hand.
pub const FIGURE_HEIGHT: f32 = 1.82;

// Palette slots (part tags). The scene maps each tag to one kit material, so a
// kicker and a keeper are the same figure under two palettes.
pub const TAG_SHIRT: u32 = 0;
pub const TAG_SHORTS: u32 = 1;
pub const TAG_SOCKS: u32 = 2;
pub const TAG_SKIN: u32 = 3;
pub const TAG_BOOTS: u32 = 4;
pub const TAG_HAIR: u32 = 5;
/// Bare hands on an outfield player; gloves on the keeper. One tag, two
/// palettes — which is the whole reason the tag is opaque to the figure module.
pub const TAG_HANDS: u32 = 6;
/// Number of distinct tags.
pub const TAG_COUNT: usize = 7;

// Part indices (the animation addresses joints by these).
pub const PELVIS: usize = 0;
pub const TORSO: usize = 1;
pub const SHOULDERS: usize = 2;
pub const HEAD: usize = 3;
pub const HAIR: usize = 4;
pub const L_THIGH: usize = 5;
pub const L_SHIN: usize = 6;
pub const L_FOOT: usize = 7;
pub const R_THIGH: usize = 8;
pub const R_SHIN: usize = 9;
pub const R_FOOT: usize = 10;
pub const L_UPPER_ARM: usize = 11;
pub const L_FOREARM: usize = 12;
pub const L_HAND: usize = 13;
pub const R_UPPER_ARM: usize = 14;
pub const R_FOREARM: usize = 15;
pub const R_HAND: usize = 16;

/// `(parent, joint offset, box size, box offset, tag)` — the joint offset is
/// from the parent's pivot; the box offset centres the limb box while it pivots
/// at the joint.
#[derive(Debug, Clone, Copy)]
pub struct PartSpec {
    pub parent: Option<u32>,
    pub offset: Vec3,
    pub box_size: Vec3,
    pub box_offset: Vec3,
    pub tag: u32,
}

const fn p(
    parent: Option<u32>,
    offset: Vec3,
    box_size: Vec3,
    box_offset: Vec3,
    tag: u32,
) -> PartSpec {
    PartSpec {
        parent,
        offset,
        box_size,
        box_offset,
        tag,
    }
}

/// Soles on y = 0, crown at y ≈ 1.82. Hips at 0.92, knees at 0.50, ankles at
/// 0.09 — a real 1.82 m footballer's skeleton, at arcade box weights.
pub const PARTS: [PartSpec; PART_COUNT] = [
    // 0 pelvis (root). Wide and squared off: this box IS the shorts, which is
    // what lets the thighs below it be bare skin.
    p(
        None,
        Vec3::new(0.0, 0.98 - FIGURE_CENTER_Y, 0.0),
        Vec3::new(0.36, 0.30, 0.23),
        Vec3::new(0.0, -0.05, 0.0),
        TAG_SHORTS,
    ),
    // 1 torso
    p(
        Some(0),
        Vec3::new(0.0, 0.28, 0.0),
        Vec3::new(0.40, 0.42, 0.24),
        Vec3::new(0.0, 0.06, 0.0),
        TAG_SHIRT,
    ),
    // 2 shoulder yoke — an ordinary human shoulder line, not a pad slab. It is
    // the parent of the head and both arms, so the whole upper body inherits the
    // ribcage counter-rotation the way the donor figure's pad girdle did.
    p(
        Some(1),
        Vec3::new(0.0, 0.24, 0.0),
        Vec3::new(0.44, 0.15, 0.24),
        Vec3::ZERO,
        TAG_SHIRT,
    ),
    // 3 head
    p(
        Some(2),
        Vec3::new(0.0, 0.10, 0.0),
        Vec3::new(0.19, 0.22, 0.20),
        Vec3::new(0.0, 0.11, 0.0),
        TAG_SKIN,
    ),
    // 4 hair cap
    p(
        Some(3),
        Vec3::new(0.0, 0.20, 0.0),
        Vec3::new(0.205, 0.075, 0.215),
        Vec3::new(0.0, 0.0, -0.006),
        TAG_HAIR,
    ),
    // 5/6/7 left thigh, shin, foot
    p(
        Some(0),
        Vec3::new(-0.09, -0.06, 0.0),
        Vec3::new(0.135, 0.42, 0.145),
        Vec3::new(0.0, -0.21, 0.0),
        TAG_SKIN,
    ),
    p(
        Some(5),
        Vec3::new(0.0, -0.42, 0.0),
        Vec3::new(0.115, 0.41, 0.125),
        Vec3::new(0.0, -0.205, 0.0),
        TAG_SOCKS,
    ),
    p(
        Some(6),
        Vec3::new(0.0, -0.41, 0.0),
        Vec3::new(0.115, 0.09, 0.26),
        Vec3::new(0.0, -0.045, 0.06),
        TAG_BOOTS,
    ),
    // 8/9/10 right thigh, shin, foot
    p(
        Some(0),
        Vec3::new(0.09, -0.06, 0.0),
        Vec3::new(0.135, 0.42, 0.145),
        Vec3::new(0.0, -0.21, 0.0),
        TAG_SKIN,
    ),
    p(
        Some(8),
        Vec3::new(0.0, -0.42, 0.0),
        Vec3::new(0.115, 0.41, 0.125),
        Vec3::new(0.0, -0.205, 0.0),
        TAG_SOCKS,
    ),
    p(
        Some(9),
        Vec3::new(0.0, -0.41, 0.0),
        Vec3::new(0.115, 0.09, 0.26),
        Vec3::new(0.0, -0.045, 0.06),
        TAG_BOOTS,
    ),
    // 11/12/13 left upper arm, forearm, hand — hung off the shoulder yoke.
    p(
        Some(2),
        Vec3::new(-0.25, -0.03, 0.0),
        Vec3::new(0.10, 0.30, 0.105),
        Vec3::new(0.0, -0.15, 0.0),
        TAG_SKIN,
    ),
    p(
        Some(11),
        Vec3::new(0.0, -0.30, 0.0),
        Vec3::new(0.085, 0.26, 0.09),
        Vec3::new(0.0, -0.13, 0.0),
        TAG_SKIN,
    ),
    p(
        Some(12),
        Vec3::new(0.0, -0.26, 0.0),
        Vec3::new(0.085, 0.11, 0.095),
        Vec3::new(0.0, -0.055, 0.0),
        TAG_HANDS,
    ),
    // 14/15/16 right upper arm, forearm, hand
    p(
        Some(2),
        Vec3::new(0.25, -0.03, 0.0),
        Vec3::new(0.10, 0.30, 0.105),
        Vec3::new(0.0, -0.15, 0.0),
        TAG_SKIN,
    ),
    p(
        Some(14),
        Vec3::new(0.0, -0.30, 0.0),
        Vec3::new(0.085, 0.26, 0.09),
        Vec3::new(0.0, -0.13, 0.0),
        TAG_SKIN,
    ),
    p(
        Some(15),
        Vec3::new(0.0, -0.26, 0.0),
        Vec3::new(0.085, 0.11, 0.095),
        Vec3::new(0.0, -0.055, 0.0),
        TAG_HANDS,
    ),
];

/// Half the distance between the hip pivots, metres — the model's own answer to
/// how far off the centreline a leg hangs, so foot placement is never asked to
/// reach sideways for a target the hip does not sit above.
pub fn hip_half_width() -> f32 {
    PARTS[R_THIGH].offset.x.abs()
}

/// Build the figure definition. Rest rotations are identity: every joint is
/// driven per tick by the animation through the rig.
pub fn soccer_figure() -> FigureDefinition {
    FigureDefinition::new(
        PARTS
            .iter()
            .map(|s| {
                let rest = Transform::new(s.offset, Quat::IDENTITY, Vec3::ONE);
                match s.parent {
                    None => FigurePart::root(rest, s.box_size, s.box_offset, s.tag),
                    Some(parent) => {
                        FigurePart::child(parent, rest, s.box_size, s.box_offset, s.tag)
                    }
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_figure::FigureApi;

    #[test]
    fn the_figure_is_a_valid_parent_before_child_hierarchy() {
        let figure = soccer_figure();
        assert_eq!(figure.part_count(), PART_COUNT);
        assert_eq!(FigureApi::new().validate(&figure), Ok(()));
        PARTS.iter().enumerate().for_each(|(index, spec)| {
            if let Some(parent) = spec.parent {
                assert!((parent as usize) < index, "part {index} looks forward");
            }
        });
    }

    #[test]
    fn the_kit_covers_the_body_and_no_football_equipment_survives() {
        let tags: Vec<u32> = PARTS.iter().map(|s| s.tag).collect();
        [
            TAG_SHIRT, TAG_SHORTS, TAG_SOCKS, TAG_SKIN, TAG_BOOTS, TAG_HAIR, TAG_HANDS,
        ]
        .iter()
        .for_each(|tag| assert!(tags.contains(tag), "tag {tag} is unused"));
        assert!(tags.iter().all(|t| (*t as usize) < TAG_COUNT));
        // Bare thighs and bare arms are what make it read as a footballer.
        assert_eq!(PARTS[L_THIGH].tag, TAG_SKIN);
        assert_eq!(PARTS[R_UPPER_ARM].tag, TAG_SKIN);
        assert_eq!(PARTS[L_SHIN].tag, TAG_SOCKS);
        assert_eq!(PARTS[R_FOOT].tag, TAG_BOOTS);
        // The head hangs off the shoulder yoke, and the yoke is a shoulder, not a
        // pad slab: it is narrower than the figure is tall by a wide margin.
        assert_eq!(PARTS[HEAD].parent, Some(SHOULDERS as u32));
        assert!(PARTS[SHOULDERS].box_size.x < 0.5);
    }

    #[test]
    fn the_model_stands_on_the_ground_at_its_stated_height() {
        // Resolve the rest pose and check the extremes of the box stack.
        let figure = soccer_figure();
        let posed = FigureApi::new()
            .posed_parts_from_joints(
                &figure,
                Transform::from_translation(Vec3::new(0.0, FIGURE_CENTER_Y, 0.0)),
                &[Quat::IDENTITY; PART_COUNT],
            )
            .expect("the rest pose resolves");
        let lowest = posed
            .iter()
            .map(|p| p.transform.translation.y - p.box_size.y * 0.5)
            .fold(f32::INFINITY, f32::min);
        let highest = posed
            .iter()
            .map(|p| p.transform.translation.y + p.box_size.y * 0.5)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(lowest.abs() < 0.02, "the soles sit at {lowest}");
        assert!(
            (highest - FIGURE_HEIGHT).abs() < 0.03,
            "the crown sits at {highest}"
        );
        assert!((hip_half_width() - 0.09).abs() < 1.0e-5);
    }
}
