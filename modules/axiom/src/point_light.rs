//! A point-light component an app spawns onto a node.

use axiom_kernel::Ratio;

use crate::color::Color;

/// A point light: a colour and an intensity that radiate from the light's scene
/// node **position**. Unlike a [`crate::directional_light::DirectionalLight`], a
/// point light has no direction — its world position (the node's world transform)
/// is what matters, so it can orbit by parenting under an animated node. The
/// engine resolves the position per frame and feeds it to the renderer, which
/// attenuates it by distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    pub color: Color,
    pub intensity: Ratio,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_color_and_intensity() {
        let light = PointLight {
            color: Color::WHITE,
            intensity: Ratio::new(2.0).expect("authored intensity is finite"),
        };
        assert_eq!(light.color, Color::WHITE);
        assert_eq!(light.intensity, Ratio::new(2.0).unwrap());
    }
}
