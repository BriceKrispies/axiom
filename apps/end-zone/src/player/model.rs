//! The original arcade-football player model: a 17-box procedural figure
//! authored through the engine's `axiom-figure` vocabulary. Exaggerated arcade
//! proportions — oversized helmet with a facemask bar, broad shoulder-pad
//! slab, sturdy torso, low center of gravity. Part colors come from a
//! [`crate::data::TeamPalette`]; construction contains zero team branches.
//!
//! Units are yards (1 world unit = 1 yard). Y up, toes point +Z,
//! parent-before-child.

use axiom::prelude::Vec3;
use axiom_figure::{FigureDefinition, FigurePart};
use axiom_math::{Quat, Transform};

/// Number of parts in the player figure.
pub const PART_COUNT: usize = 17;

/// Height of the figure's local origin above the feet (the body transform the
/// rig receives is the sim ground position raised by this).
pub const FIGURE_CENTER_Y: f32 = 1.0;

// Palette slots (part tags). The scene maps each tag to a team material.
pub const TAG_HELMET: u32 = 0;
pub const TAG_FACEMASK: u32 = 1;
pub const TAG_JERSEY: u32 = 2;
pub const TAG_PANTS: u32 = 3;
pub const TAG_SKIN: u32 = 4;
pub const TAG_SHOES: u32 = 5;
pub const TAG_TRIM: u32 = 6;
/// Number of distinct tags (palette size).
pub const TAG_COUNT: usize = 7;

// Part indices (used by the animation to address joints).
pub const PELVIS: usize = 0;
pub const TORSO: usize = 1;
pub const PADS: usize = 2;
pub const HELMET: usize = 3;
pub const FACEMASK: usize = 4;
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
/// from the parent's pivot; the box offset centers the limb box while it
/// pivots at the joint.
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

/// Feet on y≈0, helmet top ≈ 2.14. Pelvis root pivot sits at absolute y 1.08
/// (0.08 above the figure center).
pub const PARTS: [PartSpec; PART_COUNT] = [
    // 0 pelvis (root)
    p(
        None,
        Vec3::new(0.0, 1.08 - FIGURE_CENTER_Y, 0.0),
        Vec3::new(0.50, 0.30, 0.30),
        Vec3::ZERO,
        TAG_PANTS,
    ),
    // 1 torso
    p(
        Some(0),
        Vec3::new(0.0, 0.36, 0.0),
        Vec3::new(0.62, 0.48, 0.36),
        Vec3::new(0.0, 0.05, 0.0),
        TAG_JERSEY,
    ),
    // 2 shoulder-pad slab
    p(
        Some(1),
        Vec3::new(0.0, 0.30, 0.0),
        Vec3::new(0.88, 0.18, 0.46),
        Vec3::ZERO,
        TAG_TRIM,
    ),
    // 3 helmet (oversized)
    p(
        Some(1),
        Vec3::new(0.0, 0.42, 0.0),
        Vec3::new(0.34, 0.32, 0.36),
        Vec3::new(0.0, 0.12, 0.0),
        TAG_HELMET,
    ),
    // 4 facemask bar (the face opening)
    p(
        Some(3),
        Vec3::new(0.0, 0.08, 0.20),
        Vec3::new(0.28, 0.12, 0.08),
        Vec3::ZERO,
        TAG_FACEMASK,
    ),
    // 5/6/7 left thigh, shin, foot
    p(
        Some(0),
        Vec3::new(-0.14, -0.14, 0.0),
        Vec3::new(0.20, 0.46, 0.22),
        Vec3::new(0.0, -0.24, 0.0),
        TAG_PANTS,
    ),
    p(
        Some(5),
        Vec3::new(0.0, -0.48, 0.0),
        Vec3::new(0.16, 0.42, 0.18),
        Vec3::new(0.0, -0.20, 0.0),
        TAG_TRIM,
    ),
    p(
        Some(6),
        Vec3::new(0.0, -0.40, 0.0),
        Vec3::new(0.17, 0.12, 0.34),
        Vec3::new(0.0, -0.02, 0.08),
        TAG_SHOES,
    ),
    // 8/9/10 right thigh, shin, foot
    p(
        Some(0),
        Vec3::new(0.14, -0.14, 0.0),
        Vec3::new(0.20, 0.46, 0.22),
        Vec3::new(0.0, -0.24, 0.0),
        TAG_PANTS,
    ),
    p(
        Some(8),
        Vec3::new(0.0, -0.48, 0.0),
        Vec3::new(0.16, 0.42, 0.18),
        Vec3::new(0.0, -0.20, 0.0),
        TAG_TRIM,
    ),
    p(
        Some(9),
        Vec3::new(0.0, -0.40, 0.0),
        Vec3::new(0.17, 0.12, 0.34),
        Vec3::new(0.0, -0.02, 0.08),
        TAG_SHOES,
    ),
    // 11/12/13 left upper arm, forearm, hand
    p(
        Some(1),
        Vec3::new(-0.42, 0.20, 0.0),
        Vec3::new(0.16, 0.40, 0.16),
        Vec3::new(0.0, -0.20, 0.0),
        TAG_JERSEY,
    ),
    p(
        Some(11),
        Vec3::new(0.0, -0.40, 0.0),
        Vec3::new(0.13, 0.34, 0.13),
        Vec3::new(0.0, -0.17, 0.0),
        TAG_SKIN,
    ),
    p(
        Some(12),
        Vec3::new(0.0, -0.34, 0.0),
        Vec3::new(0.12, 0.14, 0.13),
        Vec3::new(0.0, -0.07, 0.0),
        TAG_SKIN,
    ),
    // 14/15/16 right upper arm, forearm, hand
    p(
        Some(1),
        Vec3::new(0.42, 0.20, 0.0),
        Vec3::new(0.16, 0.40, 0.16),
        Vec3::new(0.0, -0.20, 0.0),
        TAG_JERSEY,
    ),
    p(
        Some(14),
        Vec3::new(0.0, -0.40, 0.0),
        Vec3::new(0.13, 0.34, 0.13),
        Vec3::new(0.0, -0.17, 0.0),
        TAG_SKIN,
    ),
    p(
        Some(15),
        Vec3::new(0.0, -0.34, 0.0),
        Vec3::new(0.12, 0.14, 0.13),
        Vec3::new(0.0, -0.07, 0.0),
        TAG_SKIN,
    ),
];

/// Build the player figure definition (rest rotations are identity — every
/// joint is driven per tick by the animation through the rig).
pub fn player_figure() -> FigureDefinition {
    let parts = PARTS
        .iter()
        .map(|s| {
            let rest = Transform::new(s.offset, Quat::IDENTITY, Vec3::ONE);
            match s.parent {
                None => FigurePart::root(rest, s.box_size, s.box_offset, s.tag),
                Some(parent) => FigurePart::child(parent, rest, s.box_size, s.box_offset, s.tag),
            }
        })
        .collect();
    FigureDefinition::new(parts)
}
