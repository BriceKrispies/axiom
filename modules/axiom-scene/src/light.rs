//! Directional / point light component.

use axiom_kernel::{FieldSchema, TypeSchema};
use axiom_math::{MathApi, Vec3};

use crate::light_kind::LightKind;
use crate::scene_error::SceneError;
use crate::scene_result::SceneResult;

/// A light component, stored on the node entity it belongs to.
///
/// Plain data: a kind, a colour, and a positive finite intensity. The node the
/// light follows is the entity this component is keyed by. All validation flows
/// through [`MathApi::validate_finite`] so the light inherits the engine's
/// scalar discipline rather than rolling its own `is_finite` check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    kind: LightKind,
    color: Vec3,
    intensity: f32,
}

impl Light {
    /// The reflected shape of a light component.
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "Light",
        &[
            FieldSchema::new("kind", "u32"),
            FieldSchema::new("color", "Vec3"),
            FieldSchema::new("intensity", "f32"),
        ],
    );

    /// Build a directional light. `color` is a linear-RGB triple; each component
    /// must be finite and non-negative. `intensity` must be finite and
    /// non-negative.
    pub fn directional(math: &MathApi, color: Vec3, intensity: f32) -> SceneResult<Self> {
        Light::build(math, LightKind::Directional, color, intensity)
    }

    /// Build a point light. Same validation rules as [`Light::directional`].
    pub fn point(math: &MathApi, color: Vec3, intensity: f32) -> SceneResult<Self> {
        Light::build(math, LightKind::Point, color, intensity)
    }

    fn build(
        math: &MathApi,
        kind: LightKind,
        color: Vec3,
        intensity: f32,
    ) -> SceneResult<Self> {
        for component in [color.x, color.y, color.z, intensity] {
            if math.validate_finite(component).is_err() {
                return Err(SceneError::invalid_light_parameters(
                    "light parameters must be finite",
                ));
            }
        }
        if color.x < 0.0 || color.y < 0.0 || color.z < 0.0 {
            return Err(SceneError::invalid_light_parameters(
                "light colour components must be non-negative",
            ));
        }
        if intensity < 0.0 {
            return Err(SceneError::invalid_light_parameters(
                "light intensity must be non-negative",
            ));
        }
        Ok(Light {
            kind,
            color,
            intensity,
        })
    }

    pub const fn kind(&self) -> LightKind {
        self.kind
    }

    pub const fn color(&self) -> Vec3 {
        self.color
    }

    pub const fn intensity(&self) -> f32 {
        self.intensity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;

    fn math() -> MathApi {
        MathApi::new()
    }

    #[test]
    fn directional_light_is_built_with_valid_params() {
        let l = Light::directional(&math(), Vec3::new(1.0, 1.0, 1.0), 5.0).unwrap();
        assert_eq!(l.kind(), LightKind::Directional);
        assert_eq!(l.intensity(), 5.0);
    }

    #[test]
    fn point_light_is_built_with_valid_params() {
        let l = Light::point(&math(), Vec3::new(0.5, 0.5, 0.5), 2.0).unwrap();
        assert_eq!(l.kind(), LightKind::Point);
        assert_eq!(l.color().x, 0.5);
    }

    #[test]
    fn negative_intensity_is_rejected() {
        let err = Light::directional(&math(), Vec3::ONE, -1.0).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidLightParameters);
    }

    #[test]
    fn nan_intensity_is_rejected() {
        let err = Light::point(&math(), Vec3::ONE, f32::NAN).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidLightParameters);
    }

    #[test]
    fn negative_color_components_are_rejected_per_channel() {
        for bad in [
            Vec3::new(-0.1, 0.0, 0.0),
            Vec3::new(0.0, -0.1, 0.0),
            Vec3::new(0.0, 0.0, -0.1),
        ] {
            let err = Light::directional(&math(), bad, 1.0).unwrap_err();
            assert_eq!(err.code(), SceneErrorCode::InvalidLightParameters);
        }
    }

    #[test]
    fn nan_color_component_is_rejected() {
        let err = Light::point(&math(), Vec3::new(f32::NAN, 0.0, 0.0), 1.0).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidLightParameters);
    }

    #[test]
    fn zero_intensity_and_zero_channels_are_allowed() {
        let l = Light::directional(&math(), Vec3::ONE, 0.0).unwrap();
        assert_eq!(l.intensity(), 0.0);
        // Each channel exactly 0.0 is non-negative and accepted.
        assert_eq!(Light::directional(&math(), Vec3::new(0.0, 1.0, 1.0), 1.0).unwrap().color().x, 0.0);
        assert_eq!(Light::directional(&math(), Vec3::new(1.0, 0.0, 1.0), 1.0).unwrap().color().y, 0.0);
        assert_eq!(Light::directional(&math(), Vec3::new(1.0, 1.0, 0.0), 1.0).unwrap().color().z, 0.0);
    }

    #[test]
    fn schema_names_the_light_fields() {
        assert_eq!(Light::SCHEMA.name(), "Light");
        assert_eq!(Light::SCHEMA.fields().len(), 3);
        assert_eq!(Light::SCHEMA.fields()[1].name(), "color");
    }
}
