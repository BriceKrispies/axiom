//! Render-facing light data.

use axiom_kernel::Ratio;
use axiom_math::Vec3;

/// Render-facing light kind tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderLightKind {
    Directional,
    Point,
}

/// A render-facing light: kind, position-or-direction in world space,
/// colour, intensity. The renderer does not know about scene nodes —
/// the app pre-computes the world-space vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderLight {
    kind: RenderLightKind,
    direction_or_position_world: Vec3,
    color: Vec3,
    intensity: Ratio,
}

impl RenderLight {
    pub const fn new(
        kind: RenderLightKind,
        direction_or_position_world: Vec3,
        color: Vec3,
        intensity: Ratio,
    ) -> Self {
        RenderLight {
            kind,
            direction_or_position_world,
            color,
            intensity,
        }
    }

    pub const fn kind(&self) -> RenderLightKind {
        self.kind
    }

    pub const fn direction_or_position_world(&self) -> Vec3 {
        self.direction_or_position_world
    }

    pub const fn color(&self) -> Vec3 {
        self.color
    }

    pub const fn intensity(&self) -> Ratio {
        self.intensity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directional_light_accessors_round_trip() {
        let l = RenderLight::new(
            RenderLightKind::Directional,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::ONE,
            Ratio::new(1.0).unwrap(),
        );
        assert_eq!(l.kind(), RenderLightKind::Directional);
        assert_eq!(l.direction_or_position_world(), Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(l.color(), Vec3::ONE);
        assert_eq!(l.intensity(), Ratio::new(1.0).unwrap());
    }

    #[test]
    fn intensity_is_the_constructed_value() {
        // Kills `intensity -> 1.0`: use a value distinct from 1.0.
        let l = RenderLight::new(
            RenderLightKind::Point,
            Vec3::ZERO,
            Vec3::ONE,
            Ratio::new(3.5).unwrap(),
        );
        assert_eq!(l.intensity(), Ratio::new(3.5).unwrap());
        assert_eq!(l.intensity().get(), 3.5);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderLight::new(
            RenderLightKind::Point,
            Vec3::ZERO,
            Vec3::ZERO,
            Ratio::new(1.0).unwrap(),
        );
        let b = RenderLight::new(
            RenderLightKind::Point,
            Vec3::ZERO,
            Vec3::ZERO,
            Ratio::new(1.0).unwrap(),
        );
        let c = RenderLight::new(
            RenderLightKind::Directional,
            Vec3::ZERO,
            Vec3::ZERO,
            Ratio::new(1.0).unwrap(),
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
