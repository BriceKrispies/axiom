//! A directional-light component an app spawns onto a node.

use axiom_kernel::Ratio;
use axiom_math::Vec3;

use crate::color::Color;

/// A directional light: a world-space direction, a colour, and an intensity.
///
/// The colour and intensity attach to the light's scene node; the direction is
/// a per-frame render input (the engine feeds it to the render pipeline), which
/// is why all three live together on the authored component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalLight {
    pub direction: Vec3,
    pub color: Color,
    pub intensity: Ratio,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_direction_color_and_intensity() {
        let light = DirectionalLight {
            direction: Vec3::new(0.3, -1.0, 0.4),
            color: Color::WHITE,
            intensity: Ratio::new(1.0).expect("default intensity is finite"),
        };
        assert_eq!(light.direction, Vec3::new(0.3, -1.0, 0.4));
        assert_eq!(light.color, Color::WHITE);
        assert_eq!(light.intensity, Ratio::new(1.0).unwrap());
    }
}
