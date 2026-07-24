//! Neutral placement data: where a text sits, in screen or world space.
//!
//! The text module never imports a camera, scene, render, or GPU module. It
//! stores *both* a screen and a world placement plus a [`Space`] discriminant and
//! carries them through to the glyph batch as pure data — an app reads the space
//! and applies the matching placement to a renderer contract. Because both are
//! always present, nothing here branches on the space.

use axiom_host::Pixels;
use axiom_kernel::{Radians, Ratio};
use axiom_math::{Transform, Vec2};

use crate::text_error::{TextError, TextResult};

/// Which space a text is placed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Space {
    /// 2D screen/UI space, pixels.
    #[default]
    Screen,
    /// 3D world space, via a transform.
    World,
}

impl Space {
    /// The stable byte discriminant.
    pub const fn raw(self) -> u8 {
        [0u8, 1][self as usize]
    }

    /// Recover a space from its byte.
    pub fn from_raw(raw: u8) -> Option<Space> {
        [Self::Screen, Self::World].get(raw as usize).copied()
    }
}

/// How a world-space text orients toward the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Billboard {
    /// Keep the text's own transform orientation.
    #[default]
    Fixed,
    /// Always face the camera fully.
    Camera,
    /// Face the camera about the vertical axis only.
    Vertical,
}

impl Billboard {
    /// The stable byte discriminant.
    pub const fn raw(self) -> u8 {
        [0u8, 1, 2][self as usize]
    }

    /// Recover a billboard mode from its byte.
    pub fn from_raw(raw: u8) -> Option<Billboard> {
        [Self::Fixed, Self::Camera, Self::Vertical]
            .get(raw as usize)
            .copied()
    }
}

/// Screen/UI placement, all in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPlacement {
    /// Anchor position in screen pixels.
    pub position: Vec2,
    /// Anchor point within the text box, `0..1` (top-left `(0,0)`).
    pub anchor: Vec2,
    /// Pivot for rotation/scale within the text box, `0..1`.
    pub pivot: Vec2,
    /// Rotation.
    pub rotation: Radians,
    /// Uniform scale (`> 0`).
    pub scale: Ratio,
    /// Z ordering key (higher draws on top), in pixels.
    pub z: Pixels,
    /// Snap final positions to whole pixels.
    pub pixel_snap: bool,
}

/// World placement, via a transform plus billboard/distance policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldPlacement {
    /// The text's world transform (its local origin is the text box top-left).
    pub transform: Transform,
    /// World units per text pixel (`> 0`) — how large a pixel is in the world.
    pub units_per_pixel: Ratio,
    /// Camera-facing policy.
    pub billboard: Billboard,
    /// Maximum render distance in pixels (`>= 0`; `0` = unlimited), carried as
    /// data.
    pub max_distance: Pixels,
    /// Whether the app should depth-test the text, carried as data.
    pub depth_test: bool,
}

/// Where a text is placed. Holds both a screen and a world placement; `space`
/// selects which an app applies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextPlacement {
    /// Which placement is active.
    pub space: Space,
    /// Screen placement (used when `space == Screen`).
    pub screen: ScreenPlacement,
    /// World placement (used when `space == World`).
    pub world: WorldPlacement,
}

impl Default for TextPlacement {
    /// The default: top-left screen origin, unrotated, unit scale, no snapping.
    fn default() -> TextPlacement {
        TextPlacement {
            space: Space::Screen,
            screen: ScreenPlacement {
                position: Vec2::ZERO,
                anchor: Vec2::ZERO,
                pivot: Vec2::ZERO,
                rotation: Radians::finite_or_zero(0.0),
                scale: Ratio::finite_or_zero(1.0),
                z: Pixels::new(0.0).expect("finite default z"),
                pixel_snap: false,
            },
            world: WorldPlacement {
                transform: Transform::IDENTITY,
                units_per_pixel: Ratio::finite_or_zero(1.0),
                billboard: Billboard::Fixed,
                max_distance: Pixels::new(0.0).expect("finite default distance"),
                depth_test: true,
            },
        }
    }
}

impl TextPlacement {
    /// A screen placement at a pixel position (other fields defaulted).
    pub fn at_screen(position: Vec2) -> TextPlacement {
        let base = TextPlacement::default();
        TextPlacement {
            space: Space::Screen,
            screen: ScreenPlacement {
                position,
                ..base.screen
            },
            ..base
        }
    }

    /// A world placement at a transform (other fields defaulted).
    pub fn at_world(transform: Transform) -> TextPlacement {
        let base = TextPlacement::default();
        TextPlacement {
            space: Space::World,
            world: WorldPlacement {
                transform,
                ..base.world
            },
            ..base
        }
    }

    /// Reject invalid placement numbers: screen problems are `InvalidDimensions`,
    /// world problems `InvalidTransform`. (Typed scalars are finite by
    /// construction; only Vec2 components and the sign of scale/distance are
    /// checked.)
    pub fn validate(self) -> TextResult<()> {
        let s = self.screen;
        (s.position.x.is_finite()
            & s.position.y.is_finite()
            & s.anchor.x.is_finite()
            & s.anchor.y.is_finite()
            & s.pivot.x.is_finite()
            & s.pivot.y.is_finite()
            & (s.scale.get() > 0.0))
            .then_some(())
            .ok_or(TextError::InvalidDimensions)
            .and_then(|()| {
                let w = self.world;
                (w.transform.translation.x.is_finite()
                    & w.transform.translation.y.is_finite()
                    & w.transform.translation.z.is_finite()
                    & (w.units_per_pixel.get() > 0.0)
                    & (w.max_distance.get() >= 0.0))
                    .then_some(())
                    .ok_or(TextError::InvalidTransform)
            })
    }

    /// Append the placement: the space byte, then both sub-placements.
    pub(crate) fn write_to(self, writer: &mut axiom_kernel::BinaryWriter) {
        writer.write_u8(self.space.raw());
        let s = self.screen;
        s.position.write_to(writer);
        s.anchor.write_to(writer);
        s.pivot.write_to(writer);
        writer.write_f32(s.rotation.get());
        writer.write_f32(s.scale.get());
        writer.write_f32(s.z.get());
        writer.write_bool(s.pixel_snap);
        let w = self.world;
        w.transform.write_to(writer);
        writer.write_f32(w.units_per_pixel.get());
        writer.write_u8(w.billboard.raw());
        writer.write_f32(w.max_distance.get());
        writer.write_bool(w.depth_test);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_round_trip() {
        [Space::Screen, Space::World].into_iter().for_each(|s| {
            assert_eq!(Space::from_raw(s.raw()), Some(s));
        });
        [Billboard::Fixed, Billboard::Camera, Billboard::Vertical]
            .into_iter()
            .for_each(|b| assert_eq!(Billboard::from_raw(b.raw()), Some(b)));
        assert_eq!(Space::from_raw(9), None);
        assert_eq!(Billboard::from_raw(9), None);
    }

    #[test]
    fn constructors_and_default() {
        assert_eq!(TextPlacement::default().space, Space::Screen);
        assert_eq!(
            TextPlacement::at_screen(Vec2::new(10.0, 20.0))
                .screen
                .position,
            Vec2::new(10.0, 20.0)
        );
        assert_eq!(
            TextPlacement::at_world(Transform::IDENTITY).space,
            Space::World
        );
        assert_eq!(TextPlacement::default().validate(), Ok(()));
    }

    #[test]
    fn rejects_bad_screen_and_world() {
        let mut bad_screen = TextPlacement::default();
        bad_screen.screen.scale = Ratio::finite_or_zero(0.0);
        assert_eq!(bad_screen.validate(), Err(TextError::InvalidDimensions));
        let mut bad_world = TextPlacement::default();
        bad_world.world.units_per_pixel = Ratio::finite_or_zero(0.0);
        assert_eq!(bad_world.validate(), Err(TextError::InvalidTransform));
    }
}
